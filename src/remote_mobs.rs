//! Base poses and collision dimensions sampled from the installed 1.8.9 client.
use crate::{
    mesh::Vertex,
    models::{Dynamic, ModelContext, ATLAS},
};
use glam::{Mat4, Vec3};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::{LazyLock, Once, OnceLock}, time::Instant};
type Tri = [i32; 15];
type Frame = Vec<Tri>;
#[path = "mob_rigid_pose.rs"]
mod rigid;
static POSES: LazyLock<BTreeMap<u8, Vec<Frame>>> = LazyLock::new(|| {
    serde_json::from_reader(flate2::read::GzDecoder::new(
        &include_bytes!("vanilla/mob_poses189.json.gz")[..],
    ))
    .expect("1.8.9 creature animation poses")
});
#[derive(Deserialize)]
pub struct Species {
    pub size: [f32; 3],
    pub collides: bool,
    texture: String,
    frames: Vec<Vec<[i32; 15]>>,
}
static SPECIES: LazyLock<BTreeMap<u8, Species>> = LazyLock::new(|| {
    serde_json::from_reader(flate2::read::GzDecoder::new(
        &include_bytes!("vanilla/mob_models189.json.gz")[..],
    ))
    .expect("1.8.9 creature models")
});

#[derive(Serialize)]
pub(crate) struct RenderModelWarmup {
    initializer: &'static str,
    elapsed_ms: f64,
    species: usize,
    species_ms: f64,
    animation_sets: usize,
    animation_sets_ms: f64,
    rigs: usize,
    rigs_ms: f64,
    geometry: crate::models::RenderGeometryWarmup,
}

static MODEL_WARMUP: OnceLock<RenderModelWarmup> = OnceLock::new();
static MODEL_WARMUP_START: Once = Once::new();

fn prepare_render_models(initializer: &'static str) -> &'static RenderModelWarmup {
    MODEL_WARMUP.get_or_init(|| {
        let started = Instant::now();
        // These are the identical immutable caches used by local and remote
        // creatures. A first moving rabbit otherwise fits every species' rig
        // synchronously from Pose::draw on the render thread.
        let phase = Instant::now();
        let species = LazyLock::force(&SPECIES).len();
        let species_ms = phase.elapsed().as_secs_f64() * 1000.;
        let phase = Instant::now();
        let animation_sets = LazyLock::force(&POSES).len();
        let animation_sets_ms = phase.elapsed().as_secs_f64() * 1000.;
        let phase = Instant::now();
        let rigs = LazyLock::force(&rigid::RIGS).len();
        let rigs_ms = phase.elapsed().as_secs_f64() * 1000.;
        // A previously empty hand/world may not render an item until minutes
        // into play. Its first drop must not synchronously parse/bake all 573
        // item models. These caches are also shared by remote entity renderers.
        let geometry = crate::models::warm_render_geometry();
        RenderModelWarmup { initializer, elapsed_ms: started.elapsed().as_secs_f64() * 1000.,
            species, species_ms, animation_sets, animation_sets_ms, rigs, rigs_ms, geometry }
    })
}

/// Start once before menu resource loading, overlapping CPU work without using
/// the generation or visual-meshing pool. A failed spawn falls back at load.
pub(crate) fn start_render_model_warmup() {
    MODEL_WARMUP_START.call_once(|| {
        let _ = std::thread::Builder::new().name("mob-model-warmup".into()).spawn(|| {
            prepare_render_models("background");
        });
    });
}

/// Called by the loading worker after terrain/network setup, before publishing
/// a playable game. OnceLock also waits for any background initializer.
pub(crate) fn warm_render_models() -> &'static RenderModelWarmup {
    prepare_render_models("loading_worker")
}

/// Reporting never starts work or waits on a render frame.
pub(crate) fn render_model_cache_report() -> serde_json::Value {
    let warmup = MODEL_WARMUP.get();
    serde_json::json!({"ready": warmup.is_some(), "initialization": warmup,
        "note": "Original species, poses, rigid animation fits and immutable item/player/vehicle geometry are prepared before play; initialization overlaps menu/resource/world loading. Active-pack textures remain resolved at draw time."})
}

