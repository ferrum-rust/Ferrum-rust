//! Client-side S33 and S35 tile updates. The renderer may query these same
//! tiles for pick-block. Some tile fields persist even when writeToNBT omits
//! them, e.g. a command block's last output while tracking is disabled.
use super::{drops::DropNbt as Tag, item_nbt, middle_pick};
use crate::protocol::Reader;
use anyhow::{anyhow, Result};
use glam::IVec3;
use std::collections::BTreeMap;

type Fields = BTreeMap<String, Tag>;
fn map(t: &Tag) -> Result<&Fields> {
    item_nbt::compound(t).ok_or_else(|| anyhow!("Tile update needs a compound"))
}
fn number(m: &Fields, k: &str) -> i32 {
    item_nbt::number(m.get(k))
}
fn str_field<'a>(m: &'a Fields, k: &str) -> &'a str {
    if let Some(Tag::String(s)) = m.get(k) {
        s
    } else {
        ""
    }
}
fn compound(m: &Fields, k: &str) -> Fields {
    m.get(k)
        .and_then(item_nbt::compound)
        .cloned()
        .unwrap_or_default()
}
fn numeric(m: &Fields, k: &str) -> bool {
    m.get(k)
        .is_some_and(|v| (1..=6).contains(&item_nbt::tag_id(v)))
}
fn list(m: &Fields, k: &str) -> Vec<Tag> {
    match m.get(k) {
        Some(Tag::List {
            element: 10,
            values,
        }) => values.clone(),
        _ => vec![],
    }
}
fn list_tag(values: Vec<Tag>) -> Tag {
    Tag::List {
        element: if values.is_empty() { 0 } else { 10 },
        values,
    }
}
pub fn decode_tile(r: &mut Reader<'_>) -> Result<(IVec3, u8, Tag)> {
    let p = IVec3::from_array(r.position()?);
    let action = r.u8()?;
    let tag = item_nbt::decode(r.remaining())?.ok_or_else(|| anyhow!("Missing S35 tile data"))?;
    Ok((p, action, tag))
}
pub fn decode_sign(r: &mut Reader<'_>) -> Result<(IVec3, [String; 4])> {
    let p = IVec3::from_array(r.position()?);
    let mut lines: [String; 4] = Default::default();
    for line in &mut lines {
        *line = r.string(32767)?;
    }
    r.finish()?;
    Ok((p, lines))
}
pub fn apply_sign(stored: &Tag, editable: bool, lines: [String; 4]) -> Result<Tag> {
    let mut data = map(stored)?.clone();
    if editable {
        for (i, s) in lines.into_iter().enumerate() {
            data.insert(format!("Text{}", i + 1), Tag::String(stored_sign_text(&s)));
        }
    }
    Ok(Tag::Compound(data))
}
#[path = "tile_chat.rs"]
mod chat_component;
use chat_component::canonical as chat;

