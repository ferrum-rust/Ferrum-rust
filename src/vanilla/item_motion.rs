use super::*;
use crate::vanilla::{
    falling::{area_loaded, flammable},
    movement::{self, collision, MovementWorld},
};
use glam::IVec3;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ItemEffect {
    Sound {
        position: DVec3,
        name: String,
        volume: f32,
        pitch: f32,
    },
    Particle {
        position: DVec3,
        name: String,
        velocity: DVec3,
    },
}
pub trait ItemWorld: MovementWorld {
    fn is_remote(&self)->bool{false}
    fn full_collision_cube(&mut self, p: IVec3) -> Result<bool>;
    fn merge_nearby(&mut self, source: &mut ItemEntity) -> Result<()>;
    fn contact(&mut self, p: IVec3, state: u16, source: &mut ItemEntity) -> Result<()>;
    fn raining_at(&self, p: IVec3) -> bool;
    fn effect(&mut self, effect: ItemEffect);
}

/// Entity.pushOutOfBlocks: the existing-state collision shape chooses the exit.
pub fn push_out_of_blocks<W: MovementWorld>(
    world: &mut W,
    bounds: Aabb,
    mid: DVec3,
    velocity: &mut DVec3,
    random: &mut JavaRandom,
    mut full: impl FnMut(&mut W, IVec3) -> Result<bool>,
) -> Result<bool> {
    let p = mid.floor().as_ivec3();
    if collision::colliders(world, bounds).is_empty() && !full(world, p)? {
        return Ok(false);
    }
    let local = mid - p.as_dvec3();
    let mut best = 9999.;
    let mut push = IVec3::Y;
    for (direction, distance) in [
        (IVec3::NEG_X, local.x),
        (IVec3::X, 1. - local.x),
        (IVec3::Y, 1. - local.y),
        (IVec3::NEG_Z, local.z),
        (IVec3::Z, 1. - local.z),
    ] {
        if !full(world, p + direction)? && distance < best {
            best = distance;
            push = direction;
        }
    }
    let impulse = (random.next_float() * 0.2 + 0.1) as f64;
    if push.x != 0 {
        velocity.x = f64::from(push.x) * impulse;
    }
    if push.y != 0 {
        velocity.y = impulse;
    }
    if push.z != 0 {
        velocity.z = f64::from(push.z) * impulse;
    }
    Ok(true)
}