pub fn species(id: u8) -> Option<&'static Species> {
    SPECIES.get(&id)
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Appearance {
    pub age: i8,
    pub body: u8,
    pub variant: i32,
    pub flags: i32,
    pub subtype: u8,
}
impl Appearance {
    pub(crate) fn local(sim: &crate::gameplay::Simulation, mob: &crate::gameplay::Mob) -> Self {
        use crate::vanilla::{drops::DropNbt, item_nbt::number};
        let data = sim.command_entities.mobs.get(&mob.id);
        let tag = |key: &str| data.and_then(|d| d.extra.get(key));
        let value = |key: &str| number(tag(key));
        let id = mob.kind.vanilla_id();
        let subtype = match id {
            100 => value("Type"),
            101 => value("RabbitType"),
            98 => value("CatType"),
            51 => value("SkeletonType"),
            54 => i32::from(value("IsVillager") != 0),
            120 => value("Profession"),
            _ => 0,
        } as u8;
        let age = if matches!(id, 54 | 57) {
            (value("IsBaby") != 0) as i8
        } else {
            sim.mob_brains
                .get(&mob.id)
                .map_or(value("Age"), |brain| brain.growing_age)
                .signum() as i8
        };
        let brain = sim.mob_brains.get(&mob.id);
        let has_owner = matches!(
            tag("OwnerUUID").or_else(|| tag("Owner")),
            Some(DropNbt::String(owner)) if !owner.is_empty()
        );
        let flags = match id {
            56 => i32::from(
                value("Attacking") != 0 || brain.is_some_and(|b| b.attack_step > 10),
            ),
            58 => {
                if value("screaming") != 0
                    || brain.is_some_and(|b| {
                        b.enderman_screaming || b.anger_ticks > 0 || b.player_hit_ticks > 0
                    })
                {
                    3
                } else {
                    0
                }
            }
            61 => i32::from(
                value("OnFire") != 0 || brain.is_some_and(|b| b.attack_step > 0),
            ),
            65 => value("BatFlags"),
            68 => {
                4 * i32::from(value("Elder") != 0)
                    | 2 * i32::from(brain.is_some_and(|b| b.attack_step > 0))
            }
            95 => {
                i32::from(value("Sitting") != 0)
                    | 2 * i32::from(
                        value("Angry") != 0 || brain.is_some_and(|b| b.anger_ticks > 0),
                    )
                    | 4 * i32::from(has_owner)
            }
            98 => i32::from(value("Sitting") != 0) | 4 * i32::from(has_owner),
            100 => {
                let saddled = value("Saddle") != 0
                    || data
                        .and_then(|d| d.equipment.get(&400))
                        .and_then(Option::as_ref)
                        .is_some_and(|s| s.id == 329)
                    || tag("SaddleItem")
                        .and_then(crate::vanilla::item_nbt::compound)
                        .and_then(|m| m.get("id"))
                        .is_some_and(|id_tag| match id_tag {
                            DropNbt::String(s) => {
                                s.strip_prefix("minecraft:").unwrap_or(s) == "saddle"
                            }
                            other => crate::vanilla::item_nbt::number(Some(other)) == 329,
                        });
                let chested = value("ChestedHorse") != 0
                    || data
                        .and_then(|d| d.equipment.get(&499))
                        .and_then(Option::as_ref)
                        .is_some();
                let tame = value("Tame") != 0 || has_owner;
                let ridden = sim.ridden_horse == Some(mob.id);
                (2 * i32::from(tame))
                    | (4 * i32::from(saddled))
                    | (8 * i32::from(chested))
                    | (256 * i32::from(ridden))
            }
            _ => 0,
        };
        let variant = if id == 58 {
            let block = match tag("carried") {
                Some(DropNbt::String(name)) => crate::world_types::blocks()
                    .get(name.strip_prefix("minecraft:").unwrap_or(name))
                    .map_or(0, |v| i32::from(v.0)),
                _ => value("carried"),
            };
            block << 4 | (value("carriedData") & 15)
        } else {
            value("Variant")
        };
        Self {
            subtype,
            age,
            flags,
            variant,
            body: (value("Size").saturating_add(1)).clamp(1, 255) as u8,
        }
    }
    pub fn size(self, id: u8) -> Option<[f32; 3]> {
        let s = species(id)?;
        if !s.collides {
            return None;
        }
        let mut result = s.size;
        match id {
            55 | 62 if self.body > 0 => {
                let size = 0.51000005 * f32::from(self.body);
                result = [size, size, result[2]];
            }
            68 if self.flags & 4 != 0 => {
                result[0] = 1.9975;
                result[1] = 1.9975;
            }
            51 if self.subtype == 1 => {
                result[0] = 0.72;
                result[1] = 2.535;
            }
            _ => {
                if self.baby(id) {
                    result[0] *= 0.5;
                    result[1] *= 0.5
                }
            }
        }
        Some(result)
    }
    fn baby(self, id: u8) -> bool {
        if matches!(id, 54 | 57) {
            self.age > 0
        } else {
            matches!(id, 90..=93 | 95 | 96 | 98 | 100 | 101 | 120) && self.age < 0
        }
    }
    fn scale(self, id: u8) -> f32 {
        if self.baby(id) {
            0.5
        } else if matches!(id, 55 | 62) {
            f32::from(self.body.max(1))
        } else if id == 68 && self.flags & 4 != 0 {
            2.35
        } else if id == 51 && self.subtype == 1 {
            1.2
        } else {
            1.
        }
    }
    pub(crate) fn texture(self, id: u8) -> &'static str {
        match id {
            51 if self.subtype == 1 => "textures/entity/skeleton/wither_skeleton.png",
            54 if self.subtype == 1 => "textures/entity/zombie/zombie_villager.png",
            100 => match self.subtype {
                1 => "textures/entity/horse/donkey.png",
                2 => "textures/entity/horse/mule.png",
                3 => "textures/entity/horse/horse_zombie.png",
                4 => "textures/entity/horse/horse_skeleton.png",
                _ => [
                    "textures/entity/horse/horse_white.png",
                    "textures/entity/horse/horse_creamy.png",
                    "textures/entity/horse/horse_chestnut.png",
                    "textures/entity/horse/horse_brown.png",
                    "textures/entity/horse/horse_black.png",
                    "textures/entity/horse/horse_gray.png",
                    "textures/entity/horse/horse_darkbrown.png",
                ][(self.variant as u8 as usize) % 7],
            },
            101 => match self.subtype {
                1 => "textures/entity/rabbit/white.png",
                2 => "textures/entity/rabbit/black.png",
                3 => "textures/entity/rabbit/white_splotched.png",
                4 => "textures/entity/rabbit/gold.png",
                5 => "textures/entity/rabbit/salt.png",
                99 => "textures/entity/rabbit/caerbannog.png",
                _ => "textures/entity/rabbit/brown.png",
            },
            98 => match self.subtype {
                1 => "textures/entity/cat/black.png",
                2 => "textures/entity/cat/red.png",
                3 => "textures/entity/cat/siamese.png",
                _ => "textures/entity/cat/ocelot.png",
            },
            95 if self.flags & 2 != 0 => "textures/entity/wolf/wolf_angry.png",
            95 if self.flags & 4 != 0 => "textures/entity/wolf/wolf_tame.png",
            68 if self.flags & 4 != 0 => "textures/entity/guardian_elder.png",
            56 if self.flags & 1 != 0 => "textures/entity/ghast/ghast_shooting.png",
            120 => match self.subtype {
                1 => "textures/entity/villager/librarian.png",
                2 => "textures/entity/villager/priest.png",
                3 => "textures/entity/villager/smith.png",
                4 => "textures/entity/villager/butcher.png",
                5 => "textures/entity/villager/villager.png",
                _ => "textures/entity/villager/farmer.png",
            },
            _ => &species(id).expect("registered species").texture,
        }
    }
    pub(crate) fn horse_markings(self) -> Option<&'static str> {
        if self.subtype != 0 {
            return None;
        }
        match self.variant >> 8 & 255 {
            1 => Some("textures/entity/horse/horse_markings_white.png"),
            2 => Some("textures/entity/horse/horse_markings_whitefield.png"),
            3 => Some("textures/entity/horse/horse_markings_whitedots.png"),
            4 => Some("textures/entity/horse/horse_markings_blackdots.png"),
            _ => None,
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct Actor {
    pub species: u8,
    pub previous: Vec3,
    pub feet: Vec3,
    pub yaw: f32,
    pub head_yaw: Option<f32>,
    pub pitch: f32,
    pub gait: crate::entity_visual::Gait,
    pub appearance: Appearance,
    pub hurt: bool,
    pub death_time: u8,
    pub swing_ticks: u8,
    pub attack_ticks: u8,
    pub rose_ticks: u16,
    pub eat_ticks: u8,
    pub held: Option<crate::inventory::Stack>,
    pub armor: [Option<crate::inventory::Stack>; 4],
    pub horse_anim: crate::horse_model::HorseAnim,
    pub horse_armor: u8, // 0=none, 1=iron, 2=gold, 3=diamond
    pub collar_color: u8, // 0..=15 wool color index (0=white, 14=red default, 15=black)
    pub beam_target: Option<i32>,
    pub beam_target_pos: Option<Vec3>,
    pub slime_squish: f32,
    pub prev_slime_squish: f32,
    pub dragon_death_ticks: u16,
    pub dragon_wing_phase: Option<f32>,
    pub dragon_flight: Option<Box<crate::remote::dragon::Flight>>,
    pub invisible: bool,
}

fn local_horse_armor(data: Option<&crate::gameplay::command_entity_state::MobData>) -> u8 {
    use crate::vanilla::{drops::DropNbt, item_nbt};
    let Some(d) = data else {
        return 0;
    };
    if let Some(Some(stack)) = d.equipment.get(&401) {
        match stack.id {
            417 => return 1,
            418 => return 2,
            419 => return 3,
            _ => {}
        }
    }
    if let Some(m) = d.extra.get("ArmorItem").and_then(item_nbt::compound) {
        if let Some(id_tag) = m.get("id") {
            match id_tag {
                DropNbt::String(s) => match s.strip_prefix("minecraft:").unwrap_or(s) {
                    "iron_horse_armor" => return 1,
                    "golden_horse_armor" => return 2,
                    "diamond_horse_armor" => return 3,
                    _ => {}
                },
                other => match item_nbt::number(Some(other)) {
                    417 => return 1,
                    418 => return 2,
                    419 => return 3,
                    _ => {}
                },
            }
        }
    }
    match item_nbt::number(d.extra.get("ArmorType")) {
        1 => 1,
        2 => 2,
        3 => 3,
        417 => 1,
        418 => 2,
        419 => 3,
        _ => 0,
    }
}

pub fn draw_native(d: &mut Dynamic, c: &ModelContext<'_>, mob: &crate::gameplay::Mob) {
    let brain = c.sim.mob_brains.get(&mob.id);
    let data = c.sim.command_entities.mobs.get(&mob.id);
    let species = mob.kind.vanilla_id();
    let mut gait = brain.map(|b| b.gait).unwrap_or_default();
    // Native ground navigation uses -Z forward, whereas EntityDragon already uses Java rotationYaw.
    if species != 63 {
        gait.previous_yaw = gait.previous_yaw.map(|yaw| yaw - std::f32::consts::PI);
    }
    let appearance = Appearance::local(c.sim, mob);
    let spec = &SPECIES[&species];
    let [w, h, _] = appearance.size(species).unwrap_or(spec.size);
    let feet_now = mob.previous.lerp(mob.position, c.alpha);
    let body_java_deg = if species == 63 {
        mob.yaw.to_degrees()
    } else {
        mob.yaw.to_degrees() - 180.
    };

    let mut head_yaw = None;
    let mut pitch = data.map_or(0., |d| d.pitch);
    let engage = brain.is_some_and(|b| b.tempted || b.anger_ticks > 0 || b.attack_step > 0)
        || mob.swing_ticks > 0
        || mob.attack_ticks > 0
        || mob.rose_ticks > 0
        || (c.sim.mode == crate::gameplay::Mode::Survival
            && mob.kind.hostile()
            && feet_now.distance_squared(c.player.position) < 256.);
    if engage && mob.death_ticks == 0 {
        let look = c.player.eye(c.alpha) - (feet_now + Vec3::Y * (h * 0.85));
        let horiz = look.x.hypot(look.z);
        if horiz > 1e-3 {
            let target_yaw = (-look.x).atan2(look.z).to_degrees();
            let diff = (target_yaw - body_java_deg + 180.).rem_euclid(360.) - 180.;
            if mob.kind.hostile() || diff.abs() <= 75. {
                head_yaw = Some(target_yaw);
                if pitch == 0. {
                    pitch = (-look.y).atan2(horiz).to_degrees().clamp(-45., 45.);
                }
            }
        }
    }

    let held = if let Some(slot) = data.and_then(|d| d.equipment.get(&99)) {
        slot.clone()
    } else {
        match species {
            51 if appearance.subtype == 0 => Some(crate::inventory::Stack::new(261, 1)),
            51 if appearance.subtype == 1 => Some(crate::inventory::Stack::new(272, 1)),
            57 => Some(crate::inventory::Stack::new(283, 1)),
            _ => None,
        }
    };
    let armor = [100, 101, 102, 103]
        .map(|slot| data.and_then(|d| d.equipment.get(&slot)).and_then(Clone::clone));
    let collar_color = data
        .and_then(|d| d.extra.get("CollarColor"))
        .map_or(14, |tag| {
            (15 - (crate::vanilla::item_nbt::number(Some(tag)) & 15)) as u8
        });
    let beam_target_pos = (species == 68
        && mob.death_ticks == 0
        && brain.is_some_and(|b| b.attack_step > 0))
    .then(|| c.player.previous.lerp(c.player.position, c.alpha) + Vec3::Y * 0.9);
    let (slime_squish, prev_slime_squish) = if matches!(species, 55 | 62) {
        let dy = mob.position.y - mob.previous.y;
        let sq = if dy.abs() > 0.005 {
            (-dy * 1.8).clamp(-0.45, 0.55)
        } else {
            0.0
        };
        (sq, sq * 0.7)
    } else {
        (0.0, 0.0)
    };

    let dragon_death_ticks = if species == 63 {
        brain.map_or(0, |b| b.dragon_death_ticks)
    } else {
        0
    };
    let dragon_wing_phase = (species == 63).then(|| {
        brain.map_or((mob.age as f32 + c.alpha) * 0.05, |b| {
            let mut delta = b.dragon_wing_phase - b.dragon_prev_wing_phase;
            if delta < -0.5 {
                delta += 1.0;
            } else if delta > 0.5 {
                delta -= 1.0;
            }
            (b.dragon_prev_wing_phase + delta * c.alpha).rem_euclid(1.0)
        })
    });
    let dragon_flight = if species == 63 {
        brain.and_then(|b| b.dragon_flight.clone())
    } else {
        None
    };
    let invisible = data.is_some_and(|d| d.effects.get(14).is_some());

    let actor = Actor {
        species,
        previous: mob.previous,
        feet: mob.position,
        yaw: body_java_deg,
        head_yaw,
        pitch,
        gait,
        appearance,
        hurt: if species == 63 {
            mob.hurt > 0
        } else {
            mob.hurt > 0 || mob.health <= 0. || mob.death_ticks > 0
        },
        death_time: if species == 63 { 0 } else { mob.death_ticks },
        swing_ticks: mob.swing_ticks,
        attack_ticks: mob.attack_ticks,
        rose_ticks: mob.rose_ticks,
        eat_ticks: mob.eat_ticks,
        held,
        armor,
        horse_anim: c.sim.horse_anims.get(&mob.id).copied().unwrap_or_default(),
        horse_armor: if species == 100 {
            local_horse_armor(data)
        } else {
            0
        },
        collar_color,
        beam_target: None,
        beam_target_pos,
        slime_squish,
        prev_slime_squish,
        dragon_death_ticks,
        dragon_wing_phase,
        dragon_flight,
        invisible,
    };
    let visible = d.visible;
    draw_at(
        d,
        c,
        &actor,
        brain.is_some_and(|b| b.burning()),
        mob.age as f32 + c.alpha,
    );
    if d.visible > visible && c.hitboxes {
        let r = Vec3::new(w * 0.5, 0., w * 0.5);
        crate::models::wirebox(&mut d.opaque, feet_now - r, feet_now + r + Vec3::Y * h);
    }
}

fn mob_shadow_size(species: u8, appearance: Appearance) -> f32 {
    let base = match species {
        50 | 51 | 54 | 56 | 57 | 58 | 61 | 63 | 64 | 66 | 68 | 95 | 97 | 99 | 120 => 0.5,
        90 | 91 | 92 | 94 | 96 | 59 => 0.7,
        100 => 0.75,
        52 => 1.0,
        98 => 0.4,
        93 | 101 | 60 | 67 => 0.3,
        65 => 0.25,
        55 | 62 => 0.25 * f32::from(appearance.body.max(1)),
        53 => 3.0,
        _ => 0.5,
    };
    if appearance.baby(species) {
        base * 0.5
    } else {
        base
    }
}

#[allow(clippy::too_many_arguments)]
fn entity_box(
    mesh: &mut crate::mesh::Mesh,
    matrix: Mat4,
    min: [f32; 3],
    size: [f32; 3],
    inflate: f32,
    uv: [f32; 2],
    mirror: bool,
    sprite: crate::resources::Sprite,
    tex_size: [f32; 2],
    tint: [u8; 3],
    flags: u32,
    light: [u8; 4],
    double_sided: bool,
) {
    let [w, h, d] = size;
    let [u, v] = uv;
    let [x0, y0, z0] = [min[0] - inflate, min[1] - inflate, min[2] - inflate];
    let [x1, y1, z1] = [
        min[0] + w + inflate,
        min[1] + h + inflate,
        min[2] + d + inflate,
    ];
    let pts = [
        [x0, y0, z0],
        [x1, y0, z0],
        [x1, y1, z0],
        [x0, y1, z0],
        [x0, y0, z1],
        [x1, y0, z1],
        [x1, y1, z1],
        [x0, y1, z1],
    ]
    .map(|a| matrix.transform_point3(Vec3::from_array(a)));
    let faces = if mirror {
        [
            ([5, 1, 2, 6], [u + d, v + d, u, v + d + h]),
            ([0, 4, 7, 3], [u + d + w + d, v + d, u + d + w, v + d + h]),
            ([5, 4, 0, 1], [u + d + w, v, u + d, v + d]),
            ([2, 3, 7, 6], [u + d + w + w, v + d, u + d + w, v]),
            ([1, 0, 3, 2], [u + d + w, v + d, u + d, v + d + h]),
            (
                [4, 5, 6, 7],
                [u + d + w + d + w, v + d, u + d + w + d, v + d + h],
            ),
        ]
    } else {
        [
            ([5, 1, 2, 6], [u + d + w, v + d, u + d + w + d, v + d + h]),
            ([0, 4, 7, 3], [u, v + d, u + d, v + d + h]),
            ([5, 4, 0, 1], [u + d, v, u + d + w, v + d]),
            ([2, 3, 7, 6], [u + d + w, v + d, u + d + w + w, v]),
            ([1, 0, 3, 2], [u + d, v + d, u + d + w, v + d + h]),
            (
                [4, 5, 6, 7],
                [u + d + w + d, v + d, u + d + w + d + w, v + d + h],
            ),
        ]
    };
    let uv_scale = [
        sprite.width as f32 / (tex_size[0] * 256.0),
        sprite.height as f32 / (tex_size[1] * 256.0),
    ];
    let layer = sprite.layer | flags;
    let unlit = flags & crate::models::UNLIT != 0;
    for (indices, [u1, v1, u2, v2]) in faces {
        let p0 = pts[indices[0]];
        let p1 = pts[indices[1]];
        let p2 = pts[indices[2]];
        let normal = (p1 - p0).cross(p2 - p0).normalize_or_zero();
        let shade = if unlit {
            1.0
        } else {
            crate::models::entity_face_shade(normal)
        };
        let [r, g, b] = tint.map(|ch| (f32::from(ch) * shade).round().clamp(0.0, 255.0) as u8);
        let corners: [([f32; 3], [f32; 2]); 4] = std::array::from_fn(|k| {
            let [s, t] = [[u2, v1], [u1, v1], [u1, v2], [u2, v2]][k];
            (pts[indices[k]].to_array(), [s * uv_scale[0], t * uv_scale[1]])
        });
        let first = mesh.vertices.len() as u32;
        for (position, uv) in corners {
            mesh.vertices.push(Vertex {
                position,
                uv,
                layer,
                color: [r, g, b, 255],
                light,
            });
        }
        mesh.indices.extend([0, 1, 2, 0, 2, 3].map(|k| first + k));
        mesh.visible_faces += 1;
        if double_sided {
            let back_shade = if unlit {
                1.0
            } else {
                crate::models::entity_face_shade(-normal)
            };
            let [br, bg, bb] =
                tint.map(|ch| (f32::from(ch) * back_shade).round().clamp(0.0, 255.0) as u8);
            let back = mesh.vertices.len() as u32;
            for (position, uv) in corners {
                mesh.vertices.push(Vertex {
                    position,
                    uv,
                    layer,
                    color: [br, bg, bb, 255],
                    light,
                });
            }
            mesh.indices.extend([0, 2, 1, 0, 3, 2].map(|k| back + k));
            mesh.visible_faces += 1;
        }
    }
}

fn has_mounted_rider(c: &ModelContext<'_>, horse_feet: Vec3, horse_h: f32) -> bool {
    let check = |rider_feet: Vec3| {
        let dx = rider_feet.x - horse_feet.x;
        let dz = rider_feet.z - horse_feet.z;
        let dy = rider_feet.y - horse_feet.y;
        dx * dx + dz * dz < 0.85 && dy > 0.2 && dy < horse_h + 0.6
    };
    if c.player.third_person.riding && check(c.player.previous.lerp(c.player.position, c.alpha)) {
        return true;
    }
    c.sim
        .server_scene
        .actors
        .values()
        .any(|p| p.anim.riding && check(p.previous.lerp(p.feet, c.alpha)))
}

#[allow(clippy::too_many_arguments)]
fn draw_wolf(
    mesh: &mut crate::mesh::Mesh,
    c: &ModelContext<'_>,
    a: &Actor,
    feet: Vec3,
    yaw: f32,
    phase: f32,
    walk: f32,
    roll_z: f32,
    tex: crate::resources::Sprite,
    flags: u32,
    light: [u8; 4],
) {
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};
    let sitting = a.appearance.flags & 1 != 0;
    let angry = a.appearance.flags & 2 != 0;
    let tamed = a.appearance.flags & 4 != 0;
    let baby = a.appearance.baby(95);

    let body_deg = yaw.to_degrees();
    let net_head_yaw = a
        .head_yaw
        .map(|hy| ((hy - body_deg + 180.0).rem_euclid(360.0) - 180.0).clamp(-65.0, 65.0))
        .unwrap_or(0.0)
        .to_radians();
    let head_pitch = a.pitch.clamp(-45.0, 45.0).to_radians();

    let root = Mat4::from_translation(feet)
        * Mat4::from_rotation_y(PI - yaw)
        * Mat4::from_rotation_z(roll_z)
        * Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0))
        * Mat4::from_translation(Vec3::new(0.0, -1.5078125, 0.0))
        * Mat4::from_scale(Vec3::splat(0.0625));

    let (head_root, body_root) = if baby {
        (
            root * Mat4::from_translation(Vec3::new(0.0, 5.0, 2.0)),
            root * Mat4::from_translation(Vec3::new(0.0, 12.0, 0.0))
                * Mat4::from_scale(Vec3::splat(0.5)),
        )
    } else {
        (root, root)
    };

    let head_mat = head_root
        * Mat4::from_translation(Vec3::new(-1.0, 13.5, -7.0))
        * Mat4::from_rotation_y(net_head_yaw)
        * Mat4::from_rotation_x(head_pitch);
    let ts = [64.0, 32.0];
    entity_box(mesh, head_mat, [-2.0, -3.0, -2.0], [6.0, 6.0, 4.0], 0.0, [0.0, 0.0], false, tex, ts, [255; 3], flags, light, false);
    entity_box(mesh, head_mat, [-2.0, -5.0, 0.0], [2.0, 2.0, 1.0], 0.0, [16.0, 14.0], false, tex, ts, [255; 3], flags, light, false);
    entity_box(mesh, head_mat, [2.0, -5.0, 0.0], [2.0, 2.0, 1.0], 0.0, [16.0, 14.0], false, tex, ts, [255; 3], flags, light, false);
    entity_box(mesh, head_mat, [-0.5, 0.0, -5.0], [3.0, 3.0, 4.0], 0.0, [0.0, 10.0], false, tex, ts, [255; 3], flags, light, false);

    let (mane_mat, body_mat, tail_mat, legs) = if sitting {
        let mane = body_root
            * Mat4::from_translation(Vec3::new(0.0, 16.0, -3.0))
            * Mat4::from_rotation_x(PI * 0.4);
        let body = body_root
            * Mat4::from_translation(Vec3::new(0.0, 18.0, 0.0))
            * Mat4::from_rotation_x(FRAC_PI_4);
        let tail = body_root
            * Mat4::from_translation(Vec3::new(0.0, 21.0, 6.0))
            * Mat4::from_rotation_x(1.7278761);
        let l1 = body_root
            * Mat4::from_translation(Vec3::new(-1.5, 22.0, 2.0))
            * Mat4::from_rotation_x(-FRAC_PI_2);
        let l2 = body_root
            * Mat4::from_translation(Vec3::new(1.5, 22.0, 2.0))
            * Mat4::from_rotation_x(-FRAC_PI_2);
        let l3 = body_root
            * Mat4::from_translation(Vec3::new(-1.49, 17.0, -4.0))
            * Mat4::from_rotation_x(-0.4712389);
        let l4 = body_root
            * Mat4::from_translation(Vec3::new(1.51, 17.0, -4.0))
            * Mat4::from_rotation_x(-0.4712389);
        (mane, body, tail, [l1, l2, l3, l4])
    } else {
        let s1 = crate::vanilla::math::cos(phase * 0.6662) * 1.4 * walk;
        let s2 = crate::vanilla::math::cos(phase * 0.6662 + PI) * 1.4 * walk;
        let mane = body_root
            * Mat4::from_translation(Vec3::new(0.0, 14.0, -3.0))
            * Mat4::from_rotation_x(FRAC_PI_2);
        let body = body_root
            * Mat4::from_translation(Vec3::new(0.0, 14.0, 2.0))
            * Mat4::from_rotation_x(FRAC_PI_2);
        let tail_rx = if angry {
            FRAC_PI_2
        } else if tamed {
            1.72
        } else {
            PI / 5.0
        };
        let tail_ry = crate::vanilla::math::cos(phase * 0.6662) * 1.4 * walk;
        let tail = body_root
            * Mat4::from_translation(Vec3::new(0.0, 12.0, 8.0))
            * Mat4::from_rotation_y(tail_ry)
            * Mat4::from_rotation_x(tail_rx);
        let l1 = body_root
            * Mat4::from_translation(Vec3::new(-1.5, 16.0, 7.0))
            * Mat4::from_rotation_x(s1);
        let l2 = body_root
            * Mat4::from_translation(Vec3::new(1.5, 16.0, 7.0))
            * Mat4::from_rotation_x(s2);
        let l3 = body_root
            * Mat4::from_translation(Vec3::new(-1.5, 16.0, -4.0))
            * Mat4::from_rotation_x(s2);
        let l4 = body_root
            * Mat4::from_translation(Vec3::new(1.5, 16.0, -4.0))
            * Mat4::from_rotation_x(s1);
        (mane, body, tail, [l1, l2, l3, l4])
    };

    entity_box(mesh, mane_mat, [-4.0, -3.0, -3.0], [8.0, 6.0, 7.0], 0.0, [21.0, 0.0], false, tex, ts, [255; 3], flags, light, false);
    entity_box(mesh, body_mat, [-3.0, -2.0, -3.0], [6.0, 9.0, 6.0], 0.0, [18.0, 14.0], false, tex, ts, [255; 3], flags, light, false);
    entity_box(mesh, tail_mat, [-1.0, 0.0, -1.0], [2.0, 8.0, 2.0], 0.0, [9.0, 18.0], false, tex, ts, [255; 3], flags, light, false);
    for leg_mat in legs {
        entity_box(mesh, leg_mat, [-1.0, 0.0, -1.0], [2.0, 8.0, 2.0], 0.0, [0.0, 18.0], false, tex, ts, [255; 3], flags, light, false);
    }

    if tamed && !angry {
        let collar_sprite = c.resources.sprite("textures/entity/wolf/wolf_collar.png");
        let rgb = crate::models::DYE_TO_RGB[usize::from(a.collar_color & 15)];
        entity_box(
            mesh,
            mane_mat,
            [-4.0, -3.0, -3.0],
            [8.0, 6.0, 7.0],
            0.05,
            [21.0, 0.0],
            false,
            collar_sprite,
            ts,
            rgb,
            flags,
            light,
            false,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_sitting_ocelot(
    mesh: &mut crate::mesh::Mesh,
    a: &Actor,
    feet: Vec3,
    yaw: f32,
    roll_z: f32,
    tex: crate::resources::Sprite,
    flags: u32,
    light: [u8; 4],
) {
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};
    let baby = a.appearance.baby(98);
    let body_deg = yaw.to_degrees();
    let net_head_yaw = a
        .head_yaw
        .map(|hy| ((hy - body_deg + 180.0).rem_euclid(360.0) - 180.0).clamp(-65.0, 65.0))
        .unwrap_or(0.0)
        .to_radians();
    let head_pitch = a.pitch.clamp(-45.0, 45.0).to_radians();

    let root = Mat4::from_translation(feet)
        * Mat4::from_rotation_y(PI - yaw)
        * Mat4::from_rotation_z(roll_z)
        * Mat4::from_scale(Vec3::new(-0.8, -0.8, 0.8))
        * Mat4::from_translation(Vec3::new(0.0, -1.5078125, 0.0))
        * Mat4::from_scale(Vec3::splat(0.0625));
    let (head_root, body_root) = if baby {
        (
            root * Mat4::from_translation(Vec3::new(0.0, 10.0, -4.0))
                * Mat4::from_scale(Vec3::splat(0.75)),
            root * Mat4::from_translation(Vec3::new(0.0, 12.0, 0.0))
                * Mat4::from_scale(Vec3::splat(0.5)),
        )
    } else {
        (root, root)
    };
    let ts = [64.0, 32.0];
    let head_mat = head_root
        * Mat4::from_translation(Vec3::new(0.0, 13.0, -7.0))
        * Mat4::from_rotation_y(net_head_yaw)
        * Mat4::from_rotation_x(head_pitch);
    entity_box(mesh, head_mat, [-2.5, -2.0, -3.0], [5.0, 4.0, 5.0], 0.0, [0.0, 0.0], false, tex, ts, [255; 3], flags, light, false);
    entity_box(mesh, head_mat, [-1.5, 0.0, -4.0], [3.0, 2.0, 2.0], 0.0, [0.0, 24.0], false, tex, ts, [255; 3], flags, light, false);
    entity_box(mesh, head_mat, [-2.0, -3.0, 0.0], [1.0, 1.0, 2.0], 0.0, [0.0, 10.0], false, tex, ts, [255; 3], flags, light, false);
    entity_box(mesh, head_mat, [1.0, -3.0, 0.0], [1.0, 1.0, 2.0], 0.0, [6.0, 10.0], false, tex, ts, [255; 3], flags, light, false);

    let body_mat = body_root
        * Mat4::from_translation(Vec3::new(0.0, 13.0, -8.0))
        * Mat4::from_rotation_x(FRAC_PI_4);
    entity_box(mesh, body_mat, [-2.0, 3.0, -8.0], [4.0, 16.0, 6.0], 0.0, [20.0, 0.0], false, tex, ts, [255; 3], flags, light, false);

    let tail1 = body_root
        * Mat4::from_translation(Vec3::new(0.0, 23.0, 2.0))
        * Mat4::from_rotation_x(FRAC_PI_4);
    let tail2 = body_root
        * Mat4::from_translation(Vec3::new(0.0, 23.0, 7.5))
        * Mat4::from_rotation_x(1.7278761);
    entity_box(mesh, tail1, [-0.5, 0.0, 0.0], [1.0, 8.0, 1.0], 0.0, [0.0, 15.0], false, tex, ts, [255; 3], flags, light, false);
    entity_box(mesh, tail2, [-0.5, 0.0, 0.0], [1.0, 8.0, 1.0], 0.0, [4.0, 15.0], false, tex, ts, [255; 3], flags, light, false);

    for sx in [-1.2, 1.2] {
        let fl = body_root
            * Mat4::from_translation(Vec3::new(sx, 14.2, -5.0))
            * Mat4::from_rotation_x(-0.15707964);
        entity_box(mesh, fl, [-1.0, 0.0, 0.0], [2.0, 10.0, 2.0], 0.0, [40.0, 0.0], false, tex, ts, [255; 3], flags, light, false);
    }
    for sx in [-1.1, 1.1] {
        let bl = body_root
            * Mat4::from_translation(Vec3::new(sx, 22.0, 1.0))
            * Mat4::from_rotation_x(-FRAC_PI_2);
        entity_box(mesh, bl, [-1.0, 0.0, 1.0], [2.0, 6.0, 2.0], 0.0, [8.0, 13.0], false, tex, ts, [255; 3], flags, light, false);
    }
}

fn java_gaussian_pair(rng: &mut crate::vanilla::random::JavaRandom) -> [f32; 2] {
    for _ in 0..16 {
        let v1 = 2.0 * rng.next_double() - 1.0;
        let v2 = 2.0 * rng.next_double() - 1.0;
        let s = v1 * v1 + v2 * v2;
        if s > 0.0 && s < 1.0 {
            let mul = (-2.0 * s.ln() / s).sqrt();
            return [(v1 * mul) as f32, (v2 * mul) as f32];
        }
    }
    [0.0, 0.0]
}

fn enderman_scream_jitter(a: &Actor, feet: Vec3, age: f32, alpha: f32) -> Vec3 {
    if a.appearance.flags & 3 == 0 {
        return Vec3::ZERO;
    }
    let frame_step = (age * 60.0).floor() as i64;
    let sub_step = (alpha * 60.0) as i64;
    let seed = frame_step
        .wrapping_add(sub_step.wrapping_mul(31))
        .wrapping_mul(0x517C_C1B7_2722_0A95u64 as i64)
        .wrapping_add((feet.x.to_bits() as i64) << 17)
        .wrapping_add(feet.z.to_bits() as i64);
    let mut rng = crate::vanilla::random::JavaRandom::new(seed);
    let [gx, gz] = java_gaussian_pair(&mut rng);
    Vec3::new(gx * 0.02, 0.0, gz * 0.02)
}

#[allow(clippy::too_many_arguments)]
fn draw_enderman(
    mesh: &mut crate::mesh::Mesh,
    c: &ModelContext<'_>,
    a: &Actor,
    feet: Vec3,
    yaw: f32,
    phase: f32,
    walk: f32,
    age: f32,
    roll_z: f32,
    tex: crate::resources::Sprite,
    flags: u32,
    light: [u8; 4],
) {
    use std::f32::consts::PI;
    let screaming = a.appearance.flags & 3 != 0;
    let carrying = a.appearance.variant > 0;
    let body_deg = yaw.to_degrees();
    let net_head_yaw = a
        .head_yaw
        .map(|hy| ((hy - body_deg + 180.0).rem_euclid(360.0) - 180.0).clamp(-70.0, 70.0))
        .unwrap_or(0.0)
        .to_radians();
    let head_pitch = a.pitch.clamp(-50.0, 50.0).to_radians();

    let root = Mat4::from_translation(feet)
        * Mat4::from_rotation_y(PI - yaw)
        * Mat4::from_rotation_z(roll_z)
        * Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0))
        * Mat4::from_translation(Vec3::new(0.0, -1.5078125, 0.0))
        * Mat4::from_scale(Vec3::splat(0.0625));

    let ts = [64.0, 32.0];
    let upper_head_y = if screaming { -18.0 } else { -13.0 };
    let head_rot = Mat4::from_rotation_y(net_head_yaw) * Mat4::from_rotation_x(head_pitch);
    let head_mat = root * Mat4::from_translation(Vec3::new(0.0, upper_head_y, 0.0)) * head_rot;
    let jaw_mat = root * Mat4::from_translation(Vec3::new(0.0, -13.0, 0.0)) * head_rot;

    entity_box(mesh, head_mat, [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], 0.0, [0.0, 0.0], false, tex, ts, [255; 3], flags, light, false);
    entity_box(mesh, jaw_mat, [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], -0.5, [0.0, 16.0], false, tex, ts, [255; 3], flags, light, false);

    let eyes_sprite = c.resources.sprite("textures/entity/enderman/enderman_eyes.png");
    entity_box(
        mesh,
        head_mat,
        [-4.0, -8.0, -4.0],
        [8.0, 8.0, 8.0],
        0.02,
        [0.0, 0.0],
        false,
        eyes_sprite,
        ts,
        [255; 3],
        ATLAS | crate::models::UNLIT,
        [240, 240, 0, 0],
        false,
    );

    let body_mat = root * Mat4::from_translation(Vec3::new(0.0, -14.0, 0.0));
    entity_box(mesh, body_mat, [-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], 0.0, [32.0, 16.0], false, tex, ts, [255; 3], flags, light, false);

    let swing = if a.swing_ticks > 0 {
        ((6.0 - (a.swing_ticks as f32 - c.alpha)) / 6.0).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (r_arm_rx, l_arm_rx, r_arm_rz, l_arm_rz) = if carrying {
        (-0.5, -0.5, 0.05, -0.05)
    } else {
        let breathe_z = crate::vanilla::math::cos(age * 0.09) * 0.05 + 0.05;
        let breathe_x = crate::vanilla::math::sin(age * 0.067) * 0.05;
        let strike = -crate::vanilla::math::sin(swing * PI) * 1.1;
        let r_biped_x = crate::vanilla::math::cos(phase * 0.6662 + PI) * walk + breathe_x + strike;
        let l_biped_x = crate::vanilla::math::cos(phase * 0.6662) * walk - breathe_x;
        let r_rx = (r_biped_x * 0.5).clamp(-0.4, 0.4);
        let l_rx = (l_biped_x * 0.5).clamp(-0.4, 0.4);
        (r_rx, l_rx, breathe_z, -breathe_z)
    };
    let r_arm_mat = root
        * Mat4::from_translation(Vec3::new(-5.0, -12.0, 0.0))
        * Mat4::from_rotation_z(r_arm_rz)
        * Mat4::from_rotation_x(r_arm_rx);
    let l_arm_mat = root
        * Mat4::from_translation(Vec3::new(5.0, -12.0, 0.0))
        * Mat4::from_rotation_z(l_arm_rz)
        * Mat4::from_rotation_x(l_arm_rx);
    entity_box(mesh, r_arm_mat, [-1.0, -2.0, -1.0], [2.0, 30.0, 2.0], 0.0, [56.0, 0.0], false, tex, ts, [255; 3], flags, light, false);
    entity_box(mesh, l_arm_mat, [-1.0, -2.0, -1.0], [2.0, 30.0, 2.0], 0.0, [56.0, 0.0], true, tex, ts, [255; 3], flags, light, false);

    let r_leg_rx = (crate::vanilla::math::cos(phase * 0.6662) * 1.4 * walk * 0.5).clamp(-0.4, 0.4);
    let l_leg_rx = (crate::vanilla::math::cos(phase * 0.6662 + PI) * 1.4 * walk * 0.5).clamp(-0.4, 0.4);
    let r_leg_mat = root
        * Mat4::from_translation(Vec3::new(-2.0, -5.0, 0.0))
        * Mat4::from_rotation_x(r_leg_rx);
    let l_leg_mat = root
        * Mat4::from_translation(Vec3::new(2.0, -5.0, 0.0))
        * Mat4::from_rotation_x(l_leg_rx);
    entity_box(mesh, r_leg_mat, [-1.0, 0.0, -1.0], [2.0, 30.0, 2.0], 0.0, [56.0, 0.0], false, tex, ts, [255; 3], flags, light, false);
    entity_box(mesh, l_leg_mat, [-1.0, 0.0, -1.0], [2.0, 30.0, 2.0], 0.0, [56.0, 0.0], true, tex, ts, [255; 3], flags, light, false);
}

