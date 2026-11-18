use super::{MobKind, Mode, Outcome, Player, Simulation, Stack, World};
use crate::vanilla::{
    drops::DropNbt as Tag, hand_placement::replaceable_for_click, item_nbt, material,
    ray_trace::Hit,
};
use glam::{IVec3, Vec3};

fn wither_skull(world: &World, pos: IVec3) -> bool {
    world.get(pos) >> 4 == 144
        && world
            .tile(pos)
            .and_then(item_nbt::compound)
            .is_some_and(|tag| item_nbt::number(tag.get("SkullType")) == 1)
}
impl Simulation {
    pub(super) fn place_skull(
        &mut self,
        world: &mut World,
        player: &Player,
        item: &Stack,
        hit: Hit,
        out: &mut Outcome,
    ) {
        if !(1..6).contains(&hit.face) {
            return;
        }
        let pos = if replaceable_for_click(world, hit.position) {
            hit.position
        } else {
            if !material::is_solid(world.get(hit.position)) {
                return;
            }
            hit.adjacent()
        };
        if !replaceable_for_click(world, pos) {
            return;
        }
        let state = (144 << 4) | u16::from(hit.face);
        if !world.set(pos, state) {
            return;
        }
        let rotation = if hit.face == 1 {
            ((player.java_rotation()[0] * 16. / 360. + 0.5).floor() as i32 & 15) as i8
        } else {
            0
        };
        let mut fields = std::collections::BTreeMap::from([
            ("id".into(), Tag::String("Skull".into())),
            ("x".into(), Tag::Int(pos.x)),
            ("y".into(), Tag::Int(pos.y)),
            ("z".into(), Tag::Int(pos.z)),
            ("SkullType".into(), Tag::Byte(item.meta as i8)),
            ("Rot".into(), Tag::Byte(rotation)),
        ]);
        if let Some(owner) = item
            .nbt
            .as_deref()
            .and_then(item_nbt::compound)
            .and_then(|v| v.get("SkullOwner"))
        {
            match owner {
                Tag::String(s) if !s.trim().is_empty() => {
                    fields.insert(
                        "Owner".into(),
                        Tag::Compound([("Name".into(), Tag::String(s.clone()))].into()),
                    );
                }
                Tag::String(_) => {}
                _ => {
                    fields.insert("Owner".into(), owner.clone());
                }
            }
        }
        self.world_commands
            .edit_tile(world, pos, state, Some(Tag::Compound(fields)));
        out.changed.push(pos);
        self.place_ticks = 4;
        if self.mode == Mode::Survival {
            self.inventory.take_selected(1);
        }
        if item.meta == 1 && pos.y >= 2 && self.world_difficulty != Some(0) {
            self.build_mob(world, pos, true, out);
        }
    }
    pub(super) fn build_mob(
        &mut self,
        world: &mut World,
        last: IVec3,
        wither: bool,
        out: &mut Outcome,
    ) {
        let directions = [
            IVec3::NEG_Y,
            IVec3::Y,
            IVec3::NEG_Z,
            IVec3::Z,
            IVec3::NEG_X,
            IVec3::X,
        ];
        for down in directions {
            for side in [IVec3::X, IVec3::Z, IVec3::Y] {
                if side.dot(down) != 0 {
                    continue;
                }
                for offset in if wither { -1..=1 } else { 0..=0 } {
                    let head = last + offset * side;
                    let arms = head + down;
                    let foot = arms + down;
                    let block = |p| world.get(p) >> 4;
                    let mut pattern = vec![head, arms, foot];
                    let kind = if !wither && block(arms) == 80 && block(foot) == 80 {
                        MobKind::SnowGolem
                    } else {
                        let required = if wither { 88 } else { 42 };
                        if [arms - side, arms, arms + side, foot]
                            .into_iter()
                            .any(|p| block(p) != required)
                            || [-side, side].into_iter().any(|s| block(foot + s) != 0)
                        {
                            continue;
                        }
                        if wither {
                            if [-side, IVec3::ZERO, side]
                                .into_iter()
                                .any(|s| !wither_skull(world, head + s))
                            {
                                continue;
                            }
                            pattern.extend([head - side, head + side]);
                        } else if block(head - side) != 0 || block(head + side) != 0 {
                            continue;
                        }
                        pattern.extend([arms - side, arms + side]);
                        if wither {
                            MobKind::Wither
                        } else {
                            MobKind::IronGolem
                        }
                    };
                    for pos in pattern {
                        world.set(pos, 0);
                        self.world_commands.edit_tile(world, pos, 0, None);
                        out.changed.push(pos);
                        self.block_ticks.notify(world, pos);
                    }
                    let pos =
                        foot.as_vec3() + Vec3::new(0.5, if wither { 0.55 } else { 0.05 }, 0.5);
                    let yaw = if wither && side.cross(down).x == 0 {
                        270_f32
                    } else {
                        180_f32
                    };
                    let id = self.spawn_registered(kind, pos, yaw.to_radians());
                    if kind == MobKind::IronGolem {
                        self.command_entities
                            .mobs
                            .entry(id)
                            .or_default()
                            .extra
                            .insert("PlayerCreated".into(), Tag::Byte(1));
                    }
                    if wither {
                        self.command_entities
                            .mobs
                            .entry(id)
                            .or_default()
                            .extra
                            .insert("Invul".into(), Tag::Int(220));
                        self.mobs.last_mut().unwrap().health = kind.health() / 3.;
                    }
                    return;
                }
            }
        }
    }
}
