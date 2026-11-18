// Singleplayer neighbor notifications and BlockPistonStructureHelper's 12-block move transaction.
use super::{Outcome, Simulation};
use crate::{
    player::Player,
    vanilla::{
        collision_shapes::PistonTile,
        drops::DropNbt,
        item_nbt, material,
        notes::{self, SIDES},
        spawning::solid_top,
    },
    world::{ChunkPos, World},
};
use glam::{IVec3, Vec3};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashSet, VecDeque},
    sync::Arc,
};

#[derive(Clone, Default, Serialize, Deserialize, Debug)]
pub(super) struct Circuits {
    pub(super) moving: Vec<Motion>,
    #[serde(skip)]
    loaded: HashSet<ChunkPos>,
    #[serde(default)]
    lamps: Vec<(IVec3, u64)>,
    #[serde(default)]
    timers: Vec<Timer>,
    #[serde(default)]
    torch_history: Vec<(IVec3, u64)>,
}
#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
struct Timer {
    pos: IVec3,
    block: u16,
    at: u64,
    priority: i8,
}
#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
pub(super) struct Motion {
    p: IVec3,
    tile: PistonTile,
    current: f32,
    render_head: bool,
}

impl World {
    pub fn piston_render_progress(&self, p: IVec3, partial: f32) -> f32 {
        let Some(tile) = self.piston_at(p) else {
            return 0.;
        };
        let current = match self
            .tile(p)
            .and_then(item_nbt::compound)
            .and_then(|t| t.get("FerrumCurrent"))
        {
            Some(DropNbt::Float(n)) => *n,
            _ => tile.progress,
        };
        tile.progress + (current - tile.progress) * partial.clamp(0., 1.)
    }
    pub fn piston_renders_head(&self, p: IVec3) -> bool {
        self.tile(p)
            .and_then(item_nbt::compound)
            .and_then(|t| t.get("FerrumHead"))
            == Some(&DropNbt::Byte(1))
    }
}
impl Motion {
    fn sync(self, w: &mut World) {
        let Some(c) = w
            .chunks
            .get_mut(&(self.p.x.div_euclid(16), self.p.z.div_euclid(16)))
        else {
            return;
        };
        let c = Arc::make_mut(c);
        let key = [self.p.x.rem_euclid(16), self.p.y, self.p.z.rem_euclid(16)];
        c.pistons.insert(key, self.tile);
        let t = self.tile;
        c.tiles.insert(
            key,
            Arc::new(DropNbt::Compound(
                [
                    ("id".into(), DropNbt::String("Piston".into())),
                    ("x".into(), DropNbt::Int(self.p.x)),
                    ("y".into(), DropNbt::Int(self.p.y)),
                    ("z".into(), DropNbt::Int(self.p.z)),
                    ("blockId".into(), DropNbt::Int((t.carried >> 4).into())),
                    ("blockData".into(), DropNbt::Int((t.carried & 15).into())),
                    ("extending".into(), DropNbt::Byte(t.extending as i8)),
                    ("facing".into(), DropNbt::Int(t.facing.into())),
                    ("progress".into(), DropNbt::Float(t.progress)),
                    ("FerrumCurrent".into(), DropNbt::Float(self.current)),
                    ("FerrumHead".into(), DropNbt::Byte(self.render_head as i8)),
                ]
                .into(),
            )),
        );
    }
}

const MOBILITY: [u8; 256] = include!("vanilla/piston_mobility.rs");
fn mobility(w: &World, p: IVec3, dir: IVec3) -> u8 {
    if !w.contains(p.as_vec3()) || !(0..256).contains(&p.y) || !(0..256).contains(&(p + dir).y) {
        return 2;
    }
    let s = w.get(p);
    if matches!(s >> 4, 29 | 33) {
        return if s & 8 != 0 { 2 } else { 0 };
    }
    MOBILITY[(s >> 4) as usize]
}
#[derive(Default, Debug)]
struct Plan {
    moved: Vec<IVec3>,
    broken: Vec<IVec3>,
}
impl Plan {
    fn build(w: &World, base: IVec3, face: IVec3, extend: bool) -> Option<Self> {
        let dir = if extend { face } else { -face };
        let start = base + face * if extend { 1 } else { 2 };
        let mut plan = Self::default();
        match mobility(w, start, dir) {
            1 => {
                plan.broken.push(start);
                return Some(plan);
            }
            2 => return None,
            _ => (),
        }
        if !plan.line(w, base, start, dir) {
            return None;
        }
        let mut i = 0;
        while i < plan.moved.len() {
            let p = plan.moved[i];
            if w.get(p) >> 4 == 165 && !plan.branches(w, base, p, dir) {
                return None;
            }
            i += 1;
        }
        Some(plan)
    }
    fn branches(&mut self, w: &World, base: IVec3, p: IVec3, dir: IVec3) -> bool {
        SIDES
            .iter()
            .filter(|s| s.dot(dir) == 0)
            .all(|s| self.line(w, base, p + *s, dir))
    }
    fn line(&mut self, w: &World, base: IVec3, start: IVec3, dir: IVec3) -> bool {
        if w.get(start) == 0
            || start == base
            || self.moved.contains(&start)
            || mobility(w, start, dir) != 0
        {
            return true;
        }
        let old = self.moved.len();
        let mut n = 1;
        while w.get(start - dir * (n - 1)) >> 4 == 165 {
            let back = start - dir * n;
            if back == base || w.get(back) == 0 || mobility(w, back, dir) != 0 {
                break;
            }
            n += 1;
            if old + n as usize > 12 {
                return false;
            }
        }
        if old + n as usize > 12 {
            return false;
        }
        self.moved.extend((0..n).rev().map(|i| start - dir * i));
        let mut p = start + dir;
        loop {
            if let Some(k) = self.moved.iter().position(|q| *q == p) {
                let added = self.moved.len() - old;
                self.moved[k..].rotate_right(added);
                for i in 0..=k + added {
                    let q = self.moved[i];
                    if w.get(q) >> 4 == 165 && !self.branches(w, base, q, dir) {
                        return false;
                    }
                }
                return true;
            }
            if !w.contains(p.as_vec3()) || !(0..256).contains(&p.y) {
                return false;
            }
            if w.get(p) == 0 {
                return true;
            }
            if p == base {
                return false;
            }
            match mobility(w, p, dir) {
                2 => return false,
                1 => {
                    self.broken.push(p);
                    return true;
                }
                _ if self.moved.len() >= 12 => return false,
                _ => self.moved.push(p),
            }
            p += dir;
        }
    }
}