#[allow(clippy::too_many_arguments)]
fn draw_roosting_bat(
    mesh: &mut crate::mesh::Mesh,
    feet: Vec3,
    yaw: f32,
    roll_z: f32,
    tex: crate::resources::Sprite,
    flags: u32,
    light: [u8; 4],
) {
    use std::f32::consts::PI;
    let root = Mat4::from_translation(feet + Vec3::Y * 0.25)
        * Mat4::from_rotation_y(PI - yaw)
        * Mat4::from_rotation_z(roll_z)
        * Mat4::from_scale(Vec3::new(-0.35, -0.35, 0.35))
        * Mat4::from_translation(Vec3::new(0.0, -1.5078125, 0.0))
        * Mat4::from_scale(Vec3::splat(0.0625));
    let ts = [64.0, 64.0];

    let head_mat = root
        * Mat4::from_translation(Vec3::new(0.0, 14.0, 0.0))
        * Mat4::from_rotation_y(PI)
        * Mat4::from_rotation_x(0.2);
    entity_box(mesh, head_mat, [-3.0, -3.0, -3.0], [6.0, 6.0, 6.0], 0.0, [0.0, 0.0], false, tex, ts, [255; 3], flags, light, false);
    entity_box(mesh, head_mat, [-4.0, -6.0, -2.0], [3.0, 4.0, 1.0], 0.0, [24.0, 0.0], false, tex, ts, [255; 3], flags, light, false);
    entity_box(mesh, head_mat, [1.0, -6.0, -2.0], [3.0, 4.0, 1.0], 0.0, [24.0, 0.0], true, tex, ts, [255; 3], flags, light, false);

    let body_mat = root
        * Mat4::from_translation(Vec3::new(0.0, 16.0, 0.0))
        * Mat4::from_rotation_x(PI - 0.12);
    entity_box(mesh, body_mat, [-3.0, 4.0, -3.0], [6.0, 12.0, 6.0], 0.0, [0.0, 16.0], false, tex, ts, [255; 3], flags, light, false);
    entity_box(mesh, body_mat, [-5.0, 16.0, 0.0], [10.0, 6.0, 1.0], 0.0, [0.0, 34.0], false, tex, ts, [255; 3], flags, light, true);

    let r_wing = body_mat
        * Mat4::from_translation(Vec3::new(-3.0, 4.0, 1.5))
        * Mat4::from_rotation_x(-0.15707964)
        * Mat4::from_rotation_y(-1.2566371);
    let r_outer = r_wing
        * Mat4::from_translation(Vec3::new(-10.0, 0.0, 0.0))
        * Mat4::from_rotation_y(-1.7278761);
    entity_box(mesh, r_wing, [-10.0, 1.0, 0.0], [10.0, 16.0, 1.0], 0.0, [42.0, 0.0], false, tex, ts, [255; 3], flags, light, true);
    entity_box(mesh, r_outer, [-8.0, 1.0, 0.0], [8.0, 12.0, 1.0], 0.0, [24.0, 16.0], false, tex, ts, [255; 3], flags, light, true);

    let l_wing = body_mat
        * Mat4::from_translation(Vec3::new(3.0, 4.0, 1.5))
        * Mat4::from_rotation_x(-0.15707964)
        * Mat4::from_rotation_y(1.2566371);
    let l_outer = l_wing
        * Mat4::from_translation(Vec3::new(10.0, 0.0, 0.0))
        * Mat4::from_rotation_y(1.7278761);
    entity_box(mesh, l_wing, [0.0, 1.0, 0.0], [10.0, 16.0, 1.0], 0.0, [42.0, 0.0], true, tex, ts, [255; 3], flags, light, true);
    entity_box(mesh, l_outer, [0.0, 1.0, 0.0], [8.0, 12.0, 1.0], 0.0, [24.0, 16.0], true, tex, ts, [255; 3], flags, light, true);
}

