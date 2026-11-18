//! Minecraft 1.8.9 / protocol 47 wire primitives. Original packet registrations
//! and field order were checked against the locally installed 1.8.9 client.
#![allow(dead_code)]
use anyhow::{bail, ensure, Context, Result};
use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use std::io::{Read, Write};

pub const PROTOCOL_VERSION: i32 = 47;
// Vanilla's outer frame is a VarInt21 and its inflater caps output at 2 MiB.
pub const MAX_PACKET_BYTES: usize = 2_097_151;
pub const MAX_INFLATED_BYTES: usize = 2_097_152;

pub fn put_varint(out: &mut Vec<u8>, value: i32) {
    let mut value = value as u32;
    loop {
        if value & !0x7f == 0 {
            out.push(value as u8);
            return;
        }
        out.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Slot {
    pub id: i16,
    pub count: u8,
    pub damage: i16,
    /// Named root compound including its type/name; [0] denotes no NBT.
    pub nbt: Vec<u8>,
}

pub struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
    pub fn remaining(&self) -> &'a [u8] {
        &self.bytes[self.offset..]
    }
    pub fn finish(&self) -> Result<()> {
        ensure!(self.remaining().is_empty(), "Trailing packet data");
        Ok(())
    }
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        ensure!(n <= self.remaining().len(), "Truncated packet");
        let start = self.offset;
        self.offset += n;
        Ok(&self.bytes[start..self.offset])
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn i8(&mut self) -> Result<i8> {
        Ok(self.u8()? as i8)
    }
    pub fn bool(&mut self) -> Result<bool> {
        Ok(self.u8()? != 0)
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into()?))
    }
    pub fn i16(&mut self) -> Result<i16> {
        Ok(self.u16()? as i16)
    }
    pub fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_be_bytes(self.take(4)?.try_into()?))
    }
    pub fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_be_bytes(self.take(8)?.try_into()?))
    }
    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_bits(self.i32()? as u32))
    }
    pub fn f64(&mut self) -> Result<f64> {
        Ok(f64::from_bits(self.i64()? as u64))
    }
    pub fn varint(&mut self) -> Result<i32> {
        let mut result = 0u32;
        for i in 0..5 {
            let byte = self.u8()?;
            ensure!(i != 4 || byte & 0xf0 == 0, "VarInt exceeds 32 bits");
            result |= ((byte & 0x7f) as u32) << (i * 7);
            if byte & 0x80 == 0 {
                return Ok(result as i32);
            }
        }
        bail!("VarInt exceeds five bytes")
    }
    pub fn count(&mut self, max: usize) -> Result<usize> {
        let n = self.varint()?;
        ensure!(
            n >= 0 && n as usize <= max,
            "Invalid packet collection length {n}"
        );
        Ok(n as usize)
    }
    pub fn string(&mut self, max_utf16: usize) -> Result<String> {
        let len = self.count(max_utf16.saturating_mul(4))?;
        let value = std::str::from_utf8(self.take(len)?).context("Invalid packet UTF-8")?;
        ensure!(
            value.encode_utf16().count() <= max_utf16,
            "Packet string exceeds character limit"
        );
        Ok(value.to_owned())
    }
    pub fn position(&mut self) -> Result<[i32; 3]> {
        let p = self.i64()?;
        Ok([
            (p >> 38) as i32,
            (p << 26 >> 52) as i32,
            (p << 38 >> 38) as i32,
        ])
    }
    pub fn slot(&mut self) -> Result<Option<Slot>> {
        let id = self.i16()?;
        if id < 0 {
            return Ok(None);
        }
        let count = self.u8()?;
        let damage = self.i16()?;
        let start = self.offset;
        let kind = self.u8()?;
        if kind != 0 {
            ensure!(kind == 10, "Slot NBT root must be a compound");
            self.nbt_name()?;
            let mut nodes = 0;
            self.nbt_payload(kind, 0, &mut nodes)?;
        }
        Ok(Some(Slot {
            id,
            count,
            damage,
            nbt: self.bytes[start..self.offset].to_vec(),
        }))
    }
    fn nbt_name(&mut self) -> Result<()> {
        let len = self.u16()? as usize;
        self.take(len)?;
        Ok(())
    }
    fn nbt_payload(&mut self, kind: u8, depth: usize, nodes: &mut usize) -> Result<()> {
        *nodes += 1;
        ensure!(
            depth <= 64 && *nodes <= 100_000,
            "Slot NBT depth/node limit"
        );
        match kind {
            1..=6 => {
                let width = [0, 1, 2, 4, 8, 4, 8][kind as usize];
                self.take(width)?;
            }
            7 | 11 => {
                let n = self.i32()?;
                ensure!(n >= 0, "Negative NBT length");
                self.take(
                    (n as usize)
                        .checked_mul(if kind == 7 { 1 } else { 4 })
                        .context("NBT length overflow")?,
                )?;
            }
            8 => self.nbt_name()?,
            9 => {
                let child = self.u8()?;
                let n = self.i32()?;
                ensure!((0..=100_000).contains(&n), "NBT list limit");
                for _ in 0..n {
                    self.nbt_payload(child, depth + 1, nodes)?;
                }
            }
            10 => loop {
                let child = self.u8()?;
                if child == 0 {
                    break;
                }
                self.nbt_name()?;
                self.nbt_payload(child, depth + 1, nodes)?;
            },
            _ => bail!("Invalid 1.8 NBT tag type {kind}"),
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct Writer(pub Vec<u8>);
impl Writer {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn varint(&mut self, value: i32) -> &mut Self {
        put_varint(&mut self.0, value);
        self
    }
    pub fn u8(&mut self, value: u8) -> &mut Self {
        self.0.push(value);
        self
    }
    pub fn bool(&mut self, value: bool) -> &mut Self {
        self.u8(value as u8)
    }
    pub fn u16(&mut self, value: u16) -> &mut Self {
        self.0.extend(value.to_be_bytes());
        self
    }
    pub fn i16(&mut self, value: i16) -> &mut Self {
        self.u16(value as u16)
    }
    pub fn i32(&mut self, value: i32) -> &mut Self {
        self.0.extend(value.to_be_bytes());
        self
    }
    pub fn i64(&mut self, value: i64) -> &mut Self {
        self.0.extend(value.to_be_bytes());
        self
    }
    pub fn f32(&mut self, value: f32) -> &mut Self {
        self.0.extend(value.to_be_bytes());
        self
    }
    pub fn f64(&mut self, value: f64) -> &mut Self {
        self.0.extend(value.to_be_bytes());
        self
    }
    pub fn bytes(&mut self, bytes: &[u8]) -> &mut Self {
        self.0.extend(bytes);
        self
    }
    pub fn string(&mut self, value: &str) -> &mut Self {
        self.varint(value.len() as i32).bytes(value.as_bytes())
    }
    pub fn position(&mut self, value: [i32; 3]) -> &mut Self {
        let p = ((value[0] as i64 & 0x3ff_ffff) << 38)
            | ((value[1] as i64 & 0xfff) << 26)
            | (value[2] as i64 & 0x3ff_ffff);
        self.i64(p)
    }
    pub fn slot(&mut self, slot: Option<&Slot>) -> &mut Self {
        match slot {
            None => self.i16(-1),
            Some(s) => {
                self.i16(s.id).u8(s.count).i16(s.damage);
                self.bytes(if s.nbt.is_empty() { &[0] } else { &s.nbt })
            }
        }
    }
}

pub fn frame(id: i32, payload: &[u8], compression: Option<usize>) -> Result<Vec<u8>> {
    ensure!(id >= 0, "Negative packet ID");
    let mut body = Vec::new();
    put_varint(&mut body, id);
    body.extend(payload);
    ensure!(body.len() <= MAX_INFLATED_BYTES, "Packet too large");
    let encoded = if let Some(threshold) = compression {
        let mut encoded = Vec::new();
        if body.len() >= threshold {
            put_varint(&mut encoded, body.len() as i32);
            let mut z = ZlibEncoder::new(Vec::new(), Compression::fast());
            z.write_all(&body)?;
            encoded.extend(z.finish()?);
        } else {
            put_varint(&mut encoded, 0);
            encoded.extend(body);
        }
        encoded
    } else {
        body
    };
    ensure!(
        encoded.len() <= MAX_PACKET_BYTES,
        "Packet frame exceeds VarInt21 limit"
    );
    let mut result = Vec::with_capacity(encoded.len() + 3);
    put_varint(&mut result, encoded.len() as i32);
    result.extend(encoded);
    Ok(result)
}

#[derive(Debug)]
pub struct Packet {
    pub id: i32,
    pub data: Vec<u8>,
}
#[derive(Default)]
pub struct FrameDecoder {
    buffer: Vec<u8>,
    pub compression: Option<usize>,
}
impl FrameDecoder {
    pub fn feed(&mut self, bytes: &[u8]) -> Result<()> {
        ensure!(
            self.buffer.len().saturating_add(bytes.len()) <= MAX_PACKET_BYTES + 65_539,
            "Receive frame buffer limit"
        );
        self.buffer.extend(bytes);
        Ok(())
    }
    pub fn next(&mut self) -> Result<Option<Packet>> {
        let mut len = 0usize;
        let mut prefix = 0;
        for (i, &b) in self.buffer.iter().take(3).enumerate() {
            len |= ((b & 0x7f) as usize) << (i * 7);
            if b & 0x80 == 0 {
                prefix = i + 1;
                break;
            }
        }
        if prefix == 0 {
            ensure!(self.buffer.len() < 3, "Frame length exceeds VarInt21");
            return Ok(None);
        }
        ensure!(len != 0 && len <= MAX_PACKET_BYTES, "Invalid frame length");
        if self.buffer.len() < prefix + len {
            return Ok(None);
        }
        let encoded = &self.buffer[prefix..prefix + len];
        let body = if let Some(threshold) = self.compression {
            let mut reader = Reader::new(encoded);
            let inflated = reader.count(MAX_INFLATED_BYTES)?;
            if inflated == 0 {
                reader.remaining().to_vec()
            } else {
                ensure!(
                    inflated >= threshold,
                    "Compressed packet below compression threshold"
                );
                let mut z = ZlibDecoder::new(reader.remaining());
                let mut out = Vec::with_capacity(inflated.min(65536));
                z.by_ref()
                    .take((inflated + 1) as u64)
                    .read_to_end(&mut out)?;
                ensure!(
                    out.len() == inflated,
                    "Compressed packet size/trailing data mismatch"
                );
                out
            }
        } else {
            encoded.to_vec()
        };
        let mut reader = Reader::new(&body);
        let id = reader.varint()?;
        ensure!(id >= 0, "Negative packet ID");
        let packet = Packet {
            id,
            data: reader.remaining().to_vec(),
        };
        self.buffer.drain(..prefix + len);
        Ok(Some(packet))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Handshake,
    Status,
    Login,
    Play,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Serverbound,
    Clientbound,
}
#[derive(Clone, Copy, Debug)]
pub struct PacketInfo {
    pub state: State,
    pub direction: Direction,
    pub id: i32,
    pub name: &'static str,
}
pub fn packet_name(state: State, direction: Direction, id: i32) -> Option<&'static str> {
    PACKET_CATALOG
        .iter()
        .find(|p| p.state == state && p.direction == direction && p.id == id)
        .map(|p| p.name)
}
// Registration order is the wire ID. Kept as a complete catalog, not a claim
// that the network client currently implements every packet's game semantics.
pub static PACKET_CATALOG: &[PacketInfo] = &[
    PacketInfo {
        state: State::Handshake,
        direction: Direction::Serverbound,
        id: 0x00,
        name: "C00Handshake",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x00,
        name: "S00PacketKeepAlive",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x01,
        name: "S01PacketJoinGame",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x02,
        name: "S02PacketChat",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x03,
        name: "S03PacketTimeUpdate",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x04,
        name: "S04PacketEntityEquipment",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x05,
        name: "S05PacketSpawnPosition",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x06,
        name: "S06PacketUpdateHealth",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x07,
        name: "S07PacketRespawn",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x08,
        name: "S08PacketPlayerPosLook",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x09,
        name: "S09PacketHeldItemChange",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x0a,
        name: "S0APacketUseBed",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x0b,
        name: "S0BPacketAnimation",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x0c,
        name: "S0CPacketSpawnPlayer",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x0d,
        name: "S0DPacketCollectItem",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x0e,
        name: "S0EPacketSpawnObject",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x0f,
        name: "S0FPacketSpawnMob",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x10,
        name: "S10PacketSpawnPainting",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x11,
        name: "S11PacketSpawnExperienceOrb",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x12,
        name: "S12PacketEntityVelocity",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x13,
        name: "S13PacketDestroyEntities",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x14,
        name: "S14PacketEntity",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x15,
        name: "S14PacketEntity.S15PacketEntityRelMove",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x16,
        name: "S14PacketEntity.S16PacketEntityLook",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x17,
        name: "S14PacketEntity.S17PacketEntityLookMove",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x18,
        name: "S18PacketEntityTeleport",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x19,
        name: "S19PacketEntityHeadLook",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x1a,
        name: "S19PacketEntityStatus",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x1b,
        name: "S1BPacketEntityAttach",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x1c,
        name: "S1CPacketEntityMetadata",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x1d,
        name: "S1DPacketEntityEffect",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x1e,
        name: "S1EPacketRemoveEntityEffect",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x1f,
        name: "S1FPacketSetExperience",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x20,
        name: "S20PacketEntityProperties",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x21,
        name: "S21PacketChunkData",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x22,
        name: "S22PacketMultiBlockChange",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x23,
        name: "S23PacketBlockChange",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x24,
        name: "S24PacketBlockAction",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x25,
        name: "S25PacketBlockBreakAnim",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x26,
        name: "S26PacketMapChunkBulk",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x27,
        name: "S27PacketExplosion",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x28,
        name: "S28PacketEffect",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x29,
        name: "S29PacketSoundEffect",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x2a,
        name: "S2APacketParticles",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x2b,
        name: "S2BPacketChangeGameState",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x2c,
        name: "S2CPacketSpawnGlobalEntity",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x2d,
        name: "S2DPacketOpenWindow",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x2e,
        name: "S2EPacketCloseWindow",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x2f,
        name: "S2FPacketSetSlot",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x30,
        name: "S30PacketWindowItems",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x31,
        name: "S31PacketWindowProperty",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x32,
        name: "S32PacketConfirmTransaction",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x33,
        name: "S33PacketUpdateSign",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x34,
        name: "S34PacketMaps",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x35,
        name: "S35PacketUpdateTileEntity",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x36,
        name: "S36PacketSignEditorOpen",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x37,
        name: "S37PacketStatistics",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x38,
        name: "S38PacketPlayerListItem",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x39,
        name: "S39PacketPlayerAbilities",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x3a,
        name: "S3APacketTabComplete",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x3b,
        name: "S3BPacketScoreboardObjective",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x3c,
        name: "S3CPacketUpdateScore",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x3d,
        name: "S3DPacketDisplayScoreboard",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x3e,
        name: "S3EPacketTeams",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x3f,
        name: "S3FPacketCustomPayload",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x40,
        name: "S40PacketDisconnect",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x41,
        name: "S41PacketServerDifficulty",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x42,
        name: "S42PacketCombatEvent",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x43,
        name: "S43PacketCamera",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x44,
        name: "S44PacketWorldBorder",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x45,
        name: "S45PacketTitle",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x46,
        name: "S46PacketSetCompressionLevel",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x47,
        name: "S47PacketPlayerListHeaderFooter",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x48,
        name: "S48PacketResourcePackSend",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Clientbound,
        id: 0x49,
        name: "S49PacketUpdateEntityNBT",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Serverbound,
        id: 0x00,
        name: "C00PacketKeepAlive",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Serverbound,
        id: 0x01,
        name: "C01PacketChatMessage",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Serverbound,
        id: 0x02,
        name: "C02PacketUseEntity",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Serverbound,
        id: 0x03,
        name: "C03PacketPlayer",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Serverbound,
        id: 0x04,
        name: "C03PacketPlayer.C04PacketPlayerPosition",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Serverbound,
        id: 0x05,
        name: "C03PacketPlayer.C05PacketPlayerLook",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Serverbound,
        id: 0x06,
        name: "C03PacketPlayer.C06PacketPlayerPosLook",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Serverbound,
        id: 0x07,
        name: "C07PacketPlayerDigging",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Serverbound,
        id: 0x08,
        name: "C08PacketPlayerBlockPlacement",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Serverbound,
        id: 0x09,
        name: "C09PacketHeldItemChange",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Serverbound,
        id: 0x0a,
        name: "C0APacketAnimation",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Serverbound,
        id: 0x0b,
        name: "C0BPacketEntityAction",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Serverbound,
        id: 0x0c,
        name: "C0CPacketInput",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Serverbound,
        id: 0x0d,
        name: "C0DPacketCloseWindow",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Serverbound,
        id: 0x0e,
        name: "C0EPacketClickWindow",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Serverbound,
        id: 0x0f,
        name: "C0FPacketConfirmTransaction",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Serverbound,
        id: 0x10,
        name: "C10PacketCreativeInventoryAction",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Serverbound,
        id: 0x11,
        name: "C11PacketEnchantItem",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Serverbound,
        id: 0x12,
        name: "C12PacketUpdateSign",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Serverbound,
        id: 0x13,
        name: "C13PacketPlayerAbilities",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Serverbound,
        id: 0x14,
        name: "C14PacketTabComplete",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Serverbound,
        id: 0x15,
        name: "C15PacketClientSettings",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Serverbound,
        id: 0x16,
        name: "C16PacketClientStatus",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Serverbound,
        id: 0x17,
        name: "C17PacketCustomPayload",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Serverbound,
        id: 0x18,
        name: "C18PacketSpectate",
    },
    PacketInfo {
        state: State::Play,
        direction: Direction::Serverbound,
        id: 0x19,
        name: "C19PacketResourcePackStatus",
    },
    PacketInfo {
        state: State::Status,
        direction: Direction::Serverbound,
        id: 0x00,
        name: "C00PacketServerQuery",
    },
    PacketInfo {
        state: State::Status,
        direction: Direction::Clientbound,
        id: 0x00,
        name: "S00PacketServerInfo",
    },
    PacketInfo {
        state: State::Status,
        direction: Direction::Serverbound,
        id: 0x01,
        name: "C01PacketPing",
    },
    PacketInfo {
        state: State::Status,
        direction: Direction::Clientbound,
        id: 0x01,
        name: "S01PacketPong",
    },
    PacketInfo {
        state: State::Login,
        direction: Direction::Clientbound,
        id: 0x00,
        name: "S00PacketDisconnect",
    },
    PacketInfo {
        state: State::Login,
        direction: Direction::Clientbound,
        id: 0x01,
        name: "S01PacketEncryptionRequest",
    },
    PacketInfo {
        state: State::Login,
        direction: Direction::Clientbound,
        id: 0x02,
        name: "S02PacketLoginSuccess",
    },
    PacketInfo {
        state: State::Login,
        direction: Direction::Clientbound,
        id: 0x03,
        name: "S03PacketEnableCompression",
    },
    PacketInfo {
        state: State::Login,
        direction: Direction::Serverbound,
        id: 0x00,
        name: "C00PacketLoginStart",
    },
    PacketInfo {
        state: State::Login,
        direction: Direction::Serverbound,
        id: 0x01,
        name: "C01PacketEncryptionResponse",
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signed_varints_and_positions_match_vanilla_layout() {
        for n in [0, 1, 127, 128, 255, i32::MAX, -1, i32::MIN] {
            let mut bytes = Vec::new();
            put_varint(&mut bytes, n);
            assert_eq!(Reader::new(&bytes).varint().unwrap(), n);
            if n < 0 {
                assert_eq!(bytes.len(), 5);
            }
        }
        for p in [[0, 64, 0], [-30_000_000, 255, 30_000_000], [-1, -1, -1]] {
            let mut w = Writer::new();
            w.position(p);
            assert_eq!(Reader::new(&w.0).position().unwrap(), p);
        }
        assert!(Reader::new(&[255, 255, 255, 255, 16]).varint().is_err());
    }
    #[test]
    fn frames_handle_fragmentation_compression_and_coalescence() {
        for threshold in [None, Some(0), Some(16), Some(500)] {
            let bytes = frame(0x26, &[42; 300], threshold).unwrap();
            let mut d = FrameDecoder {
                compression: threshold,
                ..Default::default()
            };
            for &b in &bytes[..bytes.len() - 1] {
                d.feed(&[b]).unwrap();
                assert!(d.next().unwrap().is_none());
            }
            d.feed(&bytes[bytes.len() - 1..]).unwrap();
            d.feed(&bytes).unwrap();
            for _ in 0..2 {
                let p = d.next().unwrap().unwrap();
                assert_eq!(p.id, 0x26);
                assert_eq!(p.data, vec![42; 300]);
            }
            assert!(d.next().unwrap().is_none());
        }
    }
    #[test]
    fn malformed_frames_and_strings_are_bounded() {
        let mut d = FrameDecoder::default();
        d.feed(&[128, 128, 128]).unwrap();
        assert!(d.next().is_err());
        let mut d = FrameDecoder {
            compression: Some(256),
            ..Default::default()
        };
        d.feed(&[2, 1, 0]).unwrap();
        assert!(d.next().is_err());
        let mut w = Writer::new();
        w.string("😀");
        assert!(Reader::new(&w.0).string(1).is_err());
        assert_eq!(Reader::new(&w.0).string(2).unwrap(), "😀");
    }
    #[test]
    fn slot_nbt_preserves_wire_bytes_and_rejects_truncation() {
        let slot = Slot {
            id: 1,
            count: 64,
            damage: 3,
            nbt: vec![10, 0, 0, 8, 0, 1, b'x', 0, 2, b'o', b'k', 0],
        };
        let mut w = Writer::new();
        w.slot(Some(&slot)).slot(None);
        let mut r = Reader::new(&w.0);
        assert_eq!(r.slot().unwrap(), Some(slot));
        assert_eq!(r.slot().unwrap(), None);
        r.finish().unwrap();
        assert!(Reader::new(&[0, 1, 1, 0, 0, 10, 0, 0]).slot().is_err());
    }
}
