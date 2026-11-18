//! Typed item-NBT storage and wire codec. JSON saves use the binary tag encoding
//! so numeric widths, empty-list element types and nonfinite float bits survive.
use super::drops::DropNbt;
use anyhow::{bail, ensure, Context, Result};
use std::{collections::BTreeMap, sync::Arc};

pub fn tag_id(tag: &DropNbt) -> u8 {
    match tag {
        DropNbt::Byte(_) => 1,
        DropNbt::Short(_) => 2,
        DropNbt::Int(_) => 3,
        DropNbt::Long(_) => 4,
        DropNbt::Float(_) => 5,
        DropNbt::Double(_) => 6,
        DropNbt::Bytes(_) => 7,
        DropNbt::String(_) => 8,
        DropNbt::List { .. } => 9,
        DropNbt::Compound(_) => 10,
        DropNbt::Ints(_) => 11,
        DropNbt::Longs(_) => 12,
    }
}
pub fn from_tag(tag: &crate::nbt::Tag) -> DropNbt {
    use crate::nbt::Tag;
    match tag {
        Tag::Byte(n) => DropNbt::Byte(*n),
        Tag::Short(n) => DropNbt::Short(*n),
        Tag::Int(n) => DropNbt::Int(*n),
        Tag::Long(n) => DropNbt::Long(*n),
        Tag::Float(n) => DropNbt::Float(*n),
        Tag::Double(n) => DropNbt::Double(*n),
        Tag::Bytes(v) => DropNbt::Bytes(v.iter().map(|v| *v as i8).collect()),
        Tag::String(v) => DropNbt::String(v.clone()),
        Tag::List(v) => {
            let values = v.iter().map(from_tag).collect::<Vec<_>>();
            DropNbt::List {
                element: values.first().map_or(0, tag_id),
                values,
            }
        }
        Tag::EmptyList(element) => DropNbt::List {
            element: *element,
            values: vec![],
        },
        Tag::Compound(v) => {
            DropNbt::Compound(v.iter().map(|(k, v)| (k.clone(), from_tag(v))).collect())
        }
        Tag::Ints(v) => DropNbt::Ints(v.clone()),
        Tag::Longs(v) => DropNbt::Longs(v.clone()),
    }
}
pub fn number(tag: Option<&DropNbt>) -> i32 {
    match tag {
        Some(DropNbt::Byte(n)) => i32::from(*n),
        Some(DropNbt::Short(n)) => i32::from(*n),
        Some(DropNbt::Int(n)) => *n,
        Some(DropNbt::Long(n)) => *n as i32,
        Some(DropNbt::Float(n)) => {
            let x = *n as i32;
            if *n < x as f32 {
                x.wrapping_sub(1)
            } else {
                x
            }
        }
        Some(DropNbt::Double(n)) => {
            let x = *n as i32;
            if *n < x as f64 {
                x.wrapping_sub(1)
            } else {
                x
            }
        }
        _ => 0,
    }
}
pub fn compound(tag: &DropNbt) -> Option<&BTreeMap<String, DropNbt>> {
    if let DropNbt::Compound(map) = tag {
        Some(map)
    } else {
        None
    }
}
pub fn enchantments(tag: Option<&DropNbt>) -> Vec<[i16; 2]> {
    let Some(DropNbt::List {
        element: 10,
        values,
    }) = tag.and_then(compound).and_then(|map| map.get("ench"))
    else {
        return vec![];
    };
    values
        .iter()
        .map(|entry| {
            compound(entry).map_or([0, 0], |e| {
                [number(e.get("id")) as i16, number(e.get("lvl")) as i16]
            })
        })
        .collect()
}
pub fn enchantment_level(tag: Option<&DropNbt>, id: i16) -> i32 {
    let Some(DropNbt::List {
        element: 10,
        values,
    }) = tag.and_then(compound).and_then(|map| map.get("ench"))
    else {
        return 0;
    };
    values
        .iter()
        .filter_map(compound)
        .find(|entry| number(entry.get("id")) as i16 == id)
        .map_or(0, |entry| i32::from(number(entry.get("lvl")) as i16))
}
pub fn unbreakable(tag: Option<&DropNbt>) -> bool {
    number(
        tag.and_then(compound)
            .and_then(|map| map.get("Unbreakable")),
    ) as i8
        != 0
}

