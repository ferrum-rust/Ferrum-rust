//! The less frequent play messages, decoded before they reach the render thread.
use crate::protocol::{Reader, Slot};
use anyhow::{bail, ensure, Result};
use glam::IVec3;
#[cfg(test)]
#[path = "play_events_oracle_tests.rs"]
mod oracle_tests;

pub(super) fn write_sign(
    writer: &mut crate::protocol::Writer,
    position: [i32; 3],
    lines: [String; 4],
) -> Result<()> {
    writer.position(position);
    for line in lines {
        // IChatComponent.Serializer's Gson uses HTML-safe JSON and escapes the
        // two Javascript line separators even in a plain ChatComponentText.
        let text = serde_json::to_string(&line)?
            .replace('&', "\\u0026")
            .replace('<', "\\u003c")
            .replace('>', "\\u003e")
            .replace('=', "\\u003d")
            .replace('\'', "\\u0027")
            .replace('\u{2028}', "\\u2028")
            .replace('\u{2029}', "\\u2029");
        ensure!(text.encode_utf16().count() <= 384, "Sign line too long");
        writer.string(&text);
    }
    Ok(())
}

#[derive(Debug)]
pub struct MapPatch {
    pub id: i32,
    pub scale: i8,
    pub icons: Vec<[u8; 3]>,
    pub rect: [usize; 4],
    pub colors: Vec<u8>,
}
#[derive(Debug)]
pub struct Offer {
    pub buy: Slot,
    pub sell: Slot,
    pub second: Option<Slot>,
    pub disabled: bool,
    pub uses: i32,
    pub limit: i32,
}
#[derive(Debug)]
pub enum Plugin {
    Brand(String),
    Offers(i32, Vec<Offer>),
    Book,
    Other,
}
#[derive(Debug)]
pub enum Combat {
    Start,
    End(i32, i32),
    Died(i32, i32, String),
}
#[derive(Debug)]
pub enum Update {
    Bed(i32, IVec3),
    Animation(i32, u8),
    Painting(i32, String, IVec3, u8),
    Orb(i32, [f64; 3], i16),
    BlockAction(IVec3, u8, u8, u16),
    Break(i32, IVec3, u8),
    Explosion {
        center: [f32; 3],
        strength: f32,
        blocks: Vec<IVec3>,
        push: [f32; 3],
    },
    Global(i32, i8, [f64; 3]),
    Property(u8, i16, i16),
    Map(MapPatch),
    Sign(IVec3),
    Stats(Vec<(String, i32)>),
    Plugin(Plugin),
    Difficulty(u8),
    Combat(Combat),
    Camera(i32),
    EntityNbt(i32),
}
fn point(r: &mut Reader<'_>) -> Result<[f64; 3]> {
    Ok([
        f64::from(r.i32()?) / 32.,
        f64::from(r.i32()?) / 32.,
        f64::from(r.i32()?) / 32.,
    ])
}
fn floats(r: &mut Reader<'_>) -> Result<[f32; 3]> {
    let v = [r.f32()?, r.f32()?, r.f32()?];
    ensure!(
        v.iter().all(|x| x.is_finite()),
        "Invalid explosion coordinate or impulse"
    );
    Ok(v)
}
fn plugin(r: &mut Reader<'_>) -> Result<Plugin> {
    let channel = r.string(32767)?;
    ensure!(r.remaining().len() <= 1_048_576, "Oversized custom payload");
    let mut data = Reader::new(r.take(r.remaining().len())?);
    Ok(match channel.as_str() {
        "MC|Brand" | "minecraft:brand" => {
            Plugin::Brand(data.string(32767).unwrap_or_default())
        }
        "MC|BOpen" => Plugin::Book,
        "MC|TrList" => {
            let id = data.i32()?;
            let count = data.u8()?;
            let mut offers = Vec::with_capacity(usize::from(count));
            for _ in 0..count {
                let buy = data
                    .slot()?
                    .ok_or_else(|| anyhow::anyhow!("Missing merchant buy stack"))?;
                let sell = data
                    .slot()?
                    .ok_or_else(|| anyhow::anyhow!("Missing merchant sell stack"))?;
                let second = if data.bool()? { data.slot()? } else { None };
                offers.push(Offer {
                    buy,
                    sell,
                    second,
                    disabled: data.bool()?,
                    uses: data.i32()?,
                    limit: data.i32()?,
                });
            }
            Plugin::Offers(id, offers)
        }
        _ => Plugin::Other, // vanilla has no handler for unregistered mod channels
    })
}
pub fn decode(id: i32, r: &mut Reader<'_>) -> Result<Update> {
    use Update::*;
    let result = match id {
        0x0a => Bed(r.varint()?, r.position()?.into()),
        0x0b => Animation(r.varint()?, r.u8()?),
        0x10 => Painting(
            r.varint()?,
            r.string(13)?,
            r.position()?.into(),
            r.u8()? & 3,
        ),
        0x11 => Orb(r.varint()?, point(r)?, r.i16()?),
        0x24 => BlockAction(
            r.position()?.into(),
            r.u8()?,
            r.u8()?,
            (r.varint()? & 4095) as u16,
        ),
        0x25 => Break(r.varint()?, r.position()?.into(), r.u8()?),
        0x27 => {
            let center = floats(r)?;
            let strength = r.f32()?;
            ensure!(strength.is_finite(), "Invalid explosion strength");
            let count = usize::try_from(r.i32()?)?;
            ensure!(
                count <= r.remaining().len().saturating_sub(12) / 3,
                "Invalid explosion block count"
            );
            let base = center.map(|x| x as i32);
            let mut blocks = Vec::with_capacity(count);
            for _ in 0..count {
                blocks.push(IVec3::new(
                    base[0].wrapping_add(r.i8()? as i32),
                    base[1].wrapping_add(r.i8()? as i32),
                    base[2].wrapping_add(r.i8()? as i32),
                ))
            }
            Explosion {
                center,
                strength,
                blocks,
                push: floats(r)?,
            }
        }
        0x2c => Global(r.varint()?, r.i8()?, point(r)?),
        0x31 => Property(r.u8()?, r.i16()?, r.i16()?),
        0x34 => {
            let id = r.varint()?;
            let scale = r.i8()?;
            let n = r.count(r.remaining().len() / 3)?;
            let mut icons = Vec::with_capacity(n);
            for _ in 0..n {
                icons.push([r.u8()?, r.u8()?, r.u8()?]);
            }
            let w = usize::from(r.u8()?);
            let (mut rect, mut colors) = ([0; 4], Vec::new());
            if w > 0 {
                let h = usize::from(r.u8()?);
                let x = usize::from(r.u8()?);
                let y = usize::from(r.u8()?);
                ensure!(x + w <= 128 && y + h <= 128, "Map rectangle outside map");
                let n = r.count(128 * 128)?;
                ensure!(n == w * h, "Invalid map pixel count");
                rect = [x, y, w, h];
                colors.extend_from_slice(r.take(n)?);
            }
            Map(MapPatch {
                id,
                scale,
                icons,
                rect,
                colors,
            })
        }
        0x36 => Sign(r.position()?.into()),
        0x37 => {
            let n = r.count(r.remaining().len() / 2)?;
            let mut stats = Vec::new();
            for _ in 0..n {
                stats.push((r.string(32767)?, r.varint()?));
            }
            Stats(stats)
        }
        0x3f => Update::Plugin(plugin(r)?),
        0x41 => Difficulty(r.u8()? % 4),
        0x42 => Update::Combat(match r.varint()? {
            0 => self::Combat::Start,
            1 => self::Combat::End(r.varint()?, r.i32()?),
            2 => self::Combat::Died(r.varint()?, r.i32()?, r.string(32767)?),
            v => bail!("Unknown combat event {v}"),
        }),
        0x43 => Camera(r.varint()?),
        0x49 => {
            let id = r.varint()?;
            crate::vanilla::item_nbt::decode(r.take(r.remaining().len())?)?;
            EntityNbt(id)
        }
        _ => bail!("Unknown world message {id:#x}"),
    };
    r.finish()?;
    Ok(result)
}
