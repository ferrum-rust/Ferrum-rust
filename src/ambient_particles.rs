//! Vanilla 1.8.9 client-side ambient block particles (`WorldClient.doVoidFogParticles`),
//! active mob spawner flames (`MobSpawnerBaseLogic`), and rain ground splashes
//! (`EntityRenderer.addRainParticles`).

use crate::{
    gameplay::Particle,
    vanilla::{
        material,
        random::JavaRandom,
        runtime::biome_can_rain,
        spawning::solid_top,
        weather,
    },
    world::World,
};
use glam::{DVec3, IVec3};

const MAX_PARTICLES: usize = 4000;

#[derive(Clone, Debug, PartialEq)]
pub struct AmbientSound {
    pub name: &'static str,
    pub pos: DVec3,
    pub volume: f32,
    pub pitch: f32,
}

fn can_emit_particle(
    particles: &[Particle],
    particle_setting: u8,
    rng: &mut JavaRandom,
) -> bool {
    if particles.len() >= MAX_PARTICLES {
        return false;
    }
    match particle_setting {
        0 => true,
        1 => rng.next_int(3) != 0,
        _ => false,
    }
}

fn emit_sprite(
    particles: &mut Vec<Particle>,
    particle_setting: u8,
    rng: &mut JavaRandom,
    id: u8,
    pos: DVec3,
    vel: DVec3,
) {
    if !can_emit_particle(particles, particle_setting, rng) {
        return;
    }
    if let Some(p) = Particle::network_sprite(id, pos, vel, rng) {
        particles.push(p);
    }
}

fn block_top_y(state: u16) -> f64 {
    let id = state >> 4;
    let meta = state & 15;
    match id {
        8..=11 => {
            let level = if meta >= 8 { 0 } else { meta & 7 };
            1.0 - f64::from(level + 1) / 9.0
        }
        44 | 126 | 182 if meta & 8 == 0 => 0.5,
        78 => f64::from((meta & 7) + 1) / 8.0,
        88 => 0.875,
        92 => 0.5,
        93 | 94 | 149 | 150 => 0.125,
        116 | 120 => 0.75,
        140 => 0.375,
        151 | 178 => 0.375,
        171 => 0.0625,
        _ => 1.0,
    }
}

fn is_rain_exposed(world: &World, pos: IVec3) -> bool {
    let precip_y = weather::precipitation_height(world, pos.x, pos.z);
    if precip_y < 0 || precip_y > pos.y {
        return false;
    }
    let biome_id = crate::assets::biome_colors::biome_at(world, IVec3::new(pos.x, 0, pos.z));
    biome_can_rain(biome_id) && !weather::snow_at(biome_id, pos)
}

