//! CraftingManager's ordered 1.8.9 registry, including mirror, offset, wildcard
//! and item-container behavior. Map expansion needs the active world's map data.
use super::{drops::DropNbt, item_entity, item_nbt};
use crate::inventory::Stack;
use serde::Deserialize;
use std::{collections::BTreeMap, sync::OnceLock};
#[path = "crafting_dynamic.rs"]
mod dynamic;
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    fn same(actual: &Option<Stack>, expected: &Option<Spec>, case: usize, slot: usize) {
        assert_eq!(
            actual.is_some(),
            expected.is_some(),
            "craft #{case} slot={slot}"
        );
        if let (Some(a), Some(e)) = (actual, expected) {
            assert_eq!(
                [a.id, u16::from(a.count), metadata(a)],
                [e.0, u16::from(e.1), e.2],
                "craft #{case} slot={slot}"
            );
            let tag_expected = (!e.3.is_empty()).then(|| tag(&e.3));
            assert_eq!(
                a.nbt.as_deref(),
                tag_expected.as_ref(),
                "craft NBT #{case} slot={slot}"
            );
        }
    }
    #[test]
    fn all_original_recipe_classes_outputs_and_container_items_match_crafting_manager() {
        type Case = (
            usize,
            Vec<Option<Spec>>,
            Option<Spec>,
            Option<Vec<Option<Spec>>>,
        );
        let mut bytes = Vec::new();
        flate2::read::GzDecoder::new(
            &include_bytes!("../../tests/fixtures/vanilla189-crafting.json.gz")[..],
        )
        .read_to_end(&mut bytes)
        .unwrap();
        let cases: Vec<Case> = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(registry().recipes.len(), 373);
        for (n, (width, input, output, remainder)) in cases.iter().enumerate() {
            let grid = input
                .iter()
                .map(|s| s.as_ref().map(Spec::stack))
                .collect::<Vec<_>>();
            let found = recipe_with_maps(&grid, *width, |id| Some((id % 5) as i8));
            same(
                &found.as_ref().map(|r| r.output.clone()),
                output,
                n,
                usize::MAX,
            );
            if let Some(r) = found {
                for (i, (a, b)) in r
                    .remaining
                    .iter()
                    .zip(remainder.as_ref().unwrap())
                    .enumerate()
                {
                    same(a, b, n, i);
                }
            }
        }
    }
}
#[derive(Deserialize)]
struct Spec(u16, u8, u16, String);
impl Spec {
    fn stack(&self) -> Stack {
        let mut s =
            item_entity::stack_from_metadata(self.0, self.1, self.2).expect("Original recipe item");
        if !self.3.is_empty() {
            s = s.with_nbt(tag(&self.3));
        }
        s
    }
    fn matches(&self, s: &Stack) -> bool {
        self.0 == s.id && (self.2 == 32767 || self.2 == metadata(s))
    }
}
fn metadata(s: &Stack) -> u16 {
    if item_entity::facts(s.id).is_some_and(|p| p.max_damage > 0) {
        s.damage
    } else {
        s.meta
    }
}
fn tag(hex: &str) -> DropNbt {
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("Original recipe hex"))
        .collect::<Vec<_>>();
    item_nbt::decode(&bytes)
        .expect("Original recipe NBT")
        .expect("Original recipe tag")
}
#[derive(Deserialize)]
struct Rule(String, usize, Option<Vec<Option<Spec>>>, Option<Spec>, bool);
#[derive(Deserialize)]
struct Registry {
    recipes: Vec<Rule>,
    container_items: Vec<[u16; 2]>,
    patterns: Vec<(String, [Option<String>; 3], Option<Spec>)>,
    sheep_rgb: [[f32; 3]; 16],
    firework_rgb: [i32; 16],
}
fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        serde_json::from_str(include_str!("crafting_registry.json"))
            .expect("Original crafting recipe registry")
    })
}
pub struct Recipe {
    pub output: Stack,
    pub remaining: Vec<Option<Stack>>,
}
pub fn consume(
    inventory: &mut crate::inventory::Inventory,
    recipe: Recipe,
    creative: bool,
) -> Vec<Stack> {
    let mut drops = Vec::new();
    for (i, mut replacement) in recipe.remaining.into_iter().enumerate() {
        let slot = &mut inventory.crafting[i];
        if let Some(s) = slot {
            if s.count > 1 {
                s.count -= 1;
                if let Some(alias) = replacement.as_mut().filter(|r| r.identity == s.identity) {
                    alias.count = s.count;
                }
            } else {
                *slot = None;
            }
        }
        if let Some(mut s) = replacement {
            if slot.is_none() {
                *slot = Some(s);
            } else if !item_entity::add_to_player(inventory, &mut s, creative)
                .expect("Original recipe container item")
                && s.count > 0
            {
                drops.push(s);
            }
        }
    }
    drops
}
pub fn recipe(grid: &[Option<Stack>], width: usize) -> Option<Recipe> {
    recipe_with_maps(grid, width, |_| None)
}
pub fn recipe_with_maps(
    grid: &[Option<Stack>],
    width: usize,
    map_scale: impl Fn(u16) -> Option<i8>,
) -> Option<Recipe> {
    if !(2..=3).contains(&width) {
        return None;
    }
    let grid = grid.get(..width * width)?;
    let used = grid
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.as_ref().map(|_| i))
        .collect::<Vec<_>>();
    if used.is_empty() {
        return None;
    }
    for rule in &registry().recipes {
        let output = if let Some(ingredients) = &rule.2 {
            let matches = if rule.1 == 0 {
                shapeless(grid, ingredients, &used)
            } else {
                shaped(grid, width, rule.1, ingredients)
            };
            if !matches {
                continue;
            }
            let mut result = rule.3.as_ref()?.stack();
            if rule.4 {
                for s in grid.iter().flatten() {
                    if let Some(nbt) = &s.nbt {
                        result.nbt = Some(nbt.clone());
                    }
                }
            }
            result
        } else {
            let Some(s) = dynamic::output(&rule.0, grid, width, &map_scale) else {
                continue;
            };
            s
        };
        let remaining = grid
            .iter()
            .map(|slot| {
                slot.as_ref().and_then(|s| {
                    if rule.0 == "RecipeBookCloning" && s.id == 387 {
                        return Some(s.clone());
                    }
                    if rule.0 == "RecipesBanners$RecipeDuplicatePattern"
                        && s.id == 425
                        && dynamic::patterns(s) > 0
                    {
                        let mut copy = s.copied();
                        copy.count = 1;
                        return Some(copy);
                    }
                    registry()
                        .container_items
                        .iter()
                        .find_map(|[id, remainder]| {
                            (*id == s.id).then(|| Stack::new(*remainder, 1))
                        })
                })
            })
            .collect();
        return Some(Recipe { output, remaining });
    }
    None
}
fn shapeless(grid: &[Option<Stack>], ingredients: &[Option<Spec>], used: &[usize]) -> bool {
    if used.len() != ingredients.len() {
        return false;
    }
    let mut taken = 0u16;
    for &i in used {
        let item = grid[i].as_ref().unwrap();
        let Some(index) = ingredients.iter().enumerate().find_map(|(n, v)| {
            (taken & (1 << n) == 0 && v.as_ref().is_some_and(|s| s.matches(item))).then_some(n)
        }) else {
            return false;
        };
        taken |= 1 << index;
    }
    true
}
fn shaped(grid: &[Option<Stack>], width: usize, w: usize, ingredients: &[Option<Spec>]) -> bool {
    let h = ingredients.len() / w;
    if w > 3 || h > 3 {
        return false;
    }
    for x0 in 0..=3 - w {
        for y0 in 0..=3 - h {
            for mirror in [true, false] {
                if (0..9).all(|i| {
                    let x = i % 3;
                    let y = i / 3;
                    let ingredient = if x >= x0 && x < x0 + w && y >= y0 && y < y0 + h {
                        let dx = x - x0;
                        ingredients[(y - y0) * w + if mirror { w - 1 - dx } else { dx }].as_ref()
                    } else {
                        None
                    };
                    let item = (x < width && y < width)
                        .then(|| grid[y * width + x].as_ref())
                        .flatten();
                    match (ingredient, item) {
                        (None, None) => true,
                        (Some(spec), Some(s)) => spec.matches(s),
                        _ => false,
                    }
                }) {
                    return true;
                }
            }
        }
    }
    false
}
type Compound = BTreeMap<String, DropNbt>;
fn root(s: &Stack) -> Option<&Compound> {
    s.nbt.as_deref().and_then(item_nbt::compound)
}
fn named<'a>(c: Option<&'a Compound>, name: &str) -> Option<&'a Compound> {
    c?.get(name).and_then(item_nbt::compound)
}
fn nested<'a>(c: &'a mut Compound, key: &str) -> &'a mut Compound {
    let n = c
        .entry(key.to_owned())
        .or_insert_with(|| DropNbt::Compound(Compound::new()));
    if !matches!(n, DropNbt::Compound(_)) {
        *n = DropNbt::Compound(Compound::new());
    }
    match n {
        DropNbt::Compound(c) => c,
        _ => unreachable!(),
    }
}