fn resolve_beam_target(c: &ModelContext<'_>, target_id: i32) -> Option<Vec3> {
    let s = &c.sim.server_scene;
    if let Some(p) = s.actors.get(&target_id) {
        return Some(p.previous.lerp(p.feet, c.alpha) + Vec3::Y * 0.9);
    }
    if let Some(m) = s.mobs.get(&target_id) {
        let sh = SPECIES.get(&m.species).map_or(1.0, |sp| sp.size[1]);
        return Some(m.previous.lerp(m.feet, c.alpha) + Vec3::Y * (sh * 0.5));
    }
    Some(c.player.previous.lerp(c.player.position, c.alpha) + Vec3::Y * 0.9)
}

fn draw_guardian_beam(
    d: &mut Dynamic,
    c: &ModelContext<'_>,
    start_eye: Vec3,
    target_pos: Vec3,
    age: f32,
) {
    let diff = target_pos - start_eye;
    let len = diff.length();
    if len <= 0.05 {
        return;
    }
    let dir = diff / len;
    let arbitrary = if dir.y.abs() < 0.99 { Vec3::Y } else { Vec3::X };
    let base_u = dir.cross(arbitrary).normalize_or_zero();
    let base_v = dir.cross(base_u).normalize_or_zero();

    let spin = age * 0.15;
    let (ss, cs) = spin.sin_cos();
    let axis1 = base_u * cs + base_v * ss;
    let axis2 = -base_u * ss + base_v * cs;

    // 80-tick Guardian laser charge cycle: purple -> golden yellow -> bright cyan/white
    let prog = ((age % 80.0) / 80.0).clamp(0.0, 1.0);
    let r = (64.0 + 191.0 * prog * prog).round().clamp(0.0, 255.0) as u8;
    let g = (255.0 * prog).round().clamp(0.0, 255.0) as u8;
    let b = (255.0 - 128.0 * (prog * std::f32::consts::PI).sin())
        .round()
        .clamp(0.0, 255.0) as u8;

    let sprite = c.resources.sprite("textures/entity/guardian_beam.png");
    let u_max = sprite.width as f32 / 256.0;
    let v_max = sprite.height as f32 / 256.0;
    let u_half = u_max * 0.5;
    let v_band = v_max * 0.5;

    let segments = (len * 2.0).ceil().clamp(1.0, 64.0) as usize;
    let step = len / segments as f32;
    let scroll = (age * 0.08).rem_euclid(v_band.max(1e-4));

    let opaque_layer = sprite.layer | ATLAS | crate::models::UNLIT;
    let glow_layer = sprite.layer | ATLAS | crate::models::BLEND | crate::models::UNLIT;

    for i in 0..segments {
        let p0 = start_eye + dir * (i as f32 * step);
        let p1 = start_eye + dir * ((i + 1) as f32 * step);
        let v0 = scroll;
        let v1 = (scroll + v_band * 0.85).min(v_max);

        for axis in [axis1, axis2] {
            let core_off = axis * 0.2;
            let q_front = [p0 - core_off, p0 + core_off, p1 + core_off, p1 - core_off];
            let q_back = [p0 + core_off, p0 - core_off, p1 - core_off, p1 + core_off];
            let rect = [0.0, v0, u_half, v1 - v0];
            crate::models::face(&mut d.opaque, q_front, rect, opaque_layer, [r, g, b, 255]);
            crate::models::face(&mut d.opaque, q_back, rect, opaque_layer, [r, g, b, 255]);

            let glow_off = axis * 0.28;
            let g_front = [p0 - glow_off, p0 + glow_off, p1 + glow_off, p1 - glow_off];
            let g_back = [p0 + glow_off, p0 - glow_off, p1 - glow_off, p1 + glow_off];
            let glow_rect = [u_half, v0, u_half, v1 - v0];
            crate::models::face(&mut d.translucent, g_front, glow_rect, glow_layer, [r, g, b, 110]);
            crate::models::face(&mut d.translucent, g_back, glow_rect, glow_layer, [r, g, b, 110]);
        }
    }
}

