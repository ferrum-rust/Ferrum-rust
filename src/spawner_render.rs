//! 1.8.9 `TileEntityMobSpawnerRenderer` miniature spinning mob inside block 52 cages.
use crate::{
    gameplay::MobKind,
    models::{Dynamic, ModelContext},
    remote_mobs::{draw_spawner_mob, Appearance},
    vanilla::{
        drops::DropNbt,
        item_nbt::{compound, number},
    },
};
use glam::{IVec3, Mat4, Vec3};

fn resolve_species(entity_id: &str) -> Option<u8> {
    let trimmed = entity_id.trim();
    let base = trimmed.strip_prefix("minecraft:").unwrap_or(trimmed);
    if let Some(kind) = MobKind::from_name(base) {
        return Some(kind.vanilla_id());
    }
    match base {
        "MagmaCube" | "magma_cube" => Some(62),
        "PigZombie" | "zombie_pigman" => Some(57),
        "CaveSpider" | "cave_spider" => Some(59),
        "EnderDragon" | "ender_dragon" => Some(63),
        "Wither" | "wither" => Some(64),
        "Mooshroom" | "mooshroom" => Some(96),
        "SnowGolem" | "snowman" => Some(97),
        "Ocelot" | "ocelot" => Some(98),
        "IronGolem" | "villager_golem" => Some(99),
        "Horse" | "horse" => Some(100),
        _ => None,
    }
}

fn parse_spawner_nbt(tile: Option<&DropNbt>) -> (u8, Appearance) {
    let root = tile.and_then(compound);
    let spawn_data = root.and_then(|r| r.get("SpawnData")).and_then(compound);
    let entity_name = root
        .and_then(|r| match r.get("EntityId") {
            Some(DropNbt::String(s)) if !s.trim().is_empty() => Some(s.as_str()),
            _ => None,
        })
        .or_else(|| {
            spawn_data.and_then(|sd| match sd.get("id") {
                Some(DropNbt::String(s)) if !s.trim().is_empty() => Some(s.as_str()),
                _ => None,
            })
        })
        .unwrap_or("Pig");
    let species = resolve_species(entity_name).unwrap_or(90);
    let Some(sd) = spawn_data else {
        return (
            species,
            Appearance {
                body: 1,
                ..Appearance::default()
            },
        );
    };
    let value = |key: &str| number(sd.get(key));
    let subtype = match species {
        51 => value("SkeletonType"),
        54 => i32::from(value("IsVillager") != 0 || value("ZombieType") > 0),
        98 => value("CatType"),
        100 => value("Type"),
        101 => value("RabbitType"),
        120 => value("Profession"),
        _ => 0,
    } as u8;
    let age = if matches!(species, 54 | 57) {
        i8::from(value("IsBaby") != 0)
    } else if matches!(species, 90..=93 | 95 | 96 | 98 | 100 | 101 | 120) {
        value("Age").signum() as i8
    } else {
        0
    };
    let has_owner = matches!(
        sd.get("OwnerUUID").or_else(|| sd.get("Owner")),
        Some(DropNbt::String(owner)) if !owner.is_empty()
    );
    let flags = match species {
        56 => i32::from(value("Attacking") != 0),
        58 => i32::from(value("screaming") != 0),
        61 => i32::from(value("OnFire") != 0),
        65 => value("BatFlags"),
        68 => 4 * i32::from(value("Elder") != 0),
        95 => {
            i32::from(value("Sitting") != 0)
                | (2 * i32::from(value("Angry") != 0))
                | (4 * i32::from(has_owner))
        }
        98 => i32::from(value("Sitting") != 0) | (4 * i32::from(has_owner)),
        _ => 0,
    };
    let variant = if species == 58 {
        let block = match sd.get("carried") {
            Some(DropNbt::String(name)) => crate::world_types::blocks()
                .get(name.strip_prefix("minecraft:").unwrap_or(name))
                .map_or(0, |v| i32::from(v.0)),
            _ => value("carried"),
        };
        (block << 4) | (value("carriedData") & 15)
    } else {
        value("Variant")
    };
    let body = if sd.contains_key("Size") {
        value("Size").saturating_add(1).clamp(1, 16) as u8
    } else {
        1
    };
    (
        species,
        Appearance {
            age,
            body,
            variant,
            flags,
            subtype,
        },
    )
}

pub(super) fn render(d: &mut Dynamic, c: &ModelContext<'_>) {
    let age = (c.sim.total_ticks % 240000) as f32 + c.alpha;
    let cam_cx = (c.eye.x.floor() as i32).div_euclid(16);
    let cam_cz = (c.eye.z.floor() as i32).div_euclid(16);
    for dz in -5..=5 {
        for dx in -5..=5 {
            let (cx, cz) = (cam_cx + dx, cam_cz + dz);
            let Some(chunk) = c.world.chunks.get(&(cx, cz)) else {
                continue;
            };
            let cdx = (cx * 16 + 8) as f32 - c.eye.x;
            let cdz = (cz * 16 + 8) as f32 - c.eye.z;
            if cdx * cdx + cdz * cdz > 76.0 * 76.0 {
                continue;
            }
            for &packed in chunk.spawner_positions() {
                let lx = i32::from(packed & 15);
                let lz = i32::from((packed >> 4) & 15);
                let y = i32::from(packed >> 8);
                if chunk.get(lx, y, lz) >> 4 != 52 {
                    continue;
                }
                let pos = IVec3::new(cx * 16 + lx, y, cz * 16 + lz);
                let min = pos.as_vec3();
                if (min + Vec3::splat(0.5)).distance_squared(c.eye) >= 4096.0
                    || !c.frustum.visible(min, (pos + IVec3::ONE).as_vec3())
                {
                    continue;
                }
                let (species, appearance) = parse_spawner_nbt(c.world.tile(pos));
                let deg = c.sim.spawner_spin_degrees(pos, c.alpha);
                let extra_scale = match species {
                    59 => 0.7,
                    51 if appearance.subtype == 1 => 1.2,
                    55 | 62 => f32::from(appearance.body.max(1)),
                    56 => 0.5,
                    63 => 0.25,
                    68 if appearance.flags & 4 != 0 => 2.35,
                    _ => 1.0,
                };
                let cage_mat = Mat4::from_translation(min + Vec3::new(0.5, 0.4, 0.5))
                    * Mat4::from_rotation_y(deg.to_radians())
                    * Mat4::from_rotation_x((-30.0_f32).to_radians())
                    * Mat4::from_translation(Vec3::new(0.0, -0.4, 0.0))
                    * Mat4::from_scale(Vec3::splat(0.4375 * extra_scale));
                let light = if matches!(species, 61 | 62) {
                    [240, 240, 0, 0]
                } else {
                    c.world.visual_light_uv(pos, 0)
                };
                draw_spawner_mob(d, c, species, appearance, cage_mat, light, age);
            }
        }
    }
}
