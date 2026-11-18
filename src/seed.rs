//! Seed text follows GuiCreateWorld: signed Java long, otherwise UTF-16 String.hashCode.
pub fn java_hash(text: &str) -> i32 {
    text.encode_utf16().fold(0i32, |hash, unit| {
        hash.wrapping_mul(31).wrapping_add(unit as i32)
    })
}
pub fn parse_cli(text: &str) -> Result<u64, String> {
    Ok(text
        .parse::<i64>()
        .unwrap_or_else(|_| java_hash(text) as i64) as u64)
}
pub fn from_gui(text: &str, random_seed: u64) -> u64 {
    if text.is_empty() {
        return random_seed;
    }
    match text.parse::<i64>() {
        Ok(0) => random_seed,
        Ok(value) => value as u64,
        Err(_) => java_hash(text) as i64 as u64,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signed_and_text_seeds_match_java_semantics() {
        assert_eq!(parse_cli("-1").unwrap(), u64::MAX);
        assert_eq!(parse_cli("-9223372036854775808").unwrap(), i64::MIN as u64);
        assert_eq!(java_hash("hello"), 99162322);
        assert_eq!(java_hash("Aa"), java_hash("BB"));
        assert_eq!(java_hash("😀"), 1772899); // two Java UTF-16 code units, not four UTF-8 bytes.
        assert_eq!(from_gui("0", 123), 123);
        assert_eq!(from_gui("", 456), 456);
        assert_eq!(from_gui(" -1", 0), java_hash(" -1") as i64 as u64);
    }
}