fn set(w: &mut World, p: IVec3, s: u16, out: &mut Outcome) {
    if w.set(p, s) {
        out.changed.push(p);
    }
}
fn signal(w: &World, p: IVec3, side: usize, wire: bool) -> u8 {
    notes::side_power(w, p, side, wire, &|q| {
        item_nbt::number(
            w.tile(q)
                .and_then(item_nbt::compound)
                .and_then(|v| v.get("OutputSignal")),
        )
    })
}
fn is_powered(w: &World, p: IVec3) -> bool {
    SIDES
        .iter()
        .enumerate()
        .any(|(i, d)| signal(w, p + *d, i, true) > 0)
}
fn piston_power(w: &World, p: IVec3, face: usize) -> bool {
    SIDES
        .iter()
        .enumerate()
        .any(|(i, d)| i != face && signal(w, p + *d, i, true) > 0)
        || signal(w, p, 0, true) > 0
        || SIDES
            .iter()
            .enumerate()
            .skip(1)
            .any(|(i, d)| signal(w, p + IVec3::Y + *d, i, true) > 0)
}
fn wire_strength(w: &World, p: IVec3) -> u8 {
    let direct = SIDES
        .iter()
        .enumerate()
        .map(|(i, d)| signal(w, p + *d, i, false))
        .max()
        .unwrap_or(0);
    if direct == 15 {
        return direct;
    }
    let level = |q| {
        let s = w.get(q);
        if s >> 4 == 55 {
            (s & 15) as u8
        } else {
            0
        }
    };
    let clear = !material::normal_cube(w.get(p + IVec3::Y));
    let mut adjacent = 0;
    for d in &SIDES[2..] {
        let q = p + *d;
        adjacent = adjacent.max(level(q));
        if !material::normal_cube(w.get(q)) {
            adjacent = adjacent.max(level(q - IVec3::Y));
        } else if clear {
            adjacent = adjacent.max(level(q + IVec3::Y));
        }
    }
    direct.max(adjacent.saturating_sub(1))
}
fn lever_supported(w: &World, p: IVec3, state: u16) -> bool {
    let f = [0, 5, 4, 3, 2, 1, 1, 0][(state & 7) as usize];
    let q = p - SIDES[f];
    let support = w.get(q);
    if f == 1 {
        solid_top(support)
    } else {
        ((0..256).contains(&q.y) && !w.contains(q.as_vec3())) || material::normal_cube(support)
    }
}

