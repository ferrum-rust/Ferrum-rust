//! Bootstrap a renderer view through the serial native session. The loading
//! worker waits for ordered replies; the resulting chunks are immutable.
use crate::{
    gameplay::{Mob, MobKind, Simulation},
    native_session::{Command, Event, GeneratedSpawn, NativeSession, Reply, SpawnRecord, Update},
    player_save::PlayerSave,
    world::World,
    Options,
};
use anyhow::{bail, ensure, Result};
use glam::{IVec3, Vec3};

fn request(session: &mut NativeSession, command: Command) -> Result<(Reply, Update)> {
    let id = session.submit(command)?;
    match session.receive()? {
        Event::Completed {
            request_id,
            reply,
            update,
        } if id == request_id => Ok((reply, update)),
        Event::Failed { message, .. } => bail!("{message}"),
        event => bail!("Unexpected native world reply for request {id}: {event:?}"),
    }
}
fn apply(world: &mut World, spawns: &mut Vec<SpawnRecord>, update: Update, radius: i32) {
    let center = world.spawn.as_ivec3();
    for ((x, z), chunk) in update.chunks {
        if (x - (center.x >> 4)).abs() <= radius && (z - (center.z >> 4)).abs() <= radius {
            world.chunks.insert((x, z), chunk);
        }
    }
    spawns.extend(update.metadata.spawned);
}
/// Project the native generator's entity records without rerolling placement.
pub(crate) fn display_spawns(world: &World, sim: &mut Simulation, spawns: Vec<SpawnRecord>) {
    for record in spawns {
        if let GeneratedSpawn::ChestMinecart(cart) = record.spawn {
            let pos = glam::DVec3::from_array(cart.position).as_vec3();
            if world.contains(pos) {
                sim.adopt_mineshaft_cart(cart);
            }
            continue;
        }
        let id = (u64::from(record.family) << 56) | record.index;
        let mut variant = None;
        let (kind, pos, yaw, profession) = match record.spawn {
            GeneratedSpawn::Villager(v) => (
                MobKind::Villager,
                IVec3::from_array(v.position).as_vec3() + Vec3::new(0.5, 0., 0.5),
                0.,
                v.profession,
            ),
            GeneratedSpawn::Animal(a) => {
                let kind = MobKind::from(a.kind);
                if let Some(color) = a.sheep_color { variant = Some(("Color", color as i8)); }
                (
                    kind,
                    glam::DVec3::from_array(a.position).as_vec3(),
                    a.yaw,
                    None,
                )
            }
            GeneratedSpawn::Witch(p) => (MobKind::Witch, IVec3::from_array(p).as_vec3() + Vec3::new(0.5,0.,0.5), 0., None),
            GeneratedSpawn::ElderGuardian(p) => {
                variant = Some(("Elder", 1));
                (MobKind::Guardian, IVec3::from_array(p).as_vec3() + Vec3::new(0.5,0.,0.5), 0., None)
            }
            GeneratedSpawn::Feature(e) if e.kind == crate::vanilla::population::GeneratedEntityKind::EnderDragon =>
                (MobKind::EnderDragon, glam::DVec3::from_array(e.position).as_vec3(), e.yaw, None),
            _ => continue,
        };
        if !world.contains(pos) {
            continue;
        }
        if let Some(profession) = profession {
            sim.trades
                .insert(id, crate::villagers::Trader::seeded(profession, id as i64));
        }
        if let Some((key, value)) = variant {
            sim.command_entities.mobs.entry(id).or_default().extra.insert(key.into(), crate::vanilla::drops::DropNbt::Byte(value));
        }
        sim.mobs.push(Mob {
            kind,
            id,
            position: pos,
            previous: pos,
            yaw: (180. + yaw).to_radians(),
            health: kind.max_health(sim.command_entities.mobs.get(&id)),
            velocity: Vec3::ZERO,
            age: 0,
            turn_ticks: 0,
            hurt: 0,
            fuse: 0,
            swing_ticks: 0,
            death_ticks: 0,
            eat_ticks: 0,
            attack_ticks: 0,
            rose_ticks: 0,
            prev_fuse: 0,
        });
    }
}
pub(super) fn populated_spawn_view(
    options: &Options,
) -> Result<(World, Option<PlayerSave>, Vec<SpawnRecord>)> {
    ensure!(
        options.world.is_none() && options.structure_preview.is_none(),
        "Native spawn view requires a seed without an imported world or structure preview"
    );
    let mut session = NativeSession::in_memory(options.seed as i64, 1234)?;
    match session.receive()? {
        Event::Ready {
            resumed: false,
            path: None,
            ..
        } => {}
        Event::Failed { message, .. } => bail!("{message}"),
        event => bail!("Unexpected native world startup: {event:?}"),
    }
    let (reply, changes) = request(&mut session, Command::PrepareSpawn)?;
    let Reply::SpawnPrepared {
        point,
        initial_region,
    } = reply
    else {
        bail!("Native world did not prepare a spawn")
    };
    let mut world = World {
        chunks: Default::default(),
        spawn: IVec3::from_array(point).as_vec3(),
        label: format!(
            "Native 1.8.9 populated spawn ({}, {}, {}) / seed {}",
            point[0], point[1], point[2], options.seed as i64
        ),
        save_path: Default::default(),
        edits: Default::default(),
        imported: true,
        light_revision: 0,
    };
    let radius = options.render_distance as i32;
    let mut spawns = Vec::new();
    apply(&mut world, &mut spawns, changes, radius);
    let mut loaded = 0;
    while loaded < initial_region.len() {
        let (reply, changes) = request(&mut session, Command::LoadSpawnBatch { count: 64 })?;
        let Reply::SpawnAreaProgress { loaded: next } = reply else {
            bail!("Missing native spawn progress")
        };
        ensure!(
            next > loaded && next <= initial_region.len(),
            "Invalid native spawn load cursor"
        );
        loaded = next;
        apply(&mut world, &mut spawns, changes, radius);
    }
    let (reply, changes) = request(&mut session, Command::PreparePlayer { adventure: false })?;
    let Reply::PlayerPrepared { initial } = reply else {
        bail!("Native player creation failed")
    };
    apply(&mut world, &mut spawns, changes, radius);
    session.join()?;
    Ok((
        world,
        Some(PlayerSave {
            position: initial.position,
            motion: None,
        }),
        spawns,
    ))
}
