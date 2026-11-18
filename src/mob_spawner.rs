//! Server-side TileEntityMobSpawner countdown and spawn attempts.
use super::{MobKind, Simulation};
use crate::{
    player::Player,
    vanilla::{
        atmosphere,
        drops::DropNbt as Tag,
        item_nbt, material,
        movement::{collision, Aabb},
        random::JavaRandom,
    },
    world::World,
};
use glam::{DVec3, IVec3, Vec3};
use std::{collections::BTreeMap, sync::Arc};

type Fields = BTreeMap<String, Tag>;

fn number(fields: &Fields, key: &str, default: i32) -> i32 {
    fields
        .get(key)
        .map_or(default, |t| item_nbt::number(Some(t)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        gameplay::{Mode, Outcome},
        inventory::Stack,
    };

    #[test]
    fn nearby_player_activates_egg_configured_spawner_and_restores_weighted_nbt() {
        let mut world = super::super::tests::flat_world();
        let mut sim = Simulation::new(Mode::Creative, 189);
        sim.dimension = -1;
        let position = IVec3::new(8, 5, 8);
        world.set(position, 52 << 4);
        let mut egg = Stack::new(383, 1);
        egg.meta = 61;
        sim.use_item_on_block(
            &mut world,
            egg,
            (position, position + IVec3::Y),
            &mut Outcome::default(),
        );
        let mut fields = item_nbt::compound(world.tile(position).unwrap())
            .unwrap()
            .clone();
        assert_eq!(fields["EntityId"], Tag::String("Blaze".into()));
        let custom = Tag::Compound(
            [
                ("CustomName".into(), Tag::String("Forge".into())),
                ("NoAI".into(), Tag::Byte(1)),
            ]
            .into(),
        );
        let potential = Tag::Compound(
            [
                ("Type".into(), Tag::String("Rabbit".into())),
                ("Weight".into(), Tag::Int(3)),
                ("Properties".into(), Tag::Compound(Fields::new())),
            ]
            .into(),
        );
        fields.extend([
            ("Delay".into(), Tag::Short(1)),
            ("MinSpawnDelay".into(), Tag::Short(7)),
            ("MaxSpawnDelay".into(), Tag::Short(7)),
            ("MaxNearbyEntities".into(), Tag::Short(1)),
            ("SpawnCount".into(), Tag::Short(10)),
            ("SpawnData".into(), custom),
            (
                "SpawnPotentials".into(),
                Tag::List {
                    element: 10,
                    values: vec![potential],
                },
            ),
        ]);
        sim.world_commands
            .edit_tile(&mut world, position, 52 << 4, Some(Tag::Compound(fields)));
        let far = Player::new(Vec3::splat(50.));
        sim.tick_mob_spawners(&mut world, &far, 2);
        let delay = |w: &World| {
            number(
                item_nbt::compound(w.tile(position).unwrap()).unwrap(),
                "Delay",
                -999,
            )
        };
        assert_eq!(delay(&world), 1);
        let near = Player::new(Vec3::new(2., 5., 8.));
        sim.tick_mob_spawners(&mut world, &near, 2);
        assert_eq!(delay(&world), 0);
        sim.tick_mob_spawners(&mut world, &near, 2);
        assert_eq!(sim.mobs.len(), 1);
        assert_eq!(sim.mobs[0].kind, MobKind::Blaze);
        let data = &sim.command_entities.mobs[&sim.mobs[0].id];
        assert!(data.no_ai);
        assert_eq!(data.name, "Forge");
        assert_eq!(delay(&world), 7);
        let saved: Simulation = serde_json::from_slice(&serde_json::to_vec(&sim).unwrap()).unwrap();
        world.set(position, 0);
        world.set(position, 52 << 4);
        saved.world_commands.restore_tiles(&mut world, &[(0, 0)]);
        let stored = item_nbt::compound(world.tile(position).unwrap()).unwrap();
        assert_eq!(stored["EntityId"], Tag::String("Rabbit".into()));
        assert_eq!(delay(&world), 7);
    }
}

fn reset(fields: &mut Fields, rng: &mut JavaRandom) -> i32 {
    let min = number(fields, "MinSpawnDelay", 200) as i16 as i32;
    let max = number(fields, "MaxSpawnDelay", 800) as i16 as i32;
    let delay = if max > min {
        min + rng.next_int(max - min)
    } else {
        min
    };
    if let Some(Tag::List { values, .. }) = fields.get("SpawnPotentials") {
        let choices = values
            .iter()
            .filter_map(item_nbt::compound)
            .filter_map(|f| match f.get("Type") {
                Some(Tag::String(name)) => {
                    Some((number(f, "Weight", 1).max(0), name, f.get("Properties")))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let total = choices
            .iter()
            .fold(0i32, |sum, choice| sum.saturating_add(choice.0));
        if total > 0 {
            let mut choice = rng.next_int(total);
            for (weight, name, properties) in choices {
                choice -= weight;
                if choice < 0 {
                    let name = name.clone();
                    let properties = properties
                        .cloned()
                        .unwrap_or_else(|| Tag::Compound(Fields::new()));
                    fields.insert("EntityId".into(), Tag::String(name));
                    fields.insert("SpawnData".into(), properties);
                    break;
                }
            }
        }
    }
    delay
}

fn bounds(at: Vec3, (width, height): (f32, f32)) -> Aabb {
    let feet = at.as_dvec3();
    let radius = f64::from(width) * 0.5;
    Aabb {
        min: feet - DVec3::new(radius, 0., radius),
        max: feet + DVec3::new(radius, height.into(), radius),
    }
}

impl Simulation {
    pub(super) fn tick_mob_spawners(&mut self, world: &mut World, player: &Player, difficulty: u8) {
        if player.is_spectator() {
            return;
        }
        let mut active = Vec::new();
        for (&(cx, cz), chunk) in &world.chunks {
            for (&[x, y, z], tile) in &chunk.tiles {
                if chunk.get(x, y, z) >> 4 != 52 {
                    continue;
                }
                let Some(fields) = item_nbt::compound(tile) else {
                    continue;
                };
                let p = IVec3::new(cx * 16 + x, y, cz * 16 + z);
                let range = number(fields, "RequiredPlayerRange", 16) as f32;
                if range < 0.
                    || (p.as_vec3() + Vec3::splat(0.5)).distance_squared(player.position)
                        < range * range
                {
                    active.push((p, Arc::clone(tile)));
                }
            }
        }
        if active.is_empty() {
            return;
        }
        active.sort_unstable_by_key(|(p, _)| (p.x, p.y, p.z));
        let mut rng = self.block_ticks.world_random();
        for (p, tile) in active {
            let mut fields = item_nbt::compound(&tile).unwrap().clone();
            let mut delay = number(&fields, "Delay", 20);
            if delay == -1 {
                delay = reset(&mut fields, &mut rng);
            }
            if delay > 0 {
                delay -= 1;
            } else if self.spawn_cycle(world, player, difficulty, p, &fields, &mut rng) {
                delay = reset(&mut fields, &mut rng);
            } else {
                continue;
            }
            fields.insert("Delay".into(), Tag::Short(delay as i16));
            self.world_commands
                .edit_tile(world, p, 52 << 4, Some(Tag::Compound(fields)));
        }
        self.block_ticks.set_world_random(&rng);
    }

    fn spawn_cycle(
        &mut self,
        world: &World,
        player: &Player,
        difficulty: u8,
        p: IVec3,
        fields: &Fields,
        rng: &mut JavaRandom,
    ) -> bool {
        let name = match fields.get("EntityId") {
            Some(Tag::String(s)) => s.as_str(),
            _ => "Pig",
        };
        let Some(kind) = MobKind::from_name(name) else {
            return false;
        };
        let range = f64::from(number(fields, "SpawnRange", 4) as i16);
        let nearby = Aabb {
            min: p.as_dvec3(),
            max: p.as_dvec3() + DVec3::ONE,
        }
        .expand(DVec3::splat(range));
        let (rain, thunder) = self.weather.strengths(1.);
        let sky_loss = atmosphere::skylight_subtracted(self.clock as i64, rain, rain * thunder);
        let mut spawned = false;
        for _ in 0..number(fields, "SpawnCount", 4) as i16 {
            let count = self
                .mobs
                .iter()
                .filter(|m| {
                    let same_class = m.kind == kind
                        || matches!(
                            (kind, m.kind),
                            (MobKind::Cow, MobKind::Mooshroom)
                                | (MobKind::Spider, MobKind::CaveSpider)
                                | (MobKind::Slime, MobKind::MagmaCube)
                                | (MobKind::Zombie, MobKind::PigZombie)
                        );
                    same_class && nearby.intersects(bounds(m.position, self.mob_size(m)))
                })
                .count() as i32;
            if count >= number(fields, "MaxNearbyEntities", 6) {
                return true;
            }
            let x = f64::from(p.x) + (rng.next_double() - rng.next_double()) * range + 0.5;
            let y = p.y + rng.next_int(3) - 1;
            let z = f64::from(p.z) + (rng.next_double() - rng.next_double()) * range + 0.5;
            let position = Vec3::new(x as f32, y as f32, z as f32);
            let yaw = std::f32::consts::PI + rng.next_float() * std::f32::consts::TAU;
            let body = bounds(position, kind.size());
            if !world.contains(position)
                || !self.spawner_allows(
                    world,
                    position.floor().as_ivec3(),
                    kind,
                    difficulty,
                    sky_loss,
                    rng,
                )
                || (matches!(kind, MobKind::Silverfish | MobKind::Endermite)
                    && position.distance_squared(player.position) < 25.)
                || body.intersects(player.precise_bounds())
                || (kind != MobKind::Squid && !collision::colliders(world, body).is_empty())
                || (!matches!(kind, MobKind::Squid | MobKind::Guardian)
                    && crate::vanilla::movement::any_liquid(world, body))
                || self
                    .mobs
                    .iter()
                    .any(|m| body.intersects(bounds(m.position, self.mob_size(m))))
            {
                continue;
            }
            if let Some(properties) = fields.get("SpawnData").and_then(item_nbt::compound) {
                let mut entity: Fields = [
                    (
                        "Pos".into(),
                        Tag::List {
                            element: 6,
                            values: position
                                .to_array()
                                .into_iter()
                                .map(|v| Tag::Double(f64::from(v)))
                                .collect(),
                        },
                    ),
                    (
                        "Rotation".into(),
                        Tag::List {
                            element: 5,
                            values: vec![
                                Tag::Float((yaw - std::f32::consts::PI).to_degrees()),
                                Tag::Float(0.),
                            ],
                        },
                    ),
                ]
                .into();
                entity.extend(properties.clone());
                entity.insert("id".into(), Tag::String(kind.name().into()));
                let Some(mut imported) = item_nbt::encode(&Tag::Compound(entity))
                    .ok()
                    .and_then(|bytes| crate::nbt::read(&bytes).ok())
                    .and_then(|tag| crate::villagers::read_entity(&tag, (p.x >> 4, p.z >> 4), 0))
                else {
                    continue;
                };
                self.next_id = self.next_id.wrapping_add(1);
                imported.mob.id = self.next_id;
                imported.install(self);
            } else {
                self.block_ticks.set_world_random(rng);
                self.spawn_registered(kind, position, yaw);
                *rng = self.block_ticks.world_random();
            }
            self.spawn_mob_particles();
            spawned = true;
        }
        spawned
    }

    fn spawner_allows(
        &self,
        world: &World,
        p: IVec3,
        kind: MobKind,
        difficulty: u8,
        sky_loss: u8,
        rng: &mut JavaRandom,
    ) -> bool {
        let state = world.get(p);
        let water = matches!(state >> 4, 8 | 9);
        if kind == MobKind::Squid {
            return p.y > 45 && p.y < 63;
        }
        if kind.hostile() && difficulty == 0 && kind != MobKind::EnderDragon {
            return false;
        }
        let light = world.game_light(p, sky_loss, true, self.dimension == 0);
        let brightness =
            crate::vanilla::lightmap::brightness_table(self.dimension)[usize::from(light)];
        let floor = world.get(p - IVec3::Y) >> 4;
        match kind {
            MobKind::Pig
            | MobKind::Cow
            | MobKind::Chicken
            | MobKind::Sheep
            | MobKind::Rabbit
            | MobKind::Wolf
            | MobKind::Horse => floor == 2 && light > 8,
            MobKind::Mooshroom => floor == 110 && light > 8,
            MobKind::Ocelot => p.y >= 63 && matches!(floor, 2 | 18 | 161) && rng.next_int(3) != 0,
            MobKind::Bat => p.y < 63 && !rng.next_boolean() && light <= rng.next_int(4) as u8,
            MobKind::Ghast => rng.next_int(20) == 0,
            MobKind::PigZombie | MobKind::MagmaCube | MobKind::EnderDragon => true,
            MobKind::Blaze | MobKind::Endermite => brightness <= 0.5,
            MobKind::Silverfish => floor == 1 || brightness <= 0.5,
            MobKind::Guardian => {
                let sky = world.can_see_sky(p.with_y(p.y.max(63)))
                    && (p.y + 1..63).all(|y| {
                        let state = world.get(p.with_y(y));
                        material::light_opacity(state) == 0 || material::is_liquid(state)
                    });
                (water || brightness <= 0.5) && (rng.next_int(20) == 0 || !sky)
            }
            MobKind::Slime => {
                if self.flat_world && rng.next_int(4) != 1 {
                    return false;
                }
                let moon =
                    [1., 0.75, 0.5, 0.25, 0., 0.25, 0.5, 0.75][(self.clock / 24000 % 8) as usize];
                (crate::assets::biome_colors::biome_at(world, p) == 6
                    && (51..70).contains(&p.y)
                    && rng.next_float() < 0.5
                    && rng.next_float() < moon
                    && light <= rng.next_int(8) as u8)
                    || (rng.next_int(10) == 0
                        && super::natural_mobs::slime_chunk(
                            p,
                            self.world_commands.imported_seed.unwrap_or(self.spawn_seed),
                        )
                        && p.y < 40)
            }
            _ if kind.hostile() => {
                (self.dimension != 0 || world.light_uv(p, 0)[1] / 16 <= rng.next_int(32) as u8)
                    && light <= rng.next_int(8) as u8
                    && (kind != MobKind::Giant || brightness >= 0.5)
            }
            _ => true,
        }
    }
}
