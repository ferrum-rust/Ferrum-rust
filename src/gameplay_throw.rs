use super::*;
use crate::vanilla::item_entity;
use glam::DVec3;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThrowSource {
    Hotbar,
    Window,
    CreativeWindow,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        container::window_click,
        inventory::{ClickAction, SlotRef},
        vanilla::{drops::DropNbt, item_nbt},
    };
    use serde::Deserialize;
    #[derive(Deserialize)]
    struct Case {
        seed: i64,
        look: [f32; 2],
        sneak: bool,
        kind: String,
        position: [f64; 3],
        velocity: [f64; 3],
        state: String,
        age: i32,
        delay: i32,
        thrower: Option<String>,
        owner: Option<String>,
        count: u8,
        tag: i32,
        slot: u8,
        cursor: u8,
        stats: i32,
    }

    #[test]
    fn q_and_inventory_throws_match_actual_java_positions_rng_stacks_and_metadata() {
        let cases: Vec<Case> = serde_json::from_reader(flate2::read::GzDecoder::new(
            &include_bytes!("../tests/fixtures/vanilla189-player-throws.json.gz")[..],
        ))
        .unwrap();
        assert_eq!(cases.len(), 126);
        for c in cases {
            let mut s = Simulation::default();
            s.local_name = "Player".into();
            s.player_random_state = JavaRandom::new(c.seed).state();
            let mut p = Player::new(Vec3::ZERO);
            p.set_pose_f64(
                [-1.000000123456, 71.23456789, 8.7654321],
                c.look[0],
                c.look[1],
                false,
            );
            if c.sneak {
                p.keys.insert(winit::keyboard::KeyCode::ShiftLeft);
            }
            let stack = Stack::new(264, 17).with_nbt(DropNbt::Compound(
                std::collections::BTreeMap::from([("variant".into(), DropNbt::Int(191))]),
            ));
            s.inventory.selected = 3;
            match c.kind.as_str() {
                "Q" | "CtrlQ" => {
                    s.inventory.slots[3] = Some(stack);
                    s.throw_selected(&p, c.kind == "CtrlQ");
                }
                "creative" => s.throw_local_stack(&p, stack, ThrowSource::CreativeWindow),
                name => {
                    let (slot, action) = if name.starts_with("slot") {
                        s.inventory.slots[3] = Some(stack);
                        (
                            SlotRef::Player(3),
                            ClickAction::Throw {
                                all: name == "slotCtrlQ",
                            },
                        )
                    } else {
                        s.inventory.cursor = Some(stack);
                        (
                            SlotRef::Outside,
                            ClickAction::Pickup {
                                right: name == "outsideRight",
                            },
                        )
                    };
                    for item in window_click(&mut s.inventory, None, slot, action, false, 2) {
                        s.throw_local_stack(&p, item, ThrowSource::Window);
                    }
                }
            }
            assert_eq!(s.drops.len(), 1, "{}", c.kind);
            let d = &s.drops[0];
            assert!(
                (d.position - DVec3::from_array(c.position))
                    .abs()
                    .max_element()
                    < 3e-14,
                "{} {:?}",
                c.kind,
                d.position
            );
            assert!(
                (d.velocity - DVec3::from_array(c.velocity))
                    .abs()
                    .max_element()
                    < 2e-15,
                "{} {:?}",
                c.kind,
                d.velocity
            );
            assert_eq!(
                (d.age, d.pickup_delay, d.thrower.as_ref(), d.owner.as_ref()),
                (c.age, c.delay, c.thrower.as_ref(), c.owner.as_ref())
            );
            assert_eq!((d.stack.id, d.stack.count), (264, c.count));
            assert_eq!(
                item_nbt::number(
                    d.stack
                        .nbt
                        .as_deref()
                        .and_then(item_nbt::compound)
                        .and_then(|tag| tag.get("variant"))
                ),
                c.tag
            );
            assert_eq!(s.inventory.slots[3].as_ref().map_or(0, |v| v.count), c.slot);
            assert_eq!(s.inventory.cursor.as_ref().map_or(0, |v| v.count), c.cursor);
            assert_eq!(s.statistics.get("stat.drop").copied().unwrap_or(0), c.stats);
            assert_eq!(s.player_random_state.to_string(), c.state);
        }
    }
}

impl Simulation {
    /// Server-side EntityPlayer.dropItem for a forward throw. Each player owns their RNG.
    pub(crate) fn throw_stack_from(
        &mut self,
        stack: Stack,
        feet: DVec3,
        eye_height: f32,
        look: [f32; 2],
        rng: &mut JavaRandom,
        thrower: Option<&str>,
    ) -> Option<&mut Drop> {
        if stack.count == 0 {
            return None;
        }
        let pos = item_entity::throw_position(feet, eye_height);
        let velocity = item_entity::throw_velocity(look[0], look[1], rng);
        let drop = self.spawn_item_entity(stack, pos, velocity, 40);
        drop.thrower = thrower.map(String::from);
        Some(drop)
    }

    pub(crate) fn throw_local_stack(&mut self, player: &Player, stack: Stack, source: ThrowSource) {
        let mut rng = JavaRandom::from_state(self.player_random_state);
        let thrower = (source == ThrowSource::Hotbar).then(|| self.local_name.clone());
        let Some(drop) = self.throw_stack_from(
            stack,
            DVec3::from_array(player.precise_position()),
            player.eye_height(),
            player.java_rotation(),
            &mut rng,
            thrower.as_deref(),
        ) else {
            return;
        };
        if source == ThrowSource::CreativeWindow {
            // Creative inventory C10(-1) uses EntityItem.setAgeToCreativeDespawnTime.
            drop.age = 4800;
        }
        self.player_random_state = rng.state();
        if source == ThrowSource::Hotbar {
            let value = self.statistics.entry("stat.drop".to_owned()).or_default();
            *value = value.wrapping_add(1);
        }
    }
    pub fn throw_selected(&mut self, player: &Player, all: bool) {
        if self.commands.player_mode == Some(3) {
            return;
        }
        if let Some(stack) = self.inventory.take_selected(if all { u8::MAX } else { 1 }) {
            self.throw_local_stack(player, stack, ThrowSource::Hotbar);
        }
    }
}
