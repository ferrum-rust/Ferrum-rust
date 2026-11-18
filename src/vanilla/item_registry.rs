//! Names, potion properties and item/enchantment applicability sampled from the installed game.
use serde::Deserialize;
use std::{collections::BTreeMap, sync::OnceLock};

pub(crate) type Enchantment = (i32, String, i32, i32, Vec<u16>, Vec<i16>);
#[derive(Deserialize)]
pub(crate) struct Registry {
    pub items: BTreeMap<String, u16>,
    pub effects: BTreeMap<String, (i32, String, bool)>,
    pub enchantments: BTreeMap<String, Enchantment>,
}
pub(crate) fn registry() -> &'static Registry {
    static DATA: OnceLock<Registry> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!("../commands/items189.json"))
            .expect("original item registry")
    })
}