#[cfg(test)]
#[path = "mob_render_tests.rs"]
mod tests;
pub fn draw(d: &mut Dynamic, c: &ModelContext<'_>, a: &Actor, burning: bool) {
    draw_at(
        d,
        c,
        a,
        burning,
        (c.sim.total_ticks % 240000) as f32 + c.alpha,
    );
}
pub(crate) fn visual_cull_bounds(
    species: u8,
    appearance: Appearance,
    lo: Vec3,
    hi: Vec3,
) -> (Vec3, Vec3) {
    let s = appearance.scale(species);
    let (pad_lo, pad_hi) = match species {
        101 => (
            Vec3::new(0.4 * s, 0.05 * s, 0.4 * s),
            Vec3::new(0.4 * s, 0.4 * s, 0.4 * s),
        ),
        94 => (Vec3::new(0.3, 1.3, 0.3), Vec3::new(0.3, 0.1, 0.3)),
        56 => (Vec3::new(0.5, 3.8, 0.5), Vec3::new(0.5, 0.2, 0.5)),
        63 => (Vec3::new(0.5, 2.5, 0.5), Vec3::new(0.5, 0.5, 0.5)),
        68 => (
            Vec3::new(1.6 * s, 0.0, 1.6 * s),
            Vec3::new(1.6 * s, 0.2 * s, 1.6 * s),
        ),
        100 => (
            Vec3::new(1.25 * s, 0.0, 1.25 * s),
            Vec3::new(1.25 * s, 0.6 * s, 1.25 * s),
        ),
        64 | 98 => (
            Vec3::new(1.1 * s, 0.0, 1.1 * s),
            Vec3::new(1.1 * s, 0.3 * s, 1.1 * s),
        ),
        66 => (Vec3::new(0.35, 0.0, 0.35), Vec3::new(0.35, 0.65, 0.35)),
        _ => (
            Vec3::new(0.35 * s, 0.0, 0.35 * s),
            Vec3::new(0.35 * s, 0.35 * s, 0.35 * s),
        ),
    };
    (lo - pad_lo, hi + pad_hi)
}

