//! Minecraft 1.8.9 online-mode login. Credentials are submitted only to Mojang's
//! HTTPS session service. The game server receives the two RSA ciphertexts.
use crate::protocol::{Reader, Writer};
use aes::Aes128;
use anyhow::{bail, ensure, Context, Result};
use cfb8::cipher::KeyIvInit;
use rand::{rngs::OsRng, RngCore};
use rsa::{pkcs8::DecodePublicKey, traits::PublicKeyParts, Pkcs1v15Encrypt, RsaPublicKey};
use serde::Serialize;
use sha1::{Digest, Sha1};
use std::time::Duration;

const JOIN: &str = "https://sessionserver.mojang.com/session/minecraft/join";
pub(super) struct Credentials {
    token: String,
    profile: String,
}
impl Credentials {
    fn new(token: Option<String>, profile: Option<String>) -> Result<Self> {
        let token = token.filter(|v| !v.is_empty()).context(
            "정품(Online-Mode) 서버입니다. 클라이언트.exe에서 Microsoft 계정을 로그인·선택해 주세요 (Missing FERRUM189_MC_ACCESS_TOKEN)",
        )?;
        let profile = profile.context(
            "정품(Online-Mode) 서버입니다. 클라이언트.exe에서 Microsoft 계정을 로그인·선택해 주세요 (Missing FERRUM189_MC_PROFILE_ID)",
        )?;
        let simple = match profile.len() {
            32 => profile,
            36 if [8, 13, 18, 23]
                .iter()
                .all(|&i| profile.as_bytes()[i] == b'-') =>
            {
                profile.replace('-', "")
            }
            _ => bail!("FERRUM189_MC_PROFILE_ID must be a Minecraft profile UUID"),
        };
        ensure!(
            simple.len() == 32 && simple.bytes().all(|b| b.is_ascii_hexdigit()),
            "FERRUM189_MC_PROFILE_ID must be a Minecraft profile UUID"
        );
        Ok(Self {
            token,
            profile: simple.to_ascii_lowercase(),
        })
    }
    fn from_env() -> Result<Self> {
        Self::new(
            std::env::var("FERRUM189_MC_ACCESS_TOKEN").ok(),
            std::env::var("FERRUM189_MC_PROFILE_ID").ok(),
        )
    }
    fn join(&self, hash: &str) -> Result<()> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Join<'a> {
            access_token: &'a str,
            selected_profile: &'a str,
            server_id: &'a str,
        }
        let body = serde_json::to_vec(&Join {
            access_token: &self.token,
            selected_profile: &self.profile,
            server_id: hash,
        })?;
        let agent = ureq::Agent::config_builder()
            .https_only(true)
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(12)))
            .user_agent("Java/1.8.0_51")
            .build()
            .new_agent();
        let mut last_status = 0u16;
        for attempt in 0..3u64 {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(650 * attempt));
            }
            let response = match agent
                .post(JOIN)
                .header("Content-Type", "application/json")
                .header("Accept", "application/json")
                .send(body.as_slice())
            {
                Ok(resp) => resp,
                Err(_) if attempt < 2 => continue,
                Err(_) => bail!("Minecraft session service could not be reached securely"),
            };
            last_status = response.status().as_u16();
            match last_status {
                204 | 200 => return Ok(()),
                401 | 403 => {
                    bail!(
                        "Minecraft 세션 토큰이 만료되었거나 거절되었습니다. 클라이언트.exe에서 계정을 새로고침하거나 다시 로그인해 주세요 (HTTP {last_status})"
                    )
                }
                429 | 500 | 502 | 503 | 504 if attempt < 2 => continue,
                _ => break,
            }
        }
        bail!("Minecraft session service returned HTTP {last_status}")
    }
}