impl ItemEntity {
    pub fn tick(&mut self, world: &mut impl ItemWorld) -> Result<()> {
        self.ticks = self.ticks.wrapping_add(1);
        self.previous = self.position;
        self.handle_water(world);
        if world.is_remote() {
            self.fire=0;
        } else if self.fire > 0 {
            if self.fire % 20 == 0 {
                self.damage(1.);
            }
            self.fire -= 1;
        }
        if movement::touches_liquid(
            world,
            self.bounds
                .expand(DVec3::new(-0.1f32 as f64, -0.4f32 as f64, -0.1f32 as f64)),
            10,
            11,
        ) {
            self.damage(4.);
            self.fire = self.fire.max(300);
            self.fall_distance *= 0.5;
        }
        if self.position.y < -64. {
            self.dead = true;
        }
        self.first_update = false;
        if self.pickup_delay > 0 && self.pickup_delay != 32767 {
            self.pickup_delay -= 1;
        }
        self.previous = self.position;
        self.velocity.y -= 0.04f32 as f64;
        self.no_clip = self.push_out(world)?;
        self.move_body(world)?;
        let crossed = self.previous.to_array().map(|v| v as i32)
            != self.position.to_array().map(|v| v as i32);
        if crossed || self.ticks % 25 == 0 {
            if matches!(world.get(self.position.floor().as_ivec3()) >> 4, 10 | 11) {
                self.velocity.y = 0.2f32 as f64;
                let mut r = JavaRandom::from_state(self.random_state);
                self.velocity.x = ((r.next_float() - r.next_float()) * 0.2) as f64;
                self.velocity.z = ((r.next_float() - r.next_float()) * 0.2) as f64;
                let pitch = 2. + r.next_float() * 0.4;
                self.random_state = r.state();
                self.sound(world, "random.fizz", 0.4, pitch);
            }
            if !world.is_remote(){world.merge_nearby(self)?;}
        }
        let friction = if self.grounded {
            let below = IVec3::new(
                self.position.x.floor() as i32,
                self.bounds.min.y.floor() as i32 - 1,
                self.position.z.floor() as i32,
            );
            let slip: f32 = match world.get(below) >> 4 {
                79 | 174 => 0.98,
                165 => 0.8,
                _ => 0.6,
            };
            slip * 0.98
        } else {
            0.98f32
        };
        self.velocity.x *= friction as f64;
        self.velocity.y *= 0.98f32 as f64;
        self.velocity.z *= friction as f64;
        if self.grounded {
            self.velocity.y *= -0.5;
        }
        if self.age != -32768 {
            self.age = self.age.wrapping_add(1);
        }
        self.handle_water(world);
        if !world.is_remote() && self.age >= 6000 {
            self.dead = true;
        }
        Ok(())
    }
    fn damage(&mut self, amount: f32) {
        self.health = (self.health as f32 - amount) as i32;
        if self.health <= 0 {
            self.dead = true;
        }
    }
    fn sound(&self, world: &mut impl ItemWorld, name: &str, volume: f32, pitch: f32) {
        if !self.silent {
            world.effect(ItemEffect::Sound {
                position: self.position,
                name: name.into(),
                volume,
                pitch,
            });
        }
    }
    fn handle_water(&mut self, world: &mut impl ItemWorld) {
        let (wet, flow) = movement::water_acceleration(world, self.bounds);
        if let Some(flow) = flow {
            self.velocity += flow * 0.014;
        }
        if wet && !self.in_water && !self.first_update {
            self.splash(world);
        }
        self.in_water = wet;
    }
    fn splash(&mut self, world: &mut impl ItemWorld) {
        let v = self.velocity;
        let speed =
            (v.x * v.x * 0.2f32 as f64 + v.y * v.y + v.z * v.z * 0.2f32 as f64).sqrt() as f32;
        let mut r = JavaRandom::from_state(self.random_state);
        let pitch = 1. + (r.next_float() - r.next_float()) * 0.4;
        self.sound(
            world,
            "game.neutral.swim.splash",
            (speed * 0.2).min(1.),
            pitch,
        );
        let y = (self.bounds.min.y.floor() as f32 + 1.) as f64;
        for name in ["WATER_BUBBLE", "WATER_SPLASH"] {
            for _ in 0..6 {
                let x = self.position.x + ((r.next_float() * 2. - 1.) * 0.25) as f64;
                let z = self.position.z + ((r.next_float() * 2. - 1.) * 0.25) as f64;
                let mut velocity = v;
                if name == "WATER_BUBBLE" {
                    velocity.y -= (r.next_float() * 0.2) as f64;
                }
                world.effect(ItemEffect::Particle {
                    position: DVec3::new(x, y, z),
                    name: name.into(),
                    velocity,
                });
            }
        }
        self.random_state = r.state();
    }
    fn push_out(&mut self, world: &mut impl ItemWorld) -> Result<bool> {
        let mid = DVec3::new(
            self.position.x,
            (self.bounds.min.y + self.bounds.max.y) / 2.,
            self.position.z,
        );
        let mut random = JavaRandom::from_state(self.random_state);
        let pushed = push_out_of_blocks(
            world,
            self.bounds,
            mid,
            &mut self.velocity,
            &mut random,
            |world, p| world.full_collision_cube(p),
        )?;
        self.random_state = random.state();
        Ok(pushed)
    }
    fn move_body(&mut self, world: &mut impl ItemWorld) -> Result<()> {
        let mut delta = self.velocity;
        if self.no_clip {
            self.bounds = self.bounds.offset(delta);
            self.position = self.bounds.feet();
            return Ok(());
        }
        if self.in_web {
            self.in_web = false;
            delta *= DVec3::new(0.25, 0.05f32 as f64, 0.25);
            self.velocity = DVec3::ZERO;
        }
        let m = collision::move_body_with_step(world, self.bounds, delta, self.grounded, false, 0.);
        self.bounds = m.bounds;
        self.position = m.bounds.feet();
        self.grounded = m.grounded;
        self.clipped_horizontal = m.clipped[0] || m.clipped[2];
        self.clipped_vertical = m.clipped[1];
        let p = IVec3::new(
            self.position.x.floor() as i32,
            (self.position.y - 0.2f32 as f64).floor() as i32,
            self.position.z.floor() as i32,
        );
        let mut under = world.get(p);
        if under >> 4 == 0 {
            let below = world.get(p - IVec3::Y);
            if matches!(below >> 4, 85 | 107 | 113 | 139 | 183..=192) {
                under = below;
            }
        }
        if self.grounded {
            self.fall_distance = 0.;
        } else if m.delta.y < 0. {
            self.fall_distance = (self.fall_distance as f64 - m.delta.y) as f32;
        }
        if m.clipped[0] {
            self.velocity.x = 0.;
        }
        if m.clipped[2] {
            self.velocity.z = 0.;
        }
        if m.clipped[1] {
            if under >> 4 == 165 {
                if self.velocity.y < 0. {
                    self.velocity.y = -self.velocity.y;
                }
            } else {
                self.velocity.y = 0.;
            }
        }
        let min = (self.bounds.min + DVec3::splat(0.001)).floor().as_ivec3();
        let max = (self.bounds.max - DVec3::splat(0.001)).floor().as_ivec3();
        if area_loaded(world, min, max) {
            for x in min.x..=max.x {
                for y in min.y..=max.y {
                    for z in min.z..=max.z {
                        let p = IVec3::new(x, y, z);
                        let state = world.get(p);
                        match state >> 4 {
                            30 => {
                                self.in_web = true;
                                self.fall_distance = 0.;
                            }
                            88 => {
                                self.velocity.x *= 0.4;
                                self.velocity.z *= 0.4;
                            }
                            81 => self.damage(1.),
                            _ => {}
                        }
                        world.contact(p, state, self)?;
                    }
                }
            }
        }
        let wet = self.in_water
            || world.raining_at(self.position.floor().as_ivec3())
            || world.raining_at((self.position + DVec3::Y * 0.25).floor().as_ivec3());
        if flammable(world, self.bounds.expand(DVec3::splat(-0.001))) {
            self.damage(1.);
            if !wet {
                self.fire = self.fire.wrapping_add(1);
                if self.fire == 0 {
                    self.fire = 160;
                }
            }
        } else if self.fire <= 0 {
            self.fire = -1;
        }
        if wet && self.fire > 0 {
            let mut r = JavaRandom::from_state(self.random_state);
            let pitch = 1.6 + (r.next_float() - r.next_float()) * 0.4;
            self.random_state = r.state();
            self.sound(world, "random.fizz", 0.7, pitch);
            self.fire = -1;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::io::Read;
    struct World {
        layout: i32,
        time: i32,
        events: Vec<Value>,
    }
    fn bits(v: impl IntoIterator<Item = f64>) -> Vec<String> {
        v.into_iter()
            .map(|v| format!("{:x}", v.to_bits()))
            .collect()
    }
    impl MovementWorld for World {
        fn get(&self, p: IVec3) -> u16 {
            let s = self.layout;
            if !(0..256).contains(&p.y) || s == 13 {
                return 0;
            }
            if p.y == 0 {
                return (match s {
                    1 => 79,
                    2 => 174,
                    3 => 165,
                    4 => 88,
                    5 => 44,
                    _ => 1,
                }) << 4;
            }
            match s {
                6 if (2..=4).contains(&p.y) => 30 << 4,
                7 if p.y <= 3 => 9 << 4,
                8 if p.y <= 3 => 8 << 4 | p.x.rem_euclid(8) as u16,
                9 if p.y <= 3 => 10 << 4,
                10 | 14 if p.y <= 3 => 51 << 4,
                11 if p.x == 8 && p.z == 8 && (2..=4).contains(&p.y) => 16,
                12 if p.x == 0 && p.y <= 6 => 16,
                15 if p.x == 8 && p.z == 8 && p.y <= 3 => 81 << 4,
                _ => 0,
            }
        }
        fn block_boxes(&self, p: IVec3, state: u16, out: &mut Vec<Aabb>) {
            crate::vanilla::collision_shapes::emit(
                p,
                state,
                |q| self.get(q),
                None,
                |b| out.push(b),
            );
        }
        fn loaded_column(&self, _x: i32, _z: i32) -> bool {
            true
        }
    }
    impl ItemWorld for World {
        fn full_collision_cube(&mut self, p: IVec3) -> Result<bool> {
            let v = crate::vanilla::collision_shapes::full_collision_cube(
                p,
                self.get(p),
                |q| self.get(q),
                None,
            );
            self.events.push(json!(["full", p.x, p.y, p.z, v]));
            Ok(v)
        }
        fn merge_nearby(&mut self, e: &mut ItemEntity) -> Result<()> {
            let b = e.bounds.expand(DVec3::new(0.5, 0., 0.5));
            self.events.push(json!([
                "merge",
                bits(b.min.to_array().into_iter().chain(b.max.to_array()))
            ]));
            Ok(())
        }
        fn contact(&mut self, _p: IVec3, _s: u16, _e: &mut ItemEntity) -> Result<()> {
            Ok(())
        }
        fn raining_at(&self, _p: IVec3) -> bool {
            self.layout == 10 && self.time > 4
        }
        fn effect(&mut self, e: ItemEffect) {
            self.events.push(match e {
                ItemEffect::Sound {
                    name,
                    volume,
                    pitch,
                    ..
                } => json!(["sound", name, volume.to_bits(), pitch.to_bits()]),
                ItemEffect::Particle {
                    name,
                    position,
                    velocity,
                } => json!([
                    "particle",
                    name,
                    bits(position.to_array().into_iter().chain(velocity.to_array()))
                ]),
            });
        }
    }
    #[test]
    fn original_item_entity_motion_fire_water_particle_merge_queries_and_rng_match() {
        let mut bytes = Vec::new();
        flate2::read::GzDecoder::new(
            &include_bytes!("../../tests/fixtures/vanilla189-item-motion.json.gz")[..],
        )
        .read_to_end(&mut bytes)
        .unwrap();
        let cases: Value = serde_json::from_slice(&bytes).unwrap();
        for row in cases.as_array().unwrap() {
            let seed: i64 = row["seed"].as_str().unwrap().parse().unwrap();
            let position =
                DVec3::from_array(serde_json::from_value(row["position"].clone()).unwrap());
            let mut e = ItemEntity::new(
                0,
                position,
                Stack::new(264, 3),
                &mut JavaRandom::new(seed ^ 137),
                JavaRandom::new(seed).state(),
            );
            e.velocity = DVec3::from_array(serde_json::from_value(row["motion"].clone()).unwrap());
            e.age = row["age"].as_i64().unwrap() as i32;
            e.pickup_delay = row["delay"].as_i64().unwrap() as i32;
            e.fire = row["fire"].as_i64().unwrap() as i32;
            let mut w = World {
                layout: row["scenario"].as_i64().unwrap() as i32,
                time: 0,
                events: Vec::new(),
            };
            for frame in row["frames"].as_array().unwrap() {
                w.time += 1;
                w.events.clear();
                e.tick(&mut w).unwrap();
                let coords = [
                    e.position,
                    e.previous,
                    e.velocity,
                    e.bounds.min,
                    e.bounds.max,
                ]
                .into_iter()
                .flat_map(|v| v.to_array());
                let actual = json!([
                    e.ticks,
                    bits(coords),
                    [
                        e.grounded,
                        e.clipped_horizontal,
                        e.clipped_vertical,
                        e.no_clip,
                        e.in_water,
                        e.in_web,
                        e.first_update,
                        e.dead
                    ],
                    e.age,
                    e.pickup_delay,
                    e.health,
                    e.fire,
                    e.fall_distance.to_bits(),
                    e.random_state.to_string(),
                    w.events
                ]);
                assert_eq!(
                    actual, *frame,
                    "item scenario {} mode {} tick {}",
                    w.layout, row["mode"], w.time
                );
                if w.time == 5 {
                    e = serde_json::from_slice(&serde_json::to_vec(&e).unwrap()).unwrap();
                }
            }
        }
    }
}