fn utf(text: &str, out: &mut Vec<u8>) -> Result<()> {
    let mut encoded = Vec::new();
    for c in text.encode_utf16() {
        match c {
            1..=127 => encoded.push(c as u8),
            0..=2047 => {
                encoded.push((0xc0 | (c >> 6)) as u8);
                encoded.push((0x80 | (c & 63)) as u8);
            }
            _ => {
                encoded.push((0xe0 | (c >> 12)) as u8);
                encoded.push((0x80 | ((c >> 6) & 63)) as u8);
                encoded.push((0x80 | (c & 63)) as u8);
            }
        }
    }
    ensure!(encoded.len() <= 65535, "NBT modified-UTF string too long");
    out.extend_from_slice(&(encoded.len() as u16).to_be_bytes());
    out.extend(encoded);
    Ok(())
}
fn write(tag: &DropNbt, out: &mut Vec<u8>, depth: usize) -> Result<()> {
    ensure!(depth <= 64, "NBT nesting too deep");
    match tag {
        DropNbt::Byte(v) => out.push(*v as u8),
        DropNbt::Short(v) => out.extend(v.to_be_bytes()),
        DropNbt::Int(v) => out.extend(v.to_be_bytes()),
        DropNbt::Long(v) => out.extend(v.to_be_bytes()),
        DropNbt::Float(v) => out.extend(v.to_bits().to_be_bytes()),
        DropNbt::Double(v) => out.extend(v.to_bits().to_be_bytes()),
        DropNbt::Bytes(v) => {
            out.extend((v.len() as i32).to_be_bytes());
            out.extend(v.iter().map(|v| *v as u8));
        }
        DropNbt::String(v) => utf(v, out)?,
        DropNbt::List { element, values } => {
            ensure!(
                *element <= 12 && (*element != 0 || values.is_empty()),
                "Invalid list element type"
            );
            out.push(*element);
            out.extend((values.len() as i32).to_be_bytes());
            for tag in values {
                ensure!(tag_id(tag) == *element, "Heterogeneous NBT list");
                write(tag, out, depth + 1)?;
            }
        }
        DropNbt::Compound(map) => {
            for (key, tag) in map {
                out.push(tag_id(tag));
                utf(key, out)?;
                write(tag, out, depth + 1)?;
            }
            out.push(0);
        }
        DropNbt::Ints(v) => {
            out.extend((v.len() as i32).to_be_bytes());
            for n in v {
                out.extend(n.to_be_bytes());
            }
        }
        DropNbt::Longs(v) => {
            out.extend((v.len() as i32).to_be_bytes());
            for n in v {
                out.extend(n.to_be_bytes());
            }
        }
    }
    Ok(())
}
pub fn encode(tag: &DropNbt) -> Result<Vec<u8>> {
    ensure!(
        matches!(tag, DropNbt::Compound(_)),
        "Item NBT root must be a compound"
    );
    let mut out = vec![10, 0, 0];
    write(tag, &mut out, 0)?;
    Ok(out)
}
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    nodes: usize,
}
impl Reader<'_> {
    fn bytes(&mut self, n: usize) -> Result<&[u8]> {
        let end = self.at.checked_add(n).context("NBT length overflow")?;
        let data = self.bytes.get(self.at..end).context("Truncated item NBT")?;
        self.at = end;
        Ok(data)
    }
    fn raw<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.bytes(N)?.try_into().unwrap())
    }
    fn byte(&mut self) -> Result<u8> {
        Ok(self.raw::<1>()?[0])
    }
    fn length(&mut self) -> Result<usize> {
        let len = i32::from_be_bytes(self.raw()?);
        ensure!(
            (0..=1_048_576).contains(&len),
            "Invalid NBT collection length"
        );
        Ok(len as usize)
    }
    fn utf(&mut self) -> Result<String> {
        let len = u16::from_be_bytes(self.raw()?) as usize;
        let bytes = self.bytes(len)?;
        let mut chars = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            i += 1;
            let c = match b >> 4 {
                0..=7 => u16::from(b),
                12 | 13 => {
                    let x = *bytes.get(i).context("Truncated modified UTF")?;
                    i += 1;
                    ensure!(x & 0xc0 == 0x80, "Malformed modified UTF");
                    u16::from(b & 31) << 6 | u16::from(x & 63)
                }
                14 => {
                    let x = *bytes.get(i).context("Truncated modified UTF")?;
                    let y = *bytes.get(i + 1).context("Truncated modified UTF")?;
                    i += 2;
                    ensure!(
                        x & 0xc0 == 0x80 && y & 0xc0 == 0x80,
                        "Malformed modified UTF"
                    );
                    u16::from(b & 15) << 12 | u16::from(x & 63) << 6 | u16::from(y & 63)
                }
                _ => bail!("Malformed modified UTF"),
            };
            chars.push(c);
        }
        String::from_utf16(&chars)
            .context("NBT string contains an unsupported unpaired UTF-16 surrogate")
    }
    fn tag(&mut self, id: u8, depth: usize) -> Result<DropNbt> {
        self.nodes += 1;
        ensure!(
            depth <= 64 && self.nodes <= 1_048_576,
            "NBT complexity limit"
        );
        Ok(match id {
            1 => DropNbt::Byte(self.byte()? as i8),
            2 => DropNbt::Short(i16::from_be_bytes(self.raw()?)),
            3 => DropNbt::Int(i32::from_be_bytes(self.raw()?)),
            4 => DropNbt::Long(i64::from_be_bytes(self.raw()?)),
            5 => DropNbt::Float(f32::from_bits(u32::from_be_bytes(self.raw()?))),
            6 => DropNbt::Double(f64::from_bits(u64::from_be_bytes(self.raw()?))),
            7 => {
                let len = self.length()?;
                DropNbt::Bytes(self.bytes(len)?.iter().map(|v| *v as i8).collect())
            }
            8 => DropNbt::String(self.utf()?),
            9 => {
                let element = self.byte()?;
                let len = self.length()?;
                ensure!(
                    element <= 12 && (element != 0 || len == 0),
                    "Invalid list element type"
                );
                let mut values = Vec::with_capacity(len);
                for _ in 0..len {
                    values.push(self.tag(element, depth + 1)?);
                }
                DropNbt::List { element, values }
            }
            10 => {
                let mut map = BTreeMap::new();
                loop {
                    let id = self.byte()?;
                    if id == 0 {
                        break;
                    }
                    let key = self.utf()?;
                    map.insert(key, self.tag(id, depth + 1)?);
                }
                DropNbt::Compound(map)
            }
            11 => {
                let len = self.length()?;
                let mut values = Vec::with_capacity(len);
                for _ in 0..len {
                    values.push(i32::from_be_bytes(self.raw()?));
                }
                DropNbt::Ints(values)
            }
            12 => {
                let len = self.length()?;
                let mut values = Vec::with_capacity(len);
                for _ in 0..len {
                    values.push(i64::from_be_bytes(self.raw()?));
                }
                DropNbt::Longs(values)
            }
            _ => bail!("Unsupported original NBT tag {id}"),
        })
    }
}
pub fn decode(bytes: &[u8]) -> Result<Option<DropNbt>> {
    ensure!(bytes.len() <= 16_777_216, "Item NBT too large");
    let mut r = Reader {
        bytes,
        at: 0,
        nodes: 0,
    };
    let id = r.byte()?;
    if id == 0 {
        ensure!(r.at == bytes.len(), "Trailing NBT bytes");
        return Ok(None);
    }
    ensure!(id == 10, "Item NBT root must be compound");
    r.utf()?;
    let tag = r.tag(id, 0)?;
    ensure!(r.at == bytes.len(), "Trailing NBT bytes");
    Ok(Some(tag))
}
pub fn decode_modified_utf8(bytes: &[u8]) -> Result<String> {
    ensure!(bytes.len() <= 65535, "Modified UTF string too long");
    let mut framed = (bytes.len() as u16).to_be_bytes().to_vec();
    framed.extend_from_slice(bytes);
    Reader {
        bytes: &framed,
        at: 0,
        nodes: 0,
    }
    .utf()
}
fn tag_hex(tag: &DropNbt) -> Result<String> {
    let bytes = encode(tag)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
fn read_tag_hex(hex: &str) -> Result<Option<DropNbt>> {
    ensure!(
        hex.len().is_multiple_of(2) && hex.is_ascii(),
        "Invalid item NBT hex encoding"
    );
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    decode(&bytes)
}
pub mod optional_arc {
    use super::*;
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(
        tag: &Option<Arc<DropNbt>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match tag {
            None => serializer.serialize_none(),
            Some(tag) => {
                let hex = tag_hex(tag).map_err(serde::ser::Error::custom)?;
                serializer.serialize_some(&hex)
            }
        }
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Arc<DropNbt>>, D::Error> {
        let hex = Option::<String>::deserialize(deserializer)?;
        let Some(hex) = hex else {
            return Ok(None);
        };
        read_tag_hex(&hex)
            .map(|v| v.map(Arc::new))
            .map_err(serde::de::Error::custom)
    }
}

/// Retained block/entity drop records previously used the JSON representation
/// of the typed tag. Read those records and write binary NBT, including signed
/// zero and floating-point NaN payloads, for all subsequent checkpoints.
pub mod optional_owned {
    use super::*;
    use serde::{Deserialize, Deserializer, Serializer};
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Stored {
        Binary(String),
        Legacy(DropNbt),
    }
    pub fn serialize<S: Serializer>(
        tag: &Option<DropNbt>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match tag {
            None => serializer.serialize_none(),
            Some(tag) => {
                let hex = tag_hex(tag).map_err(serde::ser::Error::custom)?;
                serializer.serialize_some(&hex)
            }
        }
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<DropNbt>, D::Error> {
        match Option::<Stored>::deserialize(d)? {
            None => Ok(None),
            Some(Stored::Legacy(tag)) => Ok(Some(tag)),
            Some(Stored::Binary(hex)) => read_tag_hex(&hex).map_err(serde::de::Error::custom),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{Inventory, Stack};
    #[test]
    fn nbt_stack_equality_aliases_and_logical_copies_match_original() {
        let rows: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-item-nbt-equality.json"
        ))
        .unwrap();
        let make = |v: &serde_json::Value| {
            let mut stack = Stack::new(310, v[0].as_u64().unwrap() as u8);
            stack.damage = v[1].as_u64().unwrap() as u16;
            if let Some(hex) = v[2].as_str() {
                let bytes = (0..hex.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                    .collect::<Vec<_>>();
                stack.nbt = decode(&bytes).unwrap().map(Arc::new);
            }
            stack
        };
        for (case, row) in rows.as_array().unwrap().iter().enumerate() {
            let a = make(&row["a"]);
            let b = if row["alias"] == true {
                a.clone()
            } else {
                make(&row["b"])
            };
            assert_eq!(
                a == b,
                row["equal"].as_bool().unwrap(),
                "case{case} stack equality"
            );
            if a.damage == b.damage {
                assert_eq!(
                    a.same(&b),
                    row["tags_equal"].as_bool().unwrap(),
                    "case{case} tag equality"
                );
            }
        }
    }
    #[test]
    fn original_nbt_types_unknown_fields_unicode_and_save_roundtrip() {
        let rows: Vec<String> = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-item-nbt.json"
        ))
        .unwrap();
        for (case, hex) in rows.iter().enumerate() {
            let bytes = (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect::<Vec<_>>();
            let typed = decode(&bytes).unwrap().unwrap();
            let normalized = encode(&typed).unwrap();
            let legacy = crate::nbt::read(&bytes).unwrap();
            assert_eq!(
                encode(&from_tag(&legacy)).unwrap(),
                normalized,
                "case{case} legacy parser"
            );
            let stack = Stack::new(310, 1).with_nbt(typed);
            assert_eq!(
                stack.enchantments(),
                vec![[34, case as i16], [0, 4], [99, -1], [0, 2]]
            );
            assert_eq!(stack.unbreakable(), case % 2 == 0);
            let restored: Stack =
                serde_json::from_str(&serde_json::to_string(&stack).unwrap()).unwrap();
            assert_eq!(
                encode(restored.nbt.as_deref().unwrap()).unwrap(),
                normalized,
                "case{case} native save"
            );
            assert_ne!(stack.identity, restored.identity);
            let mut copied = stack.copied();
            if let DropNbt::Compound(map) = Arc::make_mut(copied.nbt.as_mut().unwrap()) {
                map.insert("Name".into(), DropNbt::String("changed".into()));
            }
            assert_eq!(encode(stack.nbt.as_deref().unwrap()).unwrap(), normalized);
            assert!(!stack.same(&copied));
            let mut inv = Inventory::default();
            inv.add(stack.clone());
            inv.add(copied);
            assert!(inv.slots[0].is_some() && inv.slots[1].is_some());
            assert!(
                matches!(compound(stack.nbt.as_deref().unwrap()).unwrap().get("Name"),Some(DropNbt::String(s)) if s=="NUL\0 snowman ☃ emoji 😀")
            );
        }
    }
    #[test]
    fn empty_list_element_type_is_retained_and_bad_save_hex_is_rejected() {
        let tag = DropNbt::Compound(BTreeMap::from([(
            "empty".into(),
            DropNbt::List {
                element: 5,
                values: vec![],
            },
        )]));
        let bytes = encode(&tag).unwrap();
        let legacy = crate::nbt::read(&bytes).unwrap();
        assert_eq!(encode(&from_tag(&legacy)).unwrap(), bytes);
        let bad = r#"{"id":1,"count":1,"nbt":"😀"}"#;
        assert!(serde_json::from_str::<Stack>(bad).is_err());
    }
}