fn draw_at(d: &mut Dynamic, c: &ModelContext<'_>, a: &Actor, burning: bool, age: f32) {
    d.total += 1;
    let feet = a.previous.lerp(a.feet, c.alpha);
    let spec = &SPECIES[&a.species];
    let [w, h, _] = a.appearance.size(a.species).unwrap_or(spec.size);
    let beam_end = if a.species == 68 {
        a.beam_target_pos
            .or_else(|| a.beam_target.and_then(|tid| resolve_beam_target(c, tid)))
    } else {
        None
    };
    let lo = feet - Vec3::new(w * 0.5, 0., w * 0.5);
    let hi = feet + Vec3::new(w * 0.5, h, w * 0.5);
    let (cull_lo, cull_hi) = visual_cull_bounds(a.species, a.appearance, lo, hi);
    if a.species != 63
        && (feet.distance_squared(c.eye)
            > (c.settings.entity_distance * c.settings.entity_distance) as f32
            || (!c.frustum.visible(cull_lo, cull_hi) && beam_end.is_none())
            || (c.settings.entity_culling
                && beam_end.is_none()
                && crate::models::occluded_bounds(c.world, c.eye, cull_lo, cull_hi)))
    {
        d.culled += 1;
        return;
    }
    d.visible += 1;
    let start = d.opaque.vertices.len();
    let baby = a.appearance.baby(a.species);
    let (mut yaw, phase, walk) = a.gait.sample(a.yaw.to_radians(), c.alpha, baby);
    if a.species == 63 {
        if let Some(flight) = &a.dragon_flight {
            yaw = (flight.movement_offsets(7, c.alpha, a.dragon_death_ticks > 0)[0] as f32)
                .to_radians();
        }
    }
    let roll_z = if a.species == 63 {
        0.0
    } else {
        crate::models::death_roll(a.death_time, c.alpha)
    };
    let hurt = if a.species == 63 {
        a.hurt && a.dragon_death_ticks == 0
    } else {
        a.hurt || a.death_time > 0
    };
    let tex = c.resources.sprite(a.appearance.texture(a.species));
    let light = c.world.visual_light_uv(
        (feet + Vec3::Y * (h * 0.85)).floor().as_ivec3(),
        if matches!(a.species, 61 | 62) { 15 } else { 0 },
    );
    let flags = ATLAS
        | if hurt {
            crate::entity_visual::HURT
        } else {
            0
        };

    let mut mat = Mat4::from_translation(feet)
        * Mat4::from_rotation_y(-yaw)
        * Mat4::from_rotation_z(roll_z)
        * Mat4::from_scale(Vec3::splat(a.appearance.scale(a.species)));
    if matches!(a.species, 55 | 62) {
        let squish = a.prev_slime_squish + (a.slime_squish - a.prev_slime_squish) * c.alpha;
        let sxz = (1.0 + squish).clamp(0.35, 2.5);
        mat *= Mat4::from_scale(Vec3::new(sxz, 1.0 / sxz, sxz));
    }

    if a.species == 100 {
        if !a.invisible {
            let saddled = a.appearance.flags & 4 != 0;
            let chested = a.appearance.flags & 8 != 0;
            let ridden = (a.appearance.flags & 256 != 0) || has_mounted_rider(c, feet, h);
            crate::horse_model::draw_horse(
                &mut d.opaque,
                c.resources,
                crate::horse_model::HorseDrawInput {
                    feet,
                    body_yaw_rad: yaw,
                    head_yaw_deg: a.head_yaw,
                    pitch_deg: a.pitch,
                    limb_swing: phase,
                    limb_amount: walk,
                    age_in_ticks: age,
                    alpha: c.alpha,
                    roll_z,
                    subtype: a.appearance.subtype,
                    baby,
                    saddled,
                    chested,
                    ridden,
                    base_texture: a.appearance.texture(100),
                    markings_texture: a.appearance.horse_markings(),
                    horse_armor: a.horse_armor,
                    anim: a.horse_anim,
                    light,
                    hurt_flag: flags & crate::entity_visual::HURT,
                },
            );
        }
    } else if a.species == 99 {
        if !a.invisible {
            let body_deg = yaw.to_degrees();
            let rel_head_yaw = a
                .head_yaw
                .map(|hy| ((hy - body_deg + 180.0).rem_euclid(360.0) - 180.0).clamp(-75.0, 75.0))
                .unwrap_or(0.0);
            crate::gameplay::ai::golem::model::draw_living(
                &mut d.opaque,
                crate::gameplay::ai::golem::model::Frame {
                    position: feet,
                    yaw: body_deg,
                    head_yaw: rel_head_yaw,
                    head_pitch: a.pitch,
                    swing: a.gait.swing,
                    limb_amount: a.gait.amount,
                    previous_amount: a.gait.previous_amount,
                    partial: c.alpha,
                    attack: i32::from(a.attack_ticks),
                    rose: i32::from(a.rose_ticks),
                },
                roll_z,
                tex,
                flags,
                light,
                c.assets,
                c.resources,
            );
        }
    } else if matches!(a.species, 51 | 53 | 54 | 57) {
        let entity_scale = if a.species == 53 {
            6.0
        } else if a.species == 51 && a.appearance.subtype == 1 {
            1.2
        } else {
            1.0
        };
        crate::models::draw_biped_mob(
            &mut d.opaque,
            c,
            crate::models::BipedMobInput {
                species: a.species,
                subtype: a.appearance.subtype,
                baby,
                entity_scale,
                feet,
                body_yaw_java_rad: yaw,
                head_yaw_java_deg: a.head_yaw,
                pitch_deg: a.pitch,
                limb_swing: phase,
                limb_amount: walk,
                age,
                alpha: c.alpha,
                swing_ticks: a.swing_ticks,
                roll_z,
                hurt,
                invisible: a.invisible,
                light,
                texture: a.appearance.texture(a.species),
                held: a.held.as_ref(),
                armor: [
                    a.armor[0].as_ref(),
                    a.armor[1].as_ref(),
                    a.armor[2].as_ref(),
                    a.armor[3].as_ref(),
                ],
            },
        );
    } else if a.species == 95 {
        if !a.invisible {
            draw_wolf(&mut d.opaque, c, a, feet, yaw, phase, walk, roll_z, tex, flags, light);
        }
    } else if a.species == 98 && a.appearance.flags & 1 != 0 {
        if !a.invisible {
            draw_sitting_ocelot(&mut d.opaque, a, feet, yaw, roll_z, tex, flags, light);
        }
    } else if a.species == 58 {
        let jitter = enderman_scream_jitter(a, feet, age, c.alpha);
        let jittered_feet = feet + jitter;
        let jittered_mat = Mat4::from_translation(jitter) * mat;
        if !a.invisible {
            draw_enderman(
                &mut d.opaque,
                c,
                a,
                jittered_feet,
                yaw,
                phase,
                walk,
                age,
                roll_z,
                tex,
                flags,
                light,
            );
        }
        block_layers(
            &mut d.opaque,
            c,
            a,
            jittered_mat,
            light,
            flags & crate::entity_visual::HURT,
        );
    } else if a.species == 65 && a.appearance.flags & 1 != 0 {
        if !a.invisible {
            draw_roosting_bat(&mut d.opaque, feet, yaw, roll_z, tex, flags, light);
        }
    } else {
        let poses = if a.species == 54 && a.appearance.subtype == 1 {
            &POSES[&54]
        } else {
            &spec.frames
        };
        let rest = &poses[0];
        let radians = age / std::f32::consts::TAU;
        let (frames, cycle, amount) = match a.species {
            56 => (&POSES[&56][..], radians * 0.3, 1.),
            60 | 67 => (&POSES[&a.species][..], radians * 0.9, 1.),
            61 => (&POSES[&61][..], age / 200., 1.),
            63 => (
                &POSES[&63][..],
                a.dragon_wing_phase.unwrap_or(age * 0.05),
                1.,
            ),
            64 => (&POSES[&64][..], radians * 0.1, 1.),
            65 if a.appearance.flags & 1 == 0 => (&POSES[&65][..], radians * 1.3, 1.),
            68 => (&POSES[&68][..], radians * 0.125, 1.),
            94 => (&POSES[&94][..], radians * 0.15, 1.),
            99 => (&poses[1..], phase / 13., walk),
            101 => (&POSES[&101][..], phase / 4., walk),
            _ => (&poses[1..], phase * 0.6662 / std::f32::consts::TAU, walk),
        };
        let frame = (cycle * frames.len() as f32).rem_euclid(frames.len() as f32);
        let index = frame as usize % frames.len();
        let from = &frames[index];
        let to = &frames[(index + 1) % frames.len()];
        let mut dragon_corpse_mat = mat;
        if a.species == 65 {
            mat *= Mat4::from_translation(Vec3::Y * ((age * 0.3).cos() * 0.1));
        } else if a.species == 63 {
            let mut pitch_rad = 0.0;
            let mut bank_rad = 0.0;
            if let Some(flight) = &a.dragon_flight {
                let dead = a.dragon_death_ticks > 0;
                let off0 = flight.movement_offsets(0, c.alpha, dead);
                let off5 = flight.movement_offsets(5, c.alpha, dead);
                let off10 = flight.movement_offsets(10, c.alpha, dead);
                let dy = (off5[1] - off10[1]) as f32;
                pitch_rad = (dy * 10.0).clamp(-45.0, 45.0).to_radians();
                let mut dyaw = ((off5[0] - off0[0]) % 360.0) as f32;
                if dyaw >= 180.0 {
                    dyaw -= 360.0;
                } else if dyaw < -180.0 {
                    dyaw += 360.0;
                }
                bank_rad = -(dyaw * 1.5).clamp(-45.0, 45.0).to_radians();
            }
            dragon_corpse_mat =
                mat * Mat4::from_rotation_x(pitch_rad) * Mat4::from_translation(Vec3::Z);
            mat = dragon_corpse_mat
                * Mat4::from_rotation_y(std::f32::consts::PI)
                * Mat4::from_rotation_z(bank_rad);
        } else if a.species == 94 {
            mat *= Mat4::from_translation(Vec3::Y * -0.7);
        }
        let pose = Pose {
            rest,
            from,
            to,
            amount,
            fraction: frame.fract(),
            double_sided: a.species == 65,
        };
        if a.species == 63 && a.dragon_death_ticks > 0 {
            let f = ((a.dragon_death_ticks as f32 + c.alpha) / 200.0).clamp(0.0, 1.0);
            let thresh = (f * 255.0).round().clamp(0.0, 255.0) as u8;
            let body_alpha = ((1.0 - f) * 255.0).round().clamp(0.0, 255.0) as u8;
            if !a.invisible {
                pose.draw_alpha(
                    &mut d.opaque,
                    mat,
                    c.resources
                        .sprite("textures/entity/enderdragon/dragon_exploding.png"),
                    [240, 240, thresh, 0],
                    ATLAS | crate::models::UNLIT | crate::models::NO_MIPMAP | 67108864,
                    255,
                );
                if body_alpha > 0 {
                    pose.draw_alpha(
                        &mut d.translucent,
                        mat,
                        tex,
                        light,
                        flags | crate::models::BLEND,
                        body_alpha,
                    );
                }
            }
            draw_ender_dragon_death_beams(&mut d.translucent, dragon_corpse_mat, f);
        } else if !a.invisible {
            pose.draw(&mut d.opaque, mat, tex, light, flags);
            let eyes = match a.species {
                58 => Some("textures/entity/enderman/enderman_eyes.png"),
                59 => Some("textures/entity/spider_eyes.png"),
                63 => Some("textures/entity/enderdragon/dragon_eyes.png"),
                _ => None,
            };
            if let Some(path) = eyes {
                pose.draw(
                    &mut d.opaque,
                    mat,
                    c.resources.sprite(path),
                    [240, 240, 0, 0],
                    ATLAS | crate::models::UNLIT,
                );
            }
        }
        block_layers(
            &mut d.opaque,
            c,
            a,
            mat,
            light,
            flags & crate::entity_visual::HURT,
        );
        if !a.invisible && a.species == 55 {
            let gel = &POSES[&55][0];
            Pose {
                rest: gel,
                from: gel,
                to: gel,
                amount: 0.,
                fraction: 0.,
                double_sided: false,
            }
            .draw(
                &mut d.translucent,
                mat,
                tex,
                light,
                flags | crate::models::BLEND,
            );
        } else if !a.invisible && a.species == 62 {
            // Fullbright glowing molten core inside Magma Cube
            let core_mat = mat
                * Mat4::from_scale(Vec3::new(1.0, -1.0, -1.0))
                * Mat4::from_translation(Vec3::new(0.0, -1.5078125, 0.0))
                * Mat4::from_scale(Vec3::splat(0.0625));
            entity_box(
                &mut d.opaque,
                core_mat,
                [-2.0, 18.0, -2.0],
                [4.0, 4.0, 4.0],
                0.02,
                [0.0, 16.0],
                false,
                tex,
                [64.0, 32.0],
                [255; 3],
                ATLAS | crate::models::UNLIT | (flags & crate::entity_visual::HURT),
                [240, 240, 0, 0],
                false,
            );
        }
    }

    if let Some(target_pos) = beam_end {
        draw_guardian_beam(d, c, feet + Vec3::Y * (h * 0.5), target_pos, age);
    }

    let blaze_charging = a.species == 61 && a.appearance.flags & 1 != 0;
    if burning || blaze_charging {
        c.fire(&mut d.opaque, start, feet, w, h);
    }
    if !a.invisible && !(a.species == 63 && a.dragon_death_ticks > 0) {
        crate::models::shadow_radius(
            &mut d.translucent,
            c,
            feet,
            mob_shadow_size(a.species, a.appearance),
            1.0,
            false,
        );
    }
}