impl Simulation {
    pub(crate) fn activate_circuit(&mut self, w: &mut World, p: IVec3, out: &mut Outcome) -> bool {
        let s = w.get(p);
        match s >> 4 {
            137 if self.mode == super::Mode::Creative && self.commands.player_mode != Some(3) => {
                out.open_command_block = Some(p);
            }
            71 => (), // Iron doors consume the click, and only change state on redstone updates.
            64 | 193..=197 => {
                let lower = if s & 8 != 0 { p - IVec3::Y } else { p };
                let base = w.get(lower);
                if base >> 4 != s >> 4 { return false; }
                set(w, lower, base ^ 4, out);
                out.changed.push(lower + IVec3::Y);
                let mut prng =
                    crate::vanilla::random::JavaRandom::from_state(self.particle_random[2]);
                let pitch = prng.next_float() * 0.1 + 0.9;
                self.particle_random[2] = prng.state();
                out.world_sounds.push((
                    if base & 4 == 0 { "random.door_open" } else { "random.door_close" },
                    lower.as_dvec3() + glam::DVec3::splat(0.5),
                    1.0,
                    pitch,
                ));
            }
            69 => {
                set(w, p, s ^ 8, out);
                out.world_sounds.push((
                    "random.click",
                    p.as_dvec3() + glam::DVec3::splat(0.5),
                    0.3,
                    if s & 8 == 0 { 0.6 } else { 0.5 },
                ));
            }
            93 | 94 if self.commands.player_mode != Some(2) => {
                set(w, p, (s & !15) | ((s + 4) & 15), out)
            }
            149 | 150 if self.commands.player_mode != Some(2) => {
                set(w, p, s ^ 4, out);
                out.world_sounds.push((
                    "random.click",
                    p.as_dvec3() + glam::DVec3::splat(0.5),
                    0.3,
                    if s & 4 == 0 { 0.55 } else { 0.5 },
                ));
                self.switch_comparator(w, p, out);
            }
            _ => return false,
        }
        true
    }
    pub(super) fn purge_circuit_timers_at(&mut self, p: IVec3, id: u16) {
        self.redstone.timers.retain(|t| !(t.pos == p && t.block == id));
    }
    fn break_component(&mut self, w: &mut World, p: IVec3, out: &mut Outcome) {
        let state = w.get(p);
        super::block_support::drop_broken_block(self, w, p, state);
        let id = state >> 4;
        if matches!(id, 63 | 68 | 140) {
            self.containers.remove(&super::container_key(p));
        }
        if matches!(id, 63 | 68 | 140 | 144 | 149 | 150 | 176 | 177) {
            self.world_commands.edit_tile(w, p, 0, None);
        }
        if matches!(id, 75 | 76 | 93 | 94 | 149 | 150) {
            self.purge_circuit_timers_at(p, id);
        }
        set(w, p, 0, out);
        self.block_ticks.notify(w, p);
    }
    fn start_motion(
        &mut self,
        w: &mut World,
        p: IVec3,
        carried: u16,
        facing: usize,
        extending: bool,
        render_head: bool,
        sticky_head: bool,
        out: &mut Outcome,
    ) {
        self.redstone.moving.retain(|m| m.p != p);
        set(
            w,
            p,
            36 << 4 | facing as u16 | if sticky_head { 8 } else { 0 },
            out,
        );
        let m = Motion {
            p,
            tile: PistonTile {
                carried,
                facing: facing as u8,
                extending,
                progress: 0.,
            },
            current: 0.,
            render_head,
        };
        m.sync(w);
        self.redstone.moving.push(m);
    }
    fn finish_motion(&mut self, w: &mut World, p: IVec3, out: &mut Outcome) {
        if let Some(tile) = w.piston_at(p) {
            set(w, p, tile.carried, out);
        }
        self.redstone.moving.retain(|m| m.p != p);
    }
    fn move_piston(&mut self, w: &mut World, base: IVec3, out: &mut Outcome) {
        let state = w.get(base);
        let face = (state & 7) as usize;
        if !matches!(state >> 4, 29 | 33) || face >= 6 {
            return;
        }
        let extend = piston_power(w, base, face);
        if extend == (state & 8 != 0) {
            return;
        }
        let dir = SIDES[face];
        let sticky = state >> 4 == 29;
        let head = base + dir;
        let plan = if extend {
            let Some(plan) = Plan::build(w, base, dir, true) else {
                return;
            };
            Some(plan)
        } else {
            self.finish_motion(w, head, out);
            self.start_motion(w, base, state & !8, face, false, true, sticky, out);
            let q = head + dir;
            let spit = sticky
                && w.piston_at(q)
                    .is_some_and(|m| m.extending && m.facing as usize == face);
            if spit {
                self.finish_motion(w, q, out);
            }
            set(w, head, 0, out);
            if sticky && !spit && w.get(q) != 0 && mobility(w, q, -dir) == 0 {
                Plan::build(w, base, dir, false)
            } else {
                None
            }
        };
        if let Some(plan) = plan {
            for p in plan.broken.into_iter().rev() {
                self.break_component(w, p, out);
            }
            for p in plan.moved.into_iter().rev() {
                let carried = w.get(p);
                set(w, p, 0, out);
                self.start_motion(
                    w,
                    p + if extend { dir } else { -dir },
                    carried,
                    face,
                    extend,
                    false,
                    false,
                    out,
                );
            }
        }
        if extend {
            self.start_motion(
                w,
                head,
                34 << 4 | face as u16 | if sticky { 8 } else { 0 },
                face,
                true,
                false,
                sticky,
                out,
            );
            set(w, base, state | 8, out);
        }
        let mut rng = self.block_ticks.world_random();
        let (clip, scale) = if extend {
            ("tile.piston.out", 0.25)
        } else {
            ("tile.piston.in", 0.15)
        };
        out.world_sounds.push((
            clip,
            base.as_dvec3() + glam::DVec3::splat(0.5),
            0.5,
            rng.next_float() * scale + 0.6,
        ));
        self.block_ticks.set_world_random(&rng);
    }
    fn tick_motion(&mut self, w: &mut World, player: &mut Player, out: &mut Outcome) {
        self.redstone.loaded.retain(|k| w.chunks.contains_key(k));
        let mut incoming = Vec::new();
        for (&(x, z), c) in &w.chunks {
            if self.redstone.loaded.insert((x, z)) {
                for (&[lx, y, lz], &tile) in &c.pistons {
                    let p = IVec3::new(x * 16 + lx, y, z * 16 + lz);
                    incoming.push((p, tile));
                }
            }
        }
        // Only newly imported tiles are ordered here. Already active piston
        // motions retain their event order, including multi-block pushes.
        incoming.sort_unstable_by_key(|(p, _)| p.to_array());
        for (p, tile) in incoming {
            if !self.redstone.moving.iter().any(|m| m.p == p) {
                self.redstone.moving.push(Motion {
                    p, tile, current: tile.progress, render_head: false,
                });
            }
        }
        for mut m in std::mem::take(&mut self.redstone.moving) {
            if !w.contains(m.p.as_vec3()) {
                self.redstone.moving.push(m);
                continue;
            }
            if w.get(m.p) >> 4 != 36 {
                continue;
            }
            let done = m.current >= 1.;
            m.tile.progress = m.current;
            m.current = (m.current + 0.5).min(1.);
            m.sync(w);
            if done || m.tile.extending {
                push_entities(self, w, player, m, done);
            }
            if done {
                set(w, m.p, m.tile.carried, out);
            } else {
                self.redstone.moving.push(m);
            }
        }
    }
    pub(crate) fn clear_remote_pistons(&mut self) {
        self.redstone.moving.clear();
        self.redstone.loaded.clear();
    }
    pub(crate) fn remote_piston_action(
        &mut self,
        w: &mut World,
        base: IVec3,
        action: u8,
        param: u8,
        block: u16,
        changed: &mut Vec<IVec3>,
    ) -> Option<(&'static str, f32, f32)> {
        let face = usize::from(param & 7);
        let cur_id = w.get(base) >> 4;
        if !matches!(block, 29 | 33)
            || face >= 6
            || action > 1
            || (cur_id != block && !(action == 1 && cur_id == 36))
        {
            return None;
        }
        let extend = action == 0;
        let dir = SIDES[face];
        let sticky = block == 29;
        let head = base + dir;
        let mut out = Outcome::default();
        let plan = if extend {
            Some(Plan::build(w, base, dir, true)?)
        } else {
            self.finish_motion(w, head, &mut out);
            self.start_motion(w, base, (block << 4) | face as u16, face, false, true, sticky, &mut out);
            let q = head + dir;
            let spit = sticky
                && w.piston_at(q)
                    .is_some_and(|m| m.extending && usize::from(m.facing) == face);
            if spit {
                self.finish_motion(w, q, &mut out);
            }
            set(w, head, 0, &mut out);
            if sticky && !spit && w.get(q) != 0 && mobility(w, q, -dir) == 0 {
                Plan::build(w, base, dir, false)
            } else {
                None
            }
        };
        if let Some(plan) = plan {
            for p in plan.broken.into_iter().rev() {
                set(w, p, 0, &mut out);
            }
            for p in plan.moved.into_iter().rev() {
                let carried = w.get(p);
                set(w, p, 0, &mut out);
                self.start_motion(
                    w,
                    p + if extend { dir } else { -dir },
                    carried,
                    face,
                    extend,
                    false,
                    false,
                    &mut out,
                );
            }
        }
        if extend {
            self.start_motion(
                w,
                head,
                34 << 4 | face as u16 | if sticky { 8 } else { 0 },
                face,
                true,
                false,
                sticky,
                &mut out,
            );
            set(w, base, (block << 4) | face as u16 | 8, &mut out);
        }
        changed.extend(out.changed);
        let mut rng = self.block_ticks.world_random();
        let (clip, scale) = if extend {
            ("tile.piston.out", 0.25)
        } else {
            ("tile.piston.in", 0.15)
        };
        let pitch = rng.next_float() * scale + 0.6;
        self.block_ticks.set_world_random(&rng);
        Some((clip, 0.5, pitch))
    }
    pub(crate) fn tick_remote_pistons(
        &mut self,
        w: &mut World,
        player: &mut Player,
        out: &mut Outcome,
        mut move_player: impl FnMut(&World, &mut Player, glam::DVec3),
    ) {
        self.redstone.loaded.retain(|k| w.chunks.contains_key(k));
        let mut incoming = Vec::new();
        for (&(x, z), c) in &w.chunks {
            if self.redstone.loaded.insert((x, z)) {
                for (&[lx, y, lz], &tile) in &c.pistons {
                    let p = IVec3::new(x * 16 + lx, y, z * 16 + lz);
                    incoming.push((p, tile));
                }
            }
        }
        incoming.sort_unstable_by_key(|(p, _)| p.to_array());
        for (p, tile) in incoming {
            if !self.redstone.moving.iter().any(|m| m.p == p) {
                self.redstone.moving.push(Motion {
                    p,
                    tile,
                    current: tile.progress,
                    render_head: false,
                });
            }
        }
        for mut m in std::mem::take(&mut self.redstone.moving) {
            if !w.contains(m.p.as_vec3()) {
                self.redstone.moving.push(m);
                continue;
            }
            if w.get(m.p) >> 4 != 36 {
                continue;
            }
            let done = m.current >= 1.;
            m.tile.progress = m.current;
            m.current = (m.current + 0.5).min(1.);
            m.sync(w);
            if done || m.tile.extending {
                let dir = SIDES[usize::from(m.tile.facing.min(5))].as_dvec3();
                let mut t = m.tile;
                t.progress = if t.extending { m.current } else { m.current - 1. };
                if let Some(area) = crate::vanilla::collision_shapes::single_box_for(
                    m.p,
                    36 << 4,
                    |p| w.get(p),
                    Some(t),
                ) {
                    if !player.sleeping
                        && !player.is_spectator()
                        && player.precise_bounds().intersects(area)
                    {
                        if t.carried >> 4 == 165 && t.extending {
                            let mut v = glam::DVec3::from_array(player.precise_velocity());
                            v = v * (glam::DVec3::ONE - dir.abs()) + dir;
                            player.set_velocity_f64(v.to_array());
                        } else {
                            let delta = dir * if done { 0.25 } else { 0.5625 };
                            move_player(&*w, player, delta);
                        }
                    }
                }
            }
            if done {
                set(w, m.p, m.tile.carried, out);
            } else {
                self.redstone.moving.push(m);
            }
        }
    }
    pub(super) fn update_circuits(
        &mut self,
        w: &mut World,
        player: &mut Player,
        seed: Vec<IVec3>,
        out: &mut Outcome,
    ) {
        let cursor = out.changed.len();
        self.tick_motion(w, player, out);
        self.tick_circuit_timers(w, out);
        let mut dirty = seed;
        dirty.extend_from_slice(&out.changed[cursor..]);
        let mut lamps = std::mem::take(&mut self.redstone.lamps);
        lamps.retain(|&(p, at)| {
            if at > self.total_ticks || !w.contains(p.as_vec3()) {
                return true;
            }
            if w.get(p) >> 4 == 124 && !is_powered(w, p) {
                set(w, p, 123 << 4, out);
                dirty.push(p);
            }
            false
        });
        self.redstone.lamps = lamps;
        while !dirty.is_empty() {
            let mut wires = VecDeque::new();
            let mut enqueued = HashSet::new();
            let mut pistons = Vec::new();
            let mut seen = HashSet::new();
            let mut lamps = Vec::new();
            let mut seed_block = |w: &World, p| match w.get(p) >> 4 {
                55 | 69 | 34 if enqueued.insert(p) => wires.push_back(p),
                29 | 33 if seen.insert(p) => pistons.push(p),
                27 | 66 | 157 if seen.insert(p) => lamps.push(p),
                64 | 71 | 154 | 193..=197 if seen.insert(p) => lamps.push(p),
                46 | 75 | 76 | 93 | 94 | 123 | 124 | 137 | 149 | 150 if seen.insert(p) => lamps.push(p),
                _ => (),
            };
            for &changed in &dirty {
                near(changed, |q| {
                    if q != changed || w.get(q) >> 4 != 137 { seed_block(w, q); }
                });
            }
            while let Some(p) = wires.pop_front() {
                enqueued.remove(&p);
                let state = w.get(p);
                match state >> 4 {
                    55 if !solid_top(w.get(p - IVec3::Y)) && w.get(p - IVec3::Y) >> 4 != 89 => {
                        self.break_component(w, p, out)
                    }
                    55 => {
                        let s = (55 << 4) | u16::from(wire_strength(w, p));
                        if state == s {
                            continue;
                        }
                        set(w, p, s, out);
                    }
                    69 if !lever_supported(w, p, state) => self.break_component(w, p, out),
                    34 if state & 7 < 6
                        && !matches!(w.get(p - SIDES[(state & 7) as usize]) >> 4, 29 | 33 | 36) =>
                    {
                        set(w, p, 0, out)
                    }
                    _ => continue,
                }
                near(p, |q| match w.get(q) >> 4 {
                    55 | 69 if enqueued.insert(q) => wires.push_back(q),
                    29 | 33 if seen.insert(q) => pistons.push(q),
                    27 | 66 | 157 if seen.insert(q) => lamps.push(q),
                    64 | 71 | 154 | 193..=197 if seen.insert(q) => lamps.push(q),
                    46 | 75 | 76 | 93 | 94 | 123 | 124 | 137 | 149 | 150 if seen.insert(q) => lamps.push(q),
                    _ => (),
                });
            }
            let index = out.changed.len();
            for p in lamps {
                let s = w.get(p) >> 4;
                if matches!(s, 27 | 66 | 157) {
                    super::block_support::notify_rail_power(self, w, p, out);
                    continue;
                }
                if s == 137 {
                    let state = w.get(p);
                    let powered = is_powered(w, p);
                    if powered != (state & 1 != 0) {
                        set(w, p, (state & !1) | u16::from(powered), out);
                        if powered { self.circuit_timer(p, 137, 1, 0); }
                    }
                    continue;
                }
                if s == 46 && is_powered(w, p) {
                    self.prime_block(w, p, false, out);
                    continue;
                }
                if s == 154 {
                    let state = w.get(p);
                    let locked = is_powered(w, p);
                    if locked != (state & 8 != 0) {
                        set(w, p, state ^ 8, out);
                    }
                    continue;
                }
                if crate::vanilla::hand_placement::door_item(s).is_some() {
                    let lower = if w.get(p) & 8 != 0 { p - IVec3::Y } else { p };
                    let notify_power = dirty.iter().any(|q| w.get(*q) >> 4 != s &&
                        ((*q - lower).abs().element_sum() <= 2 || (*q - lower - IVec3::Y).abs().element_sum() <= 2));
                    self.notify_door(w, p, notify_power, out);
                    continue;
                }
                if matches!(s, 75 | 76 | 93 | 94 | 149 | 150) {
                    self.notify_device(w, p, out);
                    continue;
                }
                if s == 123 && is_powered(w, p) {
                    set(w, p, 124 << 4, out);
                }
                if s == 124
                    && !is_powered(w, p)
                    && !self.redstone.lamps.iter().any(|(q, _)| *q == p)
                {
                    self.redstone.lamps.push((p, self.total_ticks + 4));
                }
            }
            for p in pistons {
                self.move_piston(w, p, out);
            }
            dirty.clear();
            dirty.extend_from_slice(&out.changed[index..]);
        }
    }
    fn notify_door(&mut self, w: &mut World, p: IVec3, notify_power: bool, out: &mut Outcome) {
        let door = w.get(p);
        let id = door >> 4;
        let lower = if door & 8 != 0 { p - IVec3::Y } else { p };
        let upper = lower + IVec3::Y;
        let base = w.get(lower);
        if base >> 4 != id {
            if w.get(upper) >> 4 == id {
                set(w, upper, 0, out);
                self.block_ticks.notify(w, upper);
            }
            return;
        }
        let top = w.get(upper);
        let unsupported = !solid_top(w.get(lower - IVec3::Y));
        if top >> 4 != id || unsupported {
            if unsupported {
                super::block_support::drop_broken_block(self, w, lower, base);
            }
            set(w, lower, 0, out);
            self.block_ticks.notify(w, lower);
            if top >> 4 == id {
                set(w, upper, 0, out);
                self.block_ticks.notify(w, upper);
            }
            return;
        }
        let powered = is_powered(w, lower) || is_powered(w, upper);
        if notify_power && powered != (top & 2 != 0) {
            set(w, upper, (top & !2) | (u16::from(powered) << 1), out);
            if powered != (base & 4 != 0) {
                set(w, lower, base ^ 4, out);
                let mut prng =
                    crate::vanilla::random::JavaRandom::from_state(self.particle_random[2]);
                let pitch = prng.next_float() * 0.1 + 0.9;
                self.particle_random[2] = prng.state();
                out.world_sounds.push((
                    if powered { "random.door_open" } else { "random.door_close" },
                    lower.as_dvec3() + glam::DVec3::splat(0.5),
                    1.0,
                    pitch,
                ));
            }
        }
    }
}
fn near(p: IVec3, mut visit: impl FnMut(IVec3)) {
    visit(p);
    for (i, &a) in SIDES.iter().enumerate() {
        visit(p + a);
        visit(p + a + a);
        for &b in &SIDES[(i | 1) + 1..] {
            visit(p + a + b);
        }
    }
}
fn push_entities(sim: &mut Simulation, w: &World, player: &mut Player, m: Motion, done: bool) {
    use crate::vanilla::{
        collision_shapes,
        movement::{collision::move_body_with_step, Aabb},
    };
    let dir = SIDES[m.tile.facing as usize].as_dvec3();
    let mut t = m.tile;
    t.progress = if t.extending {
        m.current
    } else {
        m.current - 1.
    };
    let Some(area) = collision_shapes::single_box_for(m.p, 36 << 4, |p| w.get(p), Some(t)) else {
        return;
    };
    let delta = dir * if done { 0.25 } else { 0.5625 };
    let slime = t.carried >> 4 == 165 && t.extending;
    let push = |b: Aabb| {
        let result = move_body_with_step(w, b, delta, false, false, 0.);
        result.bounds
    };
    if player.precise_bounds().intersects(area) {
        if slime {
            let mut v = glam::DVec3::from_array(player.precise_velocity());
            v = v * (glam::DVec3::ONE - dir.abs()) + dir;
            player.set_velocity_f64(v.to_array());
        } else {
            let b = push(player.precise_bounds());
            player.set_position_f64(b.feet().to_array());
        }
    }
    for item in &mut sim.drops {
        if item.bounds.intersects(area) {
            if slime {
                item.velocity = item.velocity * (glam::DVec3::ONE - dir.abs()) + dir;
            } else {
                let b = push(item.bounds);
                item.set_feet(b.feet());
            }
        }
    }
    for mob in &mut sim.mobs {
        if mob.health <= 0. {
            continue;
        }
        let (width, height) = mob.kind.adult_size(sim.command_entities.mobs.get(&mob.id));
        let scale = if sim.mob_brains.get(&mob.id).is_some_and(|b| b.growing_age < 0) { 0.5 } else { 1. };
        let b = crate::vanilla::entity_pick::body(mob.position.as_dvec3(), width * scale, height * scale);
        if b.intersects(area) {
            if slime {
                mob.velocity = mob.velocity * (Vec3::ONE - dir.as_vec3().abs()) + dir.as_vec3();
            } else {
                mob.position = push(b).feet().as_vec3();
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{gameplay::Mode, inventory::Stack, settings::Settings, world::Chunk};
    #[test]
    fn imported_piston_motion_order_does_not_depend_on_chunk_hash_keys() {
        let positions = [IVec3::new(-12, 70, 6), IVec3::new(1, 70, 7), IVec3::new(19, 70, 4)];
        for seed in 0..8 {
            let mut w = flat();
            let mut chunks = crate::world::ChunkMap::with_hasher(ahash::RandomState::with_seeds(seed, 3, 5, 7));
            chunks.extend(w.chunks);
            w.chunks = chunks;
            for p in positions.into_iter().rev() {
                w.set(p, 36 << 4);
                let c = Arc::make_mut(w.chunks.get_mut(&(p.x >> 4, p.z >> 4)).unwrap());
                c.pistons.insert([p.x & 15, p.y, p.z & 15], PistonTile {
                    carried: 16, facing: 5, extending: false, progress: 0.,
                });
            }
            let mut sim = Simulation::new(Mode::Creative, 189);
            let mut player = Player::new(w.spawn);
            sim.tick_motion(&mut w, &mut player, &mut Outcome::default());
            assert_eq!(sim.redstone.moving.iter().map(|m| m.p).collect::<Vec<_>>(), positions);
            sim.tick_motion(&mut w, &mut player, &mut Outcome::default());
            assert_eq!(sim.redstone.moving.iter().map(|m| m.p).collect::<Vec<_>>(), positions,
                "existing motion order must survive later discovery passes");
        }
    }
    fn flat() -> World {
        let mut chunks = crate::world::ChunkMap::new();
        for cx in -1..=1 {
            for cz in -1..=1 {
                let mut chunk = Chunk::empty();
                for x in 0..16 {
                    for z in 0..16 {
                        chunk.set(x, 63, z, 16);
                    }
                }
                chunks.insert((cx, cz), Arc::new(chunk));
            }
        }
        World {
            chunks,
            spawn: Vec3::new(8.5, 64., 12.5),
            label: "circuit".into(),
            save_path: Default::default(),
            edits: Default::default(),
            imported: false,
            light_revision: 0,
        }
    }
    #[test]
    fn original_piston_structure_plans_include_slime_branches_and_blocked_pushes() {
        type Case = (
            usize,
            bool,
            Vec<[i32; 4]>,
            bool,
            Vec<[i32; 3]>,
            Vec<[i32; 3]>,
        );
        let cases: Vec<Case> = serde_json::from_reader(flate2::read::GzDecoder::new(
            &include_bytes!("../tests/fixtures/vanilla189-piston-plans.json.gz")[..],
        ))
        .unwrap();
        assert_eq!(cases.len(), 320);
        let mut w = flat();
        let base = IVec3::new(8, 64, 8);
        for x in 0..16 {
            for z in 0..16 {
                w.set(IVec3::new(x, 63, z), 0);
            }
        }
        for (i, (f, ext, cells, accepted, moved, broken)) in cases.into_iter().enumerate() {
            w.set(base, (29 << 4) | f as u16);
            for &[x, y, z, s] in &cells {
                w.set(IVec3::new(x, y, z), s as u16);
            }
            let plan = Plan::build(&w, base, SIDES[f], ext);
            assert_eq!(plan.is_some(), accepted, "case {i}");
            if let Some(plan) = plan {
                assert_eq!(
                    plan.moved,
                    moved.into_iter().map(IVec3::from_array).collect::<Vec<_>>(),
                    "case {i} moves"
                );
                assert_eq!(
                    plan.broken,
                    broken
                        .into_iter()
                        .map(IVec3::from_array)
                        .collect::<Vec<_>>(),
                    "case {i} destroys"
                );
            }
            for [x, y, z, _] in cells {
                w.set(IVec3::new(x, y, z), 0);
            }
        }
    }

    #[test]
    #[ignore = "uses the installed 1.8.9 block and inventory resources"]
    fn singleplayer_dust_lever_piston_extends_and_sticky_retracts_after_right_click() {
        use crate::vanilla::ray_trace::{Hit, Kind};
        let r = crate::validation::resources();
        let settings = Settings::default();
        for sticky in [false, true] {
            let mut w = flat();
            let mut player = Player::new(w.spawn);
            let mut sim = Simulation::new(Mode::Survival, 189);
            let lever = IVec3::new(3, 64, 8);
            let base = IVec3::new(8, 64, 8);
            let center = |p: IVec3| p.as_dvec3() + glam::DVec3::splat(0.5);
            let mut out = Outcome::default();
            let mut place = |sim: &mut Simulation, w: &mut World, p: IVec3, item: Stack| {
                sim.inventory.slots[0] = Some(item.clone());
                sim.place_held_block(
                    w,
                    &player,
                    &item,
                    Hit {
                        position: p - IVec3::Y,
                        face: 1,
                        point: center(p) - glam::DVec3::Y * 0.5,
                        kind: Kind::Block,
                    },
                    &mut out,
                );
            };
            place(&mut sim, &mut w, lever, Stack::new(69, 1));
            for x in 4..8 {
                place(&mut sim, &mut w, IVec3::new(x, 64, 8), Stack::new(331, 1));
                assert!(sim.inventory.held().is_none());
                assert_eq!(w.get(IVec3::new(x, 64, 8)), 55 << 4);
            }
            let id = if sticky { 29 } else { 33 };
            w.set(base, id << 4 | 5);
            w.set(base + IVec3::X, 16);
            let eye = [3.5, 64., 10.5];
            player.set_pose_f64(
                eye,
                180.,
                ((1.62_f32 - 0.25) / 2.).atan().to_degrees(),
                true,
            );
            sim.tick(&mut w, &mut player, r, &settings, false, true);
            assert_ne!(
                w.get(lever) & 8,
                0,
                "regular empty-hand click must toggle the lever"
            );
            for x in 4..8 {
                assert_eq!(w.get(IVec3::new(x, 64, 8)) & 15, (19 - x) as u16);
            }
            assert_eq!(w.get(base), id << 4 | 13);
            assert_eq!(
                w.piston_at(base + IVec3::X).unwrap().carried,
                34 << 4 | 5 | if sticky { 8 } else { 0 }
            );
            assert_eq!(w.piston_at(base + IVec3::X * 2).unwrap().carried, 16);
            for _ in 0..3 {
                sim.tick(&mut w, &mut player, r, &settings, false, false);
            }
            assert_eq!(w.get(base + IVec3::X) >> 4, 34);
            assert_eq!(w.get(base + IVec3::X * 2), 16);
            sim.tick(&mut w, &mut player, r, &settings, false, true);
            assert_eq!(w.get(lever) & 8, 0);
            assert_eq!(w.get(base) >> 4, 36);
            assert!(w.piston_renders_head(base));
            for _ in 0..3 {
                sim.tick(&mut w, &mut player, r, &settings, false, false);
            }
            assert_eq!(w.get(base), id << 4 | 5);
            assert_eq!(w.get(base + IVec3::X), if sticky { 16 } else { 0 });
            assert_eq!(w.get(base + IVec3::X * 2), if sticky { 0 } else { 16 });
            for x in 4..8 {
                assert_eq!(w.get(IVec3::new(x, 64, 8)), 55 << 4);
            }
        }
    }
}
impl Simulation {
    fn circuit_timer(&mut self, p: IVec3, block: u16, delay: u64, priority: i8) {
        if !self
            .redstone
            .timers
            .iter()
            .any(|t| t.pos == p && t.block == block)
        {
            self.redstone.timers.push(Timer {
                pos: p,
                block,
                at: self.total_ticks + delay,
                priority,
            });
        }
    }
    fn stored_signal(&self, w: &World, p: IVec3) -> Option<u8> {
        let state = w.get(p);
        let n = match state >> 4 {
            28 => return Some(self.detector_rail_comparator_signal(p, state)),
            118 => state & 3,
            92 => 14 - (state & 7) * 2,
            120 => {
                if state & 4 != 0 {
                    15
                } else {
                    0
                }
            }
            23 | 54 | 61 | 62 | 117 | 130 | 146 | 154 | 158 => {
                let c = self.containers.get(&super::container_key(p));
                if let Some(c) = c {
                    let count = c.slots.iter().flatten().count();
                    let fullness = c
                        .slots
                        .iter()
                        .flatten()
                        .map(|s| s.count as f32 / crate::inventory::stack_limit(s.id).max(1) as f32)
                        .sum::<f32>();
                    (fullness / c.slots.len().max(1) as f32 * 14.).floor() as u16
                        + u16::from(count > 0)
                } else {
                    0
                }
            }
            84 => {
                let record = item_nbt::number(
                    w.tile(p)
                        .and_then(item_nbt::compound)
                        .and_then(|v| v.get("Record")),
                );
                (record - 2255).clamp(0, 12) as u16
            }
            137 => item_nbt::number(
                w.tile(p)
                    .and_then(item_nbt::compound)
                    .and_then(|v| v.get("SuccessCount")),
            )
            .clamp(0, 15) as u16,
            _ => return None,
        };
        Some(n.min(15) as u8)
    }
    fn diode_inputs(&self, w: &World, p: IVec3, s: u16) -> (u8, u8) {
        let face = notes::diode_facing(s);
        let q = p + SIDES[face];
        let a = w.get(q);
        let mut input = signal(w, q, face, true);
        if a >> 4 == 55 {
            input = input.max((a & 15) as u8);
        }
        let repeater = matches!(s >> 4, 93 | 94);
        if !repeater {
            if let Some(value) = self.stored_signal(w, q) {
                input = value;
            } else if input < 15 && material::normal_cube(a) {
                if let Some(value) = self.stored_signal(w, q + SIDES[face]) {
                    input = value;
                }
            }
        }
        (input, notes::diode_side_input(w, p, repeater))
    }
    fn write_comparator(&mut self, w: &mut World, p: IVec3, signal: u8) {
        let state = w.get(p);
        self.world_commands.edit_tile(
            w,
            p,
            state,
            Some(DropNbt::Compound(
                [
                    ("id".into(), DropNbt::String("Comparator".into())),
                    ("x".into(), DropNbt::Int(p.x)),
                    ("y".into(), DropNbt::Int(p.y)),
                    ("z".into(), DropNbt::Int(p.z)),
                    ("OutputSignal".into(), DropNbt::Int(signal.into())),
                ]
                .into(),
            )),
        );
    }
    pub(super) fn diode_placed(&mut self, w: &mut World, p: IVec3, s: u16) {
        if s >> 4 == 149 {
            self.write_comparator(w, p, 0);
        }
        let (input, side) = self.diode_inputs(w, p, s);
        if input > 0 && (s >> 4 == 93 || input >= side) {
            self.circuit_timer(p, s >> 4, 1, 0);
        }
    }
    fn switch_comparator(&mut self, w: &mut World, p: IVec3, out: &mut Outcome) {
        let s = w.get(p);
        self.update_comparator(w,p,s,out);
    }
    fn update_comparator(&mut self,w:&mut World,p:IVec3,s:u16,out:&mut Outcome){
        let (input, side) = self.diode_inputs(w, p, s);
        let output = if s & 4 == 0 {
            input
        } else {
            input.saturating_sub(side)
        };
        let old = notes::comparator_tile(w, p) as u8;
        if old != output || s & 4 == 0 {
            let on = input > 0 && input >= side;
            let previous_on=s>>4==150 || s&8!=0;
            if previous_on!=on {set(w,p,(s&!8)|if on{8}else{0},out);}
            self.write_comparator(w, p, output);
            if old != output && previous_on==on {
                out.changed.push(p);
            }
        }
    }
    fn notify_device(&mut self, w: &mut World, p: IVec3, out: &mut Outcome) {
        let s = w.get(p);
        let id = s >> 4;
        if matches!(id, 75 | 76) {
            let face = torch_face(s);
            let q = p - SIDES[face];
            let under = w.get(q);
            let ok = if face == 1 {
                solid_top(under) || matches!(under >> 4, 20 | 85 | 95 | 113 | 139 | 188..=192)
            } else {
                !w.contains(q.as_vec3()) || (material::opaque(under) && material::full_cube(under))
            };
            if !ok {
                self.break_component(w, p, out);
                return;
            }
            if (id == 76) == (signal(w, q, face ^ 1, true) > 0) {
                self.circuit_timer(p, id, 2, 0);
            }
            return;
        }
        if !solid_top(w.get(p - IVec3::Y)) {
            self.break_component(w, p, out);
            return;
        }
        let (input, side) = self.diode_inputs(w, p, s);
        let repeater = matches!(id, 93 | 94);
        let delay = if repeater {
            u64::from(1 + ((s & 15) >> 2)) * 2
        } else {
            2
        };
        let rear = notes::diode_facing(s) ^ 1;
        let behind = w.get(p + SIDES[rear]);
        let chained =
            matches!(behind >> 4, 93 | 94 | 149 | 150) && notes::diode_facing(behind) != rear;
        let priority = if repeater {
            if chained {
                -3
            } else if id == 94 {
                -2
            } else {
                -1
            }
        } else if chained {
            -1
        } else {
            0
        };
        let changing = if repeater {
            side == 0 && (id == 94) != (input > 0)
        } else {
            let output = if s & 4 == 0 {
                input
            } else {
                input.saturating_sub(side)
            };
            (id == 150 || s & 8 != 0) != (input > 0 && input >= side)
                || i32::from(output) != notes::comparator_tile(w, p)
        };
        if changing {
            self.circuit_timer(p, id, delay, priority);
        }
    }
    fn tick_circuit_timers(&mut self, w: &mut World, out: &mut Outcome) {
        let now = self.total_ticks;
        self.redstone
            .torch_history
            .retain(|&(_, at)| now.saturating_sub(at) <= 60);
        let mut due = Vec::new();
        self.redstone.timers.retain(|t| {
            if t.at <= now && w.contains(t.pos.as_vec3()) {
                due.push(*t);
                false
            } else {
                true
            }
        });
        due.sort_by_key(|t| (t.at, t.priority));
        for t in due {
            let p = t.pos;
            let s = w.get(p);
            let id = s >> 4;
            if id != t.block {
                continue;
            }
            match id {
                137 => out.command_blocks.push(p),
                75 | 76 => {
                    let f = torch_face(s);
                    let off = signal(w, p - SIDES[f], f ^ 1, true) > 0;
                    if id == 76 && off {
                        set(w, p, (75 << 4) | (s & 15), out);
                        self.redstone.torch_history.push((p, now));
                        if self
                            .redstone
                            .torch_history
                            .iter()
                            .filter(|(q, _)| *q == p)
                            .count()
                            >= 8
                        {
                            let mut rng = self.block_ticks.world_random();
                            out.world_sounds.push((
                                "random.fizz",
                                p.as_dvec3() + glam::DVec3::splat(0.5),
                                0.5,
                                2.6 + (rng.next_float() - rng.next_float()) * 0.8,
                            ));
                            self.block_ticks.set_world_random(&rng);
                            let mut prng =
                                crate::vanilla::random::JavaRandom::from_state(self.particle_random[2]);
                            for _ in 0..5 {
                                if self.particles.len() >= 4000 {
                                    break;
                                }
                                let at = p.as_dvec3()
                                    + glam::DVec3::new(
                                        prng.next_double() * 0.6 + 0.2,
                                        prng.next_double() * 0.6 + 0.2,
                                        prng.next_double() * 0.6 + 0.2,
                                    );
                                if let Some(smoke) = super::Particle::network_sprite(
                                    11,
                                    at,
                                    glam::DVec3::ZERO,
                                    &mut prng,
                                ) {
                                    self.particles.push(smoke);
                                }
                            }
                            self.particle_random[2] = prng.state();
                            self.circuit_timer(p, 75, 160, 0);
                        }
                    } else if id == 75
                        && !off
                        && self
                            .redstone
                            .torch_history
                            .iter()
                            .filter(|(q, _)| *q == p)
                            .count()
                            < 8
                    {
                        set(w, p, (76 << 4) | (s & 15), out);
                    }
                }
                93 | 94 => {
                    let (input, side) = self.diode_inputs(w, p, s);
                    if side > 0 {
                        continue;
                    }
                    if id == 94 && input == 0 {
                        set(w, p, (93 << 4) | (s & 15), out);
                    } else if id == 93 {
                        set(w, p, (94 << 4) | (s & 15), out);
                        if input == 0 {
                            self.circuit_timer(p, 94, u64::from(1 + ((s & 15) >> 2)) * 2, -1);
                        }
                    }
                }
                149 | 150 => {
                    if id==150 {
                        let old=notes::comparator_tile(w,p);
                        set(w,p,149<<4|(s&15)|8,out);
                        self.write_comparator(w,p,old as u8);
                    }
                    self.update_comparator(w,p,s,out);
                }
                _ => (),
            }
        }
    }
}
fn torch_face(s: u16) -> usize {
    [1, 5, 4, 3, 2, 1, 1, 1][(s & 7) as usize]
}
#[cfg(test)]
#[test]
fn java_repeater_comparator_updates_and_six_face_item_placement(){
    use crate::{inventory::Stack,vanilla::{hand_placement,ray_trace::{Hit,Kind}},world::Chunk};
    type Device=(u16,u16,u16,u8,Vec<(u16,u64,i8)>,u16,u8,u16,u8);
    type Place=(u16,i32,usize,bool,u8,u16);
    let (devices,placements):(Vec<Device>,Vec<Place>)=serde_json::from_reader(flate2::read::GzDecoder::new(&include_bytes!("../tests/fixtures/vanilla189-circuit-uses.json.gz")[..])).unwrap();
    let base=IVec3::new(8,64,8);
    let mut w=World{chunks:[((0,0),Arc::new(Chunk::empty()))].into(),spawn:Vec3::ZERO,label:String::new(),save_path:Default::default(),edits:Default::default(),imported:false,light_revision:0};
    let mut sim=Simulation::default();sim.total_ticks=100;
    w.set(base-IVec3::Y,16);
    assert_eq!(devices.len(),256);
    for (i,(s,front,side,prior,scheduled,after,output,clicked,next_output)) in devices.into_iter().enumerate(){
        w.set(base,s);
        let f=notes::diode_facing(s);
        let right=[0,1,5,4,2,3][f];
        w.set(base+SIDES[f],front);w.set(base+SIDES[right],side);
        sim.write_comparator(&mut w,base,prior);
        sim.redstone.timers.clear();
        let mut out=Outcome::default();
        sim.notify_device(&mut w,base,&mut out);
        assert_eq!(sim.redstone.timers.iter().map(|t|(t.block,t.at-100,t.priority)).collect::<Vec<_>>(),scheduled,"device {i} scheduling state {s} front {front} side {side}");
        sim.redstone.timers=vec![Timer{pos:base,block:s>>4,at:100,priority:0}];
        sim.tick_circuit_timers(&mut w,&mut out);
        assert_eq!(w.get(base),after,"device {i} update state {s} front {front} side {side}");
        if s>>4>=149 {assert_eq!(notes::comparator_tile(&w,base),i32::from(output),"device {i} output");}
        assert!(sim.activate_circuit(&mut w,base,&mut out));
        assert_eq!(w.get(base),clicked,"device {i} click");
        if s>>4>=149 {assert_eq!(notes::comparator_tile(&w,base),i32::from(next_output),"device {i} clicked output");}
        for dir in &SIDES[2..]{w.set(base+*dir,0);}
    }
    assert_eq!(placements.len(),144);
    for (id,yaw,face,ok,left,state) in placements{
        for y in -2..=1{for x in -1..=1{for z in -1..=1{w.set(base+IVec3::new(x,y,z),0);}}}
        w.set(base,16);
        let at=base+SIDES[face];
        if face!=0{w.set(at-IVec3::Y,16);}
        let hit=Hit{position:base,face:face as u8,kind:Kind::Block,point:base.as_dvec3()+glam::DVec3::splat(0.5)};
        let mut player=Player::new(Vec3::ZERO);
        player.set_pose_f64([11.5,64.,11.5],(yaw*90)as f32,0.,true);
        let stack=Stack::new(id,2);
        let input=hand_placement::Input::from_hit(&player,&stack,hit);
        let action=hand_placement::plan(&w,input,|_|false).unwrap();
        assert_eq!((action.consume,action.accepted),(ok,ok),"item {id} yaw {yaw} face {face}");
        let mut out=Outcome::default(); sim.inventory.slots[0]=Some(stack.clone());
        sim.place_held_block(&mut w,&player,&stack,hit,&mut out);
        assert_eq!((w.get(at),sim.inventory.held().unwrap().count),(state,left),"item {id} yaw {yaw} face {face}");
    }
}
