use super::*;
use crate::inventory::{ClickAction, Inventory, SlotRef};
use flate2::read::GzDecoder;
use serde::Deserialize;
use std::{collections::HashMap, sync::OnceLock};
use text_field::TextField;

pub const SEARCH: usize = 5;
pub const PLAYER: usize = 11;
const TABS: &str = "textures/gui/container/creative_inventory/tabs.png";
#[derive(Deserialize)]
struct Entry {
    id: u16,
    meta: u16,
    #[serde(default)]
    nbt: Vec<u8>,
    #[serde(skip)]
    template: OnceLock<Stack>,
    glows: bool,
    category: i8,
    rarity: String,
    tip: Vec<String>,
    advanced: Vec<String>,
    mesh: usize,
}
impl Entry {
    fn stack(&self) -> Stack {
        self.template
            .get_or_init(|| {
                let mut stack = Stack::new(self.id, 1);
                stack.meta = self.meta;
                if !self.nbt.is_empty() {
                    stack.nbt = crate::vanilla::item_nbt::decode(&self.nbt)
                        .expect("original creative NBT")
                        .map(std::sync::Arc::new);
                }
                stack
            })
            .clone()
    }
}
type Tab = (String, usize, Vec<usize>);
type Face = (String, u32, u16, [i32; 16]);
#[derive(Deserialize)]
struct Catalog {
    regions: HashMap<String, [f32; 4]>,
    dyes: [u32; 16],
    tabs: Vec<Tab>,
    entries: Vec<Entry>,
    meshes: Vec<Vec<Face>>,
    #[serde(skip)]
    matches: HashMap<(u16, u16), Vec<usize>>,
    #[cfg(test)]
    clicks: Vec<([usize; 3], Option<[u16; 3]>, Option<[u16; 3]>)>,
}
fn data() -> &'static Catalog {
    static DATA: OnceLock<Catalog> = OnceLock::new();
    DATA.get_or_init(|| {
        let mut c: Catalog = serde_json::from_reader(GzDecoder::new(
            &include_bytes!("creative_catalog.json.gz")[..],
        ))
        .expect("original creative registry fixture");
        for (i, row) in c.entries.iter().enumerate() {
            c.matches.entry((row.id, row.meta)).or_default().push(i);
        }
        c
    })
}
pub(crate) fn dye_colors() -> &'static [u32; 16] {
    &data().dyes
}
fn index_of(s: &Stack) -> Option<usize> {
    let ids = data()
        .matches
        .get(&(s.id, s.meta))
        .or_else(|| (s.id != 373).then(|| data().matches.get(&(s.id, 0))).flatten())?;
    ids.iter()
        .copied()
        .find(|&i| {
            if s.nbt.is_none() {
                data().entries[i].nbt.is_empty()
            } else {
                data().entries[i].stack().same_tags(s)
            }
        })
        .or_else(|| ids.first().copied())
}