fn draw_ender_dragon_death_beams(
    mesh: &mut crate::mesh::Mesh,
    dragon_corpse_mat: Mat4,
    f: f32,
) {
    let f1 = if f > 0.8 {
        ((f - 0.8) / 0.2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let center_alpha = (255.0 * (1.0 - f1)).clamp(0.0, 255.0) as u8;
    if center_alpha == 0 {
        return;
    }
    let mut beam_mat = dragon_corpse_mat
        * Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0))
        * Mat4::from_translation(Vec3::new(0.0, -2.5078125, -2.0));
    let mut rng = crate::vanilla::random::JavaRandom::new(432);
    let layer = crate::models::SOLID | crate::models::UNLIT | crate::models::BLEND;
    let light = [240, 240, 0, 0];
    let limit = (f + f * f) * 0.5 * 60.0;
    let mut n = 0usize;
    while (n as f32) < limit && n < 60 {
        beam_mat *= Mat4::from_rotation_x((rng.next_float() * 360.0).to_radians());
        beam_mat *= Mat4::from_rotation_y((rng.next_float() * 360.0).to_radians());
        beam_mat *= Mat4::from_rotation_z((rng.next_float() * 360.0).to_radians());
        beam_mat *= Mat4::from_rotation_x((rng.next_float() * 360.0).to_radians());
        beam_mat *= Mat4::from_rotation_y((rng.next_float() * 360.0).to_radians());
        beam_mat *= Mat4::from_rotation_z((rng.next_float() * 360.0 + f * 90.0).to_radians());
        let f2 = rng.next_float() * 20.0 + 5.0 + f1 * 10.0;
        let f3 = rng.next_float() * 2.0 + 1.0 + f1 * 2.0;

        let p0 = beam_mat.transform_point3(Vec3::ZERO);
        let p1 = beam_mat.transform_point3(Vec3::new(-0.866 * f3, f2, -0.5 * f3));
        let p2 = beam_mat.transform_point3(Vec3::new(0.866 * f3, f2, -0.5 * f3));
        let p3 = beam_mat.transform_point3(Vec3::new(0.0, f2, f3));

        let base = mesh.vertices.len() as u32;
        mesh.vertices.extend([
            Vertex {
                position: p0.to_array(),
                uv: [0.0, 0.0],
                layer,
                color: [255, 255, 255, center_alpha],
                light,
            },
            Vertex {
                position: p1.to_array(),
                uv: [0.0, 0.0],
                layer,
                color: [255, 0, 255, 0],
                light,
            },
            Vertex {
                position: p2.to_array(),
                uv: [0.0, 0.0],
                layer,
                color: [255, 0, 255, 0],
                light,
            },
            Vertex {
                position: p3.to_array(),
                uv: [0.0, 0.0],
                layer,
                color: [255, 0, 255, 0],
                light,
            },
        ]);
        mesh.indices.extend(
            [
                0, 1, 2, 0, 2, 1, //
                0, 2, 3, 0, 3, 2, //
                0, 3, 1, 0, 1, 3,
            ]
            .map(|k| base + k),
        );
        mesh.visible_faces += 3;
        n += 1;
    }
}

