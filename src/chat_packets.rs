use crate::protocol::{Reader, Writer};
use anyhow::{ensure, Result};

pub(super) fn incoming(r: &mut Reader<'_>) -> Result<(String, u8)> {
    let chat = (r.string(32767)?, r.u8()?);
    r.finish()?;
    Ok(chat)
}

pub(super) fn send(w: &mut Writer, message: &str) -> Result<()> {
    ensure!(
        message.encode_utf16().count() <= 100
            && !message
                .chars()
                .any(|c| c < ' ' || c == '\u{7f}' || c == '§'),
        "Invalid chat message"
    );
    w.string(message);
    Ok(())
}
pub(super) fn request(w: &mut Writer, input: &str, target: Option<[i32; 3]>) -> Result<()> {
    ensure!(
        input.len() <= 32767,
        "Tab completion exceeds packet string limit"
    );
    w.string(input).bool(target.is_some());
    if let Some(position) = target {
        w.position(position);
    }
    Ok(())
}
pub(super) fn matches(r: &mut Reader<'_>) -> Result<Vec<String>> {
    let count = r.varint()?;
    // Even an empty match needs one length byte inside the bounded play frame.
    ensure!(
        count >= 0 && count as usize <= r.remaining().len(),
        "Invalid tab-completion count"
    );
    let result = (0..count).map(|_| r.string(32767)).collect::<Result<_>>()?;
    r.finish()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packet_payloads_match_installed_189_jar() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/vanilla189-chat-packets.json"
        ))
        .unwrap();
        let expected = |key: &str| {
            let hex = oracle[key].as_str().unwrap();
            (0..hex.len())
                .step_by(2)
                .map(|n| u8::from_str_radix(&hex[n..n + 2], 16).unwrap())
                .collect::<Vec<_>>()
        };
        let mut w = Writer::new();
        send(&mut w, "서버 chat").unwrap();
        assert_eq!(w.0, expected("send"));
        for (key, input, target) in [
            ("request", "/time set ", None),
            ("target", "안녕", Some([-12345, 64, 101])),
        ] {
            let mut w = Writer::new();
            request(&mut w, input, target).unwrap();
            assert_eq!(w.0, expected(key));
        }
        assert_eq!(
            matches(&mut Reader::new(&expected("choices"))).unwrap(),
            ["사과", "사탕", "한글"]
        );
        for pos in 0..=2 {
            assert_eq!(
                incoming(&mut Reader::new(&expected(&format!("incoming{pos}")))).unwrap(),
                ("\"안녕\"".to_owned(), pos)
            );
        }
        assert!(matches(&mut Reader::new(&[2, 0])).is_err());
    }
}
