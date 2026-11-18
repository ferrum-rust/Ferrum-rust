use super::*;
pub(super) fn output(
    kind: &str,
    grid: &[Option<Stack>],
    width: usize,
    scale: &impl Fn(u16) -> Option<i8>,
) -> Option<Stack> {
    let items = grid.iter().flatten().collect::<Vec<_>>();
    match kind {
        "RecipeRepairItem" => repair(&items),
        "RecipesArmorDyes" => dye(&items),
        "RecipeFireworks" => fireworks(&items),
        "RecipeBookCloning" => clone_book(&items),
        "RecipesMapCloning" => clone_map(&items),
        "RecipesMapExtending" => extend_map(grid, width, scale),
        "RecipesBanners$RecipeAddPattern" => add_pattern(grid),
        "RecipesBanners$RecipeDuplicatePattern" => copy_banner(&items),
        _ => None,
    }
}
fn one(s: &Stack) -> Stack {
    let mut s = s.copied();
    s.count = 1;
    s
}
fn repair(items: &[&Stack]) -> Option<Stack> {
    let &[a, b] = items else {
        return None;
    };
    if a.id != b.id || a.count != 1 || b.count != 1 {
        return None;
    }
    let max = i32::from(item_entity::facts(a.id)?.max_damage);
    if max == 0 {
        return None;
    }
    let damage = (i32::from(a.damage) + i32::from(b.damage) - max - max * 5 / 100).max(0);
    item_entity::stack_from_metadata(a.id, 1, damage as u16).ok()
}
fn dye(items: &[&Stack]) -> Option<Stack> {
    let mut armor = None;
    let mut sum = [0; 3];
    let mut maximum = 0;
    let mut count = 0;
    let mut has_dye = false;
    for &s in items {
        if (298..=301).contains(&s.id) {
            if armor.replace(s).is_some() {
                return None;
            }
            if let Some(DropNbt::Int(color)) =
                named(root(s), "display").and_then(|c| c.get("color"))
            {
                let rgb =
                    [color >> 16 & 255, color >> 8 & 255, color & 255].map(|c| c as f32 / 255.);
                maximum = (maximum as f32 + rgb[0].max(rgb[1].max(rgb[2])) * 255.) as i32;
                for i in 0..3 {
                    sum[i] = (sum[i] as f32 + rgb[i] * 255.) as i32;
                }
                count += 1;
            }
        } else if s.id == 351 {
            let data = if s.meta < 16 { s.meta as usize } else { 0 };
            let rgb = registry().sheep_rgb[data].map(|c| (c * 255.) as i32);
            maximum += *rgb.iter().max().unwrap();
            for i in 0..3 {
                sum[i] += rgb[i];
            }
            count += 1;
            has_dye = true;
        } else {
            return None;
        }
    }
    if !has_dye {
        return None;
    }
    let s = armor?;
    let avg = sum.map(|n| n / count);
    let bright = maximum as f32 / count as f32;
    let max = *avg.iter().max().unwrap() as f32;
    let [r, g, b] = avg.map(|c| (c as f32 * bright / max) as i32);
    let mut tag = root(s).cloned().unwrap_or_default();
    nested(&mut tag, "display").insert("color".into(), DropNbt::Int((((r << 8) + g) << 8) + b));
    Some(one(s).with_nbt(DropNbt::Compound(tag)))
}
fn fireworks(items: &[&Stack]) -> Option<Stack> {
    let mut powder = 0;
    let mut paper = 0;
    let mut stars = Vec::new();
    let mut colors = Vec::new();
    let mut modifiers = 0;
    let mut shapes = 0;
    let mut shape = 0i8;
    let mut explosion = Compound::new();
    for &s in items {
        match s.id {
            289 => powder += 1,
            339 => paper += 1,
            402 => stars.push(s),
            351 => colors.push(registry().firework_rgb[(s.meta & 15) as usize]),
            348 | 264 => {
                modifiers += 1;
                explosion.insert(
                    if s.id == 348 {
                        "Flicker".into()
                    } else {
                        "Trail".into()
                    },
                    DropNbt::Byte(1),
                );
            }
            385 | 288 | 371 | 397 => {
                shapes += 1;
                shape = match s.id {
                    385 => 1,
                    288 => 4,
                    371 => 2,
                    _ => 3,
                };
            }
            _ => return None,
        }
    }
    if powder > 3 || paper > 1 {
        return None;
    }
    if powder >= 1 && paper == 1 && modifiers == 0 && shapes == 0 && colors.is_empty() {
        let mut rocket = Stack::new(401, 1);
        if !stars.is_empty() {
            let list = stars
                .iter()
                .filter_map(|s| named(root(s), "Explosion").cloned().map(DropNbt::Compound))
                .collect::<Vec<_>>();
            let tag = Compound::from([
                (
                    "Explosions".into(),
                    DropNbt::List {
                        element: if list.is_empty() { 0 } else { 10 },
                        values: list,
                    },
                ),
                ("Flight".into(), DropNbt::Byte(powder)),
            ]);
            rocket = rocket.with_nbt(DropNbt::Compound(Compound::from([(
                "Fireworks".into(),
                DropNbt::Compound(tag),
            )])));
        }
        return Some(rocket);
    }
    if powder == 1 && paper == 0 && stars.is_empty() && !colors.is_empty() && shapes <= 1 {
        explosion.insert("Colors".into(), DropNbt::Ints(colors));
        explosion.insert("Type".into(), DropNbt::Byte(shape));
        return Some(
            Stack::new(402, 1).with_nbt(DropNbt::Compound(Compound::from([(
                "Explosion".into(),
                DropNbt::Compound(explosion),
            )]))),
        );
    }
    if powder == 0
        && paper == 0
        && stars.len() == 1
        && !colors.is_empty()
        && shapes == 0
        && modifiers == 0
    {
        let s = stars[0];
        let mut tag = root(s)?.clone();
        let DropNbt::Compound(e) = tag.get_mut("Explosion")? else {
            return None;
        };
        e.insert("FadeColors".into(), DropNbt::Ints(colors));
        return Some(one(s).with_nbt(DropNbt::Compound(tag)));
    }
    None
}
fn template<'a>(items: &[&'a Stack], original: u16, blank: u16) -> Option<(&'a Stack, u8)> {
    let mut found = None;
    let mut count = 0;
    for &s in items {
        if s.id == original {
            if found.replace(s).is_some() {
                return None;
            }
        } else if s.id == blank {
            count += 1;
        } else {
            return None;
        }
    }
    (count > 0).then_some((found?, count))
}
fn clone_book(items: &[&Stack]) -> Option<Stack> {
    let (s, count) = template(items, 387, 386)?;
    let mut tag = root(s)?.clone();
    let generation = item_nbt::number(tag.get("generation"));
    if generation >= 2 {
        return None;
    }
    tag.insert(
        "generation".into(),
        DropNbt::Int(generation.wrapping_add(1)),
    );
    Some(Stack::new(387, count).with_nbt(DropNbt::Compound(tag)))
}
fn clone_map(items: &[&Stack]) -> Option<Stack> {
    let (s, count) = template(items, 358, 395)?;
    let mut out = item_entity::stack_from_metadata(358, count + 1, s.meta).ok()?;
    if let Some(DropNbt::String(name)) = named(root(s), "display").and_then(|c| c.get("Name")) {
        let mut tag = Compound::new();
        nested(&mut tag, "display").insert("Name".into(), DropNbt::String(name.clone()));
        out = out.with_nbt(DropNbt::Compound(tag));
    }
    Some(out)
}
fn extend_map(
    grid: &[Option<Stack>],
    width: usize,
    scale: &impl Fn(u16) -> Option<i8>,
) -> Option<Stack> {
    if width != 3
        || !grid.iter().enumerate().all(|(i, s)| {
            s.as_ref()
                .is_some_and(|s| s.id == if i == 4 { 358 } else { 339 })
        })
    {
        return None;
    }
    let map = grid[4].as_ref()?;
    if scale(map.meta)? >= 4 {
        return None;
    }
    let mut tag = root(map).cloned().unwrap_or_default();
    tag.insert("map_is_scaling".into(), DropNbt::Byte(1));
    Some(one(map).with_nbt(DropNbt::Compound(tag)))
}
pub(super) fn patterns(s: &Stack) -> usize {
    match named(root(s), "BlockEntityTag").and_then(|c| c.get("Patterns")) {
        Some(DropNbt::List {
            element: 10,
            values,
        }) => values.len(),
        _ => 0,
    }
}
fn base(s: &Stack) -> i32 {
    match named(root(s), "BlockEntityTag").and_then(|c| c.get("Base")) {
        Some(tag) => item_nbt::number(Some(tag)),
        None => i32::from(s.meta),
    }
}
fn copy_banner(items: &[&Stack]) -> Option<Stack> {
    let &[a, b] = items else {
        return None;
    };
    if a.id != 425 || b.id != 425 || base(a) != base(b) {
        return None;
    }
    match (patterns(a) > 0, patterns(b) > 0) {
        (true, false) => Some(one(a)),
        (false, true) => Some(one(b)),
        _ => None,
    }
}
fn add_pattern(grid: &[Option<Stack>]) -> Option<Stack> {
    let banners = grid
        .iter()
        .flatten()
        .filter(|s| s.id == 425)
        .collect::<Vec<_>>();
    let &[banner] = banners.as_slice() else {
        return None;
    };
    if patterns(banner) >= 6 {
        return None;
    }
    let (pattern, _, _) = registry().patterns.iter().find(|(_, rows, special)| {
        if let Some(spec) = special {
            let mut ingredient = false;
            let mut dye = false;
            for s in grid.iter().flatten() {
                if s.id == 425 {
                    continue;
                }
                if s.id == 351 && !dye {
                    dye = true;
                } else if !ingredient && s.id == spec.0 && metadata(s) == spec.2 {
                    ingredient = true;
                } else {
                    return false;
                }
            }
            ingredient
        } else {
            let Some(first) = &rows[0] else {
                return false;
            };
            if grid.len() != rows.len() * first.len() {
                return false;
            }
            let mut color = None;
            grid.iter().enumerate().all(|(i, s)| {
                let wants = rows[i / 3]
                    .as_ref()
                    .is_some_and(|r| r.as_bytes()[i % 3] != b' ');
                match s {
                    None => !wants,
                    Some(s) if s.id == 425 => !wants,
                    Some(s) if s.id == 351 && wants => {
                        let same = color.is_none_or(|m| m == s.meta);
                        color = Some(s.meta);
                        same
                    }
                    _ => false,
                }
            })
        }
    })?;
    let dye = grid
        .iter()
        .flatten()
        .find(|s| s.id == 351)
        .map_or(0, |s| i32::from(s.meta));
    let mut tag = root(banner).cloned().unwrap_or_default();
    let tile = nested(&mut tag, "BlockEntityTag");
    let list = tile
        .entry("Patterns".into())
        .or_insert_with(|| DropNbt::List {
            element: 0,
            values: vec![],
        });
    if !matches!(list, DropNbt::List { .. }) {
        *list = DropNbt::List {
            element: 0,
            values: vec![],
        };
    }
    if let DropNbt::List { element, values } = list {
        if *element == 0 || *element == 10 || values.is_empty() {
            *element = 10;
            values.push(DropNbt::Compound(Compound::from([
                ("Pattern".into(), DropNbt::String(pattern.clone())),
                ("Color".into(), DropNbt::Int(dye)),
            ])));
        }
    }
    Some(one(banner).with_nbt(DropNbt::Compound(tag)))
}
