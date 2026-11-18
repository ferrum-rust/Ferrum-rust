use super::super::snbt;
use super::*;
use crate::gameplay::container_key;
use crate::vanilla::{item_nbt, middle_pick};
use crate::world::World;
use glam::Vec3;

#[derive(Clone, Copy)]
struct Area {
    lo: IVec3,
    hi: IVec3,
}
impl Area {
    fn new(a: IVec3, b: IVec3) -> Self {
        Self {
            lo: a.min(b),
            hi: a.max(b),
        }
    }
    fn volume(self) -> i64 {
        let [x, y, z] = (self.hi - self.lo + IVec3::ONE).to_array();
        i64::from(x) * i64::from(y) * i64::from(z)
    }
    fn loaded(self, w: &World) -> bool {
        (0..256).contains(&self.lo.y)
            && (0..256).contains(&self.hi.y)
            && (self.lo.x.div_euclid(16)..=self.hi.x.div_euclid(16)).all(|x| {
                (self.lo.z.div_euclid(16)..=self.hi.z.div_euclid(16))
                    .all(|z| w.chunks.contains_key(&(x, z)))
            })
    }
    fn positions(self) -> impl Iterator<Item = IVec3> {
        (self.lo.z..=self.hi.z).flat_map(move |z| {
            (self.lo.y..=self.hi.y)
                .flat_map(move |y| (self.lo.x..=self.hi.x).map(move |x| IVec3::new(x, y, z)))
        })
    }
    fn interior(self, p: IVec3) -> bool {
        p.cmpgt(self.lo).all() && p.cmplt(self.hi).all()
    }
}
fn tile(ctx: &Context<'_>, p: IVec3) -> Option<DropNbt> {
    let s = ctx.world.get(p);
    middle_pick::tile_snapshot(
        s >> 4,
        p,
        ctx.world.tile(p),
        ctx.sim.containers.get(&container_key(p)),
    )
}
fn put_tile(ctx: &mut Context<'_>, p: IVec3, s: u16, mut tag: DropNbt) {
    let DropNbt::Compound(m) = &mut tag else {
        return;
    };
    for (k, v) in ["x", "y", "z"].into_iter().zip(p.to_array()) {
        m.insert(k.into(), DropNbt::Int(v));
    }
    if let Some(mut c) = crate::container::Container::for_block(s >> 4) {
        if let Some(DropNbt::List { values, .. }) = m.get("Items") {
            for value in values {
                let Some(values) = item_nbt::compound(value) else {
                    continue;
                };
                let n = item_nbt::number(values.get("Slot")) as usize;
                if let Some(slot) = c.slots.get_mut(n) {
                    *slot = item_nbt::encode(value)
                        .ok()
                        .and_then(|b| crate::nbt::read(&b).ok())
                        .and_then(|t| crate::inventory::from_nbt(&t));
                }
            }
        }
        c.burn = item_nbt::number(m.get("BurnTime")).max(0) as u32;
        c.transfer_cooldown = item_nbt::number(m.get("TransferCooldown"));
        c.burn_total = c.burn.max(1);
        c.cook = item_nbt::number(m.get("CookTime")).max(0) as u32;
        ctx.sim.containers.insert(container_key(p), c);
    }
    ctx.sim.world_commands.edit_tile(ctx.world, p, s, Some(tag));
}
fn erase(ctx: &mut Context<'_>, p: IVec3) {
    let old = ctx.world.get(p);
    if old != 0 {
        if let Some(c) = ctx.sim.containers.remove(&container_key(p)) {
            for item in c.slots.into_iter().flatten() {
                ctx.sim
                    .spawn_drop(item, p.as_vec3() + Vec3::splat(0.5), Vec3::ZERO);
            }
        }
        ctx.sim.destroy_command_drops(old, p);
        write(ctx, p, 0, None);
    }
}
fn write(ctx: &mut Context<'_>, p: IVec3, state: u16, tag: Option<DropNbt>) -> bool {
    if ctx.world.get(p) == state {
        return false;
    }
    ctx.world.set(p, state);
    ctx.sim.containers.remove(&container_key(p));
    ctx.sim.world_commands.edit_tile(ctx.world, p, state, None);
    if let Some(tag) = tag {
        put_tile(ctx, p, state, tag);
    }
    ctx.changed.push(p);
    ctx.sim.block_ticks.notify(ctx.world, p);
    if state >> 4 == 19 {
        ctx.sim.on_sponge_added(ctx.world, p, state, None);
        ctx.changed.append(&mut ctx.sim.pending_changed_blocks);
    }
    true
}
fn place(ctx: &mut Context<'_>, p: IVec3, new: u16, data: Option<&DropNbt>) -> bool {
    if tile(ctx, p).is_some() {
        write(ctx, p, 0, None);
    }
    let tag = data.and_then(|d| {
        let mut t = middle_pick::tile_snapshot(new >> 4, p, None, None)?;
        snbt::merge(&mut t, d.clone());
        Some(t)
    });
    write(ctx, p, new, tag)
}
fn parse_tag(args: &[&str], key: &str) -> CmdResult<DropNbt> {
    snbt::parse(&args.join(" ")).map_err(|e| {
        error(
            key,
            vec![json!(e
                .get("text")
                .and_then(Value::as_str)
                .or_else(|| e.get("with").and_then(|v| v.get(0)).and_then(Value::as_str))
                .unwrap_or("Invalid tag"))],
        )
    })
}
fn finish(ctx: &mut Context<'_>, name: &str, count: i32) -> Reply {
    ctx.results[1] = count;
    response(name, vec![json!(count)])
}
fn block_args(p: IVec3) -> Vec<Value> {
    p.to_array().map(|v| json!(v)).to_vec()
}
fn without_pos(tag: Option<DropNbt>) -> Option<DropNbt> {
    tag.map(|mut t| {
        if let DropNbt::Compound(o) = &mut t {
            for k in ["x", "y", "z"] {
                o.remove(k);
            }
        }
        t
    })
}
fn simple(ctx: &mut Context<'_>, name: &str, args: &[&str]) -> Reply {
    let p = blockpos(ctx, args)?;
    let area = Area::new(p, p);
    if !area.loaded(ctx.world) {
        return Err(fail(&format!("commands.{name}.outOfWorld")));
    }
    let old = ctx.world.get(p);
    if name == "blockdata" {
        let mut t = tile(ctx, p).ok_or_else(|| fail("commands.blockdata.notValid"))?;
        let previous = t.clone();
        snbt::merge(
            &mut t,
            parse_tag(&args[3..], "commands.blockdata.tagError")?,
        );
        if without_pos(Some(t.clone())) == without_pos(Some(previous)) {
            return Err(error("commands.blockdata.failed", vec![json!(show(&t))]));
        }
        put_tile(ctx, p, old, t);
        ctx.changed.push(p);
        ctx.results[1] = 1;
        return response(
            "commands.blockdata.success",
            vec![json!(show(&tile(ctx, p).unwrap()))],
        );
    }
    let id = block_id(args[3])?;
    if name == "testforblock" {
        let wanted = args
            .get(4)
            .map(|s| integer(s, -1, 15))
            .transpose()?
            .unwrap_or(-1);
        let mut values = block_args(p);
        let nbt = if args.len() > 5 && middle_pick::tile_snapshot(id, p, None, None).is_some() {
            Some(parse_tag(&args[5..], "commands.setblock.tagError")?)
        } else {
            None
        };
        if old >> 4 != id {
            let label = |id| {
                crate::world_types::blocks()
                    .iter()
                    .find(|(_, v)| v.0 == id)
                    .map(|(k, _)| k.as_str())
                    .unwrap_or("air")
            };
            values.extend([json!(label(old >> 4)), json!(label(id))]);
            return Err(error("commands.testforblock.failed.tile", values));
        }
        if wanted >= 0 && old & 15 != wanted as u16 {
            values.extend([json!(old & 15), json!(wanted)]);
            return Err(error("commands.testforblock.failed.data", values));
        }
        if let Some(query) = nbt {
            let t = tile(ctx, p)
                .ok_or_else(|| error("commands.testforblock.failed.tileEntity", values.clone()))?;
            if !snbt::matches(&query, &t) {
                return Err(error("commands.testforblock.failed.nbt", values));
            }
        }
        ctx.results[1] = 1;
        return response("commands.testforblock.success", values);
    }
    let new = state(
        args[3],
        args.get(4)
            .map(|s| integer(s, 0, 15))
            .transpose()?
            .unwrap_or(0),
    )?;
    let data = if args.len() >= 7 && middle_pick::tile_snapshot(id, p, None, None).is_some() {
        Some(parse_tag(&args[6..], "commands.setblock.tagError")?)
    } else {
        None
    };
    match args.get(5).copied() {
        Some("keep") if old != 0 => return Err(fail("commands.setblock.noChange")),
        Some("destroy") => {
            erase(ctx, p);
            if new == 0 {
                return response("commands.setblock.success", vec![]);
            }
        }
        _ => {}
    }
    if !place(ctx, p, new, data.as_ref()) {
        return Err(fail("commands.setblock.noChange"));
    }
    ctx.results[1] = 1;
    response("commands.setblock.success", vec![])
}
fn fill(ctx: &mut Context<'_>, args: &[&str], area: Area) -> Reply {
    let new = state(
        args[6],
        args.get(7)
            .map(|s| integer(s, 0, 15))
            .transpose()?
            .unwrap_or(0),
    )?;
    let mode = args.get(8).copied().unwrap_or("replace");
    let tiled = middle_pick::tile_snapshot(new >> 4, area.lo, None, None).is_some();
    let tag = if tiled && args.len() >= 10 {
        Some(parse_tag(&args[9..], "commands.fill.tagError")?)
    } else {
        None
    };
    let mut filter = None;
    let mut data = None;
    if !tiled && mode == "replace" {
        filter = args.get(9).map(|s| block_id(s)).transpose()?;
        data = args
            .get(10)
            .map(|s| integer(s, i32::MIN, i32::MAX))
            .transpose()?;
    }
    let mut count = 0;
    for p in area.positions() {
        let old = ctx.world.get(p);
        if filter.is_some_and(|id| id != old >> 4) || data.is_some_and(|m| i32::from(old & 15) != m)
        {
            continue;
        }
        if mode == "keep" && old != 0 {
            continue;
        }
        if area.interior(p) && matches!(mode, "hollow" | "outline") {
            if mode == "hollow" {
                write(ctx, p, 0, None);
            }
            continue;
        }
        if mode == "destroy" {
            erase(ctx, p)
        }
        if place(ctx, p, new, tag.as_ref()) {
            count += 1;
        }
    }
    if count == 0 {
        return Err(fail("commands.fill.failed"));
    }
    finish(ctx, "commands.fill.success", count)
}
fn compare_clone(ctx: &mut Context<'_>, args: &[&str], src: Area, dst: Area, copy: bool) -> Reply {
    let filter = args.get(9).copied().unwrap_or("replace");
    let masked = filter == "masked";
    let mode = args.get(10).copied().unwrap_or("normal");
    let shift = dst.lo - src.lo;
    if !copy {
        let mut n = 0;
        for p in src.positions() {
            let old = ctx.world.get(p);
            if masked && old == 0 {
                continue;
            }
            if old != ctx.world.get(p + shift)
                || without_pos(tile(ctx, p)) != without_pos(tile(ctx, p + shift))
            {
                return Err(fail("commands.compare.failed"));
            }
            n += 1;
        }
        return finish(ctx, "commands.compare.success", n);
    }
    if !matches!(mode, "force" | "move") && src.lo.cmple(dst.hi).all() && dst.lo.cmple(src.hi).all()
    {
        return Err(fail("commands.clone.noOverlap"));
    }
    let wanted = if filter == "filtered" {
        Some(block_id(args.get(11).ok_or_else(|| usage("clone"))?)?)
    } else {
        None
    };
    let meta = if wanted.is_some() {
        args.get(12).map(|s| integer(s, 0, 15)).transpose()?
    } else {
        None
    };
    let mut entries = Vec::new();
    for p in src.positions() {
        let s = ctx.world.get(p);
        if (masked && s == 0)
            || wanted.is_some_and(|v| v != s >> 4)
            || meta.is_some_and(|v| v != i32::from(s & 15))
        {
            continue;
        }
        entries.push((p, s, tile(ctx, p)));
    }
    if mode == "move" {
        for (p, _, _) in &entries {
            write(ctx, *p, 0, None);
        }
    }
    for (p, _, _) in entries.iter().rev() {
        write(ctx, *p + shift, 166 << 4, None);
    }
    let mut count = 0;
    for (p, s, t) in entries {
        if write(ctx, p + shift, s, t) {
            count += 1;
        }
    }
    if count == 0 {
        return Err(fail("commands.clone.failed"));
    }
    finish(ctx, "commands.clone.success", count)
}
pub(super) fn run(ctx: &mut Context<'_>, name: &str, args: &[&str]) -> Reply {
    let min = match name {
        "fill" => 7,
        "clone" | "testforblocks" => 9,
        _ => 4,
    };
    if args.len() < min {
        return Err(usage(name));
    }
    ctx.results[1] = 0;
    if min == 4 {
        return simple(ctx, name, args);
    }
    let area = Area::new(blockpos(ctx, args)?, blockpos(ctx, &args[3..])?);
    let op = if name == "testforblocks" {
        "compare"
    } else {
        name
    };
    let limit = if op == "compare" { 524288 } else { 32768 };
    if area.volume() > limit {
        return Err(error(
            &format!("commands.{op}.tooManyBlocks"),
            vec![json!(area.volume()), json!(limit)],
        ));
    }
    if !area.loaded(ctx.world) {
        return Err(fail(&format!("commands.{op}.outOfWorld")));
    }
    if name == "fill" {
        return fill(ctx, args, area);
    }
    let lo = blockpos(ctx, &args[6..])?;
    let dst = Area::new(lo, lo + (area.hi - area.lo));
    if !dst.loaded(ctx.world) {
        return Err(fail(&format!("commands.{op}.outOfWorld")));
    }
    compare_clone(ctx, args, area, dst, name == "clone")
}
fn show(t: &DropNbt) -> String {
    match t {
        DropNbt::Byte(n) => format!("{n}b"),
        DropNbt::Short(n) => format!("{n}s"),
        DropNbt::Long(n) => format!("{n}L"),
        DropNbt::Int(n) => n.to_string(),
        DropNbt::Float(n) => format!("{n}f"),
        DropNbt::Double(n) => format!("{n}d"),
        DropNbt::String(s) => format!("\"{}\"", s.replace('"', "\\\"")),
        DropNbt::Bytes(v) => format!("[{} bytes]", v.len()),
        DropNbt::Ints(v) => format!(
            "[{}]",
            v.iter().map(|n| format!("{n},")).collect::<String>()
        ),
        DropNbt::Longs(v) => format!("{v:?}"),
        DropNbt::List { values, .. } => format!(
            "[{}]",
            values
                .iter()
                .enumerate()
                .map(|(i, x)| format!("{i}:{}", show(x)))
                .collect::<Vec<_>>()
                .join(",")
        ),
        DropNbt::Compound(v) => format!(
            "{{{}}}",
            v.iter()
                .map(|(k, x)| format!("{k}:{}", show(x)))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}
