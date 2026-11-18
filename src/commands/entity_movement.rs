use super::*;
use crate::vanilla::{material, random::JavaRandom, respawn::BedSpawn};
use glam::DVec2;
pub(super) fn rotation(ctx: &Context<'_>, t: Target) -> [f32; 2] {
    match t {
        Target::Player => ctx.player.java_rotation(),
        Target::Mob(id) => [
            ctx.sim
                .mobs
                .iter()
                .find(|m| m.id == id)
                .map_or(0., |m| m.yaw.to_degrees() - 180.),
            ctx.sim
                .command_entities
                .mobs
                .get(&id)
                .map_or(0., |d| d.pitch),
        ],
        Target::Orb(id) => [
            ctx.sim
                .xp_orbs
                .iter()
                .find(|o| o.id == id)
                .map_or(0., |o| o.yaw),
            0.,
        ],
        _ => [0., 0.],
    }
}
pub(super) fn set(ctx: &mut Context<'_>, t: Target, p: DVec3, r: [f32; 2]) {
    match t {
        Target::Block(_) => (),
        Target::Player => ctx.player.set_pose_f64(p.to_array(), r[0], r[1], false),
        Target::Mob(id) => {
            if let Some(m) = ctx.sim.mobs.iter_mut().find(|m| m.id == id) {
                m.position = p.as_vec3();
                m.previous = m.position;
                m.yaw = (180. + r[0]).to_radians();
                ctx.sim.command_entities.mobs.entry(id).or_default().pitch = r[1];
                if let Some(brain) = ctx.sim.mob_brains.get_mut(&id) {
                    brain.movement = None;
                    brain.path_target = None;
                }
            }
        }
        Target::Orb(id) => {
            if let Some(o) = ctx.sim.xp_orbs.iter_mut().find(|o| o.id == id) {
                let delta = p - o.position;
                o.bounds.min += delta;
                o.bounds.max += delta;
                o.position = p;
                o.previous = p;
                o.yaw = r[0];
            }
        }
        Target::Drop(id) => {
            if let Some(d) = ctx.sim.drops.iter_mut().find(|d| d.id == id) {
                d.set_feet(p);
                d.previous = d.position
            }
        }
    }
}
fn wrap(x: f32) -> f32 {
    (x + 180.).rem_euclid(360.) - 180.
}
pub(super) fn teleport(ctx: &mut Context<'_>, a: &[&str]) -> Reply {
    if a.is_empty() {
        return Err(usage("tp"));
    }
    let (t, a) = if matches!(a.len(), 2 | 4 | 6) {
        (ctx.one(a[0])?, &a[1..])
    } else {
        (player_target(ctx, None)?, a)
    };
    let name = ctx.target_name(t);
    if a.len() == 1 {
        let dest = ctx.one(a[0])?;
        set(ctx, t, ctx.position(dest), rotation(ctx, dest));
        if t == Target::Player {
            ctx.player.set_velocity_f64([0.; 3]);
        }
        return notify(
            ctx,
            "commands.tp.success",
            vec![json!(name), json!(ctx.target_name(dest))],
        );
    }
    if a.len() < 3 {
        return Err(usage("tp"));
    }
    let p = position(ctx.position(t), a, true)?;
    // CommandTeleport bounds the parsed offsets, not the sum after applying "~".
    for i in [0,2]{bounded(coordinate(0.,a[i],true)?,-30000000.,30000000.)?;}
    let [y, pitch] = rotation(ctx, t);
    let angles = [
        a.get(3).copied().unwrap_or("~"),
        a.get(4).copied().unwrap_or("~"),
    ];
    let mut r = [
        coordinate(f64::from(y), angles[0], false)? as f32,
        coordinate(f64::from(pitch), angles[1], false)? as f32,
    ];
    if t == Target::Player {
        for i in 0..2 {
            r[i] = if angles[i].starts_with('~') {
                r[i] - [y, pitch][i]
            } else {
                wrap(r[i])
            };
        }
        if r[1].abs() > 90. {
            r[1] = wrap(180. - r[1]);
            r[0] = wrap(r[0] + 180.);
        }
        for i in 0..2 {
            if angles[i].starts_with('~') {
                r[i] += [y, pitch][i]
            }
        }
        let mut v = ctx.player.precise_velocity();
        for (i, s) in a[..3].iter().enumerate() {
            if !s.starts_with('~') {
                v[i] = 0.;
            }
        }
        ctx.player.set_velocity_f64(v);
    } else {
        r = r.map(wrap);
        if r[1].abs() > 90. {
            r[1] = wrap(180. - r[1]);
            r[0] = wrap(r[0] + 180.);
        }
    }
    set(ctx, t, p, r);
    notify(
        ctx,
        "commands.tp.success.coordinates",
        vec![json!(name), json!(p.x), json!(p.y), json!(p.z)],
    )
}
pub(super) fn spawnpoint(ctx: &mut Context<'_>, a: &[&str]) -> Reply {
    if matches!(a.len(), 2 | 3) {
        return Err(usage("spawnpoint"));
    }
    player_target(ctx, a.first().copied())?;
    let p = if a.len() >= 4 {block_position(ctx,&a[1..],true)?}else{ctx.position(Target::Player).floor().as_ivec3()};
    ctx.sim.bed_spawn = Some(BedSpawn {
        point: p,
        forced: true,
    });
    notify(
        ctx,
        "commands.spawnpoint.success",
        vec![
            json!(ctx.options.username),
            json!(p.x),
            json!(p.y),
            json!(p.z),
        ],
    )
}
fn real(raw: &str, min: f64) -> CmdResult<f64> {
    let n = raw
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| error("commands.generic.num.invalid", vec![json!(raw)]))?;
    if n < min {
        Err(error(
            "commands.generic.double.tooSmall",
            vec![json!(n), json!(min)],
        ))
    } else {
        Ok(n)
    }
}
fn surface(world: &crate::world::World, p: DVec2) -> Option<i32> {
    let x = p.x.floor() as i32;
    let z = p.y.floor() as i32;
    if !world
        .chunks
        .contains_key(&(x.div_euclid(16), z.div_euclid(16)))
    {
        return None;
    }
    for y in (0..256).rev() {
        let s = world.get(IVec3::new(x, y, z));
        if s >> 4 != 0 {
            return (!material::is_liquid(s) && s >> 4 != 51).then_some(y + 1);
        }
    }
    None
}
pub(super) fn spread(ctx: &mut Context<'_>, a: &[&str]) -> Reply {
    if a.len() < 6 {
        return Err(usage("spreadplayers"));
    }
    let center = DVec2::new(
        coordinate(ctx.origin.x.floor(), a[0], true)?,
        coordinate(ctx.origin.z.floor(), a[1], true)?,
    );
    let separation = real(a[2], 0.)?;
    let extent = real(a[3], separation + 1.)?;
    let teams = match a[4] {
        "true" | "1" => true,
        "false" | "0" => false,
        _ => return Err(error("commands.generic.boolean.invalid", vec![json!(a[4])])),
    };
    let mut grouped = Vec::new();
    let mut names = Vec::new();
    for raw in &a[5..] {
        let ts = if raw.starts_with('@') {
            ctx.targets(raw)?
        } else {
            vec![player_target(ctx, Some(raw))?]
        };
        if ts.is_empty() {
            return Err(error("commands.generic.entity.notFound", vec![]));
        }
        for t in ts {
            let team = if teams && t == Target::Player {
                ctx.sim
                    .commands
                    .board
                    .members
                    .get(&ctx.options.username)
                    .cloned()
            } else {
                None
            };
            let i = if teams {
                names.iter().position(|s| s == &team).unwrap_or(names.len())
            } else {
                names.len()
            };
            if i == names.len() {
                names.push(team)
            }
            grouped.push((t, i));
        }
    }
    ctx.results[2] = grouped.len() as i32;
    let typ = if teams { "teams" } else { "players" };
    let low = center - DVec2::splat(extent);
    let high = center + DVec2::splat(extent);
    let mut rng = JavaRandom::new(JavaRandom::independent_seed());
    let mut random = || low + (high - low) * DVec2::new(rng.next_double(), rng.next_double());
    let mut points: Vec<_> = names.iter().map(|_| random()).collect();
    for iteration in 1..=10000 {
        let mut changed = false;
        let mut minimum = f64::from(f32::MAX);
        for i in 0..points.len() {
            let p = points[i];
            let mut count = 0;
            let mut towards = DVec2::ZERO;
            for (j, &other) in points.iter().enumerate() {
                if i == j {
                    continue;
                }
                let delta = other - p;
                let dist = delta.length();
                minimum = minimum.min(dist);
                if dist < separation {
                    towards += delta;
                    count += 1;
                }
            }
            if count > 0 {
                towards /= count as f64;
                let len = towards.length() as f32;
                points[i] = if len > 0. {
                    p - towards / f64::from(len)
                } else {
                    random()
                };
                changed = true
            }
            let safe = points[i].clamp(low, high);
            changed |= safe != points[i];
            points[i] = safe;
        }
        if !changed {
            for p in &mut points {
                if surface(ctx.world, *p).is_none() {
                    *p = random();
                    changed = true
                }
            }
        }
        if iteration == 10000 {
            return Err(error(
                &format!("commands.spreadplayers.failure.{typ}"),
                vec![
                    json!(points.len()),
                    json!(center.x),
                    json!(center.y),
                    json!(format!("{minimum:.2}")),
                ],
            ));
        }
        if !changed {
            let mut average = 0.;
            for &(t, i) in &grouped {
                let p = points[i];
                let y = surface(ctx.world, p).unwrap();
                set(
                    ctx,
                    t,
                    DVec3::new(p.x.floor() + 0.5, y as f64, p.y.floor() + 0.5),
                    rotation(ctx, t),
                );
                average += points
                    .iter()
                    .enumerate()
                    .filter(|(j, _)| *j != i)
                    .map(|(_, other)| p.distance(*other))
                    .fold(f64::MAX, f64::min);
            }
            let mut out = vec![tr(
                &format!("commands.spreadplayers.spreading.{typ}"),
                vec![
                    json!(grouped.len()),
                    json!(extent),
                    json!(center.x),
                    json!(center.y),
                    json!(separation),
                ],
            )];
            out.extend(notify(
                ctx,
                &format!("commands.spreadplayers.success.{typ}"),
                vec![json!(points.len()), json!(center.x), json!(center.y)],
            )?);
            if points.len() > 1 {
                out.push(tr(
                    &format!("commands.spreadplayers.info.{typ}"),
                    vec![
                        json!(format!("{:.2}", average / grouped.len() as f64)),
                        json!(iteration),
                    ],
                ))
            }
            return Ok(out);
        }
    }
    unreachable!()
}
