//! Bounded big-endian NBT reader for legacy Anvil saves.
use anyhow::{bail, ensure, Context, Result};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
};

#[allow(dead_code)]
#[derive(Debug)]
pub enum Tag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    Bytes(Vec<u8>),
    String(String),
    List(Vec<Tag>),
    EmptyList(u8),
    Compound(BTreeMap<String, Tag>),
    Ints(Vec<i32>),
    Longs(Vec<i64>),
}
impl Tag {
    pub fn number(&self) -> Option<f64> {
        match self {
            Self::Long(v) => Some(*v as f64),
            Self::Float(v) => Some(*v as f64),
            Self::Double(v) => Some(*v),
            _ => self.int().map(|v| v as f64),
        }
    }
    pub fn string(&self) -> Option<&str> {
        if let Self::String(v) = self {
            Some(v)
        } else {
            None
        }
    }
    pub fn get(&self, key: &str) -> Option<&Tag> {
        if let Self::Compound(map) = self {
            map.get(key)
        } else {
            None
        }
    }
    pub fn int(&self) -> Option<i32> {
        match self {
            Self::Byte(v) => Some(*v as i32),
            Self::Short(v) => Some(*v as i32),
            Self::Int(v) => Some(*v),
            _ => None,
        }
    }
    pub fn bytes(&self) -> Option<&[u8]> {
        if let Self::Bytes(v) = self {
            Some(v)
        } else {
            None
        }
    }
    pub fn list(&self) -> Option<&[Tag]> {
        match self {
            Self::List(v) => Some(v),
            Self::EmptyList(_) => Some(&[]),
            _ => None,
        }
    }
}
struct Reader<'a> {
    data: Cursor<&'a [u8]>,
    nodes: usize,
}
impl Reader<'_> {
    fn raw<const N: usize>(&mut self) -> Result<[u8; N]> {
        let mut a = [0; N];
        self.data.read_exact(&mut a).context("Truncated NBT")?;
        Ok(a)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.raw::<1>()?[0])
    }
    fn string(&mut self) -> Result<String> {
        let n = u16::from_be_bytes(self.raw()?) as usize;
        let mut bytes = vec![0; n];
        self.data.read_exact(&mut bytes)?;
        crate::vanilla::item_nbt::decode_modified_utf8(&bytes)
    }
    fn count(&mut self, stride: usize) -> Result<usize> {
        let count = i32::from_be_bytes(self.raw()?);
        ensure!(count >= 0, "Negative NBT array length");
        let count = count as usize;
        let remaining = self.data.get_ref().len() - self.data.position() as usize;
        ensure!(
            count <= 2_000_000 && count.saturating_mul(stride) <= remaining,
            "NBT collection exceeds input/size limit"
        );
        Ok(count)
    }
    fn tag(&mut self, kind: u8, depth: usize) -> Result<Tag> {
        self.nodes += 1;
        ensure!(
            depth <= 64 && self.nodes <= 2_000_000,
            "NBT nesting/node limit exceeded"
        );
        Ok(match kind {
            1 => Tag::Byte(self.u8()? as i8),
            2 => Tag::Short(i16::from_be_bytes(self.raw()?)),
            3 => Tag::Int(i32::from_be_bytes(self.raw()?)),
            4 => Tag::Long(i64::from_be_bytes(self.raw()?)),
            5 => Tag::Float(f32::from_be_bytes(self.raw()?)),
            6 => Tag::Double(f64::from_be_bytes(self.raw()?)),
            7 => {
                let n = self.count(1)?;
                let mut v = vec![0; n];
                self.data.read_exact(&mut v)?;
                Tag::Bytes(v)
            }
            8 => Tag::String(self.string()?),
            9 => {
                let kind = self.u8()?;
                let n = self.count(1)?;
                let mut v = Vec::with_capacity(n.min(4096));
                for _ in 0..n {
                    v.push(self.tag(kind, depth + 1)?);
                }
                if v.is_empty() {
                    Tag::EmptyList(kind)
                } else {
                    Tag::List(v)
                }
            }
            10 => {
                let mut map = BTreeMap::new();
                loop {
                    let kind = self.u8()?;
                    if kind == 0 {
                        break;
                    }
                    let key = self.string()?;
                    let value = self.tag(kind, depth + 1)?;
                    map.insert(key, value);
                }
                Tag::Compound(map)
            }
            11 => {
                let n = self.count(4)?;
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    v.push(i32::from_be_bytes(self.raw()?));
                }
                Tag::Ints(v)
            }
            12 => {
                let n = self.count(8)?;
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    v.push(i64::from_be_bytes(self.raw()?));
                }
                Tag::Longs(v)
            }
            _ => bail!("Unsupported NBT tag {kind}"),
        })
    }
}
pub fn read(bytes: &[u8]) -> Result<Tag> {
    ensure!(bytes.len() <= 32 * 1024 * 1024, "NBT document too large");
    let mut reader = Reader {
        data: Cursor::new(bytes),
        nodes: 0,
    };
    let kind = reader.u8()?;
    ensure!(kind == 10, "NBT root must be a compound");
    reader.string()?;
    reader.tag(kind, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_negative_int() {
        let bytes = [10, 0, 0, 3, 0, 1, b'x', 255, 255, 255, 254, 0];
        assert_eq!(read(&bytes).unwrap().get("x").unwrap().int(), Some(-2));
    }
    #[test]
    fn rejects_truncated_and_hostile_arrays() {
        assert!(read(&[10, 0, 0, 7, 0, 0, 127, 255, 255, 255]).is_err());
        assert!(read(&[10, 0, 0, 7, 0, 0, 255, 255, 255, 255]).is_err());
        assert!(read(&[10, 0]).is_err());
    }
}
