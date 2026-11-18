use super::*;
type Args<'a> = BTreeMap<&'a str, &'a str>;

fn parts(s: &str) -> Option<(char, Args<'_>)> {
    let mut chars = s.strip_prefix('@')?.chars();
    let code = chars.next()?;
    if !matches!(code, 'p' | 'a' | 'r' | 'e') {
        return None;
    }
    let tail = chars.as_str();
    let mut args = BTreeMap::new();
    if !tail.is_empty() {
        let body = tail.strip_prefix('[')?.strip_suffix(']')?;
        for (i, word) in body.split(',').enumerate() {
            if word.is_empty() {
                continue;
            }
            if let Some((k, v)) = word.split_once('=') {
                args.insert(k, v);
            } else {
                args.insert(*["x", "y", "z", "r"].get(i)?, word);
            }
        }
    }
    Some((code, args))
}
fn num(args: &Args<'_>, k: &str, d: i32) -> i32 {
    args.get(k).and_then(|v| v.parse().ok()).unwrap_or(d)
}
pub(super) fn multiple(s: &str) -> bool {
    parts(s)
        .is_some_and(|(c, args)| num(&args, "c", if matches!(c, 'e' | 'a') { 0 } else { 1 }) != 1)
}
fn filter_name(needle: Option<&&str>, s: &str) -> bool {
    needle.is_none_or(|v| {
        v.strip_prefix('!')
            .map_or(*v == s, |excluded| s != excluded)
    })
}
fn angle(a: f32, min: i32, max: i32) -> bool {
    let v = (a.floor() as i32).rem_euclid(360);
    let lo = min.rem_euclid(360);
    let hi = max.rem_euclid(360);
    if lo <= hi {
        v >= lo && v <= hi
    } else {
        v >= lo || v <= hi
    }
}
impl Context<'_> {
    fn all(&self) -> Vec<Target> {
        std::iter::once(Target::Player)
            .chain(self.sim.mobs.iter().map(|e| Target::Mob(e.id)))
            .chain(self.sim.drops.iter().map(|e| Target::Drop(e.id)))
            .chain(
                self.sim
                    .xp_orbs
                    .iter()
                    .filter(|e| !e.dead)
                    .map(|e| Target::Orb(e.id)),
            )
            .collect()
    }
    pub(super) fn target_key(&self, t: Target) -> String {
        let (kind, id) = match t {
            Target::Block(p) => return format!("block,{},{},{}", p.x, p.y, p.z),
            Target::Player => return self.options.username.clone(),
            Target::Mob(i) => (1, i),
            Target::Drop(i) => (2, i),
            Target::Orb(i) => (3, i as u32 as u64),
        };
        format!(
            "f1890000-0000-400{kind}-{:04x}-{:012x}",
            id >> 48,
            id & 0xffffffffffff
        )
    }
    fn target_type(&self, t: Target) -> String {
        match t {
            Target::Block(_) => "@".into(),
            Target::Player => "Player".into(),
            Target::Mob(i) => self
                .sim
                .mobs
                .iter()
                .find(|e| e.id == i)
                .map(|e| e.kind.name().to_owned())
                .unwrap_or_default(),
            Target::Drop(_) => "Item".into(),
            Target::Orb(_) => "XPOrb".into(),
        }
    }
    pub(super) fn target_name(&self, t: Target) -> String {
        if let Target::Block(p) = t {
            return self.world.tile(p).and_then(crate::vanilla::item_nbt::compound)
                .and_then(|m| m.get("CustomName"))
                .and_then(|v| if let crate::vanilla::drops::DropNbt::String(s)=v {Some(s.clone())} else {None})
                .unwrap_or_else(|| "@".into());
        }
        if let Target::Mob(i) = t {
            if let Some(m) = self.sim.command_entities.mobs.get(&i) {
                if !m.name.is_empty() {
                    return m.name.clone();
                }
            }
        }
        if t == Target::Player {
            self.options.username.clone()
        } else {
            self.target_type(t)
        }
    }
    pub(super) fn position(&self, t: Target) -> DVec3 {
        match t {
            Target::Block(p) => p.as_dvec3() + DVec3::splat(0.5),
            Target::Player => self.player.precise_position().into(),
            Target::Mob(i) => self
                .sim
                .mobs
                .iter()
                .find(|e| e.id == i)
                .map_or(DVec3::ZERO, |e| e.position.as_dvec3()),
            Target::Drop(i) => self
                .sim
                .drops
                .iter()
                .find(|e| e.id == i)
                .map_or(DVec3::ZERO, |e| e.position),
            Target::Orb(i) => self
                .sim
                .xp_orbs
                .iter()
                .find(|e| e.id == i)
                .map_or(DVec3::ZERO, |e| e.position),
        }
    }
    pub(super) fn targets(&self, s: &str) -> CmdResult<Vec<Target>> {
        let Some((code, args)) = parts(s) else {
            return self
                .all()
                .into_iter()
                .find(|&t| {
                    self.target_key(t) == s
                        || (t == Target::Player && s.eq_ignore_ascii_case(&self.options.username))
                })
                .map(|t| vec![t])
                .ok_or_else(|| error("commands.generic.entity.notFound", vec![]));
        };
        if !self.allowed("give") {
            return Ok(vec![]);
        }
        if let Some(raw)=args.get("type") {
            let name=raw.strip_prefix('!').unwrap_or(raw);
            if name!="Player" && !CATALOG.entities.contains(name){return Err(error("commands.generic.entity.invalidType",vec![json!(name)]))}
        }
        let p = self.origin.floor().as_ivec3();
        let base = IVec3::new(
            num(&args, "x", p.x),
            num(&args, "y", p.y),
            num(&args, "z", p.z),
        )
        .as_dvec3();
        let n = num(&args, "c", if matches!(code, 'e' | 'a') { 0 } else { 1 });
        let mode = self
            .sim
            .commands
            .player_mode
            .unwrap_or(u8::from(self.sim.mode == Mode::Creative)) as i32;
        let mut hits = self.all();
        hits.retain(|&t| {
            let loc = self.position(t);
            let player = t == Target::Player;
            if !(player || code == 'e' || (code == 'r' && args.contains_key("type"))) {
                return false;
            }
            if matches!(code, 'e' | 'r') && !filter_name(args.get("type"), &self.target_type(t)) {
                return false;
            }
            if !filter_name(args.get("name"), &self.target_name(t)) {
                return false;
            }
            let key = self.target_key(t);
            let board = &self.sim.commands.board;
            if !filter_name(
                args.get("team"),
                board.members.get(&key).map_or("", String::as_str),
            ) {
                return false;
            }
            if (args.contains_key("l") || args.contains_key("lm"))
                && (!player
                    || self.sim.xp.level < num(&args, "lm", -1)
                    || self.sim.xp.level > num(&args, "l", i32::MAX))
            {
                return false;
            }
            if args.contains_key("m")
                && num(&args, "m", -1) >= 0
                && (!player || mode != num(&args, "m", -1))
            {
                return false;
            }
            let dist = loc.distance_squared(base + DVec3::splat(0.5)) as i32;
            let (mn, mx) = (num(&args, "rm", -1), num(&args, "r", -1));
            if (mn >= 0 && dist < mn.saturating_mul(mn))
                || (mx >= 0 && dist > mx.saturating_mul(mx))
            {
                return false;
            }
            if ["dx", "dy", "dz"].iter().any(|k| args.contains_key(k)) {
                let end = base
                    + IVec3::new(
                        num(&args, "dx", 0),
                        num(&args, "dy", 0),
                        num(&args, "dz", 0),
                    )
                    .as_dvec3();
                let lo = base.min(end);
                let hi = base.max(end) + DVec3::ONE;
                let (width, height) = match t {
                    Target::Mob(i) => self
                        .sim
                        .mobs
                        .iter()
                        .find(|e| e.id == i)
                        .map_or((0.6, 1.8), |e| self.sim.mob_size(e)),
                    Target::Player => (0.6, 1.8),
                    _ => (0.25, 0.25),
                };
                let half = DVec3::new(width as f64 / 2., 0., width as f64 / 2.);
                if (loc - half).cmpge(hi).any()
                    || (loc + half + DVec3::Y * height as f64).cmple(lo).any()
                {
                    return false;
                }
            }
            let [yaw, pitch] = if player {
                self.player.java_rotation()
            } else if let Target::Mob(i) = t {
                [
                    self.sim
                        .mobs
                        .iter()
                        .find(|e| e.id == i)
                        .map_or(0., |e| e.yaw.to_degrees() - 180.),
                    self.sim
                        .command_entities
                        .mobs
                        .get(&i)
                        .map_or(0., |e| e.pitch),
                ]
            } else {
                [0., 0.]
            };
            if !angle(yaw, num(&args, "rym", 0), num(&args, "ry", 359))
                || !angle(pitch, num(&args, "rxm", 0), num(&args, "rx", 359))
            {
                return false;
            }
            for (k, s) in &args {
                if let Some(obj) = k.strip_prefix("score_") {
                    let bound = s.parse().unwrap_or(1);
                    let min = obj.strip_suffix("_min");
                    let Some(score) = board.score(&key, min.unwrap_or(obj)) else {
                        return false;
                    };
                    if min.is_some() && score < bound || min.is_none() && score > bound {
                        return false;
                    }
                }
            }
            true
        });
        if code == 'r' {
            use crate::vanilla::random::JavaRandom;
            let mut r = JavaRandom::new(JavaRandom::independent_seed());
            for i in (1..hits.len()).rev() {
                hits.swap(i, r.next_int((i + 1) as i32) as usize)
            }
        } else {
            hits.sort_by(|a, b| {
                self.position(*a)
                    .distance_squared(base)
                    .total_cmp(&self.position(*b).distance_squared(base))
            });
            if n == 1 && hits.contains(&self.sender) {
                return Ok(vec![self.sender]);
            }
        }
        if n < 0 {
            hits.reverse()
        }
        if n != 0 {
            hits.truncate(n.unsigned_abs() as usize)
        }
        Ok(hits)
    }
    pub(super) fn one(&self, s: &str) -> CmdResult<Target> {
        let hits = self.targets(s)?;
        if hits.len() == 1 {
            Ok(hits[0])
        } else {
            Err(error("commands.generic.entity.notFound", vec![]))
        }
    }
}
