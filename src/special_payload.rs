//! Vanilla custom-payload actions. Each payload preserves its own protocol-47 body.
use crate::protocol::{Slot, Writer};
use anyhow::{ensure, Result};
#[derive(Clone, Debug, PartialEq)]
pub enum Payload {
    Anvil(String),
    Beacon(i32, i32),
    Book { signed: bool, slot: Slot },
}
impl Payload {
    pub fn write(&self, w: &mut Writer) -> Result<()> {
        match self {
            Self::Anvil(s) => {
                ensure!(
                    s.encode_utf16().count() <= 30,
                    "Anvil name exceeds the original text field"
                );
                w.string("MC|ItemName").string(s);
            }
            Self::Beacon(a, b) => {
                w.string("MC|Beacon").i32(*a).i32(*b);
            }
            Self::Book { signed, slot } => {
                ensure!(
                    slot.id == if *signed { 387 } else { 386 },
                    "Wrong book item"
                );
                w.string(if *signed { "MC|BSign" } else { "MC|BEdit" })
                    .slot(Some(slot));
            }
        }
        ensure!(w.0.len() <= 32767, "Custom payload too large");
        Ok(())
    }
}
