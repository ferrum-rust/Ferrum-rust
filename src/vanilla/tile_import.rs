//! TileEntity.createAndLoadEntity followed by writeToNBT during Ctrl+pick.
//! Stored Anvil tags remain intact; this constructs the client-visible copy.
use super::{collision_shapes, drops::DropNbt as Tag, item_nbt, middle_pick, tile_updates};
use anyhow::{anyhow, Result};
use glam::IVec3;
use std::collections::BTreeMap;
type Fields = BTreeMap<String, Tag>;
fn number(m: &Fields, k: &str) -> i32 {
    item_nbt::number(m.get(k))
}
fn text<'a>(m: &'a Fields, k: &str) -> &'a str {
    if let Some(Tag::String(s)) = m.get(k) {
        s
    } else {
        ""
    }
}
fn list<'a>(m: &'a Fields, k: &str) -> &'a [Tag] {
    if let Some(Tag::List {
        element: 10,
        values,
    }) = m.get(k)
    {
        values
    } else {
        &[]
    }
}
fn list_tag(v: Vec<Tag>) -> Tag {
    Tag::List {
        element: if v.is_empty() { 0 } else { 10 },
        values: v,
    }
}
fn stack(m: &Fields) -> Option<Fields> {
    let id = match m.get("id") {
        Some(Tag::String(s)) => middle_pick::named_item(s).or_else(|| s.parse::<u16>().ok())?,
        t => u16::try_from(item_nbt::number(t) as i16).ok()?,
    };
    let name = middle_pick::item_name(id)?;
    let mut out = Fields::from([
        ("id".into(), Tag::String(name.into())),
        ("Count".into(), Tag::Byte(number(m, "Count") as i8)),
        (
            "Damage".into(),
            Tag::Short((number(m, "Damage") as i16).max(0)),
        ),
    ]);
    if let Some(Tag::Compound(tag)) = m.get("tag") {
        out.insert("tag".into(), Tag::Compound(tag.clone()));
    }
    Some(out)
}
fn container(incoming: &Fields, out: &mut Fields, size: usize) {
    out.insert("Lock".into(), Tag::String(text(incoming, "Lock").into()));
    let mut slots = vec![None; size];
    for t in list(incoming, "Items") {
        let Some(m) = item_nbt::compound(t) else {
            continue;
        };
        let slot = number(m, "Slot") as u8;
        if let Some(index) = slots.get_mut(usize::from(slot)) {
            *index = stack(m).map(|mut s| {
                s.insert("Slot".into(), Tag::Byte(slot as i8));
                Tag::Compound(s)
            });
        }
    }
    out.insert(
        "Items".into(),
        list_tag(slots.into_iter().flatten().collect()),
    );
    custom_name(incoming, out);
}
fn custom_name(m: &Fields, out: &mut Fields) {
    if !text(m, "CustomName").is_empty() {
        out.insert(
            "CustomName".into(),
            Tag::String(text(m, "CustomName").into()),
        );
    }
}
pub(super) fn load(id: u16, pos: IVec3, raw: &Tag) -> Result<Tag> {
    let stored = item_nbt::compound(raw).ok_or_else(|| anyhow!("Tile root is not a compound"))?;
    let defaults =
        middle_pick::default_at(id, pos).ok_or_else(|| anyhow!("Unknown tile provider {id}"))?;
    let action = match id {
        52 => 1,
        137 => 2,
        138 => 3,
        144 => 4,
        140 => 5,
        176 | 177 => 6,
        _ => 0,
    };
    if action != 0 {
        return tile_updates::Live::from_written(&defaults)?
            .update(id, action, raw)?
            .ok_or_else(|| anyhow!("Wrong tile type"));
    }
    let mut out = item_nbt::compound(&defaults).unwrap().clone();
    out.retain(|k, _| matches!(k.as_str(), "id" | "x" | "y" | "z"));
    match id {
        23 | 158 => container(stored, &mut out, 9),
        54 | 146 => container(stored, &mut out, 27),
        61 | 62 => {
            container(stored, &mut out, 3);
            for k in ["BurnTime", "CookTime", "CookTimeTotal"] {
                out.insert(k.into(), Tag::Short(number(stored, k) as i16));
            }
        }
        117 => {
            container(stored, &mut out, 4);
            out.insert(
                "BrewTime".into(),
                Tag::Short(number(stored, "BrewTime") as i16),
            );
        }
        154 => {
            container(stored, &mut out, 5);
            out.insert(
                "TransferCooldown".into(),
                Tag::Int(number(stored, "TransferCooldown")),
            );
        }
        25 => {
            out.insert(
                "note".into(),
                Tag::Byte((number(stored, "note") as i8).clamp(0, 24)),
            );
        }
        36 => {
            let state = collision_shapes::carried_state(
                number(stored, "blockId"),
                number(stored, "blockData"),
            )?;
            out.insert("blockId".into(), Tag::Int(i32::from(state >> 4)));
            out.insert("blockData".into(), Tag::Int(i32::from(state & 15)));
            out.insert(
                "facing".into(),
                Tag::Int((number(stored, "facing") % 6).abs()),
            );
            let progress = match stored.get("progress") {
                Some(Tag::Float(x)) => *x,
                Some(Tag::Double(x)) => *x as f32,
                x => item_nbt::number(x) as f32,
            };
            out.insert("progress".into(), Tag::Float(progress));
            out.insert(
                "extending".into(),
                Tag::Byte(i8::from(number(stored, "extending") as i8 != 0)),
            );
        }
        63 | 68 => {
            for n in 1..=4 {
                let key = format!("Text{n}");
                let raw = text(stored, &key);
                out.insert(key, Tag::String(tile_updates::stored_sign_text(raw)));
            }
            if let Some(stats) = tile_updates::command_stats(stored, None) {
                out.insert("CommandStats".into(), stats);
            }
        }
        84 => {
            if let Some(Tag::Compound(record)) = stored.get("RecordItem") {
                if let Some(record) = stack(record) {
                    out.insert("RecordItem".into(), Tag::Compound(record));
                }
            } else if let Ok(id) = u16::try_from(number(stored, "Record")) {
                if let Some(name) = middle_pick::item_name(id) {
                    out.insert(
                        "RecordItem".into(),
                        Tag::Compound(Fields::from([
                            ("id".into(), Tag::String(name.into())),
                            ("Count".into(), Tag::Byte(1)),
                            ("Damage".into(), Tag::Short(0)),
                        ])),
                    );
                }
            }
        }
        116 => custom_name(stored, &mut out),
        149 | 150 => {
            out.insert(
                "OutputSignal".into(),
                Tag::Int(number(stored, "OutputSignal")),
            );
        }
        _ => {}
    }
    Ok(Tag::Compound(out))
}
