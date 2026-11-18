//! Microsoft OAuth2 (Device Code + Refresh) -> Xbox Live -> XSTS -> Minecraft Services
//! authentication and Windows DPAPI-encrypted local account persistence.
#![allow(dead_code)]

use anyhow::{anyhow, bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const MS_CLIENT_ID: &str = "00000000441cc96b";
const MS_SCOPE: &str = "service::user.auth.xboxlive.com::MBI_SSL";
const MS_DEVICE_CODE_URL: &str = "https://login.live.com/oauth20_connect.srf";
const MS_TOKEN_URL: &str = "https://login.live.com/oauth20_token.srf";
const XBL_AUTH_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_AUTH_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MC_XBOX_LOGIN_URL: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const MC_LAUNCHER_LOGIN_URL: &str = "https://api.minecraftservices.com/launcher/login";
const MC_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";

#[derive(Clone, Debug)]
pub struct DeviceCodePrompt {
    pub user_code: String,
    pub device_code: String,
    pub verification_uri: String,
    pub interval_secs: u64,
    pub expires_in_secs: u64,
}

#[derive(Clone, Debug)]
pub enum PollOutcome {
    Pending,
    SlowDown,
    Authorized(AuthenticatedAccount),
}

#[derive(Clone, Debug)]
pub struct AuthenticatedAccount {
    pub uuid: String,
    pub username: String,
    pub mc_access_token: String,
    pub ms_refresh_token: String,
    pub expires_at_epoch_secs: u64,
}

#[derive(Serialize, Deserialize)]
struct SecretPayload {
    ms_refresh_token: String,
    mc_access_token: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredAccount {
    pub uuid: String,
    pub username: String,
    pub expires_at_epoch_secs: u64,
    pub encrypted_secrets_hex: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountStore {
    #[serde(default)]
    pub selected_uuid: Option<String>,
    #[serde(default = "default_offline_name")]
    pub offline_username: String,
    #[serde(default)]
    pub accounts: Vec<StoredAccount>,
}

fn default_offline_name() -> String {
    "FerrumPlayer".into()
}

impl Default for AccountStore {
    fn default() -> Self {
        Self {
            selected_uuid: None,
            offline_username: default_offline_name(),
            accounts: Vec::new(),
        }
    }
}

pub fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn project_root() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            // If running from workspace root (e.g. C:\Users\a\Desktop\hi\클라이언트.exe)
            if exe_dir.join("Cargo.toml").is_file() || exe_dir.join(".local").is_dir() {
                return exe_dir.to_path_buf();
            }
            // If running from target\release\launcher.exe or target\debug\...
            if let Some(parent2) = exe_dir.parent().and_then(Path::parent) {
                if parent2.join("Cargo.toml").is_file() || parent2.join(".local").is_dir() {
                    return parent2.to_path_buf();
                }
            }
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

pub fn default_store_path() -> PathBuf {
    project_root().join(".local").join("ms_accounts.json")
}

pub fn normalize_uuid(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    let simple = match trimmed.len() {
        32 => trimmed.to_owned(),
        36 if [8, 13, 18, 23]
            .iter()
            .all(|&i| trimmed.as_bytes()[i] == b'-') =>
        {
            trimmed.replace('-', "")
        }
        _ => bail!("유효하지 않은 마인크래프트 프로필 UUID 형식입니다"),
    };
    ensure!(
        simple.len() == 32 && simple.bytes().all(|b| b.is_ascii_hexdigit()),
        "유효하지 않은 마인크래프트 프로필 UUID입니다"
    );
    Ok(simple.to_ascii_lowercase())
}

pub fn dashed_uuid(simple: &str) -> String {
    if simple.len() == 32 {
        format!(
            "{}-{}-{}-{}-{}",
            &simple[0..8],
            &simple[8..12],
            &simple[12..16],
            &simple[16..20],
            &simple[20..32]
        )
    } else {
        simple.to_owned()
    }
}

fn url_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 16);
    for &b in input.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn decode_hex(hex: &str) -> Result<Vec<u8>> {
    let trimmed = hex.trim();
    ensure!(trimmed.len() % 2 == 0, "손상된 암호화 토큰 데이터 길이");
    let mut out = Vec::with_capacity(trimmed.len() / 2);
    for chunk in trimmed.as_bytes().chunks_exact(2) {
        let s = std::str::from_utf8(chunk).context("손상된 암호화 토큰 문자열")?;
        let byte = u8::from_str_radix(s, 16).context("손상된 16진수 암호화 토큰")?;
        out.push(byte);
    }
    Ok(out)
}

#[cfg(windows)]
fn dpapi_protect(plain: &[u8]) -> Result<Vec<u8>> {
    use std::ptr;
    #[repr(C)]
    struct DataBlob {
        cb_data: u32,
        pb_data: *mut u8,
    }
    #[link(name = "crypt32")]
    unsafe extern "system" {
        fn CryptProtectData(
            data_in: *const DataBlob,
            data_descr: *const u16,
            optional_entropy: *const DataBlob,
            reserved: *mut core::ffi::c_void,
            prompt_struct: *const core::ffi::c_void,
            flags: u32,
            data_out: *mut DataBlob,
        ) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LocalFree(mem: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    }

    let input = DataBlob {
        cb_data: plain.len() as u32,
        pb_data: plain.as_ptr() as *mut u8,
    };
    let mut output = DataBlob {
        cb_data: 0,
        pb_data: ptr::null_mut(),
    };
    // CRYPTPROTECT_UI_FORBIDDEN = 0x1
    let ok = unsafe {
        CryptProtectData(
            &input,
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null(),
            0x1,
            &mut output,
        )
    };
    ensure!(ok != 0 && !output.pb_data.is_null(), "Windows DPAPI 토큰 암호화 실패");
    let slice =
        unsafe { std::slice::from_raw_parts(output.pb_data, output.cb_data as usize).to_vec() };
    unsafe {
        LocalFree(output.pb_data.cast());
    }
    Ok(slice)
}

#[cfg(windows)]
fn dpapi_unprotect(cipher: &[u8]) -> Result<Vec<u8>> {
    use std::ptr;
    #[repr(C)]
    struct DataBlob {
        cb_data: u32,
        pb_data: *mut u8,
    }
    #[link(name = "crypt32")]
    unsafe extern "system" {
        fn CryptUnprotectData(
            data_in: *const DataBlob,
            data_descr: *mut *mut u16,
            optional_entropy: *const DataBlob,
            reserved: *mut core::ffi::c_void,
            prompt_struct: *const core::ffi::c_void,
            flags: u32,
            data_out: *mut DataBlob,
        ) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LocalFree(mem: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    }

    let input = DataBlob {
        cb_data: cipher.len() as u32,
        pb_data: cipher.as_ptr() as *mut u8,
    };
    let mut output = DataBlob {
        cb_data: 0,
        pb_data: ptr::null_mut(),
    };
    let ok = unsafe {
        CryptUnprotectData(
            &input,
            ptr::null_mut(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null(),
            0x1,
            &mut output,
        )
    };
    ensure!(
        ok != 0 && !output.pb_data.is_null(),
        "저장된 Microsoft 계정 토큰을 복호화할 수 없습니다 (다시 로그인해 주세요)"
    );
    let slice =
        unsafe { std::slice::from_raw_parts(output.pb_data, output.cb_data as usize).to_vec() };
    unsafe {
        LocalFree(output.pb_data.cast());
    }
    Ok(slice)
}

#[cfg(not(windows))]
fn dpapi_protect(plain: &[u8]) -> Result<Vec<u8>> {
    Ok(plain.to_vec())
}

#[cfg(not(windows))]
fn dpapi_unprotect(cipher: &[u8]) -> Result<Vec<u8>> {
    Ok(cipher.to_vec())
}

impl StoredAccount {
    pub fn from_authenticated(auth: &AuthenticatedAccount) -> Result<Self> {
        let payload = serde_json::to_vec(&SecretPayload {
            ms_refresh_token: auth.ms_refresh_token.clone(),
            mc_access_token: auth.mc_access_token.clone(),
        })?;
        let encrypted = dpapi_protect(&payload)?;
        Ok(Self {
            uuid: normalize_uuid(&auth.uuid)?,
            username: auth.username.clone(),
            expires_at_epoch_secs: auth.expires_at_epoch_secs,
            encrypted_secrets_hex: encode_hex(&encrypted),
        })
    }

    fn decrypt_secrets(&self) -> Result<SecretPayload> {
        let bytes = decode_hex(&self.encrypted_secrets_hex)?;
        let mut plain = dpapi_unprotect(&bytes)?;
        let res = serde_json::from_slice::<SecretPayload>(&plain)
            .context("저장된 토큰 페이로드 파싱 실패");
        plain.fill(0);
        res
    }

    pub fn needs_refresh(&self) -> bool {
        now_epoch_secs().saturating_add(300) >= self.expires_at_epoch_secs
    }

    pub fn to_authenticated(&self) -> Result<AuthenticatedAccount> {
        let secrets = self.decrypt_secrets()?;
        Ok(AuthenticatedAccount {
            uuid: self.uuid.clone(),
            username: self.username.clone(),
            mc_access_token: secrets.mc_access_token,
            ms_refresh_token: secrets.ms_refresh_token,
            expires_at_epoch_secs: self.expires_at_epoch_secs,
        })
    }
}

impl AccountStore {
    pub fn load() -> Self {
        Self::load_from(&default_store_path()).unwrap_or_default()
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.is_file() {
            return Ok(Self::default());
        }
        let data = fs::read(path)?;
        let mut store: Self = serde_json::from_slice(&data)?;
        if store.offline_username.trim().is_empty() {
            store.offline_username = default_offline_name();
        }
        if let Some(sel) = &store.selected_uuid {
            if !store.accounts.iter().any(|a| &a.uuid == sel) {
                store.selected_uuid = store.accounts.first().map(|a| a.uuid.clone());
            }
        }
        Ok(store)
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&default_store_path())
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(self)?;
        let tmp = path.with_extension(format!("json.{:032x}.tmp", rand::random::<u128>()));
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        let result = (|| -> std::io::Result<()> {
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&tmp, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result.context("Microsoft 계정 파일을 저장하지 못했습니다")
    }

    pub fn upsert_authenticated(&mut self, auth: &AuthenticatedAccount) -> Result<()> {
        let stored = StoredAccount::from_authenticated(auth)?;
        if let Some(existing) = self.accounts.iter_mut().find(|a| a.uuid == stored.uuid) {
            *existing = stored.clone();
        } else {
            self.accounts.push(stored.clone());
        }
        self.selected_uuid = Some(stored.uuid);
        Ok(())
    }

    pub fn remove_uuid(&mut self, uuid: &str) {
        self.accounts.retain(|a| a.uuid != uuid);
        if self.selected_uuid.as_deref() == Some(uuid) {
            self.selected_uuid = self.accounts.first().map(|a| a.uuid.clone());
        }
    }

    pub fn selected(&self) -> Option<&StoredAccount> {
        let id = self.selected_uuid.as_deref()?;
        self.accounts.iter().find(|a| a.uuid == id)
    }

    pub fn ensure_fresh_selected(&mut self, path: &Path) -> Result<Option<AuthenticatedAccount>> {
        let Some(current) = self.selected().cloned() else {
            return Ok(None);
        };
        let auth = current.to_authenticated()?;
        if !current.needs_refresh() && !auth.mc_access_token.is_empty() {
            return Ok(Some(auth));
        }
        let refreshed = refresh_minecraft_session(&auth.ms_refresh_token)?;
        self.upsert_authenticated(&refreshed)?;
        self.save_to(path)?;
        Ok(Some(refreshed))
    }
}

const HTTP_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) FerrumLauncher/0.4";

fn https_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .https_only(true)
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(18)))
        .user_agent(HTTP_USER_AGENT)
        .build()
        .new_agent()
}

fn post_form(agent: &ureq::Agent, url: &str, pairs: &[(&str, &str)]) -> Result<(u16, serde_json::Value)> {
    let mut body = String::new();
    for (i, (k, v)) in pairs.iter().enumerate() {
        if i > 0 {
            body.push('&');
        }
        body.push_str(&url_encode(k));
        body.push('=');
        body.push_str(&url_encode(v));
    }
    let resp = agent
        .post(url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .send(body.as_bytes())
        .map_err(|e| anyhow!("Microsoft 인증 서버 연결 실패: {e}"))?;
    let status = resp.status().as_u16();
    let mut bytes = Vec::new();
    resp.into_body()
        .into_reader()
        .take(262_144)
        .read_to_end(&mut bytes)?;
    let json: serde_json::Value = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| serde_json::json!({ "raw_status": status }));
    Ok((status, json))
}

fn post_json(agent: &ureq::Agent, url: &str, payload: &serde_json::Value) -> Result<(u16, serde_json::Value)> {
    let raw = serde_json::to_vec(payload)?;
    let resp = agent
        .post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .send(raw.as_slice())
        .map_err(|e| anyhow!("인증 서버 요청 실패 ({url}): {e}"))?;
    let status = resp.status().as_u16();
    let mut bytes = Vec::new();
    resp.into_body()
        .into_reader()
        .take(262_144)
        .read_to_end(&mut bytes)?;
    let json: serde_json::Value = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| serde_json::json!({ "raw_status": status }));
    Ok((status, json))
}

fn public_oauth_error(json: &serde_json::Value) -> &str {
    match json["error"].as_str() {
        Some(code @ ("access_denied" | "invalid_client" | "invalid_grant" | "invalid_request"
            | "invalid_scope" | "unauthorized_client" | "temporarily_unavailable"
            | "server_error" | "interaction_required" | "unsupported_grant_type"
            | "unsupported_response_type")) => code,
        _ => "알 수 없는 오류",
    }
}

pub fn start_device_code_flow() -> Result<DeviceCodePrompt> {
    let agent = https_agent();
    let (status, json) = post_form(
        &agent,
        MS_DEVICE_CODE_URL,
        &[
            ("client_id", MS_CLIENT_ID),
            ("scope", MS_SCOPE),
            ("response_type", "device_code"),
        ],
    )?;
    if status != 200 {
        let desc = public_oauth_error(&json);
        bail!("Microsoft 기기 코드 발급 실패 (HTTP {status}): {desc}");
    }
    let user_code = json["user_code"]
        .as_str()
        .context("Microsoft 응답에 user_code가 없습니다")?
        .to_owned();
    let device_code = json["device_code"]
        .as_str()
        .context("Microsoft 응답에 device_code가 없습니다")?
        .to_owned();
    let verification_uri = json["verification_uri"]
        .as_str()
        .or_else(|| json["verification_url"].as_str())
        .unwrap_or("https://www.microsoft.com/link")
        .to_owned();
    let interval_secs = json["interval"].as_u64().unwrap_or(5).max(2);
    let expires_in_secs = json["expires_in"].as_u64().unwrap_or(900);
    Ok(DeviceCodePrompt {
        user_code,
        device_code,
        verification_uri,
        interval_secs,
        expires_in_secs,
    })
}

pub fn poll_device_code_with_progress(
    device_code: &str,
    on_approved: impl FnOnce(),
) -> Result<PollOutcome> {
    let agent = https_agent();
    let (status, json) = post_form(
        &agent,
        MS_TOKEN_URL,
        &[
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("client_id", MS_CLIENT_ID),
            ("device_code", device_code),
        ],
    )?;
    finish_device_poll(status, &json, on_approved, |access, refresh| {
        complete_xbox_and_minecraft(&agent, access, refresh)
    })
}

fn finish_device_poll(
    status: u16,
    json: &serde_json::Value,
    on_approved: impl FnOnce(),
    exchange: impl FnOnce(&str, String) -> Result<AuthenticatedAccount>,
) -> Result<PollOutcome> {
    if status != 200 {
        return match json["error"].as_str().unwrap_or("") {
            "authorization_pending" => Ok(PollOutcome::Pending),
            "slow_down" => Ok(PollOutcome::SlowDown),
            "authorization_declined" | "access_denied" => {
                bail!("사용자가 Microsoft 로그인 승인을 거부했습니다.")
            }
            "expired_token" | "bad_verification_code" => {
                bail!("인증 코드 유효 시간이 만료되었습니다. 다시 시도해 주세요.")
            }
            _ => {
                let desc = public_oauth_error(json);
                bail!("Microsoft 토큰 확인 실패 (HTTP {status}): {desc}")
            }
        };
    }

    let ms_access = json["access_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .context("Microsoft 응답에 access_token이 없습니다")?;
    let ms_refresh = json["refresh_token"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    on_approved();
    let auth = exchange(ms_access, ms_refresh)?;
    Ok(PollOutcome::Authorized(auth))
}

pub fn refresh_minecraft_session(ms_refresh_token: &str) -> Result<AuthenticatedAccount> {
    ensure!(
        !ms_refresh_token.trim().is_empty(),
        "리프레시 토큰이 없습니다. Microsoft 계정을 다시 추가해 주세요."
    );
    let agent = https_agent();
    let (status, json) = post_form(
        &agent,
        MS_TOKEN_URL,
        &[
            ("client_id", MS_CLIENT_ID),
            ("refresh_token", ms_refresh_token),
            ("grant_type", "refresh_token"),
            ("scope", MS_SCOPE),
        ],
    )?;
    if status != 200 {
        let desc = public_oauth_error(&json);
        bail!("Microsoft 계정 세션 갱신 실패 ({desc}). 계정을 다시 로그인해 주세요.");
    }
    let ms_access = json["access_token"]
        .as_str()
        .context("갱신 응답에 access_token이 없습니다")?;
    let next_refresh = json["refresh_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or(ms_refresh_token)
        .to_owned();
    complete_xbox_and_minecraft(&agent, ms_access, next_refresh)
}

fn complete_xbox_and_minecraft(
    agent: &ureq::Agent,
    ms_access_token: &str,
    ms_refresh_token: String,
) -> Result<AuthenticatedAccount> {
    // 1. Xbox Live Authenticate (try RpsTicket "t=<token>" first, fallback to "d=<token>")
    let mut xbl_resp = None;
    for prefix in ["t=", "d="] {
        let ticket = format!("{prefix}{ms_access_token}");
        let (status, json) = post_json(
            agent,
            XBL_AUTH_URL,
            &serde_json::json!({
                "Properties": {
                    "AuthMethod": "RPS",
                    "SiteName": "user.auth.xboxlive.com",
                    "RpsTicket": ticket,
                },
                "RelyingParty": "http://auth.xboxlive.com",
                "TokenType": "JWT"
            }),
        )?;
        if status == 200 && json["Token"].as_str().is_some() {
            xbl_resp = Some(json);
            break;
        }
    }
    let xbl_json = xbl_resp.context("Xbox Live 인증(XBL) 토큰 발급에 실패했습니다.")?;
    let xbl_token = xbl_json["Token"]
        .as_str()
        .context("XBL 응답에 Token이 없습니다")?;
    let uhs = xbl_json["DisplayClaims"]["xui"][0]["uhs"]
        .as_str()
        .context("XBL 응답에 사용자 해시(uhs)가 없습니다")?;

    // 2. XSTS Authorize
    let (xsts_status, xsts_json) = post_json(
        agent,
        XSTS_AUTH_URL,
        &serde_json::json!({
            "Properties": {
                "SandboxId": "RETAIL",
                "UserTokens": [xbl_token]
            },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT"
        }),
    )?;
    if xsts_status != 200 {
        let xerr = xsts_json["XErr"].as_u64().unwrap_or(0);
        let msg = match xerr {
            2148916233 => {
                "이 Microsoft 계정에 Xbox 프로필이 없습니다. xbox.com에 한 번 로그인하여 게이머태그를 생성해 주세요."
            }
            2148916235 => "현재 국가/지역에서 Xbox Live 서비스를 이용할 수 없습니다.",
            2148916236 | 2148916237 => "Microsoft 계정 성인 인증이 필요합니다 (xbox.com).",
            2148916238 => {
                "미성년자(가족 보호) 계정입니다. Microsoft 가족 그룹에서 멀티플레이어 설정을 허용해 주세요."
            }
            _ => "XSTS 보안 토큰 발급에 실패했습니다.",
        };
        bail!("{msg} (HTTP {xsts_status}, XErr {xerr})");
    }
    let xsts_token = xsts_json["Token"]
        .as_str()
        .context("XSTS 응답에 Token이 없습니다")?;
    let uhs = xsts_json["DisplayClaims"]["xui"][0]["uhs"]
        .as_str()
        .unwrap_or(uhs);

    // 3. Minecraft Services Login (login_with_xbox with /launcher/login fallback)
    let identity = format!("XBL3.0 x={uhs};{xsts_token}");
    let (mut mc_status, mut mc_json) = post_json(
        agent,
        MC_XBOX_LOGIN_URL,
        &serde_json::json!({
            "identityToken": identity
        }),
    )?;
    if mc_status != 200 || mc_json["access_token"].as_str().is_none() {
        let (alt_status, alt_json) = post_json(
            agent,
            MC_LAUNCHER_LOGIN_URL,
            &serde_json::json!({
                "xtoken": identity,
                "platform": "PC_LAUNCHER"
            }),
        )?;
        if alt_status == 200 && alt_json["access_token"].as_str().is_some() {
            mc_status = alt_status;
            mc_json = alt_json;
        }
    }
    ensure!(
        mc_status == 200,
        "Minecraft Services 로그인 거부됨 (HTTP {mc_status})"
    );
    let mc_access_token = mc_json["access_token"]
        .as_str()
        .context("Minecraft Services 응답에 access_token이 없습니다")?
        .to_owned();
    let expires_in = mc_json["expires_in"].as_u64().unwrap_or(86400);

    // 4. Fetch Java Edition Profile (/minecraft/profile)
    let prof_resp = agent
        .get(MC_PROFILE_URL)
        .header("Authorization", &format!("Bearer {mc_access_token}"))
        .header("Accept", "application/json")
        .call()
        .map_err(|e| anyhow!("마인크래프트 프로필 조회 실패: {e}"))?;
    let prof_status = prof_resp.status().as_u16();
    let mut prof_bytes = Vec::new();
    prof_resp
        .into_body()
        .into_reader()
        .take(262_144)
        .read_to_end(&mut prof_bytes)?;
    if prof_status == 404 {
        bail!("이 Microsoft 계정에는 마인크래프트 Java Edition 라이선스(프로필)가 없습니다.");
    }
    ensure!(
        prof_status == 200,
        "마인크래프트 프로필 조회 오류 (HTTP {prof_status})"
    );
    let prof_json: serde_json::Value =
        serde_json::from_slice(&prof_bytes).context("마인크래프트 프로필 JSON 파싱 오류")?;
    let raw_id = prof_json["id"]
        .as_str()
        .context("프로필에 UUID(id)가 없습니다")?;
    let username = prof_json["name"]
        .as_str()
        .context("프로필에 닉네임(name)이 없습니다")?
        .to_owned();

    Ok(AuthenticatedAccount {
        uuid: normalize_uuid(raw_id)?,
        username,
        mc_access_token,
        ms_refresh_token,
        expires_at_epoch_secs: now_epoch_secs().saturating_add(expires_in),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notify_approval_only_after_receiving_access_and_before_exchange() {
        let approved = std::cell::Cell::new(false);
        for (status, response) in [
            (400, serde_json::json!({"error":"authorization_pending"})),
            (400, serde_json::json!({"error":"slow_down"})),
            (400, serde_json::json!({"error":"access_denied"})),
            (200, serde_json::json!({"access_token":""})),
        ] {
            let _ = finish_device_poll(status, &response, || approved.set(true), |_, _| {
                panic!("Incomplete authorization started an account exchange")
            });
            assert!(!approved.get());
        }
        let response = serde_json::json!({"access_token":"fixture", "refresh_token":"refresh"});
        let result = finish_device_poll(200, &response, || approved.set(true), |access, refresh| {
            assert!(approved.get());
            assert_eq!(access, "fixture");
            assert_eq!(refresh, "refresh");
            bail!("Xbox service unavailable")
        });
        assert_eq!(result.unwrap_err().to_string(), "Xbox service unavailable");
    }

    #[test]
    fn provider_error_description_does_not_echo_credentials() {
        let response = serde_json::json!({
            "error":"invalid_grant",
            "error_description":"A request containing private-fixture-value failed."
        });
        let result = finish_device_poll(400, &response, || panic!(), |_, _| panic!());
        let message = result.unwrap_err().to_string();
        assert!(message.contains("invalid_grant") && !message.contains("private-fixture-value"));
    }

    #[cfg(windows)]
    #[test]
    fn account_save_can_retry_after_destination_is_locked() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = std::env::temp_dir().join(format!("ferrum-store-{:032x}", rand::random::<u128>()));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("accounts.json");
        let mut store = AccountStore::default();
        store.save_to(&path).unwrap();
        let lock = fs::OpenOptions::new().read(true).share_mode(1).open(&path).unwrap();
        store.offline_username = "NextPlayer".into();
        assert!(store.save_to(&path).is_err());
        assert_eq!(AccountStore::load_from(&path).unwrap().offline_username, "FerrumPlayer");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        drop(lock);
        store.save_to(&path).unwrap();
        assert_eq!(AccountStore::load_from(&path).unwrap().offline_username, "NextPlayer");
        fs::remove_file(path).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn dpapi_and_account_store_round_trip() {
        let path = std::env::temp_dir().join(format!(
            "ferrum189-ms-auth-test-{}.json",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        let auth = AuthenticatedAccount {
            uuid: "01234567-89ab-cdef-0123-456789abcdef".into(),
            username: "FerrumTester".into(),
            mc_access_token: "mc-secret-token-123".into(),
            ms_refresh_token: "ms-refresh-token-456".into(),
            expires_at_epoch_secs: now_epoch_secs() + 3600,
        };
        let mut store = AccountStore::default();
        store.upsert_authenticated(&auth).unwrap();
        store.save_to(&path).unwrap();

        let raw_disk = fs::read_to_string(&path).unwrap();
        assert!(!raw_disk.contains("mc-secret-token-123"));
        assert!(!raw_disk.contains("ms-refresh-token-456"));

        let loaded = AccountStore::load_from(&path).unwrap();
        assert_eq!(
            loaded.selected_uuid.as_deref(),
            Some("0123456789abcdef0123456789abcdef")
        );
        let decrypted = loaded.selected().unwrap().to_authenticated().unwrap();
        assert_eq!(decrypted.username, "FerrumTester");
        assert_eq!(decrypted.mc_access_token, "mc-secret-token-123");
        assert_eq!(decrypted.ms_refresh_token, "ms-refresh-token-456");

        let _ = fs::remove_file(&path);
    }
}
