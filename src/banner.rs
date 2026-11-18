use crate::{
    gameplay::{Mode, Outcome, Simulation},
    inventory::Stack,
    vanilla::{drops::DropNbt, item_nbt, material, ray_trace::Hit},
    world::World,
};
use glam::IVec3;
#[cfg(test)]
use glam::Vec3;
use std::collections::BTreeMap;

type Tag = BTreeMap<String, DropNbt>;
#[path = "banner_render.rs"]
pub(crate) mod render;
#[cfg(test)]
#[path = "banner_tests.rs"]
mod tests;
#[path = "banner_texture.rs"]
pub(crate) mod texture;
const SIDES: [IVec3; 6] = [
    IVec3::NEG_Y,
    IVec3::Y,
    IVec3::NEG_Z,
    IVec3::Z,
    IVec3::NEG_X,
    IVec3::X,
];

fn item_tag(stack: &Stack) -> Option<&Tag> {
    stack
        .nbt
        .as_deref()
        .and_then(item_nbt::compound)?
        .get("BlockEntityTag")
        .and_then(item_nbt::compound)
}
fn base(tag: Option<&Tag>, fallback: u16) -> i32 {
    match tag.and_then(|t| t.get("Base")) {
        Some(
            v @ (DropNbt::Byte(_)
            | DropNbt::Short(_)
            | DropNbt::Int(_)
            | DropNbt::Long(_)
            | DropNbt::Float(_)
            | DropNbt::Double(_)),
        ) => item_nbt::number(Some(v)),
        _ => i32::from(fallback & 15),
    }
}
fn patterns(tag: &Tag) -> Option<DropNbt> {
    match tag.get("Patterns")? {
        list @ DropNbt::List { element: 10, .. } => Some(list.clone()),
        _ => Some(DropNbt::List {
            element: 0,
            values: Vec::new(),
        }),
    }
}
fn tile(stack: &Stack, p: IVec3) -> DropNbt {
    let tag = item_tag(stack);
    let mut nbt = Tag::from([
        ("Base".into(), DropNbt::Int(base(tag, stack.meta))),
        ("id".into(), DropNbt::String("Banner".into())),
        ("x".into(), DropNbt::Int(p.x)),
        ("y".into(), DropNbt::Int(p.y)),
        ("z".into(), DropNbt::Int(p.z)),
    ]);
    if let Some(list) = tag.and_then(patterns) {
        nbt.insert("Patterns".into(), list);
    }
    DropNbt::Compound(nbt)
}
pub(crate) fn dropped(world: &World, p: IVec3) -> Stack {
    let written = world.tile(p).and_then(item_nbt::compound);
    let color = base(written, 0);
    let mut block_tag = Tag::from([("Base".into(), DropNbt::Int(color))]);
    if let Some(list) = written.and_then(patterns) {
        block_tag.insert("Patterns".into(), list);
    }
    let mut stack = Stack::new(425, 1);
    stack.meta = color as u16;
    stack.with_nbt(DropNbt::Compound(Tag::from([(
        "BlockEntityTag".into(),
        DropNbt::Compound(block_tag),
    )])))
}

impl Simulation {
    pub(crate) fn place_banner(
        &mut self,
        world: &mut World,
        stack: &Stack,
        hit: Hit,
        yaw: f32,
        out: &mut Outcome,
    ) {
        if !(1..6).contains(&hit.face) || !material::is_solid(world.get(hit.position)) {
            return;
        }
        let p = hit.adjacent();
        if !material::replaceable(world.get(p))
            || SIDES[2..].iter().any(|&d| world.get(p + d) >> 4 == 81)
        {
            return;
        }
        let state = if hit.face == 1 {
            let rotation = (f64::from((yaw + 180.) * 16. / 360.) + 0.5).floor() as i32 & 15;
            176 << 4 | rotation as u16
        } else {
            177 << 4 | u16::from(hit.face)
        };
        if world.set(p, state) {
            self.world_commands
                .edit_tile(world, p, state, Some(tile(stack, p)));
            out.changed.push(p);
            if self.mode == Mode::Survival {
                self.inventory.take_selected(1);
            }
            self.place_ticks = 4;
        }
    }
    pub(crate) fn check_banner_support(
        &mut self,
        world: &mut World,
        source: IVec3,
        out: &mut Outcome,
    ) {
        if material::is_solid(world.get(source)) {
            return;
        }
        for (face, &d) in SIDES.iter().enumerate().skip(1) {
            let p = source + d;
            let state = world.get(p);
            let lost = if face == 1 {
                state >> 4 == 176
            } else {
                state == (177 << 4 | face as u16)
            };
            if lost {
                crate::gameplay::block_support::drop_broken_block(self, world, p, state);
                if world.set(p, 0) {
                    self.world_commands.edit_tile(world, p, 0, None);
                    out.changed.push(p);
                    self.block_ticks.notify(world, p);
                }
            }
        }
    }
}