/// LayerMooshroomMushroom, LayerSnowmanHead and LayerHeldBlock render baked
/// block quads from the active pack in the living model's flipped coordinates.
fn block_layers(
    mesh: &mut crate::mesh::Mesh,
    c: &ModelContext<'_>,
    a: &Actor,
    mat: Mat4,
    light: [u8; 4],
    hurt: u32,
) {
    if a.invisible && a.species != 58 {
        return;
    }
    block_layers_for(mesh, c, a.species, a.appearance, mat, light, hurt);
}

fn block_layers_for(
    mesh: &mut crate::mesh::Mesh,
    c: &ModelContext<'_>,
    species: u8,
    appearance: Appearance,
    mat: Mat4,
    light: [u8; 4],
    hurt: u32,
) {
    if !matches!(species, 58 | 96 | 97) {
        return;
    }
    let tr = |x, y, z| Mat4::from_translation(Vec3::new(x, y, z));
    let ry = |d: f32| Mat4::from_rotation_y(d.to_radians());
    let scale = |x, y, z| Mat4::from_scale(Vec3::new(x, y, z));
    let root = mat * scale(1., -1., -1.) * tr(0., -1.5078125, 0.);
    let mut block = |state, transform: Mat4| {
        let Some(faces) = c.assets.block_models.carried(state) else {
            return;
        };
        for f in faces {
            let start = mesh.vertices.len() as u32;
            for v in f.vertices {
                mesh.vertices.push(Vertex {
                    position: transform.transform_point3(v.position).to_array(),
                    uv: v.uv,
                    layer: f.layer | hurt,
                    color: [f.shade, f.shade, f.shade, 255],
                    light,
                });
            }
            let indices = if transform.determinant() < 0. {
                [0, 2, 1, 0, 3, 2]
            } else {
                [0, 1, 2, 0, 2, 3]
            };
            mesh.indices.extend(indices.map(|i| start + i));
            mesh.visible_faces += 1;
        }
    };
    match species {
        96 if !appearance.baby(96) => {
            let back = root * scale(1., -1., 1.) * tr(0.2, 0.35, 0.5) * ry(42.);
            let centered = tr(-0.5, -0.5, 0.5) * ry(90.);
            block(40 << 4, back * centered);
            block(40 << 4, back * tr(0.1, 0., -0.6) * ry(42.) * centered);
            block(
                40 << 4,
                root * tr(0., 0.25, -0.5)
                    * scale(1., -1., 1.)
                    * tr(0., 0.7, -0.2)
                    * ry(12.)
                    * centered,
            );
        }
        97 => block(
            // The inventory model is block/pumpkin (north facing); world metadata 0 faces south.
            (86 << 4) | 2,
            root * tr(0., 0.25 - 0.34375, 0.)
                * ry(180.)
                * scale(0.625, -0.625, -0.625)
                * tr(-0.5, -0.5, -0.5),
        ),
        58 if appearance.variant > 0 => block(
            appearance.variant as u16,
            root * tr(0., 0.6875, -0.75)
                * Mat4::from_rotation_x(20_f32.to_radians())
                * ry(45.)
                * tr(0.25, 0.1875, 0.25)
                * scale(-0.5, -0.5, 0.5)
                * ry(90.),
        ),
        _ => (),
    }
}

pub(crate) fn draw_spawner_mob(
    d: &mut Dynamic,
    c: &ModelContext<'_>,
    species: u8,
    appearance: Appearance,
    cage_mat: Mat4,
    light: [u8; 4],
    age: f32,
) {
    let Some(spec) = SPECIES.get(&species) else {
        return;
    };
    let mut mat = cage_mat;
    if appearance.baby(species) {
        mat *= Mat4::from_scale(Vec3::splat(0.5));
    }
    let poses = if species == 54 && appearance.subtype == 1 {
        &POSES[&54]
    } else {
        &spec.frames
    };
    let rest = &poses[0];
    let radians = age / std::f32::consts::TAU;
    let (frames, cycle, amount) = match species {
        56 => (&POSES[&56][..], radians * 0.3, 1.0),
        60 | 67 => (&POSES[&species][..], radians * 0.9, 1.0),
        61 => (&POSES[&61][..], age / 200.0, 1.0),
        63 => (&POSES[&63][..], age * 0.05, 1.0),
        64 => (&POSES[&64][..], radians * 0.1, 1.0),
        65 if appearance.flags & 1 == 0 => (&POSES[&65][..], radians * 1.3, 1.0),
        68 => (&POSES[&68][..], radians * 0.125, 1.0),
        94 => (&POSES[&94][..], radians * 0.15, 1.0),
        _ => (&poses[..1], 0.0, 0.0),
    };
    let frame = (cycle * frames.len() as f32).rem_euclid(frames.len() as f32);
    let index = frame as usize % frames.len();
    let from = &frames[index];
    let to = &frames[(index + 1) % frames.len()];
    if species == 65 {
        mat *= Mat4::from_translation(Vec3::Y * ((age * 0.3).cos() * 0.1));
    } else if species == 63 {
        mat *= Mat4::from_translation(Vec3::Z) * Mat4::from_rotation_y(std::f32::consts::PI);
    } else if species == 94 {
        mat *= Mat4::from_translation(Vec3::Y * -0.7);
    }
    let flags = ATLAS;
    let tex = c.resources.sprite(appearance.texture(species));
    let pose = Pose {
        rest,
        from,
        to,
        amount,
        fraction: frame.fract(),
        double_sided: species == 65,
    };
    pose.draw(&mut d.opaque, mat, tex, light, flags);
    if species == 100 {
        if let Some(markings) = appearance.horse_markings() {
            pose.draw(
                &mut d.opaque,
                mat,
                c.resources.sprite(markings),
                light,
                ATLAS | crate::models::BLEND,
            );
        }
    }
    let eyes = match species {
        52 | 59 => Some("textures/entity/spider_eyes.png"),
        58 => Some("textures/entity/enderman/enderman_eyes.png"),
        63 => Some("textures/entity/enderdragon/dragon_eyes.png"),
        _ => None,
    };
    if let Some(path) = eyes {
        pose.draw(
            &mut d.opaque,
            mat,
            c.resources.sprite(path),
            [240, 240, 0, 0],
            ATLAS | crate::models::UNLIT,
        );
    }
    block_layers_for(&mut d.opaque, c, species, appearance, mat, light, 0);
    if species == 55 {
        let gel = &POSES[&55][0];
        Pose {
            rest: gel,
            from: gel,
            to: gel,
            amount: 0.0,
            fraction: 0.0,
            double_sided: false,
        }
        .draw(
            &mut d.translucent,
            mat,
            tex,
            light,
            flags | crate::models::BLEND,
        );
    } else if species == 62 {
        let core_mat = mat
            * Mat4::from_scale(Vec3::new(1.0, -1.0, -1.0))
            * Mat4::from_translation(Vec3::new(0.0, -1.5078125, 0.0))
            * Mat4::from_scale(Vec3::splat(0.0625));
        entity_box(
            &mut d.opaque,
            core_mat,
            [-2.0, 18.0, -2.0],
            [4.0, 4.0, 4.0],
            0.02,
            [0.0, 16.0],
            false,
            tex,
            [64.0, 32.0],
            [255; 3],
            ATLAS | crate::models::UNLIT,
            [240, 240, 0, 0],
            false,
        );
    }
}

struct Pose<'a> {
    rest: &'a [Tri],
    from: &'a [Tri],
    to: &'a [Tri],
    amount: f32,
    fraction: f32,
    double_sided: bool,
}
impl Pose<'_> {
    fn draw(
        &self,
        mesh: &mut crate::mesh::Mesh,
        mat: Mat4,
        tex: crate::resources::Sprite,
        light: [u8; 4],
        flags: u32,
    ) {
        self.draw_alpha(mesh, mat, tex, light, flags, 255);
    }

    fn draw_alpha(
        &self,
        mesh: &mut crate::mesh::Mesh,
        mat: Mat4,
        tex: crate::resources::Sprite,
        light: [u8; 4],
        flags: u32,
        alpha: u8,
    ) {
        let uv = [
            tex.width as f32 / (4096. * 256.),
            tex.height as f32 / (4096. * 256.),
        ];
        let blend = self.amount > 0.
            && self.rest.len() == self.from.len()
            && self.rest.len() == self.to.len();
        let rig = blend
            .then(|| rigid::RIGS.get(&(self.rest.as_ptr() as usize)))
            .flatten();
        let samples = rig.and_then(|r| {
            Some((
                r.frames.get(&(self.from.as_ptr() as usize))?,
                r.frames.get(&(self.to.as_ptr() as usize))?,
            ))
        });
        for (part, triangles) in self.rest.chunks(12).enumerate() {
            let transform = if let (Some(rig), Some((from, to))) = (rig, samples) {
                mat * rig.parts[part].matrix(from[part], to[part], self.fraction, self.amount)
            } else {
                mat
            };
            for tri in triangles {
                let mut pos = [Vec3::ZERO; 3];
                for (j, p) in pos.iter_mut().enumerate() {
                    *p = transform.transform_point3(rigid::point(tri, j));
                }
                let normal = (pos[1] - pos[0]).cross(pos[2] - pos[0]).normalize_or_zero();
                let shade = if flags & crate::models::UNLIT != 0 {
                    1.0
                } else {
                    crate::models::entity_face_shade(normal)
                };
                let base = mesh.vertices.len() as u32;
                for (j, p) in pos.into_iter().enumerate() {
                    mesh.vertices.push(Vertex {
                        position: p.to_array(),
                        uv: [tri[j * 5 + 3] as f32 * uv[0], tri[j * 5 + 4] as f32 * uv[1]],
                        color: [
                            (255. * shade) as u8,
                            (255. * shade) as u8,
                            (255. * shade) as u8,
                            alpha,
                        ],
                        layer: tex.layer | flags,
                        light,
                    });
                }
                mesh.indices.extend([base, base + 1, base + 2]);
                // RendererLivingEntity disables culling. The bat texture leaves
                // the opposite wing-box face transparent, so its textured face
                // must also remain visible from behind throughout the flap.
                if self.double_sided {
                    mesh.indices.extend([base, base + 2, base + 1]);
                }
                mesh.visible_faces += 1;
            }
        }
    }
}
