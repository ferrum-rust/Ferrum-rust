//! Minecraft.runTick, clickMouse/sendClickBlockToController and
//! PlayerControllerMP block-damage bookkeeping at the 20 Hz input boundary.
use crate::inventory::{tool, Stack, Tool};
use glam::IVec3;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug)]
pub struct Block {
    pub pos: IVec3,
    pub face: u8,
    pub state: u16,
    pub increment: f32,
    pub in_border: bool,
    pub can_edit: bool,
    pub adjacent_fire: bool,
}
impl Block {
    pub fn adjacent(&self) -> IVec3 {
        self.pos
            + [
                IVec3::NEG_Y,
                IVec3::Y,
                IVec3::NEG_Z,
                IVec3::Z,
                IVec3::NEG_X,
                IVec3::X,
            ][self.face as usize]
    }
}
#[derive(Clone, Copy, Debug)]
pub enum Target {
    Miss,
    Entity,
    Block(Block),
}
pub fn target<I>(
    world: &crate::world::World,
    player: &crate::player::Player,
    sim: &crate::gameplay::Simulation,
    hit: Option<super::entity_pick::Hit<I>>,
    can_edit: bool,
) -> Target {
    use super::{entity_pick::Hit, mining};
    match hit {
        Some(Hit::Entity { .. }) => Target::Entity,
        Some(Hit::Block(hit)) => {
            let p = hit.position;
            let state = world.get(p);
            let c = mining::Conditions {
                grounded: player.grounded,
                eyes_in_water: super::movement::eyes_in_water(
                    world,
                    glam::DVec3::from_array(player.precise_position()),
                    player.sneaking(),
                ),
                aqua_affinity: sim.inventory.armor_enchantment(6) > 0,
                haste: sim.effects.get(3).map(|e| e.amplifier),
                fatigue: sim.effects.get(4).map(|e| e.amplifier),
            };
            Target::Block(Block {
                pos: p,
                face: hit.face,
                state,
                increment: mining::increment(
                    state,
                    sim.inventory.slots[sim.inventory.selected].as_ref(),
                    mining::hardness(state),
                    c,
                ),
                in_border: sim.world_border.contains(p, super::click_border::now()),
                can_edit,
                adjacent_fire: world.get(hit.adjacent()) >> 4 == 51,
            })
        }
        _ => Target::Miss,
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Dig {
    Start,
    Abort,
    Stop,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    Swing,
    Attack,
    Dig(Dig, IVec3, u8),
    Clicked(IVec3),
    Destroy(IVec3),
    Extinguish(IVec3),
    Stage(IVec3, i32),
    StepSound(IVec3, u16),
    HitEffect(IVec3, u8),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Controller {
    pub current: IVec3,
    pub hitting: bool,
    pub damage: f32,
    pub sound_ticks: f32,
    pub hit_delay: i32,
    pub click_counter: i32,
    current_item: Option<Stack>,
    #[serde(skip)]
    queued: u32,
    #[serde(skip)]
    physical_input: bool,
    #[serde(skip)]
    was_down: bool,
}
impl Default for Controller {
    fn default() -> Self {
        Self {
            current: IVec3::splat(-1),
            hitting: false,
            damage: 0.,
            sound_ticks: 0.,
            hit_delay: 0,
            click_counter: 0,
            current_item: None,
            queued: 0,
            physical_input: false,
            was_down: false,
        }
    }
}
impl Controller {
    fn refresh_item(&mut self, held: Option<&Stack>) {
        // PlayerControllerMP keeps the original ItemStack reference. A tag
        // mutated in place is still present when a later, equal copy replaces it.
        if let (Some(previous), Some(now)) = (&mut self.current_item, held) {
            if previous.identity == now.identity {
                *previous = now.clone();
            }
        }
    }
    pub fn queue_press(&mut self) {
        self.physical_input = true;
        self.queued = self.queued.saturating_add(1);
    }
    pub fn presses(&mut self, down: bool) -> u32 {
        let n = std::mem::take(&mut self.queued);
        let n = if self.physical_input {
            n
        } else {
            u32::from(down && !self.was_down)
        };
        self.was_down = down;
        n
    }
    fn same_target(&self, b: Block, item: Option<&Stack>) -> bool {
        if self.current != b.pos {
            return false;
        }
        match (&self.current_item, item) {
            (None, None) => true,
            (Some(last), Some(now)) => {
                last.identity == now.identity
                    || (last.id == now.id
                        && last.nbt == now.nbt
                        && (crate::vanilla::armor::maximum_damage(now.id) > 0
                            && !now.unbreakable()
                            || (last.meta == now.meta && last.damage == now.damage)))
            }
            _ => false,
        }
    }
    fn destroy(
        &mut self,
        b: &mut Block,
        creative: bool,
        item: Option<&Stack>,
        out: &mut Vec<Action>,
    ) {
        if !b.can_edit
            || creative && item.is_some_and(|s| tool(s.id).0 == Tool::Sword)
            || b.state == 0
        {
            return;
        }
        out.push(Action::Destroy(b.pos));
        b.state = 0;
        self.current.y = -1;
    }
    fn creative(&mut self, b: &mut Block, item: Option<&Stack>, out: &mut Vec<Action>) {
        out.push(Action::Dig(Dig::Start, b.pos, b.face));
        if b.adjacent_fire {
            out.push(Action::Extinguish(b.adjacent()));
            b.adjacent_fire = false;
        } else {
            self.destroy(b, true, item, out);
        }
        self.hit_delay = 5;
    }
    fn click_block(
        &mut self,
        b: &mut Block,
        creative: bool,
        item: Option<&Stack>,
        out: &mut Vec<Action>,
    ) -> bool {
        if !b.can_edit || !b.in_border {
            return false;
        }
        if creative {
            self.creative(b, item, out);
        } else if !self.hitting || !self.same_target(*b, item) {
            if self.hitting {
                out.push(Action::Dig(Dig::Abort, self.current, b.face));
            }
            out.push(Action::Dig(Dig::Start, b.pos, b.face));
            if b.state != 0 && self.damage == 0. {
                out.push(Action::Clicked(b.pos));
            }
            if b.state != 0 && b.increment >= 1. {
                self.hitting = false;
                self.damage = 0.;
                self.sound_ticks = 0.;
                self.destroy(b, false, item, out);
            } else {
                self.hitting = true;
                self.current = b.pos;
                self.current_item = item.cloned();
                self.damage = 0.;
                self.sound_ticks = 0.;
                out.push(Action::Stage(self.current, -1));
            }
        }
        true
    }
    pub fn press_phase(
        &mut self,
        target: &mut Target,
        presses: u32,
        creative: bool,
        item: Option<&Stack>,
        using: bool,
    ) -> Vec<Action> {
        self.refresh_item(item);
        self.click_counter = self.click_counter.saturating_sub(1).max(0);
        let mut out = vec![];
        if using {
            return out;
        }
        for _ in 0..presses {
            if self.click_counter > 0 {
                break;
            }
            out.push(Action::Swing);
            match target {
                Target::Entity => out.push(Action::Attack),
                Target::Block(b) if b.state != 0 => {
                    self.click_block(b, creative, item, &mut out);
                }
                _ if !creative => self.click_counter = 10,
                _ => {}
            }
        }
        out
    }
    fn damage_block(
        &mut self,
        b: &mut Block,
        creative: bool,
        item: Option<&Stack>,
        out: &mut Vec<Action>,
    ) -> bool {
        if self.hit_delay > 0 {
            self.hit_delay -= 1;
            return true;
        }
        if creative && b.in_border {
            self.creative(b, item, out);
            return true;
        }
        if !self.same_target(*b, item) {
            return self.click_block(b, creative, item, out);
        }
        if b.state == 0 {
            self.hitting = false;
            return false;
        }
        self.damage += b.increment;
        if self.sound_ticks % 4. == 0. {
            out.push(Action::StepSound(b.pos, b.state));
        }
        self.sound_ticks += 1.;
        if self.damage >= 1. {
            self.hitting = false;
            out.push(Action::Dig(Dig::Stop, b.pos, b.face));
            self.destroy(b, creative, item, out);
            self.damage = 0.;
            self.sound_ticks = 0.;
            self.hit_delay = 5;
        }
        out.push(Action::Stage(self.current, (self.damage * 10.) as i32 - 1));
        true
    }
    pub fn held_phase(
        &mut self,
        target: &mut Target,
        down: bool,
        creative: bool,
        item: Option<&Stack>,
        using: bool,
    ) -> Vec<Action> {
        self.refresh_item(item);
        if !down {
            self.click_counter = 0
        }
        let mut out = vec![];
        if self.click_counter > 0 || using {
            return out;
        }
        if down {
            if let Target::Block(b) = target {
                if b.state != 0 && self.damage_block(b, creative, item, &mut out) {
                    out.push(Action::HitEffect(b.pos, b.face));
                    out.push(Action::Swing);
                }
                return out;
            }
        }
        if self.hitting {
            out.push(Action::Dig(Dig::Abort, self.current, 0));
            self.hitting = false;
            self.damage = 0.;
            out.push(Action::Stage(self.current, -1));
        }
        out
    }
}
include!("click_sounds_data.rs");
pub fn step_sound(state: u16) -> Option<(&'static str, f32, f32)> {
    let (index, volume, pitch) = SOUNDS.get((state >> 4) as usize).copied()?;
    let clips = CLIPS[index].1;
    if clips.is_empty() {
        return None;
    }
    Some((super::hand_placement::sample_clip(clips), volume, pitch))
}
#[cfg(test)]
#[path = "click_controller_tests.rs"]
mod tests;