pub(crate) struct Ciphers {
    pub send: cfb8::Encryptor<Aes128>,
    pub receive: cfb8::Decryptor<Aes128>,
}
impl Ciphers {
    pub(crate) fn new(secret: &[u8; 16]) -> Self {
        Self {
            send: cfb8::Encryptor::new(secret.into(), secret.into()),
            receive: cfb8::Decryptor::new(secret.into(), secret.into()),
        }
    }
}
// Java BigInteger(digest).toString(16) uses signed, big-endian two's complement.
pub(crate) fn signed_hex(mut digest: [u8; 20]) -> String {
    let negative = digest[0] & 0x80 != 0;
    if negative {
        let mut carry = true;
        for b in digest.iter_mut().rev() {
            let (n, overflow) = (!*b).overflowing_add(u8::from(carry));
            *b = n;
            carry = overflow;
        }
    }
    let hex = digest
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let significant = hex.trim_start_matches('0');
    if significant.is_empty() {
        "0".into()
    } else if negative {
        format!("-{significant}")
    } else {
        significant.to_owned()
    }
}
pub(super) fn negotiate(
    r: &mut Reader<'_>,
    join: impl FnOnce(&str) -> Result<()>,
) -> Result<(Writer, Ciphers)> {
    let id = r.string(20)?;
    let key_len = r.count(1024)?;
    let der = r.take(key_len)?;
    let token_len = r.count(512)?;
    let token = r.take(token_len)?;
    r.finish()?;
    let key = RsaPublicKey::from_public_key_der(der).context("Invalid server RSA public key")?;
    ensure!(
        key.size() >= 128 && key.size() <= 512 && token_len > 0 && token_len <= key.size() - 11,
        "Invalid server encryption challenge"
    );
    let mut secret = [0u8; 16];
    OsRng
        .try_fill_bytes(&mut secret)
        .context("Could not generate Minecraft session key")?;
    let latin1: Vec<_> = id
        .chars()
        .map(|c| u8::try_from(c as u32).unwrap_or(b'?'))
        .collect();
    let digest = Sha1::new()
        .chain_update(&latin1)
        .chain_update(secret)
        .chain_update(der)
        .finalize();
    join(&signed_hex(digest.into()))?;
    let mut response = Writer::new();
    for cleartext in [secret.as_slice(), token] {
        let cipher = key
            .encrypt(&mut OsRng, Pkcs1v15Encrypt, cleartext)
            .context("Cannot encrypt server challenge")?;
        response.varint(cipher.len() as i32).bytes(&cipher);
    }
    Ok((response, Ciphers::new(&secret)))
}
pub(super) fn join_session(hash: &str) -> Result<()> {
    Credentials::from_env()?.join(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsa::{pkcs8::EncodePublicKey, RsaPrivateKey};
    #[test]
    fn java_signed_sha1_and_private_login_inputs() {
        for (input, want) in [
            ("Notch", "4ed1f46bbe04bc756bcb17c0c7ce3e4632f06a48"),
            ("jeb_", "-7c9d5b0044c130109a5d7b5fb5c317c02b4e28c1"),
            ("simon", "88e16a1019277b15d58faf0541e11910eb756f6"),
        ] {
            assert_eq!(signed_hex(Sha1::digest(input).into()), want);
        }
        assert!(Credentials::new(None, None).is_err());
        assert!(Credentials::new(Some("test-token".into()), Some("bad-id".into())).is_err());
        assert_eq!(
            Credentials::new(
                Some("test-token".into()),
                Some("01234567-89AB-CDEF-0123-456789ABCDEF".into())
            )
            .unwrap()
            .profile,
            "0123456789abcdef0123456789abcdef"
        );
    }
    #[test]
    fn challenge_roundtrip_and_cfb8_state_cross_packet_boundaries() {
        let key = RsaPrivateKey::new(&mut OsRng, 1024).unwrap();
        let der = RsaPublicKey::from(&key).to_public_key_der().unwrap();
        let mut request = Writer::new();
        request
            .string("")
            .varint(der.as_ref().len() as i32)
            .bytes(der.as_ref())
            .varint(4)
            .bytes(&[4, 3, 2, 1]);
        let mut hash = String::new();
        let (response, mut client) = negotiate(&mut Reader::new(&request.0), |value| {
            hash = value.into();
            Ok(())
        })
        .unwrap();
        let mut reply = Reader::new(&response.0);
        let mut decrypt = || {
            let n = reply.count(256).unwrap();
            key.decrypt(Pkcs1v15Encrypt, reply.take(n).unwrap())
                .unwrap()
        };
        let secret: [u8; 16] = decrypt().try_into().unwrap();
        assert_eq!(decrypt(), [4, 3, 2, 1]);
        reply.finish().unwrap();
        assert_eq!(
            hash,
            signed_hex(
                Sha1::new()
                    .chain_update(secret)
                    .chain_update(der.as_ref())
                    .finalize()
                    .into()
            )
        );
        let mut peer = Ciphers::new(&secret);
        let plain = b"two encrypted Minecraft frame fragments";
        let mut to_peer = plain.to_vec();
        for chunk in to_peer.chunks_mut(3) {
            client.send.encrypt(chunk);
        }
        for chunk in to_peer.chunks_mut(7) {
            peer.receive.decrypt(chunk);
        }
        assert_eq!(&to_peer, plain);
        let mut from_peer = plain.to_vec();
        for chunk in from_peer.chunks_mut(9) {
            peer.send.encrypt(chunk);
        }
        for chunk in from_peer.chunks_mut(1) {
            client.receive.decrypt(chunk);
        }
        assert_eq!(&from_peer, plain);
        assert!(negotiate(&mut Reader::new(&request.0), |_| bail!(
            "private test rejected session"
        ))
        .is_err());
    }
}