pub fn tick_block_and_rain_particles(
    world: &World,
    player_feet: DVec3,
    camera_eye: DVec3,
    creative_holding_barrier: bool,
    raining: bool,
    rain_strength: f32,
    particle_setting: u8,
    fancy_graphics: bool,
    tick_counter: u64,
    particles: &mut Vec<Particle>,
    sounds: &mut Vec<AmbientSound>,
) {
    let seed = (tick_counter as i64)
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    let mut rng = JavaRandom::new(seed);

    // 1. WorldClient.doVoidFogParticles: 1000 random samples in +/-16 triangular distribution.
    let origin = player_feet.floor().as_ivec3();
    for _ in 0..1000 {
        let offset = IVec3::new(
            rng.next_int(16) - rng.next_int(16),
            rng.next_int(16) - rng.next_int(16),
            rng.next_int(16) - rng.next_int(16),
        );
        let pos = origin + offset;
        if !(0..256).contains(&pos.y) {
            continue;
        }
        let state = world.get(pos);
        let id = state >> 4;
        if id == 0 {
            continue;
        }
        let meta = state & 15;
        let base = pos.as_dvec3();
        let center = base + DVec3::splat(0.5);

        match id {
            166 => {
                if creative_holding_barrier && can_emit_particle(particles, particle_setting, &mut rng)
                {
                    particles.push(Particle::item(166, 0, true, center, DVec3::ZERO, &mut rng));
                }
            }
            50 => {
                let (dx, dy, dz) = match meta {
                    1 => (-0.27, 0.22, 0.0),
                    2 => (0.27, 0.22, 0.0),
                    3 => (0.0, 0.22, -0.27),
                    4 => (0.0, 0.22, 0.27),
                    _ => (0.0, 0.0, 0.0),
                };
                let p = DVec3::new(base.x + 0.5 + dx, base.y + 0.7 + dy, base.z + 0.5 + dz);
                emit_sprite(particles, particle_setting, &mut rng, 11, p, DVec3::ZERO);
                emit_sprite(particles, particle_setting, &mut rng, 26, p, DVec3::ZERO);
            }
            76 => {
                let jx = (rng.next_double() - 0.5) * 0.2;
                let jy = (rng.next_double() - 0.5) * 0.2;
                let jz = (rng.next_double() - 0.5) * 0.2;
                let (dx, dy, dz) = match meta {
                    1 => (-0.27, 0.22, 0.0),
                    2 => (0.27, 0.22, 0.0),
                    3 => (0.0, 0.22, -0.27),
                    4 => (0.0, 0.22, 0.27),
                    _ => (0.0, 0.0, 0.0),
                };
                let p = DVec3::new(
                    base.x + 0.5 + jx + dx,
                    base.y + 0.7 + jy + dy,
                    base.z + 0.5 + jz + dz,
                );
                emit_sprite(particles, particle_setting, &mut rng, 30, p, DVec3::ZERO);
            }
            62 => {
                let cy = base.y + rng.next_double() * 6.0 / 16.0;
                let d3 = 0.52;
                let d4 = rng.next_double() * 0.6 - 0.3;
                let p = match meta & 7 {
                    4 => DVec3::new(base.x + 0.5 - d3, cy, base.z + 0.5 + d4),
                    5 => DVec3::new(base.x + 0.5 + d3, cy, base.z + 0.5 + d4),
                    2 => DVec3::new(base.x + 0.5 + d4, cy, base.z + 0.5 - d3),
                    _ => DVec3::new(base.x + 0.5 + d4, cy, base.z + 0.5 + d3),
                };
                emit_sprite(particles, particle_setting, &mut rng, 11, p, DVec3::ZERO);
                emit_sprite(particles, particle_setting, &mut rng, 26, p, DVec3::ZERO);
            }
            51 => {
                if rng.next_int(24) == 0 {
                    sounds.push(AmbientSound {
                        name: "fire.fire",
                        pos: center,
                        volume: 1.0 + rng.next_float(),
                        pitch: rng.next_float() * 0.7 + 0.3,
                    });
                }
                let down = world.get(pos - IVec3::Y);
                if solid_top(down) {
                    for _ in 0..3 {
                        let p = DVec3::new(
                            base.x + rng.next_double(),
                            base.y + rng.next_double() * 0.5 + 0.5,
                            base.z + rng.next_double(),
                        );
                        emit_sprite(particles, particle_setting, &mut rng, 12, p, DVec3::ZERO);
                    }
                } else {
                    let mut any_wall = false;
                    if (world.get(pos - IVec3::X) >> 4) != 0 {
                        any_wall = true;
                        for _ in 0..2 {
                            let p = DVec3::new(
                                base.x + rng.next_double() * 0.1,
                                base.y + rng.next_double(),
                                base.z + rng.next_double(),
                            );
                            emit_sprite(particles, particle_setting, &mut rng, 12, p, DVec3::ZERO);
                        }
                    }
                    if (world.get(pos + IVec3::X) >> 4) != 0 {
                        any_wall = true;
                        for _ in 0..2 {
                            let p = DVec3::new(
                                base.x + 1.0 - rng.next_double() * 0.1,
                                base.y + rng.next_double(),
                                base.z + rng.next_double(),
                            );
                            emit_sprite(particles, particle_setting, &mut rng, 12, p, DVec3::ZERO);
                        }
                    }
                    if (world.get(pos - IVec3::Z) >> 4) != 0 {
                        any_wall = true;
                        for _ in 0..2 {
                            let p = DVec3::new(
                                base.x + rng.next_double(),
                                base.y + rng.next_double(),
                                base.z + rng.next_double() * 0.1,
                            );
                            emit_sprite(particles, particle_setting, &mut rng, 12, p, DVec3::ZERO);
                        }
                    }
                    if (world.get(pos + IVec3::Z) >> 4) != 0 {
                        any_wall = true;
                        for _ in 0..2 {
                            let p = DVec3::new(
                                base.x + rng.next_double(),
                                base.y + rng.next_double(),
                                base.z + 1.0 - rng.next_double() * 0.1,
                            );
                            emit_sprite(particles, particle_setting, &mut rng, 12, p, DVec3::ZERO);
                        }
                    }
                    if (world.get(pos + IVec3::Y) >> 4) != 0 || !any_wall {
                        for _ in 0..2 {
                            let p = DVec3::new(
                                base.x + rng.next_double(),
                                base.y + 1.0 - rng.next_double() * 0.1,
                                base.z + rng.next_double(),
                            );
                            emit_sprite(particles, particle_setting, &mut rng, 12, p, DVec3::ZERO);
                        }
                    }
                }
            }
            90 => {
                if rng.next_int(100) == 0 {
                    sounds.push(AmbientSound {
                        name: "portal.portal",
                        pos: center,
                        volume: 0.5,
                        pitch: rng.next_float() * 0.4 + 0.8,
                    });
                }
                let x_neighbors_portal = (world.get(pos - IVec3::X) >> 4) == 90
                    || (world.get(pos + IVec3::X) >> 4) == 90;
                for _ in 0..4 {
                    let mut px = base.x + f64::from(rng.next_float());
                    let py = base.y + f64::from(rng.next_float());
                    let mut pz = base.z + f64::from(rng.next_float());
                    let mut vx = (f64::from(rng.next_float()) - 0.5) * 0.5;
                    let vy = (f64::from(rng.next_float()) - 0.5) * 0.5;
                    let mut vz = (f64::from(rng.next_float()) - 0.5) * 0.5;
                    let dir = rng.next_int(2) * 2 - 1;
                    if !x_neighbors_portal {
                        px = base.x + 0.5 + 0.25 * f64::from(dir);
                        vx = f64::from(rng.next_float() * 2.0 * dir as f32);
                    } else {
                        pz = base.z + 0.5 + 0.25 * f64::from(dir);
                        vz = f64::from(rng.next_float() * 2.0 * dir as f32);
                    }
                    emit_sprite(
                        particles,
                        particle_setting,
                        &mut rng,
                        24,
                        DVec3::new(px, py, pz),
                        DVec3::new(vx, vy, vz),
                    );
                }
            }
            116 => {
                for dx in -2i32..=2 {
                    for dz in -2i32..=2 {
                        if (-1..=1).contains(&dx) && (-1..=1).contains(&dz) {
                            continue;
                        }
                        if rng.next_int(16) != 0 {
                            continue;
                        }
                        for dy in 0..=1 {
                            if (world.get(pos + IVec3::new(dx, dy, dz)) >> 4) == 47
                                && (world.get(pos + IVec3::new(dx / 2, 0, dz / 2)) >> 4) == 0
                            {
                                let table_top = DVec3::new(base.x + 0.5, base.y + 2.0, base.z + 0.5);
                                let vel = DVec3::new(
                                    f64::from(dx) + f64::from(rng.next_float()) - 0.5,
                                    f64::from(dy) - f64::from(rng.next_float()) - 1.0,
                                    f64::from(dz) + f64::from(rng.next_float()) - 0.5,
                                );
                                emit_sprite(
                                    particles,
                                    particle_setting,
                                    &mut rng,
                                    25,
                                    table_top,
                                    vel,
                                );
                            }
                        }
                    }
                }
            }
            55 if meta > 0 => {
                let f = f32::from(meta) / 15.0;
                let r = f * 0.6 + 0.4;
                let g = (f * f * 0.7 - 0.5).max(0.0);
                let b = (f * f * 0.6 - 0.7).max(0.0);
                let p = DVec3::new(
                    base.x + 0.5 + (f64::from(rng.next_float()) - 0.5) * 0.2,
                    base.y + 0.0625,
                    base.z + 0.5 + (f64::from(rng.next_float()) - 0.5) * 0.2,
                );
                emit_sprite(
                    particles,
                    particle_setting,
                    &mut rng,
                    30,
                    p,
                    DVec3::new(f64::from(r), f64::from(g), f64::from(b)),
                );
            }
            94 => {
                let cx = base.x + 0.5 + (f64::from(rng.next_float()) - 0.5) * 0.2;
                let cy = base.y + 0.4 + (f64::from(rng.next_float()) - 0.5) * 0.2;
                let cz = base.z + 0.5 + (f64::from(rng.next_float()) - 0.5) * 0.2;
                let delay = f64::from(((meta >> 2) & 3) + 1);
                let f = if rng.next_int(2) != 0 {
                    (delay * 2.0 - 1.0) / 16.0
                } else {
                    -5.0 / 16.0
                };
                let (dx, dz) = match meta & 3 {
                    0 => (0.0, f),
                    1 => (-f, 0.0),
                    2 => (0.0, -f),
                    _ => (f, 0.0),
                };
                emit_sprite(
                    particles,
                    particle_setting,
                    &mut rng,
                    30,
                    DVec3::new(cx + dx, cy, cz + dz),
                    DVec3::ZERO,
                );
            }
            74 => {
                for face in 0..6 {
                    let mut px = base.x + f64::from(rng.next_float());
                    let mut py = base.y + f64::from(rng.next_float());
                    let mut pz = base.z + f64::from(rng.next_float());
                    let neighbor = match face {
                        0 => {
                            py = base.y - 0.0625;
                            pos - IVec3::Y
                        }
                        1 => {
                            py = base.y + 1.0625;
                            pos + IVec3::Y
                        }
                        2 => {
                            pz = base.z - 0.0625;
                            pos - IVec3::Z
                        }
                        3 => {
                            pz = base.z + 1.0625;
                            pos + IVec3::Z
                        }
                        4 => {
                            px = base.x - 0.0625;
                            pos - IVec3::X
                        }
                        _ => {
                            px = base.x + 1.0625;
                            pos + IVec3::X
                        }
                    };
                    if !material::opaque(world.get(neighbor)) {
                        emit_sprite(
                            particles,
                            particle_setting,
                            &mut rng,
                            30,
                            DVec3::new(px, py, pz),
                            DVec3::ZERO,
                        );
                    }
                }
            }
            8 | 9 => {
                if (1..8).contains(&meta) {
                    if rng.next_int(64) == 0 {
                        sounds.push(AmbientSound {
                            name: "liquid.water",
                            pos: center,
                            volume: rng.next_float() * 0.25 + 0.75,
                            pitch: rng.next_float() + 0.5,
                        });
                    }
                } else if rng.next_int(10) == 0 {
                    let p = DVec3::new(
                        base.x + f64::from(rng.next_float()),
                        base.y + f64::from(rng.next_float()),
                        base.z + f64::from(rng.next_float()),
                    );
                    emit_sprite(particles, particle_setting, &mut rng, 7, p, DVec3::ZERO);
                }
                if rng.next_int(10) == 0 && solid_top(world.get(pos - IVec3::Y)) {
                    let below2 = world.get(pos - IVec3::Y * 2);
                    if !material::blocks_movement(below2) && !material::is_liquid(below2) {
                        let p = DVec3::new(
                            base.x + f64::from(rng.next_float()),
                            base.y - 1.05,
                            base.z + f64::from(rng.next_float()),
                        );
                        emit_sprite(particles, particle_setting, &mut rng, 18, p, DVec3::ZERO);
                    }
                }
            }
            10 | 11 => {
                let above = world.get(pos + IVec3::Y);
                if !material::opaque(above) && !material::is_liquid(above) {
                    if rng.next_int(100) == 0 {
                        let sp = DVec3::new(
                            base.x + f64::from(rng.next_float()),
                            base.y + 1.0,
                            base.z + f64::from(rng.next_float()),
                        );
                        emit_sprite(particles, particle_setting, &mut rng, 27, sp, DVec3::ZERO);
                        sounds.push(AmbientSound {
                            name: "liquid.lavapop",
                            pos: sp,
                            volume: 0.2 + rng.next_float() * 0.2,
                            pitch: 0.9 + rng.next_float() * 0.15,
                        });
                    }
                    if rng.next_int(200) == 0 {
                        sounds.push(AmbientSound {
                            name: "liquid.lava",
                            pos: center,
                            volume: 0.2 + rng.next_float() * 0.2,
                            pitch: 0.9 + rng.next_float() * 0.15,
                        });
                    }
                }
                if rng.next_int(10) == 0 && solid_top(world.get(pos - IVec3::Y)) {
                    let below2 = world.get(pos - IVec3::Y * 2);
                    if !material::blocks_movement(below2) && !material::is_liquid(below2) {
                        let p = DVec3::new(
                            base.x + f64::from(rng.next_float()),
                            base.y - 1.05,
                            base.z + f64::from(rng.next_float()),
                        );
                        emit_sprite(particles, particle_setting, &mut rng, 19, p, DVec3::ZERO);
                    }
                }
            }
            18 | 161 => {
                if (raining || rain_strength > 0.2)
                    && rain_strength > 0.2
                    && is_rain_exposed(world, pos + IVec3::Y)
                    && !solid_top(world.get(pos - IVec3::Y))
                    && rng.next_int(15) == 1
                {
                    let p = DVec3::new(
                        base.x + f64::from(rng.next_float()),
                        base.y - 0.05,
                        base.z + f64::from(rng.next_float()),
                    );
                    emit_sprite(particles, particle_setting, &mut rng, 18, p, DVec3::ZERO);
                }
            }
            19 if meta == 1 => {
                let face = rng.next_int(6);
                if face != 1 {
                    let dir = match face {
                        0 => -IVec3::Y,
                        2 => -IVec3::Z,
                        3 => IVec3::Z,
                        4 => -IVec3::X,
                        _ => IVec3::X,
                    };
                    if face == 0 || !solid_top(world.get(pos + dir)) {
                        let p = match face {
                            0 => DVec3::new(
                                base.x + rng.next_double(),
                                base.y - 0.05,
                                base.z + rng.next_double(),
                            ),
                            2 => DVec3::new(
                                base.x + rng.next_double(),
                                base.y + rng.next_double() * 0.75 + 0.1,
                                base.z - 0.05,
                            ),
                            3 => DVec3::new(
                                base.x + rng.next_double(),
                                base.y + rng.next_double() * 0.75 + 0.1,
                                base.z + 1.05,
                            ),
                            4 => DVec3::new(
                                base.x - 0.05,
                                base.y + rng.next_double() * 0.75 + 0.1,
                                base.z + rng.next_double(),
                            ),
                            _ => DVec3::new(
                                base.x + 1.05,
                                base.y + rng.next_double() * 0.75 + 0.1,
                                base.z + rng.next_double(),
                            ),
                        };
                        emit_sprite(particles, particle_setting, &mut rng, 18, p, DVec3::ZERO);
                    }
                }
            }
            110 => {
                if rng.next_int(10) == 0 {
                    let p = DVec3::new(
                        base.x + f64::from(rng.next_float()),
                        base.y + 1.1,
                        base.z + f64::from(rng.next_float()),
                    );
                    emit_sprite(particles, particle_setting, &mut rng, 22, p, DVec3::ZERO);
                }
            }
            117 => {
                let p = DVec3::new(
                    base.x + 0.4 + f64::from(rng.next_float()) * 0.2,
                    base.y + 0.7 + f64::from(rng.next_float()) * 0.3,
                    base.z + 0.4 + f64::from(rng.next_float()) * 0.2,
                );
                emit_sprite(particles, particle_setting, &mut rng, 11, p, DVec3::ZERO);
            }
            130 => {
                for _ in 0..3 {
                    let j = rng.next_int(2) * 2 - 1;
                    let k = rng.next_int(2) * 2 - 1;
                    let px = base.x + 0.5 + 0.25 * f64::from(j);
                    let py = base.y + f64::from(rng.next_float());
                    let pz = base.z + 0.5 + 0.25 * f64::from(k);
                    let vx = f64::from(rng.next_float() * j as f32);
                    let vy = (f64::from(rng.next_float()) - 0.5) * 0.125;
                    let vz = f64::from(rng.next_float() * k as f32);
                    emit_sprite(
                        particles,
                        particle_setting,
                        &mut rng,
                        24,
                        DVec3::new(px, py, pz),
                        DVec3::new(vx, vy, vz),
                    );
                }
            }
            119 => {
                let p = DVec3::new(
                    base.x + f64::from(rng.next_float()),
                    base.y + 0.8,
                    base.z + f64::from(rng.next_float()),
                );
                emit_sprite(particles, particle_setting, &mut rng, 11, p, DVec3::ZERO);
            }
            52 => {
                let has_tile = world
                    .chunks
                    .get(&(pos.x.div_euclid(16), pos.z.div_euclid(16)))
                    .is_some_and(|c| {
                        c.tiles
                            .contains_key(&[pos.x.rem_euclid(16), pos.y, pos.z.rem_euclid(16)])
                    });
                if !has_tile && center.distance_squared(player_feet) < 256.0 {
                    let smoke_pos = DVec3::new(
                        base.x + f64::from(rng.next_float()),
                        base.y + f64::from(rng.next_float()),
                        base.z + f64::from(rng.next_float()),
                    );
                    let flame_pos = DVec3::new(
                        base.x + f64::from(rng.next_float()),
                        base.y + f64::from(rng.next_float()),
                        base.z + f64::from(rng.next_float()),
                    );
                    emit_sprite(particles, particle_setting, &mut rng, 11, smoke_pos, DVec3::ZERO);
                    emit_sprite(particles, particle_setting, &mut rng, 26, flame_pos, DVec3::ZERO);
                }
            }
            _ => {}
        }
    }

    // 2. MobSpawnerBaseLogic: active block 52 tiles within 16m in the 3x3 chunk neighborhood.
    let pcx = origin.x.div_euclid(16);
    let pcz = origin.z.div_euclid(16);
    for cz in pcz - 1..=pcz + 1 {
        for cx in pcx - 1..=pcx + 1 {
            let Some(chunk) = world.chunks.get(&(cx, cz)) else {
                continue;
            };
            for &[lx, y, lz] in chunk.tiles.keys() {
                if !(0..256).contains(&y) {
                    continue;
                }
                let pos = IVec3::new(cx * 16 + lx, y, cz * 16 + lz);
                if (world.get(pos) >> 4) != 52 {
                    continue;
                }
                let center = pos.as_dvec3() + DVec3::splat(0.5);
                if center.distance_squared(player_feet) < 256.0 {
                    let smoke_pos = DVec3::new(
                        f64::from(pos.x) + f64::from(rng.next_float()),
                        f64::from(pos.y) + f64::from(rng.next_float()),
                        f64::from(pos.z) + f64::from(rng.next_float()),
                    );
                    let flame_pos = DVec3::new(
                        f64::from(pos.x) + f64::from(rng.next_float()),
                        f64::from(pos.y) + f64::from(rng.next_float()),
                        f64::from(pos.z) + f64::from(rng.next_float()),
                    );
                    emit_sprite(particles, particle_setting, &mut rng, 11, smoke_pos, DVec3::ZERO);
                    emit_sprite(particles, particle_setting, &mut rng, 26, flame_pos, DVec3::ZERO);
                }
            }
        }
    }

    // 3. EntityRenderer.addRainParticles: ground rain splashes & lava rain sizzle.
    if rain_strength > 0.0 {
        let mut count =
            ((if fancy_graphics { 100.0 } else { 50.0 }) * rain_strength * rain_strength) as usize;
        if particle_setting == 1 {
            count >>= 1;
        } else if particle_setting >= 2 {
            count = 0;
        }
        let eye_block = camera_eye.floor().as_ivec3();
        let mut sound_candidate: Option<(DVec3, bool)> = None;
        let mut sound_hits = 0i32;

        for _ in 0..count {
            let x = eye_block.x + rng.next_int(21) - 10;
            let z = eye_block.z + rng.next_int(21) - 10;
            if !world.chunks.contains_key(&(x >> 4, z >> 4)) {
                continue;
            }
            let biome_id = crate::assets::biome_colors::biome_at(world, IVec3::new(x, 0, z));
            if !biome_can_rain(biome_id) {
                continue;
            }
            let precip_y = weather::precipitation_height(world, x, z);
            if precip_y <= 0 || precip_y > eye_block.y + 10 || precip_y < eye_block.y - 10 {
                continue;
            }
            if weather::snow_at(biome_id, IVec3::new(x, precip_y, z)) {
                continue;
            }
            let below = IVec3::new(x, precip_y - 1, z);
            let below_state = world.get(below);
            let at_state = world.get(IVec3::new(x, precip_y, z));
            let rx = f64::from(rng.next_float());
            let rz = f64::from(rng.next_float());

            if matches!(at_state >> 4, 10 | 11) || matches!(below_state >> 4, 10 | 11) {
                let lava_pos = if matches!(at_state >> 4, 10 | 11) {
                    IVec3::new(x, precip_y, z)
                } else {
                    below
                };
                let top = block_top_y(world.get(lava_pos));
                let p = DVec3::new(
                    f64::from(x) + rx,
                    f64::from(lava_pos.y) + top + 0.1,
                    f64::from(z) + rz,
                );
                if particles.len() < MAX_PARTICLES {
                    if let Some(smoke) = Particle::network_sprite(11, p, DVec3::ZERO, &mut rng) {
                        particles.push(smoke);
                    }
                }
            } else if material::blocks_movement(below_state) || material::is_liquid(below_state) {
                let top = block_top_y(below_state);
                let p = DVec3::new(
                    f64::from(x) + rx,
                    f64::from(below.y) + top + 0.1,
                    f64::from(z) + rz,
                );
                sound_hits += 1;
                if rng.next_int(sound_hits) == 0 {
                    let muffled = precip_y > eye_block.y + 1
                        && weather::precipitation_height(world, eye_block.x, eye_block.z)
                            > eye_block.y;
                    sound_candidate = Some((p, muffled));
                }
                if particles.len() < MAX_PARTICLES {
                    if let Some(drop) = Particle::network_sprite(39, p, DVec3::ZERO, &mut rng) {
                        particles.push(drop);
                    }
                }
            }
        }

        if let Some((pos, muffled)) = sound_candidate {
            if tick_counter % 3 == 0 && rng.next_int(3) == 0 {
                let (volume, pitch) = if muffled { (0.1, 0.5) } else { (0.2, 1.0) };
                sounds.push(AmbientSound {
                    name: "ambient.weather.rain",
                    pos,
                    volume,
                    pitch,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{vanilla::drops::DropNbt, world::Chunk};
    use std::sync::Arc;

    fn flat_stone_world() -> World {
        let mut world = World {
            chunks: Default::default(),
            spawn: glam::Vec3::ZERO,
            label: String::new(),
            save_path: Default::default(),
            edits: Default::default(),
            imported: false,
            light_revision: 0,
        };
        for cz in -1..=1 {
            for cx in -1..=1 {
                let mut chunk = Chunk::empty();
                for z in 0..16 {
                    for x in 0..16 {
                        chunk.set(x, 63, z, 1 << 4);
                    }
                }
                chunk.top = 64;
                world.chunks.insert((cx, cz), Arc::new(chunk));
            }
        }
        world
    }

    #[test]
    fn minimal_setting_suppresses_ambient_and_rain_particles() {
        let mut world = flat_stone_world();
        world.set(IVec3::new(1, 64, 1), 50 << 4 | 5);
        world.set(IVec3::new(2, 64, 2), 166 << 4);
        let mut particles = Vec::new();
        let mut sounds = Vec::new();
        tick_block_and_rain_particles(
            &world,
            DVec3::new(1.5, 64.0, 1.5),
            DVec3::new(1.5, 65.62, 1.5),
            true,
            true,
            1.0,
            2,
            true,
            1,
            &mut particles,
            &mut sounds,
        );
        assert!(particles.is_empty());
    }

    #[test]
    fn torch_barrier_spawner_and_rain_emit_expected_particles() {
        let mut world = flat_stone_world();
        world.set(IVec3::new(1, 64, 1), 50 << 4 | 5);
        world.set(IVec3::new(2, 64, 1), 166 << 4);
        world.set(IVec3::new(3, 64, 1), 52 << 4);
        Arc::make_mut(world.chunks.get_mut(&(0, 0)).unwrap())
            .tiles
            .insert([3, 64, 1], Arc::new(DropNbt::Compound(Default::default())));

        let mut particles = Vec::new();
        let mut sounds = Vec::new();
        for t in 0..10 {
            tick_block_and_rain_particles(
                &world,
                DVec3::new(1.5, 64.0, 1.5),
                DVec3::new(1.5, 65.62, 1.5),
                true,
                true,
                1.0,
                0,
                true,
                t,
                &mut particles,
                &mut sounds,
            );
        }
        assert!(
            particles.iter().any(|p| p.item_texture() == Some((166, 0))),
            "holding barrier in creative must reveal barrier particles"
        );
        assert!(particles.len() > 20);
    }
}
