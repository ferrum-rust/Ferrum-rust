//! Mojangson syntax accepted by the 1.8 command interpreter (before typed arrays).
use super::{error, CmdResult};
use crate::vanilla::drops::DropNbt as Tag;
use serde_json::json;
use std::collections::BTreeMap;

fn split(raw: &str, delim: char) -> Result<Vec<&str>, String> {
    let mut pieces = Vec::new();
    let (mut start, mut quoted, mut escape) = (0, false, false);
    let mut stack = Vec::new();
    for (i, c) in raw.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if quoted && c == '\\' {
            escape = true;
            continue;
        }
        if c == '"' {
            quoted = !quoted;
            continue;
        }
        if quoted {
            continue;
        }
        match c {
            '{' | '[' => stack.push(c),
            '}' | ']' => {
                if stack.pop() != Some(if c == '}' { '{' } else { '[' }) {
                    return Err("Unbalanced brackets".into());
                }
            }
            _ if c == delim && stack.is_empty() => {
                pieces.push(raw[start..i].trim());
                start = i + c.len_utf8();
            }
            _ => (),
        }
    }
    if quoted || !stack.is_empty() {
        return Err("Unbalanced quotation or brackets".into());
    }
    pieces.push(raw[start..].trim());
    Ok(pieces)
}
fn text(s: &str) -> String {
    s.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(s)
        .replace("\\\"", "\"")
        .replace("\\\\", "\\")
}
fn kind(t: &Tag) -> u8 {
    match t {
        Tag::Byte(_) => 1,
        Tag::Short(_) => 2,
        Tag::Int(_) => 3,
        Tag::Long(_) => 4,
        Tag::Float(_) => 5,
        Tag::Double(_) => 6,
        Tag::Bytes(_) => 7,
        Tag::String(_) => 8,
        Tag::List { .. } => 9,
        Tag::Compound(_) => 10,
        Tag::Ints(_) => 11,
        Tag::Longs(_) => 12,
    }
}
fn read(raw: &str, depth: u8) -> Result<Tag, String> {
    if depth > 64 {
        return Err("Tag nesting limit exceeded".into());
    }
    let s = raw.trim();
    if let Some(body) = s.strip_prefix('{') {
        let body = body.strip_suffix('}').ok_or("Missing closing brace")?;
        let mut map = BTreeMap::new();
        for entry in split(body, ',')?.into_iter().filter(|e| !e.is_empty()) {
            let parts = split(entry, ':')?;
            if parts.len() < 2 {
                return Err(format!(
                    "Unable to locate name/value separator for string: {entry}"
                ));
            }
            let offset = entry.find(':').unwrap_or(0);
            // Quoted keys can contain their own colon.
            let val = entry[parts[0].len().max(offset)..]
                .trim_start()
                .strip_prefix(':')
                .ok_or("Missing colon")?;
            map.insert(text(parts[0]), read(val, depth + 1)?);
        }
        return Ok(Tag::Compound(map));
    }
    if let Some(body) = s.strip_prefix('[') {
        let body = body.strip_suffix(']').ok_or("Missing closing bracket")?;
        let pieces = split(body, ',')?;
        if !body.trim().is_empty()
            && body.chars().all(|c| {
                c.is_ascii_digit() || c.is_ascii_whitespace() || matches!(c, ',' | '-' | '+')
            })
        {
            if let Ok(nums) = pieces
                .iter()
                .filter(|s| !s.is_empty())
                .map(|s| s.parse())
                .collect()
            {
                return Ok(Tag::Ints(nums));
            }
        }
        let mut values = Vec::new();
        let mut element = 0;
        for part in pieces.into_iter().filter(|p| !p.is_empty()) {
            let value = part
                .split_once(':')
                .filter(|(n, _)| n.parse::<usize>().is_ok())
                .map_or(part, |(_, v)| v);
            let t = read(value, depth + 1)?;
            if element == 0 {
                element = kind(&t)
            }
            if element == kind(&t) {
                values.push(t)
            }
        }
        return Ok(Tag::List { element, values });
    }
    if s.starts_with('"') {
        return Ok(Tag::String(text(s)));
    }
    let value = s.get(..s.len().saturating_sub(1));
    let decimal = |s: &str| {
        let n = s.strip_prefix(['+', '-']).unwrap_or(s);
        !n.is_empty()
            && n.as_bytes().last().is_some_and(u8::is_ascii_digit)
            && n.bytes().all(|b| b.is_ascii_digit() || b == b'.')
            && n.matches('.').count() <= 1
    };
    let typed = value.filter(|v| decimal(v)).and_then(|n| {
        match s.as_bytes().last()?.to_ascii_lowercase() {
            b'b' => n.parse().ok().map(Tag::Byte),
            b's' => n.parse().ok().map(Tag::Short),
            b'l' => n.parse().ok().map(Tag::Long),
            b'f' => n.parse().ok().map(Tag::Float),
            b'd' => n.parse().ok().map(Tag::Double),
            _ => None,
        }
    });
    Ok(typed
        .or_else(|| s.parse().ok().map(Tag::Int))
        .or_else(|| {
            (s.contains('.') && decimal(s))
                .then(|| s.parse().ok().map(Tag::Double))
                .flatten()
        })
        .unwrap_or_else(|| match s.to_ascii_lowercase().as_str() {
            "true" => Tag::Byte(1),
            "false" => Tag::Byte(0),
            _ => Tag::String(text(s)),
        }))
}
pub(super) fn parse(raw: &str) -> CmdResult<Tag> {
    read(raw, 0)
        .and_then(|v| {
            if matches!(v, Tag::Compound(_)) {
                Ok(v)
            } else {
                Err("Expected a compound tag".into())
            }
        })
        .map_err(|e| error("commands.generic.summon.tagError", vec![json!(e)]))
}
pub(super) fn matches(pattern: &Tag, value: &Tag) -> bool {
    match (pattern, value) {
        (Tag::Compound(a), Tag::Compound(b)) => a
            .iter()
            .all(|(k, v)| b.get(k).is_some_and(|other| matches(v, other))),
        (Tag::List { values: a, .. }, Tag::List { values: b, .. }) => {
            if a.is_empty() {
                b.is_empty()
            } else {
                a.iter().all(|v| b.iter().any(|other| matches(v, other)))
            }
        }
        _ => pattern == value,
    }
}
pub(super) fn merge(target: &mut Tag, patch: Tag) {
    if let Tag::Compound(to) = target {
        if let Tag::Compound(from) = patch {
            for (k, v) in from {
                if let Some(old) = to.get_mut(&k) {
                    if matches!((&*old, &v), (Tag::Compound(_), Tag::Compound(_))) {
                        merge(old, v);
                        continue;
                    }
                }
                to.insert(k, v);
            }
        }
    }
}