#[derive(Clone)]
pub struct Live {
    state: Fields,
}
impl Live {
    pub fn from_written(written: &Tag) -> Result<Self> {
        Ok(Self {
            state: map(written)?.clone(),
        })
    }
    pub fn update(&mut self, id: u16, action: u8, packet: &Tag) -> Result<Option<Tag>> {
        let valid = matches!(
            (id, action),
            (52, 1) | (137, 2) | (138, 3) | (144, 4) | (140, 5) | (176 | 177, 6)
        );
        if !valid {
            return Ok(None);
        }
        let input = map(packet)?;
        for key in ["x", "y", "z"] {
            self.state.insert(key.into(), Tag::Int(number(input, key)));
        }
        match action {
            1 => self.spawner(input),
            2 => self.command(input)?,
            3 => {
                self.state
                    .insert("Lock".into(), Tag::String(str_field(input, "Lock").into()));
                for key in ["Primary", "Secondary"] {
                    let raw = number(input, key);
                    self.state.insert(
                        key.into(),
                        Tag::Int(if matches!(raw, 1 | 3 | 5 | 8 | 10 | 11) {
                            raw
                        } else {
                            0
                        }),
                    );
                }
                self.state
                    .insert("Levels".into(), Tag::Int(number(input, "Levels")));
            }
            4 => self.skull(input),
            5 => {
                let item = match input.get("Item") {
                    Some(Tag::String(s)) => middle_pick::named_item(s),
                    value => u16::try_from(item_nbt::number(value)).ok(),
                };
                self.state.insert(
                    "Item".into(),
                    Tag::String(item.and_then(middle_pick::item_name).unwrap_or("").into()),
                );
                self.state
                    .insert("Data".into(), Tag::Int(number(input, "Data")));
            }
            6 => {
                self.state
                    .insert("Base".into(), Tag::Int(number(input, "Base")));
                self.state
                    .insert("Patterns".into(), list_tag(list(input, "Patterns")));
            }
            _ => unreachable!(),
        }
        let mut written = self.state.clone();
        if action == 1 && str_field(&written, "EntityId").is_empty() {
            written.retain(|k, _| matches!(k.as_str(), "id" | "x" | "y" | "z"));
        }
        if action == 2 && number(&written, "TrackOutput") == 0 {
            written.remove("LastOutput");
        }
        Ok(Some(Tag::Compound(written)))
    }
    fn spawner(&mut self, input: &Fields) {
        let name = str_field(input, "EntityId");
        let spawn = input.get("SpawnData").and_then(item_nbt::compound);
        let normalized = cart_name(name, spawn);
        self.state
            .insert("EntityId".into(), Tag::String(normalized.clone()));
        self.state
            .insert("Delay".into(), Tag::Short(number(input, "Delay") as i16));
        for (test, keys) in [
            (
                "MinSpawnDelay",
                &["MinSpawnDelay", "MaxSpawnDelay", "SpawnCount"][..],
            ),
            (
                "MaxNearbyEntities",
                &["MaxNearbyEntities", "RequiredPlayerRange"][..],
            ),
            ("SpawnRange", &["SpawnRange"][..]),
        ] {
            if numeric(input, test) {
                for &k in keys {
                    self.state
                        .insert(k.into(), Tag::Short(number(input, k) as i16));
                }
            }
        }
        let mut potentials: Vec<_> = list(input, "SpawnPotentials")
            .iter()
            .filter_map(|t| {
                let v = item_nbt::compound(t)?;
                let props = compound(v, "Properties");
                Some(weighted(
                    cart_name(str_field(v, "Type"), Some(&props)),
                    props,
                    number(v, "Weight"),
                ))
            })
            .collect();
        self.state.remove("SpawnData");
        self.state.remove("SpawnPotentials");
        if let Some(data) = spawn {
            self.state
                .insert("SpawnData".into(), Tag::Compound(data.clone()));
            if potentials.is_empty() {
                potentials.push(weighted(normalized, data.clone(), 1));
            }
        }
        if !potentials.is_empty() {
            self.state
                .insert("SpawnPotentials".into(), list_tag(potentials));
        }
    }
    fn command(&mut self, input: &Fields) -> Result<()> {
        self.state.insert(
            "Command".into(),
            Tag::String(str_field(input, "Command").into()),
        );
        self.state.insert(
            "SuccessCount".into(),
            Tag::Int(number(input, "SuccessCount")),
        );
        if let Some(Tag::String(s)) = input.get("CustomName") {
            self.state
                .insert("CustomName".into(), Tag::String(s.clone()));
        }
        if let Some(Tag::Byte(v)) = input.get("TrackOutput") {
            self.state
                .insert("TrackOutput".into(), Tag::Byte(i8::from(*v != 0)));
        }
        if number(&self.state, "TrackOutput") != 0 {
            if let Some(Tag::String(output)) = input.get("LastOutput") {
                self.state
                    .insert("LastOutput".into(), Tag::String(chat(output)?));
            }
        }
        if let Some(stats) = command_stats(input, self.state.get("CommandStats")) {
            self.state.insert("CommandStats".into(), stats);
        } else {
            self.state.remove("CommandStats");
        }
        Ok(())
    }
    fn skull(&mut self, input: &Fields) {
        let value = number(input, "SkullType") as i8;
        self.state.insert("SkullType".into(), Tag::Byte(value));
        self.state
            .insert("Rot".into(), Tag::Byte(number(input, "Rot") as i8));
        if value != 3 {
            return;
        }
        if let Some(Tag::Compound(owner)) = input.get("Owner") {
            if let Some(profile) = read_profile(owner) {
                self.state.insert("Owner".into(), profile);
            } else {
                self.state.remove("Owner");
            }
        } else if !str_field(input, "ExtraType").is_empty() {
            self.state.insert(
                "Owner".into(),
                Tag::Compound(BTreeMap::from([(
                    "Name".into(),
                    Tag::String(str_field(input, "ExtraType").into()),
                )])),
            );
        }
    }
}
pub(super) fn command_stats(input: &Fields, previous: Option<&Tag>) -> Option<Tag> {
    let input = compound(input, "CommandStats");
    let mut stats = previous
        .and_then(item_nbt::compound)
        .cloned()
        .unwrap_or_default();
    for key in [
        "SuccessCount",
        "AffectedBlocks",
        "AffectedEntities",
        "AffectedItems",
        "QueryResult",
    ] {
        let a = format!("{key}Name");
        let b = format!("{key}Objective");
        if let (Some(Tag::String(name)), Some(Tag::String(objective))) =
            (input.get(&a), input.get(&b))
        {
            if name.is_empty() || objective.is_empty() {
                stats.remove(&a);
                stats.remove(&b);
            } else {
                stats.insert(a, Tag::String(name.clone()));
                stats.insert(b, Tag::String(objective.clone()));
            }
        }
    }
    (!stats.is_empty()).then_some(Tag::Compound(stats))
}
pub(super) fn stored_sign_text(raw: &str) -> String {
    chat(raw).unwrap_or_else(|_| chat_component::quote(raw))
}
fn cart_name(name: &str, props: Option<&Fields>) -> String {
    if name != "Minecart" {
        return name.into();
    }
    let kind = props.map_or(0, |v| number(v, "Type"));
    [
        "MinecartRideable",
        "MinecartChest",
        "MinecartFurnace",
        "MinecartTNT",
        "MinecartSpawner",
        "MinecartHopper",
        "MinecartCommandBlock",
    ]
    .get(kind as usize)
    .copied()
    .unwrap_or("MinecartRideable")
    .into()
}
fn weighted(name: String, props: Fields, weight: i32) -> Tag {
    Tag::Compound(BTreeMap::from([
        ("Type".into(), Tag::String(name)),
        ("Properties".into(), Tag::Compound(props)),
        ("Weight".into(), Tag::Int(weight)),
    ]))
}
fn read_profile(m: &Fields) -> Option<Tag> {
    let name = str_field(m, "Name");
    let uuid = str_field(m, "Id");
    if name.is_empty() && uuid.is_empty() {
        return None;
    }
    let mut out = Fields::new();
    if !name.is_empty() {
        out.insert("Name".into(), Tag::String(name.into()));
    }
    let parts: Vec<_> = uuid.split('-').collect();
    if parts.len() == 5
        && parts
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(s, n)| s.len() == n && s.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        out.insert("Id".into(), Tag::String(uuid.to_ascii_lowercase()));
    }
    let mut properties = Fields::new();
    for (k, v) in compound(m, "Properties") {
        if let Tag::List {
            element: 10,
            values,
        } = v
        {
            let entries: Vec<_> = values
                .iter()
                .filter_map(|v| {
                    let p = item_nbt::compound(v)?;
                    let mut data =
                        Fields::from([("Value".into(), Tag::String(str_field(p, "Value").into()))]);
                    if let Some(Tag::String(s)) = p.get("Signature") {
                        data.insert("Signature".into(), Tag::String(s.clone()));
                    }
                    Some(Tag::Compound(data))
                })
                .collect();
            if !entries.is_empty() {
                properties.insert(k, list_tag(entries));
            }
        }
    }
    if !properties.is_empty() {
        out.insert("Properties".into(), Tag::Compound(properties));
    }
    Some(Tag::Compound(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    #[derive(Deserialize)]
    struct Cases {
        tile: Vec<(u16, String, String, String)>,
        sign: Vec<(bool, String, String, String)>,
        chat: Vec<(String, String)>,
    }
    fn bytes(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }
    fn tag(s: &str) -> Tag {
        item_nbt::decode(&bytes(s)).unwrap().unwrap()
    }
    #[test]
    fn original_s35_and_s33_packets_dispatch_to_live_tile_entities_match() {
        let cases: Cases = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-tile-updates.json"
        ))
        .unwrap();
        assert_eq!((cases.tile.len(), cases.sign.len()), (186, 18));
        let mut active = None;
        for (i, (id, before, wire, after)) in cases.tile.into_iter().enumerate() {
            let before = tag(&before);
            if i % 31 == 0 {
                active = Some(Live::from_written(&before).unwrap());
            }
            let wire = bytes(&wire);
            let (p, action, packet) = decode_tile(&mut Reader::new(&wire)).unwrap();
            assert_eq!(p, IVec3::new(-3, 63, 19));
            let actual = active
                .as_mut()
                .unwrap()
                .update(id, action, &packet)
                .unwrap()
                .unwrap_or(before);
            assert_eq!(
                actual,
                tag(&after),
                "original S35 case {i}, block {id}, action {action}"
            );
        }
        assert_eq!(cases.chat.len(), 110);
        for (input, canonical) in cases.chat {
            assert_eq!(
                chat(&input).unwrap(),
                canonical,
                "original component {input}"
            );
        }
        for (editable, before, wire, after) in cases.sign {
            let wire = bytes(&wire);
            let (p, lines) = decode_sign(&mut Reader::new(&wire)).unwrap();
            assert_eq!(p, IVec3::new(-3, 63, 19));
            assert_eq!(
                apply_sign(&tag(&before), editable, lines).unwrap(),
                tag(&after)
            );
        }
    }
}