#[derive(Clone, Copy)]
struct PotionSpec {
    id: u8,
    name: &'static str,
    bad: bool,
    liquid_color: u32,
    effectiveness: f64,
    instant: bool,
    requirement: Option<&'static str>,
    amplifier: Option<&'static str>,
    attribute: Option<(&'static str, f64, u8)>,
}

impl PotionSpec {
    fn modifier_amount(self, amplifier: i32) -> Option<(&'static str, f64, u8)> {
        let (attr, base, op) = self.attribute?;
        let scale = f64::from(amplifier + 1);
        let amount = match self.id {
            5 => 1.3 * scale,
            18 => f64::from(-0.5_f32 * (amplifier + 1) as f32),
            _ => base * scale,
        };
        Some((attr, amount, op))
    }
}

const POTIONS: [PotionSpec; 23] = [
    PotionSpec { id: 1, name: "potion.moveSpeed", bad: false, liquid_color: 8171462, effectiveness: 1.0, instant: false, requirement: Some("!0 & 1 & !2 & !3 & 1+6"), amplifier: Some("5"), attribute: Some(("generic.movementSpeed", 0.2_f32 as f64, 2)) },
    PotionSpec { id: 2, name: "potion.moveSlowdown", bad: true, liquid_color: 5926017, effectiveness: 0.5, instant: false, requirement: Some("!0 & 1 & !2 & 3 & 3+6"), amplifier: None, attribute: Some(("generic.movementSpeed", -0.15_f32 as f64, 2)) },
    PotionSpec { id: 3, name: "potion.digSpeed", bad: false, liquid_color: 14270531, effectiveness: 1.5, instant: false, requirement: None, amplifier: Some("5"), attribute: None },
    PotionSpec { id: 4, name: "potion.digSlowDown", bad: true, liquid_color: 4866583, effectiveness: 0.5, instant: false, requirement: None, amplifier: None, attribute: None },
    PotionSpec { id: 5, name: "potion.damageBoost", bad: false, liquid_color: 9643043, effectiveness: 1.0, instant: false, requirement: Some("0 & !1 & !2 & 3 & 3+6"), amplifier: Some("5"), attribute: Some(("generic.attackDamage", 2.5, 2)) },
    PotionSpec { id: 6, name: "potion.heal", bad: false, liquid_color: 16262179, effectiveness: 1.0, instant: true, requirement: Some("0 & !1 & 2 & !3"), amplifier: Some("5"), attribute: None },
    PotionSpec { id: 7, name: "potion.harm", bad: true, liquid_color: 4393481, effectiveness: 0.5, instant: true, requirement: Some("!0 & !1 & 2 & 3"), amplifier: Some("5"), attribute: None },
    PotionSpec { id: 8, name: "potion.jump", bad: false, liquid_color: 2293580, effectiveness: 1.0, instant: false, requirement: Some("0 & 1 & !2 & 3 & 3+6"), amplifier: Some("5"), attribute: None },
    PotionSpec { id: 9, name: "potion.confusion", bad: true, liquid_color: 5578058, effectiveness: 0.25, instant: false, requirement: None, amplifier: None, attribute: None },
    PotionSpec { id: 10, name: "potion.regeneration", bad: false, liquid_color: 13458603, effectiveness: 0.25, instant: false, requirement: Some("0 & !1 & !2 & !3 & 0+6"), amplifier: Some("5"), attribute: None },
    PotionSpec { id: 11, name: "potion.resistance", bad: false, liquid_color: 10044730, effectiveness: 1.0, instant: false, requirement: None, amplifier: Some("5"), attribute: None },
    PotionSpec { id: 12, name: "potion.fireResistance", bad: false, liquid_color: 14981690, effectiveness: 1.0, instant: false, requirement: Some("0 & 1 & !2 & !3 & 0+6"), amplifier: None, attribute: None },
    PotionSpec { id: 13, name: "potion.waterBreathing", bad: false, liquid_color: 3035801, effectiveness: 1.0, instant: false, requirement: Some("0 & !1 & 2 & 3 & 2+6"), amplifier: None, attribute: None },
    PotionSpec { id: 14, name: "potion.invisibility", bad: false, liquid_color: 8356754, effectiveness: 1.0, instant: false, requirement: Some("!0 & 1 & 2 & 3 & 2+6"), amplifier: None, attribute: None },
    PotionSpec { id: 15, name: "potion.blindness", bad: true, liquid_color: 2039587, effectiveness: 0.25, instant: false, requirement: None, amplifier: None, attribute: None },
    PotionSpec { id: 16, name: "potion.nightVision", bad: false, liquid_color: 0x1f1fa1, effectiveness: 1.0, instant: false, requirement: Some("!0 & 1 & 2 & !3 & 2+6"), amplifier: None, attribute: None },
    PotionSpec { id: 17, name: "potion.hunger", bad: true, liquid_color: 5797459, effectiveness: 0.5, instant: false, requirement: None, amplifier: None, attribute: None },
    PotionSpec { id: 18, name: "potion.weakness", bad: true, liquid_color: 0x484d48, effectiveness: 0.5, instant: false, requirement: Some("!0 & !1 & !2 & 3 & 3+6"), amplifier: None, attribute: Some(("generic.attackDamage", 2.0, 0)) },
    PotionSpec { id: 19, name: "potion.poison", bad: true, liquid_color: 5149489, effectiveness: 0.25, instant: false, requirement: Some("!0 & !1 & 2 & !3 & 2+6"), amplifier: Some("5"), attribute: None },
    PotionSpec { id: 20, name: "potion.wither", bad: true, liquid_color: 3484199, effectiveness: 0.25, instant: false, requirement: None, amplifier: None, attribute: None },
    PotionSpec { id: 21, name: "potion.healthBoost", bad: false, liquid_color: 16284963, effectiveness: 1.0, instant: false, requirement: None, amplifier: None, attribute: Some(("generic.maxHealth", 4.0, 0)) },
    PotionSpec { id: 22, name: "potion.absorption", bad: false, liquid_color: 0x2552a5, effectiveness: 1.0, instant: false, requirement: None, amplifier: None, attribute: None },
    PotionSpec { id: 23, name: "potion.saturation", bad: false, liquid_color: 16262179, effectiveness: 1.0, instant: true, requirement: None, amplifier: None, attribute: None },
];

const POTION_PREFIXES: [&str; 32] = [
    "potion.prefix.mundane",
    "potion.prefix.uninteresting",
    "potion.prefix.bland",
    "potion.prefix.clear",
    "potion.prefix.milky",
    "potion.prefix.diffuse",
    "potion.prefix.artless",
    "potion.prefix.thin",
    "potion.prefix.awkward",
    "potion.prefix.flat",
    "potion.prefix.bulky",
    "potion.prefix.bungling",
    "potion.prefix.buttered",
    "potion.prefix.smooth",
    "potion.prefix.suave",
    "potion.prefix.debonair",
    "potion.prefix.thick",
    "potion.prefix.elegant",
    "potion.prefix.fancy",
    "potion.prefix.charming",
    "potion.prefix.dashing",
    "potion.prefix.refined",
    "potion.prefix.cordial",
    "potion.prefix.sparkling",
    "potion.prefix.potent",
    "potion.prefix.foul",
    "potion.prefix.odorless",
    "potion.prefix.rank",
    "potion.prefix.harsh",
    "potion.prefix.acrid",
    "potion.prefix.gross",
    "potion.prefix.stinky",
];

fn potion_spec(id: u8) -> Option<PotionSpec> {
    (1..=23).contains(&id).then(|| POTIONS[usize::from(id - 1)])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PotionEffectData {
    pub id: u8,
    pub duration: i32,
    pub amplifier: i32,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CustomPotionEffectData {
    pub id: u8,
    pub duration: i32,
    pub amplifier: i32,
    pub ambient: bool,
    pub show_particles: bool,
}

pub(crate) fn potion_is_instant(id: u8) -> bool {
    potion_spec(id).is_some_and(|s| s.instant)
}

pub(crate) fn potion_effects_for_meta(meta: u16) -> Vec<PotionEffectData> {
    potion_effects_from_meta(meta)
}

pub(crate) fn potion_effects_for_stack(s: &Stack) -> Vec<PotionEffectData> {
    potion_effects(s)
}

#[allow(dead_code)]
pub(crate) fn custom_potion_effects_for_stack(s: &Stack) -> Vec<CustomPotionEffectData> {
    full_potion_effects(s)
}

pub(crate) fn potion_color_for_meta(meta: u16) -> u32 {
    let mut s = Stack::new(373, 1);
    s.meta = meta;
    potion_color_and_glint(&s).0
}

fn parse_potion_effects(expr: &[u8], start: isize, end: isize, meta: i32) -> i32 {
    if start < 0 || end < 0 || start >= end || start as usize >= expr.len() {
        return 0;
    }
    let s = start as usize;
    let e = (end as usize).min(expr.len());
    if let Some(rel) = expr[s..e].iter().position(|&b| b == b'|') {
        let mid = start + rel as isize;
        let left = parse_potion_effects(expr, start, mid - 1, meta);
        if left > 0 {
            return left;
        }
        let right = parse_potion_effects(expr, mid + 1, end, meta);
        return right.max(0);
    }
    if let Some(rel) = expr[s..e].iter().position(|&b| b == b'&') {
        let mid = start + rel as isize;
        let left = parse_potion_effects(expr, start, mid - 1, meta);
        if left <= 0 {
            return 0;
        }
        let right = parse_potion_effects(expr, mid + 1, end, meta);
        if right <= 0 {
            return 0;
        }
        return left.max(right);
    }
    let eval = |invert: bool, has_mul: bool, negate: bool, cmp: i32, bit: i32, factor: i32| -> i32 {
        let mut v = if invert {
            i32::from((meta & (1 << bit)) == 0)
        } else if cmp != -1 {
            let count = (meta as u32).count_ones() as i32;
            i32::from(
                (cmp == 0 && count == bit)
                    || (cmp == 1 && count > bit)
                    || (cmp == 2 && count < bit),
            )
        } else {
            i32::from((meta & (1 << bit)) != 0)
        };
        if has_mul {
            v *= factor;
        }
        if negate {
            v = -v;
        }
        v
    };
    let (mut mul_next, mut has_mul, mut has_num, mut invert, mut negate) =
        (false, false, false, false, false);
    let (mut cmp, mut bit, mut factor, mut total) = (-1, 0, 0, 0);
    for &c in &expr[s..e] {
        match c {
            b'0'..=b'9' => {
                if mul_next {
                    factor = i32::from(c - b'0');
                    has_mul = true;
                } else {
                    bit = bit * 10 + i32::from(c - b'0');
                    has_num = true;
                }
            }
            b'*' => mul_next = true,
            b'!' | b'-' | b'=' | b'<' | b'>' | b'+' => {
                if has_num {
                    total += eval(invert, has_mul, negate, cmp, bit, factor);
                    (mul_next, has_mul, has_num, invert, negate) =
                        (false, false, false, false, false);
                    (cmp, bit, factor) = (-1, 0, 0);
                }
                match c {
                    b'!' => invert = true,
                    b'-' => negate = true,
                    b'=' => cmp = 0,
                    b'<' => cmp = 2,
                    b'>' => cmp = 1,
                    _ => {}
                }
            }
            _ => {}
        }
    }
    if has_num {
        total += eval(invert, has_mul, negate, cmp, bit, factor);
    }
    total
}

fn potion_effects_from_meta(meta: u16) -> Vec<PotionEffectData> {
    let n = i32::from(meta);
    let mut out = Vec::new();
    for spec in POTIONS {
        let Some(req) = spec.requirement else { continue };
        let mut dur = parse_potion_effects(req.as_bytes(), 0, req.len() as isize, n);
        if dur <= 0 {
            continue;
        }
        let mut amp = spec
            .amplifier
            .map_or(0, |a| parse_potion_effects(a.as_bytes(), 0, a.len() as isize, n));
        if amp < 0 {
            amp = 0;
        }
        if spec.instant {
            dur = 1;
        } else {
            dur = 1200 * (dur * 3 + (dur - 1) * 2);
            dur >>= amp;
            dur = (f64::from(dur) * spec.effectiveness + 0.5).floor() as i32;
            if meta & 0x4000 != 0 {
                dur = (f64::from(dur) * 0.75 + 0.5 + 0.5).floor() as i32;
            }
        }
        out.push(PotionEffectData {
            id: spec.id,
            duration: dur,
            amplifier: amp,
        });
    }
    out
}

fn has_custom_potion_effects(s: &Stack) -> bool {
    matches!(
        s.nbt
            .as_deref()
            .and_then(crate::vanilla::item_nbt::compound)
            .and_then(|t| t.get("CustomPotionEffects")),
        Some(crate::vanilla::drops::DropNbt::List { .. })
    )
}

fn full_potion_effects(s: &Stack) -> Vec<CustomPotionEffectData> {
    if let Some(crate::vanilla::drops::DropNbt::List { values, .. }) = s
        .nbt
        .as_deref()
        .and_then(crate::vanilla::item_nbt::compound)
        .and_then(|t| t.get("CustomPotionEffects"))
    {
        let mut out = Vec::new();
        for entry in values {
            let Some(map) = crate::vanilla::item_nbt::compound(entry) else {
                continue;
            };
            let id = crate::vanilla::item_nbt::number(map.get("Id")) as i8;
            if !(1..=23).contains(&id) {
                continue;
            }
            let amplifier = i32::from(crate::vanilla::item_nbt::number(map.get("Amplifier")) as i8);
            let duration = crate::vanilla::item_nbt::number(map.get("Duration")) as i32;
            let ambient = crate::vanilla::item_nbt::number(map.get("Ambient")) != 0;
            let show_particles = map
                .get("ShowParticles")
                .map_or(true, |v| crate::vanilla::item_nbt::number(Some(v)) != 0);
            out.push(CustomPotionEffectData {
                id: id as u8,
                duration,
                amplifier,
                ambient,
                show_particles,
            });
        }
        return out;
    }
    potion_effects_from_meta(s.meta)
        .into_iter()
        .map(|e| CustomPotionEffectData {
            id: e.id,
            duration: e.duration,
            amplifier: e.amplifier,
            ambient: false,
            show_particles: true,
        })
        .collect()
}

fn potion_effects(s: &Stack) -> Vec<PotionEffectData> {
    full_potion_effects(s)
        .into_iter()
        .map(|e| PotionEffectData {
            id: e.id,
            duration: e.duration,
            amplifier: e.amplifier,
        })
        .collect()
}

fn potion_prefix_key(meta: u16) -> &'static str {
    let flag = |bit: u16, weight: usize| if (meta & (1 << bit)) != 0 { weight } else { 0 };
    let idx = flag(5, 16) | flag(4, 8) | flag(3, 4) | flag(2, 2) | flag(1, 1);
    POTION_PREFIXES[idx]
}

fn format_decimal(v: f64) -> String {
    let rounded = (v * 1000.0).round() / 1000.0;
    if rounded == 0.0 {
        return "0".into();
    }
    let s = format!("{rounded:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

pub(crate) fn potion_color_and_glint(s: &Stack) -> (u32, bool) {
    let effects = full_potion_effects(s);
    if let Some(custom_color) = s
        .nbt
        .as_deref()
        .and_then(crate::vanilla::item_nbt::compound)
        .and_then(|t| t.get("CustomPotionColor"))
    {
        let rgb = (crate::vanilla::item_nbt::number(Some(custom_color)) as u32) & 0xffffff;
        return (rgb, !effects.is_empty());
    }
    if effects.is_empty() {
        return (0x385dc6, false);
    }
    let (mut r, mut g, mut b, mut weight) = (0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32);
    for eff in &effects {
        if !eff.show_particles {
            continue;
        }
        let Some(spec) = potion_spec(eff.id) else {
            continue;
        };
        let c = spec.liquid_color;
        for _ in 0..=eff.amplifier.max(0) {
            r += ((c >> 16) & 0xff) as f32 / 255.0;
            g += ((c >> 8) & 0xff) as f32 / 255.0;
            b += (c & 0xff) as f32 / 255.0;
            weight += 1.0;
        }
    }
    if weight <= 0.0 {
        return (0x385dc6, true);
    }
    let ch = |v: f32| ((v / weight * 255.0) as u32) & 0xff;
    ((ch(r) << 16) | (ch(g) << 8) | ch(b), true)
}

fn potion_tooltip_lines(
    stack: &Stack,
    r: &Resources,
    language: &str,
    advanced: bool,
    hide_flags: i32,
) -> Vec<String> {
    let custom = has_custom_potion_effects(stack);
    let effects = potion_effects(stack);
    let title = if stack.meta == 0 && !custom {
        r.tr(language, "item.emptyPotion.name").trim().to_owned()
    } else {
        let prefix = if stack.meta & 0x4000 != 0 {
            format!("{} ", r.tr(language, "potion.prefix.grenade").trim())
        } else {
            String::new()
        };
        if let Some(first) = effects.first().and_then(|e| potion_spec(e.id)) {
            let postfix = format!("{}.postfix", first.name);
            format!("{prefix}{}", r.tr(language, &postfix).trim())
        } else {
            format!(
                "{} {}",
                r.tr(language, potion_prefix_key(stack.meta)).trim(),
                r.tr(language, "item.potion.name").trim()
            )
        }
    };
    let head = if advanced {
        format!("{title}§r (#{:04}/{})", stack.id, stack.meta)
    } else {
        format!("{title}§r")
    };
    let mut lines = vec![head];
    if (hide_flags & 0x20) == 0 && (stack.meta != 0 || custom) {
        let mut modifiers: Vec<(&'static str, f64, u8)> = Vec::new();
        if !effects.is_empty() {
            for eff in &effects {
                let Some(spec) = potion_spec(eff.id) else {
                    continue;
                };
                let mut label = r.tr(language, spec.name).trim().to_owned();
                if let Some(m) = spec.modifier_amount(eff.amplifier) {
                    let pos = modifiers
                        .iter()
                        .rposition(|x| x.0 == m.0)
                        .map_or(modifiers.len(), |i| i + 1);
                    modifiers.insert(pos, m);
                }
                if eff.amplifier > 0 {
                    let pot = r
                        .tr(language, &format!("potion.potency.{}", eff.amplifier))
                        .trim()
                        .to_owned();
                    label.push(' ');
                    label.push_str(&pot);
                }
                if eff.duration > 20 {
                    let secs = eff.duration / 20;
                    label.push_str(&format!(" ({}:{:02})", secs / 60, secs % 60));
                }
                lines.push(format!("{}{label}", if spec.bad { "§c" } else { "§7" }));
            }
        } else {
            lines.push(format!("§7{}", r.tr(language, "potion.empty").trim()));
        }
        if !modifiers.is_empty() {
            lines.push(String::new());
            lines.push(format!("§5{}", r.tr(language, "potion.effects.whenDrank")));
            for (attr_key, amount, op) in modifiers {
                let scaled = if op == 1 || op == 2 {
                    amount * 100.0
                } else {
                    amount
                };
                if amount > 0.0 {
                    let tpl = format!(
                        "⟦attribute.modifier.plus.{op}|{}|⟦attribute.name.{attr_key}⟧⟧",
                        format_decimal(scaled)
                    );
                    lines.push(format!("§9{}", localized(&tpl, r, language)));
                } else if amount < 0.0 {
                    let tpl = format!(
                        "⟦attribute.modifier.take.{op}|{}|⟦attribute.name.{attr_key}⟧⟧",
                        format_decimal(-scaled)
                    );
                    lines.push(format!("§c{}", localized(&tpl, r, language)));
                }
            }
        }
    }
    if advanced {
        lines.push("§8minecraft:potion".into());
    }
    lines
}
fn localized(template: &str, r: &Resources, language: &str) -> String {
    let mut line = template.to_owned();
    while let Some(a) = line.rfind('⟦') {
        let b = a + '⟦'.len_utf8();
        let Some(n) = line[b..].find('⟧') else {
            break;
        };
        let end = b + n;
        let mut parts = line[b..end].split('|');
        let mut value = r.tr(language, parts.next().unwrap());
        let args: Vec<&str> = parts.collect();
        if !args.is_empty() {
            for (i, arg) in args.iter().enumerate() {
                let numbered = format!("%{}$s", i + 1);
                if value.contains(&numbered) {
                    value = value.replace(&numbered, arg)
                } else {
                    value = value.replacen("%s", arg, 1)
                }
            }
            value = value.replace("%%", "%");
        }
        line.replace_range(a..end + '⟧'.len_utf8(), &value);
    }
    line
}
fn plain(s: &str) -> String {
    let mut chars = s.chars();
    let mut plain = String::new();
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
        } else {
            plain.push(c);
        }
    }
    plain
}
#[derive(Default)]
pub(crate) struct State {
    pub tab: usize,
    pub search: TextField,
    pub slim: bool,
    filtered: Vec<usize>,
    scroll: f32,
    motion: std::cell::Cell<Option<(u64, [f64; 2], [f64; 2])>>,
    pub dragging: bool,
    pub clear_search: bool,
}
impl State {
    pub(super) fn indicators(&self, sim: Option<&Simulation>, stats: &Stats<'_>) -> [f64; 2] {
        let Some(sim) = sim else { return [0.; 2] };
        let (previous, mut angle, mut vel) =
            self.motion.get().unwrap_or((u64::MAX, [0.; 2], [0.; 2]));
        if previous == sim.total_ticks {
            return angle;
        }
        let target = if sim.dimension == 0 {
            [
                (-f64::from(stats.yaw) - std::f64::consts::FRAC_PI_2
                    + (f64::from(stats.spawn[2].floor()) - stats.position[2])
                        .atan2(f64::from(stats.spawn[0].floor()) - stats.position[0]))
                    / std::f64::consts::TAU,
                f64::from(crate::vanilla::lightmap::celestial_angle(
                    sim.clock as i64,
                    1.,
                    sim.dimension,
                )),
            ]
        } else {
            let mut rand = crate::vanilla::random::JavaRandom::new(
                sim.total_ticks as i64 ^ sim.player_random_state as i64,
            );
            [rand.next_double(), rand.next_double()]
        };
        for i in 0..2 {
            let d = (target[i] - angle[i] + 0.5).rem_euclid(1.) - 0.5;
            let max = if i == 0 {
                1. / std::f64::consts::TAU
            } else {
                1.
            };
            vel[i] = (vel[i] + d.clamp(-max, max) * 0.1) * 0.8;
            angle[i] += vel[i];
        }
        self.motion.set(Some((sim.total_ticks, angle, vel)));
        angle
    }
    pub fn set_skin(&mut self, name: &str, sim: Option<&Simulation>) {
        self.slim = sim
            .and_then(|s| {
                s.presentation.local_slim().or_else(|| {
                    s.presentation
                        .players
                        .iter()
                        .find(|(_, p)| p.name == name)
                        .map(|(&u, _)| s.presentation.slim_for(u))
                })
            })
            .unwrap_or_else(|| super::creative_preview::offline_slim(name));
    }
    pub fn select(&mut self, tab: usize, r: &Resources, lang: &str, advanced: bool) {
        self.tab = tab;
        self.scroll = 0.;
        self.dragging = false;
        self.search = TextField::default();
        self.clear_search = false;
        self.refresh(r, lang, advanced);
    }
    pub fn refresh(&mut self, r: &Resources, lang: &str, advanced: bool) {
        if self.tab != SEARCH {
            return;
        }
        self.scroll = 0.;
        let q = self.search.value.to_lowercase();
        self.filtered = data().tabs[SEARCH]
            .2
            .iter()
            .copied()
            .filter(|&i| {
                let row = &data().entries[i];
                q.is_empty()
                    || (if advanced { &row.advanced } else { &row.tip })
                        .iter()
                        .any(|s| plain(&localized(s, r, lang)).to_lowercase().contains(&q))
            })
            .collect();
    }
    pub fn list(&self) -> &[usize] {
        if self.tab == SEARCH {
            &self.filtered
        } else {
            &data().tabs[self.tab].2
        }
    }
    pub fn wheel(&mut self, wheel: f32) {
        let length = self.list().len();
        if length > 45 && wheel != 0. {
            self.scroll = (self.scroll - wheel.signum() / (length / 9 - 5) as f32).clamp(0., 1.);
        }
    }
    pub fn drag(&mut self, y: f32, height: f32) {
        if self.dragging {
            self.scroll =
                ((y - ((height.ceil() - 136.) / 2.).floor() - 18. - 7.5) / 97.).clamp(0., 1.)
        }
    }
    fn start(&self) -> usize {
        (self.scroll * self.list().len().div_ceil(9).saturating_sub(5) as f32 + 0.5) as usize * 9
    }
}
#[derive(Clone, Copy, Debug)]
pub enum Hit {
    Tab(usize),
    Cell(Option<usize>),
    Bin,
    Scroll,
    Search,
}
pub(crate) fn catalog_click(
    inventory: &mut Inventory,
    cell: Option<usize>,
    action: ClickAction,
    right: bool,
) -> Option<Stack> {
    let item = cell.map(|i| data().entries[i].stack().copied());
    let copy_full = |mut s: Stack| {
        s.count = crate::inventory::stack_limit(s.id);
        s
    };
    match action {
        ClickAction::Hotbar(h) if h < 9 => {
            if let Some(s) = item {
                inventory.slots[h] = Some(copy_full(s));
            }
        }
        ClickAction::Clone => {
            if inventory.cursor.is_none() {
                inventory.cursor = item.map(copy_full);
            }
        }
        ClickAction::Throw { all } => return if all { item.map(copy_full) } else { item },
        ClickAction::QuickMove | ClickAction::Pickup { .. } => {
            let shift = matches!(action, ClickAction::QuickMove);
            let right = right || matches!(action, ClickAction::Pickup { right: true });
            match (&mut inventory.cursor, item) {
                (Some(c), Some(s)) if c.id == s.id && c.meta == s.meta && c.damage == s.damage => {
                    if right {
                        if c.count <= 1 {
                            inventory.cursor = None
                        } else {
                            c.count -= 1
                        }
                    } else if shift {
                        c.count = crate::inventory::stack_limit(c.id)
                    } else {
                        c.count = (c.count + 1).min(crate::inventory::stack_limit(c.id))
                    }
                }
                (None, Some(s)) => inventory.cursor = Some(if shift { copy_full(s) } else { s }),
                _ => inventory.cursor = None,
            }
        }
        _ => {}
    }
    None
}
pub(crate) fn player_click(
    inventory: &mut Inventory,
    tab: usize,
    slot: SlotRef,
    action: ClickAction,
) -> Vec<Stack> {
    if tab != PLAYER {
        if let (SlotRef::Player(i @ 0..=8), ClickAction::QuickMove) = (slot, &action) {
            inventory.slots[i] = None;
            return vec![];
        }
        if let ClickAction::Collect { reverse } = action {
            if let (SlotRef::Player(i @ 0..=8), Some(c)) = (slot, inventory.cursor.as_mut()) {
                if inventory.slots[i].is_none() {
                    for pass in 0..2 {
                        for j in 0..9 {
                            let j = if reverse { 8 - j } else { j };
                            let Some(s) = &mut inventory.slots[j] else {
                                continue;
                            };
                            let limit = crate::inventory::stack_limit(c.id);
                            if !s.same(c) || (pass == 0 && s.count == limit) {
                                continue;
                            }
                            let n = s.count.min(limit.saturating_sub(c.count));
                            s.count -= n;
                            c.count += n;
                            if s.count == 0 {
                                inventory.slots[j] = None
                            }
                        }
                    }
                }
            }
            return vec![];
        }
    }
    if tab == PLAYER && slot == SlotRef::Outside && matches!(action, ClickAction::Pickup { .. }) {
        return inventory.cursor.take().into_iter().collect();
    }
    crate::container::window_click(inventory, None, slot, action, true, 2)
}
pub(crate) fn discard(inv: &mut Inventory, all: bool) {
    if all {
        inv.slots.fill(None);
        inv.crafting.fill(None);
    } else {
        inv.cursor = None;
    }
}
pub(crate) fn atlas_region(name: &str) -> Option<[f32; 4]> {
    data().regions.get(name).copied()
}

pub(crate) fn world_glint(stack: &Stack) -> bool {
    matches!(stack.id, 384 | 387 | 399 | 403)
        || (stack.id == 322 && stack.meta > 0)
        || (stack.id == 373 && potion_color_and_glint(stack).1)
        || matches!(
            stack
                .nbt
                .as_deref()
                .and_then(crate::vanilla::item_nbt::compound)
                .and_then(|tag| tag.get("ench")),
            Some(crate::vanilla::drops::DropNbt::List { .. })
        )
}

pub(crate) fn has_glint(stack: &Stack) -> bool {
    world_glint(stack)
}

pub(super) fn draw_icon(p: &mut Painter<'_>, s: &Stack, x: f32, y: f32) -> bool {
    if s.id == 166 {
        let sprite = p.r.sprite("textures/items/barrier.png");
        let start = p.frame.vertices.len();
        p.sprite(
            sprite,
            Rect { x, y, w: 16., h: 16. },
            [0., 0., sprite.width as f32, sprite.height as f32],
            WHITE,
        );
        if has_glint(s) {
            let [rx, ry, rw, rh] = data()
                .regions
                .get("items/barrier")
                .copied()
                .unwrap_or([0., 0., 1. / 32., 1. / 32.]);
            let glint = p.r.sprite("textures/misc/enchanted_item_glint.png");
            let corners = [[0., 0.], [0., 1.], [1., 1.], [1., 0.]];
            for (vertex, j) in p.frame.vertices[start..].iter_mut().zip([0, 1, 2, 0, 2, 3]) {
                let u = rx + rw * corners[j][0];
                let v = ry + rh * corners[j][1];
                for (k, angle, phase) in [
                    (0, -50f32, (p.time % 3.) / 3.),
                    (2, 10f32, -(p.time % 4.873) / 4.873),
                ] {
                    let (sin, cos) = angle.to_radians().sin_cos();
                    vertex.glint[k] = 8. * (u * cos - v * sin) + phase;
                    vertex.glint[k + 1] = 8. * (u * sin + v * cos);
                }
                vertex.layer |= glint.layer << 17;
            }
        }
        return true;
    }
    let uncolored_egg = s.id == 383 && index_of(s).is_none();
    let i = index_of(s).or_else(|| match s.id {
        373 => {
            let template_meta = if s.meta & 0x4000 != 0 { 16385 } else { 8193 };
            data().matches.get(&(373, template_meta))?.first().copied()
        }
        31 if s.meta == 0 => data().matches.get(&(32, 0))?.first().copied(),
        383 => data().matches.get(&(383, 50))?.first().copied(),
        _ => None,
    });
    let Some(i) = i else { return false };
    let row = &data().entries[i];
    let mesh = &data().meshes[row.mesh];
    if mesh.is_empty() {
        return false;
    }
    let banner = (s.id == 425).then(|| p.r.banners.stack(p.r, s));
    let skull = (s.id == 397 && s.meta == 3).then(|| {
        crate::player_skins::resolve_skull_skin(
            crate::player_skins::item_skull_owner(s.nbt.as_deref()),
            p.r,
            None,
        )
    });
    let potion = (s.id == 373).then(|| potion_color_and_glint(s));
    let glows = has_glint(s);
    for (texture, rgb, light, c) in mesh {
        let rgb = if uncolored_egg {
            0x00FF_FFFF
        } else {
            match potion {
                Some((custom_rgb, _)) if texture == "items/potion_overlay" => custom_rgb,
                _ => *rgb,
            }
        };
        let (layer, u, v, ox, oy) = if let Some(name) = texture.strip_prefix("blocks/") {
            let Some(layer) = p.a.materials.get(name) else {
                continue;
            };
            (*layer | 65536, 1., 1., 0., 0.)
        } else if texture.starts_with("creative/banner_") {
            let design = banner.expect("banner icon belongs to the banner item");
            (
                design.layer,
                0.25,
                0.25,
                design.origin[0],
                design.origin[1],
            )
        } else if let Some(region) = skull.filter(|_| texture == "entity/steve") {
            (
                region.layer,
                region.scale[0],
                region.scale[1],
                region.origin[0],
                region.origin[1],
            )
        } else if matches!(s.id, 345 | 347) {
            let sprite = p.r.sprite(&format!("textures/{texture}.png#frames"));
            let frame = (p.indicators[usize::from(s.id == 347)].rem_euclid(1.)
                * f64::from(sprite.height)) as u32;
            (
                sprite.layer,
                1. / 16.,
                1. / 16.,
                (frame % 16) as f32 / 16.,
                (frame / 16) as f32 / 16.,
            )
        } else {
            let sprite = p.r.sprite(&format!("textures/{texture}.png"));
            (
                sprite.layer,
                sprite.width as f32 / 256.,
                sprite.height as f32 / 256.,
                0.,
                0.,
            )
        };
        let points =
            std::array::from_fn(|j| [x + c[j * 4] as f32 / 4096., y + c[j * 4 + 1] as f32 / 4096.]);
        let uv = std::array::from_fn(|j| {
            [
                ox + c[j * 4 + 2] as f32 / 4096. * u,
                oy + c[j * 4 + 3] as f32 / 4096. * v,
            ]
        });
        let fac = *light as f32 / (4096. * 255.);
        let [_, r, g, b] = rgb.to_be_bytes();
        let start = p.frame.vertices.len();
        p.polygon(
            points,
            uv,
            layer,
            [r as f32 * fac, g as f32 * fac, b as f32 * fac, 1.],
        );
        if glows {
            let [rx, ry, rw, rh] = data()
                .regions
                .get(texture)
                .copied()
                .unwrap_or([0.0, 0.0, 1.0 / 32.0, 1.0 / 32.0]);
            let glint = p.r.sprite("textures/misc/enchanted_item_glint.png");
            for (vertex, j) in p.frame.vertices[start..].iter_mut().zip([0, 1, 2, 0, 2, 3]) {
                let u = rx + rw * c[j * 4 + 2] as f32 / 4096.;
                let v = ry + rh * c[j * 4 + 3] as f32 / 4096.;
                for (k, angle, phase) in [
                    (0, -50f32, (p.time % 3.) / 3.),
                    (2, 10f32, -(p.time % 4.873) / 4.873),
                ] {
                    let (sin, cos) = angle.to_radians().sin_cos();
                    vertex.glint[k] = 8. * (u * cos - v * sin) + phase;
                    vertex.glint[k + 1] = 8. * (u * sin + v * cos);
                }
                vertex.layer |= glint.layer << 17;
            }
        }
    }
    true
}
fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect { x, y, w, h }
}
fn push(p: &mut Painter<'_>, r: Rect, action: Action) {
    p.frame.buttons.push(Button {
        rect: r,
        action,
        enabled: true,
    });
}
fn draw_tab(p: &mut Painter<'_>, tab: usize, selected: usize, x: f32, y: f32) {
    let col = tab % 6;
    let top = tab < 6;
    let dx = if col == 5 { 167. } else { (col * 29) as f32 };
    let yy = y + if top { -28. } else { 132. };
    let xx = x + dx;
    p.image(
        TABS,
        rect(xx, yy, 28., 32.),
        [
            (col * 28) as f32,
            (if top { 0 } else { 64 } + if tab == selected { 32 } else { 0 }) as f32,
            28.,
            32.,
        ],
        WHITE,
    );
    let s = data().entries[data().tabs[tab].1].stack();
    p.item(s, xx + 6., yy + if top { 9. } else { 7. }, true);
}
pub(super) fn tooltip(p: &mut Painter<'_>, lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    let width = lines.iter().map(|s| p.width(s)).fold(0., f32::max);
    let [mut x, mut y] = p.mouse.map(f32::floor);
    x += 12.;
    y -= 12.;
    let height = if lines.len() == 1 {
        8.
    } else {
        10. * lines.len() as f32
    };
    if x + width > p.frame.size[0].ceil() {
        x -= 28. + width;
    }
    y = y.min(p.frame.size[1].ceil() - height - 6.);
    let fill = [16. / 255., 0., 16. / 255., 240. / 255.];
    let top = [80. / 255., 0., 1., 80. / 255.];
    let bottom = [40. / 255., 0., 127. / 255., 80. / 255.];
    p.rect(x - 3., y - 4., width + 6., height + 8., fill);
    p.rect(x - 4., y - 3., 1., height + 6., fill);
    p.rect(x + width + 3., y - 3., 1., height + 6., fill);
    for xx in [x - 3., x + width + 2.] {
        main_menu::gradient(p, rect(xx, y - 2., 1., height + 4.), top, bottom);
    }
    p.rect(x - 3., y - 3., width + 6., 1., top);
    p.rect(x - 3., y + height + 2., width + 6., 1., bottom);
    for (n, s) in lines.iter().enumerate() {
        p.text(x, y, s, WHITE);
        y += if n == 0 { 12. } else { 10. };
    }
}
pub(super) fn held_item_name(p: &Painter<'_>, stack: &Stack) -> String {
    use crate::vanilla::{drops::DropNbt as Tag, item_nbt};
    if let Some(Tag::String(name)) = stack
        .nbt
        .as_deref()
        .and_then(item_nbt::compound)
        .and_then(|t| t.get("display"))
        .and_then(item_nbt::compound)
        .and_then(|d| d.get("Name"))
    {
        return format!("§o{name}");
    }
    if stack.id == 373 {
        let head = potion_tooltip_lines(stack, p.r, &p.s.language, false, 0x20)
            .into_iter()
            .next()
            .unwrap_or_default();
        return head.strip_suffix("§r").unwrap_or(&head).to_owned();
    }
    p.r.item_name(&p.s.language, stack.id, stack.meta)
}

pub(super) fn item_tip(p: &Painter<'_>, stack: &Stack, search: bool, advanced: bool) -> Vec<String> {
    item_tip_lines(p.r, &p.s.language, stack, search, advanced)
}

fn enchant_translation_key(id: i16) -> Option<&'static str> {
    Some(match id {
        0 => "enchantment.protect.all",
        1 => "enchantment.protect.fire",
        2 => "enchantment.protect.fall",
        3 => "enchantment.protect.explosion",
        4 => "enchantment.protect.projectile",
        5 => "enchantment.oxygen",
        6 => "enchantment.waterWorker",
        7 => "enchantment.thorns",
        8 => "enchantment.waterWalker",
        16 => "enchantment.damage.all",
        17 => "enchantment.damage.undead",
        18 => "enchantment.damage.arthropods",
        19 => "enchantment.knockback",
        20 => "enchantment.fire",
        21 => "enchantment.lootBonus",
        32 => "enchantment.digging",
        33 => "enchantment.untouching",
        34 => "enchantment.durability",
        35 => "enchantment.lootBonusDigger",
        48 => "enchantment.arrowDamage",
        49 => "enchantment.arrowKnockback",
        50 => "enchantment.arrowFire",
        51 => "enchantment.arrowInfinite",
        61 => "enchantment.lootBonusFishing",
        62 => "enchantment.fishingSpeed",
        _ => return None,
    })
}

fn roman_numeral(lvl: i32) -> String {
    if !(1..=3999).contains(&lvl) {
        return lvl.to_string();
    }
    let mut rem = lvl;
    let mut out = String::new();
    for (val, sym) in [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ] {
        while rem >= val {
            out.push_str(sym);
            rem -= val;
        }
    }
    out
}

fn format_enchantment(r: &Resources, language: &str, id: i16, lvl: i32) -> Option<String> {
    let key = enchant_translation_key(id)?;
    let name = r.tr(language, key);
    let level_key = format!("enchantment.level.{lvl}");
    let level_tr = r.tr(language, &level_key);
    let level = if level_tr == level_key {
        roman_numeral(lvl)
    } else {
        level_tr
    };
    Some(format!("{name} {level}"))
}

fn parse_enchant_entry(tag: &crate::vanilla::drops::DropNbt) -> Option<(i16, i32)> {
    let map = crate::vanilla::item_nbt::compound(tag)?;
    let id = crate::vanilla::item_nbt::number(Some(map.get("id")?)) as i16;
    let lvl = crate::vanilla::item_nbt::number(map.get("lvl")) as i32;
    Some((id, lvl))
}

fn add_attack_damage_bonus(tpl: &str, bonus: f64) -> Option<String> {
    const PREFIX: &str = "⟦attribute.modifier.plus.0|";
    const SUFFIX: &str = "|⟦attribute.name.generic.attackDamage⟧⟧";
    let start = tpl.find(PREFIX)?;
    let num_start = start + PREFIX.len();
    let rest = &tpl[num_start..];
    let num_len = rest.find(SUFFIX)?;
    let base: f64 = rest[..num_len].parse().ok()?;
    let mut patched = String::with_capacity(tpl.len() + 8);
    patched.push_str(&tpl[..num_start]);
    patched.push_str(&format_decimal(base + bonus));
    patched.push_str(&rest[num_len..]);
    Some(patched)
}

fn item_tip_lines(
    r: &Resources,
    language: &str,
    stack: &Stack,
    search: bool,
    advanced: bool,
) -> Vec<String> {
    use crate::vanilla::{drops::DropNbt as Tag, item_nbt};
    let tags = stack.nbt.as_deref().and_then(item_nbt::compound);
    let hide_flags = tags
        .and_then(|t| t.get("HideFlags"))
        .map_or(0, |t| item_nbt::number(Some(t)) as i32);
    let ench_list = tags.and_then(|t| match t.get("ench") {
        Some(Tag::List { values, .. }) => Some(values.as_slice()),
        _ => None,
    });
    let enchanted = ench_list.is_some();
    let sharpness_bonus: f64 = ench_list
        .into_iter()
        .flatten()
        .filter_map(parse_enchant_entry)
        .filter(|&(id, lvl)| id == 16 && lvl > 0)
        .map(|(_, lvl)| f64::from(1.25_f32 * lvl as f32))
        .sum();
    let matched = index_of(stack);
    let (rarity, category, mut lines) = if stack.id == 373 {
        (
            if enchanted { "§b" } else { "§f" },
            matched.map_or(-1, |i| data().entries[i].category),
            potion_tooltip_lines(stack, r, language, advanced, hide_flags),
        )
    } else if stack.id == 383 {
        let base = r.item_name(language, stack.id, stack.meta);
        let head = if advanced {
            format!("{base}§r (#{:04}/{})", stack.id, stack.meta)
        } else {
            format!("{base}§r")
        };
        let mut v = vec![head];
        if advanced {
            v.push("§8minecraft:spawn_egg".into());
        }
        (
            if enchanted { "§b" } else { "§f" },
            matched.map_or(-1, |i| data().entries[i].category),
            v,
        )
    } else if stack.id == 403 {
        let stored = tags.and_then(|t| match t.get("StoredEnchantments") {
            Some(Tag::List { values, .. }) => Some(values.as_slice()),
            _ => None,
        });
        let rarity = if stored.is_some_and(|v| !v.is_empty()) {
            "§e"
        } else {
            "§f"
        };
        let base = r.item_name(language, stack.id, stack.meta);
        let head = if advanced {
            if stack.meta == 0 {
                format!("{base}§r (#{:04})", stack.id)
            } else {
                format!("{base}§r (#{:04}/{})", stack.id, stack.meta)
            }
        } else {
            format!("{base}§r")
        };
        let mut v = vec![head];
        if (hide_flags & 32) == 0 {
            if let Some(values) = stored {
                for entry in values {
                    if let Some((id, lvl)) = parse_enchant_entry(entry) {
                        if let Some(line) = format_enchantment(r, language, id, lvl) {
                            v.push(line);
                        }
                    }
                }
            }
        }
        if advanced {
            v.push("§8minecraft:enchanted_book".into());
        }
        (
            rarity,
            matched.map_or(-1, |i| data().entries[i].category),
            v,
        )
    } else if let Some(i) = matched {
        let e = &data().entries[i];
        let rarity = if enchanted && e.rarity == "§f" {
            "§b"
        } else {
            e.rarity.as_str()
        };
        (
            rarity,
            e.category,
            (if advanced { &e.advanced } else { &e.tip })
                .iter()
                .map(|s| {
                    if sharpness_bonus > 0.0 {
                        if let Some(patched) = add_attack_damage_bonus(s, sharpness_bonus) {
                            return localized(&patched, r, language);
                        }
                    }
                    localized(s, r, language)
                })
                .collect(),
        )
    } else {
        let base = r.item_name(language, stack.id, stack.meta);
        let head = if advanced {
            if stack.meta == 0 {
                format!("{base}§r (#{:04})", stack.id)
            } else {
                format!("{base}§r (#{:04}/{})", stack.id, stack.meta)
            }
        } else {
            format!("{base}§r")
        };
        let mut v = vec![head];
        if advanced {
            let def = if stack.id < 256 {
                r.registry.blocks.get(&stack.id)
            } else {
                r.registry.items.get(&stack.id)
            };
            if let Some(d) = def {
                v.push(format!("§8minecraft:{}", d.name));
            }
        }
        (if enchanted { "§b" } else { "§f" }, -1, v)
    };
    if (hide_flags & 1) == 0 {
        if let Some(values) = ench_list {
            let mut ench_lines = Vec::new();
            for entry in values {
                if let Some((id, lvl)) = parse_enchant_entry(entry) {
                    if let Some(line) = format_enchantment(r, language, id, lvl) {
                        let line_plain = plain(&line);
                        if !lines.iter().skip(1).any(|s| plain(s) == line_plain)
                            && !ench_lines.iter().any(|s: &String| plain(s) == line_plain)
                        {
                            ench_lines.push(line);
                        }
                    }
                }
            }
            if !ench_lines.is_empty() {
                let at = lines
                    .iter()
                    .skip(1)
                    .position(|s| s.is_empty() || s.starts_with("§8"))
                    .map_or(lines.len(), |p| p + 1);
                lines.splice(at..at, ench_lines);
            }
        }
    }
    if let Some(display) = tags
        .and_then(|t| t.get("display"))
        .and_then(item_nbt::compound)
    {
        if let Some(Tag::String(name)) = display.get("Name") {
            if let Some((_, rest)) = lines[0].split_once("§r") {
                lines[0] = format!("§o{name}§r{rest}");
            }
        }
        if let Some(Tag::List { values, .. }) = display.get("Lore") {
            let at = lines
                .iter()
                .position(|s| s.is_empty() || s.starts_with("§8"))
                .unwrap_or(lines.len());
            lines.splice(
                at..at,
                values.iter().filter_map(|s| {
                    if let Tag::String(line) = s {
                        Some(format!("§5§o{line}"))
                    } else {
                        None
                    }
                }),
            );
        }
    }
    if advanced {
        if stack.damage > 0 {
            let max = crate::vanilla::armor::maximum_damage(stack.id);
            if max > 0 {
                let at = lines
                    .iter()
                    .position(|s| s.starts_with("§8"))
                    .unwrap_or(lines.len());
                lines.insert(
                    at,
                    format!("Durability: {} / {}", max - i32::from(stack.damage), max),
                );
            }
        }
        if let Some(tags) = tags {
            if !lines.iter().any(|s| s.starts_with("§8NBT:")) {
                lines.push(format!("§8NBT: {} tag(s)", tags.len()));
            }
        }
    }
    if search && category >= 0 {
        lines.insert(
            1,
            format!("§l§9{}", r.tr(language, &data().tabs[category as usize].0)),
        );
    }
    for (i, s) in lines.iter_mut().enumerate() {
        s.insert_str(0, if i == 0 { rarity } else { "§7" });
    }
    lines
}
pub(super) fn cell(
    p: &mut Painter<'_>,
    x: f32,
    y: f32,
    stack: Option<&Stack>,
    action: Action,
    tip: &mut Option<Stack>,
) {
    let hit = rect(x - 1., y - 1., 18., 18.);
    if let Some(s) = stack {
        p.item(s.clone(), x, y, true);
    }
    if hit.contains(p.mouse) {
        p.rect(x, y, 16., 16., [1., 1., 1., 128. / 255.]);
        *tip = stack.cloned();
    }
    push(p, hit, action);
}
pub(super) fn draw(p: &mut Painter<'_>, gui: &Gui, sim: &Simulation) {
    let state = &gui.creative;
    let tab = state.tab;
    let [w, h] = p.frame.size.map(f32::ceil);
    let has_effects = sim.effects.ordered().next().is_some();
    let x = super::inventory_left(w, 195., has_effects);
    let y = ((h - 136.) / 2.).trunc();
    main_menu::gradient(
        p,
        rect(0., 0., w, h),
        [16. / 255., 16. / 255., 16. / 255., 192. / 255.],
        [16. / 255., 16. / 255., 16. / 255., 208. / 255.],
    );
    for i in 0..12 {
        if i != tab {
            draw_tab(p, i, tab, x, y)
        }
    }
    let bg = match tab {
        PLAYER => "inventory",
        SEARCH => "item_search",
        _ => "items",
    };
    p.image(
        &format!("textures/gui/container/creative_inventory/tab_{bg}.png"),
        rect(x, y, 195., 136.),
        [0., 0., 195., 136.],
        WHITE,
    );
    if tab == SEARCH {
        state
            .search
            .draw_plain(p, rect(x + 82., y + 6., 89., 9.), 15, |_| {
                Action::Creative(Hit::Search)
            });
    }
    if tab != PLAYER {
        p.image(
            TABS,
            rect(x + 175., y + 18. + (95. * state.scroll).floor(), 12., 15.),
            [
                if state.list().len() > 45 { 232. } else { 244. },
                0.,
                12.,
                15.,
            ],
            WHITE,
        );
        push(
            p,
            rect(x + 175., y + 18., 14., 112.),
            Action::Creative(Hit::Scroll),
        );
    }
    draw_tab(p, tab, tab, x, y);
    if tab == PLAYER {
        let look = [x + 43. - p.mouse[0], y + 15. - p.mouse[1]];
        super::creative_preview::draw(p, x + 43., y + 45., 20., look, sim, state.slim);
    }
    if tab != PLAYER {
        p.text_pass(
            x + 8.,
            y + 6.,
            &p.tr(&data().tabs[tab].0),
            [64. / 255., 64. / 255., 64. / 255., 1.],
        );
    }
    let mut hovered = None;
    let inv = &sim.inventory;
    if tab == PLAYER {
        for (i, kind) in ["helmet", "chestplate", "leggings", "boots"]
            .iter()
            .enumerate()
        {
            let j = 39 - i;
            let xx = x + 9. + 54. * (i / 2) as f32;
            let yy = y + 6. + 27. * (i % 2) as f32;
            if inv.slots[j].is_none() {
                let icon =
                    p.r.sprite(&format!("textures/items/empty_armor_slot_{kind}.png"));
                p.sprite(
                    icon,
                    rect(xx, yy, 16., 16.),
                    [0., 0., icon.width as f32, icon.height as f32],
                    WHITE,
                );
            }
            cell(
                p,
                xx,
                yy,
                inv.slots[j].as_ref(),
                Action::Slot(j),
                &mut hovered,
            );
        }
        for i in 0..27 {
            cell(
                p,
                x + 9. + 18. * (i % 9) as f32,
                y + 54. + 18. * (i / 9) as f32,
                inv.slots[i + 9].as_ref(),
                Action::Slot(i + 9),
                &mut hovered,
            );
        }
        cell(
            p,
            x + 173.,
            y + 112.,
            None,
            Action::Creative(Hit::Bin),
            &mut hovered,
        );
    } else {
        let list = state.list();
        let start = state.start();
        for j in 0..45 {
            let cell_id = list.get(start + j).copied();
            let stack = cell_id.map(|i| data().entries[i].stack());
            cell(
                p,
                x + 9. + 18. * (j % 9) as f32,
                y + 18. + 18. * (j / 9) as f32,
                stack.as_ref(),
                Action::Creative(Hit::Cell(cell_id)),
                &mut hovered,
            );
        }
    }
    for i in 0..9 {
        cell(
            p,
            x + 9. + 18. * i as f32,
            y + 112.,
            inv.slots[i].as_ref(),
            Action::Slot(i),
            &mut hovered,
        );
    }
    let mut tip = Vec::new();
    if let Some(c) = &inv.cursor {
        p.item(
            c.clone(),
            p.mouse[0].floor() - 8.,
            p.mouse[1].floor() - 8.,
            true,
        );
    } else if let Some(s) = hovered {
        tip = item_tip(p, &s, tab == SEARCH, gui.advanced);
    }
    if has_effects {
        super::creative_preview::effects(p, x - 124., y, sim);
    }
    for i in 0..12 {
        let col = i % 6;
        let xx = x + if col == 5 { 169. } else { (29 * col) as f32 };
        let yy = y + if i < 6 { -32. } else { 136. };
        push(p, rect(xx, yy, 29., 33.), Action::Creative(Hit::Tab(i)));
        if rect(xx + 2., yy + 2., 25., 29.).contains(p.mouse) {
            tip = vec![p.tr(&data().tabs[i].0)];
        }
    }
    if tab == PLAYER && rect(x + 172., y + 111., 18., 18.).contains(p.mouse) {
        tip = vec![p.tr("inventory.binSlot")];
    }
    tooltip(p, &tip);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_creative_catalog_click_results() {
        let d = data();
        for (meta, (id, damage)) in [(1,0), (4,0), (98,0), (98,1), (98,2), (98,3)].into_iter().enumerate() {
            let mut egg = Stack::new(97, 1);
            egg.meta = meta as u16;
            let mut host = Stack::new(id, 1);
            host.meta = damage;
            assert_eq!(d.entries[index_of(&egg).unwrap()].mesh, d.entries[index_of(&host).unwrap()].mesh);
        }
        let mut inv = Inventory::default();
        assert_eq!(
            d.tabs.iter().map(|t| t.2.len()).collect::<Vec<_>>(),
            [158, 126, 37, 12, 59, 678, 32, 33, 46, 73, 48, 0]
        );
        let summarize = |s: &Option<Stack>| s.as_ref().map(|v| [v.id, v.meta, v.count.into()]);
        for &(ref step, cursor, hotbar) in &d.clicks {
            let [slot, button, mode] = *step;
            let action = match mode {
                0 => ClickAction::Pickup { right: button == 1 },
                1 => ClickAction::QuickMove,
                2 => ClickAction::Hotbar(button),
                3 => ClickAction::Clone,
                _ => unreachable!(),
            };
            catalog_click(&mut inv, Some(d.tabs[0].2[slot]), action, button == 1);
            assert_eq!(
                summarize(&inv.cursor),
                cursor,
                "original creative click {step:?}"
            );
            assert_eq!(summarize(&inv.slots[2]), hotbar, "original hotbar {step:?}");
        }
        let book = d.entries.iter().find(|r| r.id == 403).unwrap().stack();
        assert!(book.nbt.is_some());
        assert_eq!(index_of(&book).map(|i| d.entries[i].stack()), Some(book));
    }
    #[test]
    fn creative_tabs_scroll_and_discard_without_touching_other_slots() {
        let mut state = State::default();
        state.wheel(-120.);
        assert_eq!(state.start(), 9);
        for _ in 0..40 {
            state.wheel(-1.);
        }
        assert_eq!(state.start(), 117);
        let mut inv = Inventory::default();
        inv.slots[0] = Some(Stack::new(1, 25));
        inv.slots[9] = Some(Stack::new(2, 5));
        player_click(&mut inv, 0, SlotRef::Player(0), ClickAction::QuickMove);
        assert!(inv.slots[0].is_none());
        assert_eq!(inv.slots[9].as_ref().unwrap().count, 5);
        inv.cursor = Some(Stack::new(1, 3));
        discard(&mut inv, true);
        assert!(inv.slots.iter().all(Option::is_none));
        assert_eq!(inv.cursor.as_ref().unwrap().count, 3);
        discard(&mut inv, false);
        assert!(inv.cursor.is_none());
    }
    #[test]
    fn potion_tooltips_and_colors_match_catalog_and_custom_variants() {
        use crate::vanilla::drops::DropNbt as Tag;
        use std::{collections::BTreeMap, sync::Arc};

        let r = Resources::load().unwrap();
        let d = data();
        let mut checked = 0usize;
        for row in d.entries.iter().filter(|e| e.id == 373) {
            checked += 1;
            let stack = row.stack();
            let overlay_rgb = d.meshes[row.mesh]
                .iter()
                .find(|(tex, _, _, _)| tex == "items/potion_overlay")
                .map(|(_, rgb, _, _)| *rgb)
                .expect("potion_overlay quad");
            assert_eq!(
                potion_color_and_glint(&stack),
                (overlay_rgb, row.glows),
                "potion color/glint mismatch for meta={}",
                row.meta
            );
            for advanced in [false, true] {
                let raw = if advanced { &row.advanced } else { &row.tip };
                let mut expected: Vec<String> =
                    raw.iter().map(|s| localized(s, &r, "en_us")).collect();
                for (i, line) in expected.iter_mut().enumerate() {
                    line.insert_str(0, if i == 0 { row.rarity.as_str() } else { "§7" });
                }
                let actual = item_tip_lines(&r, "en_us", &stack, false, advanced);
                assert_eq!(
                    actual, expected,
                    "potion tooltip mismatch for meta={} advanced={advanced}",
                    row.meta
                );
                for line in &actual {
                    assert!(
                        !line.contains("%%"),
                        "double percent left in tooltip line {line:?} for meta={}",
                        row.meta
                    );
                }
            }
        }
        assert_eq!(checked, 63);

        // Non-catalog combined tier+extended bitmask (0x4062 = 16482: Extended Splash Speed II)
        let mut display = BTreeMap::new();
        display.insert("Name".into(), Tag::String("§cSpeed 2 (II)".into()));
        display.insert(
            "Lore".into(),
            Tag::List {
                element: 8,
                values: vec![Tag::String("§7Click to get this item".into())],
            },
        );
        let mut root = BTreeMap::new();
        root.insert("display".into(), Tag::Compound(display.clone()));

        let mut non_catalog = Stack::new(373, 3);
        non_catalog.meta = 16482;
        non_catalog.nbt = Some(Arc::new(Tag::Compound(root)));
        assert_eq!(potion_color_and_glint(&non_catalog), (0x7cafc6, true));
        assert_eq!(
            item_tip_lines(&r, "en_us", &non_catalog, false, false),
            vec![
                "§f§o§cSpeed 2 (II)§r",
                "§7§7Speed II (3:00)",
                "§7§5§o§7Click to get this item",
                "§7",
                "§7§5When Applied:",
                "§7§9+40% Speed",
            ]
        );

        // CustomPotionEffects NBT on splash base meta=16384 (0x4000) with 6:00 Speed II
        let mut effect = BTreeMap::new();
        effect.insert("Id".into(), Tag::Byte(1));
        effect.insert("Amplifier".into(), Tag::Byte(1));
        effect.insert("Duration".into(), Tag::Int(7200));
        let mut custom_root = BTreeMap::new();
        custom_root.insert("display".into(), Tag::Compound(display));
        custom_root.insert(
            "CustomPotionEffects".into(),
            Tag::List {
                element: 10,
                values: vec![Tag::Compound(effect)],
            },
        );
        let mut custom_potion = Stack::new(373, 3);
        custom_potion.meta = 16384;
        custom_potion.nbt = Some(Arc::new(Tag::Compound(custom_root)));
        assert_eq!(potion_color_and_glint(&custom_potion), (0x7cafc6, true));
        assert_eq!(
            item_tip_lines(&r, "en_us", &custom_potion, false, false),
            vec![
                "§f§o§cSpeed 2 (II)§r",
                "§7§7Speed II (6:00)",
                "§7§5§o§7Click to get this item",
                "§7",
                "§7§5When Applied:",
                "§7§9+40% Speed",
            ]
        );

        for (meta, expected) in [
            (0, "Spawn"),
            (63, "Spawn Ender Dragon"),
            (91, "Spawn Sheep"),
            (99, "Spawn Iron Golem"),
            (100, "Spawn Horse"),
        ] {
            assert_eq!(r.item_name("en_us", 383, meta), expected);
            let mut egg = Stack::new(383, 1);
            egg.meta = meta;
            assert_eq!(
                item_tip_lines(&r, "en_us", &egg, false, false),
                vec![format!("§f{expected}§r")]
            );
            assert_eq!(
                item_tip_lines(&r, "en_us", &egg, false, true),
                vec![
                    format!("§f{expected}§r (#0383/{meta})"),
                    "§7§8minecraft:spawn_egg".to_owned(),
                ]
            );
        }
    }

    #[test]
    fn glint_and_dynamic_enchantment_tooltips_cover_books_empty_ench_and_sharpness() {
        use crate::vanilla::drops::DropNbt as Tag;
        use std::{collections::BTreeMap, sync::Arc};

        let r = Resources::load().unwrap();
        let d = data();

        // Intrinsic glint items: 403 (enchanted_book), 387 (written_book), 384, 399
        for id in [384u16, 387, 399, 403] {
            let stack = Stack::new(id, 1);
            assert!(world_glint(&stack), "expected world_glint for id={id}");
            assert!(has_glint(&stack), "expected has_glint for id={id}");
        }
        let mut apple = Stack::new(322, 1);
        assert!(!world_glint(&apple));
        apple.meta = 1;
        assert!(world_glint(&apple));

        // Unenchanted regular items do not glint; empty {ench:[]} triggers glint
        let mut stick = Stack::new(280, 1);
        assert!(!world_glint(&stick));
        let mut empty_ench_root = BTreeMap::new();
        empty_ench_root.insert(
            "ench".into(),
            Tag::List {
                element: 0,
                values: Vec::new(),
            },
        );
        stick.nbt = Some(Arc::new(Tag::Compound(empty_ench_root.clone())));
        assert!(world_glint(&stick));
        assert!(has_glint(&stick));

        let mut skull = Stack::new(397, 1);
        skull.nbt = Some(Arc::new(Tag::Compound(empty_ench_root)));
        assert!(world_glint(&skull));

        // Every catalog Enchanted Book (403) still matches its single-enchantment tooltip
        let mut book_count = 0usize;
        for row in d.entries.iter().filter(|e| e.id == 403) {
            book_count += 1;
            let stack = row.stack();
            for advanced in [false, true] {
                let raw = if advanced { &row.advanced } else { &row.tip };
                let mut expected: Vec<String> =
                    raw.iter().map(|s| localized(s, &r, "en_us")).collect();
                for (i, line) in expected.iter_mut().enumerate() {
                    line.insert_str(0, if i == 0 { row.rarity.as_str() } else { "§7" });
                }
                assert_eq!(item_tip_lines(&r, "en_us", &stack, false, advanced), expected);
            }
        }
        assert!(book_count > 0);

        // Unenchanted 403 uses white §f rarity and no static Protection I line
        let plain_book = Stack::new(403, 1);
        assert_eq!(
            item_tip_lines(&r, "en_us", &plain_book, false, false),
            vec!["§fEnchanted Book§r"]
        );

        // Multi-enchanted 403 StoredEnchantments (including Roman numeral fallback > 10)
        let ench_tag = |id: i16, lvl: i16| {
            let mut m = BTreeMap::new();
            m.insert("id".into(), Tag::Short(id));
            m.insert("lvl".into(), Tag::Short(lvl));
            Tag::Compound(m)
        };
        let mut multi_book_root = BTreeMap::new();
        multi_book_root.insert(
            "StoredEnchantments".into(),
            Tag::List {
                element: 10,
                values: vec![ench_tag(16, 5), ench_tag(34, 3), ench_tag(32, 12)],
            },
        );
        let mut multi_book = Stack::new(403, 1);
        multi_book.nbt = Some(Arc::new(Tag::Compound(multi_book_root.clone())));
        assert_eq!(
            item_tip_lines(&r, "en_us", &multi_book, false, false),
            vec![
                "§eEnchanted Book§r",
                "§7Sharpness V",
                "§7Unbreaking III",
                "§7Efficiency XII",
            ]
        );
        multi_book_root.insert("HideFlags".into(), Tag::Int(32));
        multi_book.nbt = Some(Arc::new(Tag::Compound(multi_book_root)));
        assert_eq!(
            item_tip_lines(&r, "en_us", &multi_book, false, false),
            vec!["§eEnchanted Book§r"]
        );

        // Enchanted Diamond Sword (276): §b rarity, ench lines, and Sharpness V (+6.25 -> +13.25)
        let mut sword_root = BTreeMap::new();
        sword_root.insert(
            "ench".into(),
            Tag::List {
                element: 10,
                values: vec![ench_tag(16, 5), ench_tag(34, 3)],
            },
        );
        let mut sword = Stack::new(276, 1);
        sword.nbt = Some(Arc::new(Tag::Compound(sword_root)));
        assert!(world_glint(&sword));
        assert_eq!(
            item_tip_lines(&r, "en_us", &sword, false, false),
            vec![
                "§bDiamond Sword§r",
                "§7Sharpness V",
                "§7Unbreaking III",
                "§7",
                "§7§9+13.25 Attack Damage",
            ]
        );
    }
}
