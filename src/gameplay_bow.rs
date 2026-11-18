//! ItemBow's right-click gate and release action in the integrated world.
use super::{Mode, Outcome, Simulation, Stack};
use crate::vanilla::random::JavaRandom;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub(crate) struct BowShot {
    pub speed: f32,
    pub damage: f64,
    pub punch: i32,
    pub fire_ticks: i32,
    pub critical: bool,
    pub pickup: u8,
}

impl Simulation {
    pub(super) fn start_bow(&mut self, bow: &Stack) -> bool {
        if bow.id != 261 || (self.mode != Mode::Creative && self.arrow_slot().is_none()) {
            return false;
        }
        self.item_use.start_for(bow.identity, 72000);
        true
    }

    fn arrow_slot(&self) -> Option<usize> {
        self.inventory.slots.iter().take(36).position(|slot| {
            slot.as_ref()
                .is_some_and(|stack| stack.id == 262 && stack.count > 0)
        })
    }

    /// Call before the item-use controller clears the held item on button release.
    pub(super) fn release_bow(&mut self, player_pos: glam::DVec3, out: &mut Outcome) -> Option<BowShot> {
        let bow = self.inventory.held()?;
        if bow.id != 261 || self.item_use.identity() != Some(bow.identity) {
            return None;
        }
        let used = 72000 - self.item_use.remaining();
        self.item_use.stop();
        let infinite = self.mode == Mode::Creative || bow.enchantment_level(51) > 0;
        let arrow = self.arrow_slot();
        if !infinite && arrow.is_none() {
            return None;
        }
        let charge = used as f32 / 20.;
        let power = ((charge * charge + charge * 2.) / 3.).min(1.);
        if f64::from(power) < 0.1 {
            return None;
        }
        let damage = if bow.enchantment_level(48) > 0 {
            2. + f64::from(bow.enchantment_level(48)) * 0.5 + 0.5
        } else {
            2.
        };
        let shot = BowShot {
            speed: (power * 2.) * 1.5,
            damage,
            punch: bow.enchantment_level(49).max(0),
            fire_ticks: if bow.enchantment_level(50) > 0 {
                2000
            } else {
                0
            },
            critical: power == 1.,
            pickup: if infinite { 2 } else { 1 },
        };
        self.wear_slot(self.inventory.selected, 1, false);
        let mut random = JavaRandom::new(super::random(&mut self.rng) as i64);
        let pitch = 1. / (random.next_float() * 0.4 + 1.2) + power * 0.5;
        out.world_sounds.push(("random.bow", player_pos, 1., pitch));
        if !infinite {
            let slot = &mut self.inventory.slots[arrow.unwrap()];
            let stack = slot.as_mut().unwrap();
            stack.count -= 1;
            if stack.count == 0 {
                *slot = None;
            }
        }
        let stat = self
            .statistics
            .entry("stat.useItem.minecraft.bow".into())
            .or_default();
        *stat = stat.wrapping_add(1);
        Some(shot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vanilla::drops::DropNbt as Tag;

    #[derive(Deserialize, Debug)]
    struct Case {
        creative: bool,
        ammo: u8,
        charge: i32,
        enchants: [i16; 4],
        initial_damage: u16,
        remove_ammo: bool,
        started: bool,
        fired: bool,
        remaining_ammo: u8,
        bow_count: u8,
        bow_damage: u16,
        used_stat: i32,
        sounds: usize,
        fov: f32,
        damage: Option<f64>,
        critical: Option<bool>,
        punch: Option<i32>,
        fire: Option<i32>,
        pickup: Option<u8>,
    }

    fn enchanted_bow(levels: [i16; 4]) -> Stack {
        let enchants = [48, 49, 50, 51]
            .into_iter()
            .zip(levels)
            .filter(|(_, level)| *level != 0)
            .map(|(id, level)| {
                Tag::Compound(
                    [
                        ("id".into(), Tag::Short(id)),
                        ("lvl".into(), Tag::Short(level)),
                    ]
                    .into(),
                )
            })
            .collect();
        Stack::new(261, 1).with_nbt(Tag::Compound(
            [(
                "ench".into(),
                Tag::List {
                    element: 10,
                    values: enchants,
                },
            )]
            .into(),
        ))
    }

    #[test]
    fn installed_itembow_start_charge_release_ammo_enchantments_and_breakage_match() {
        let cases: Vec<Case> = serde_json::from_reader(flate2::read::GzDecoder::new(
            &include_bytes!("../tests/fixtures/vanilla189-bow-use.json.gz")[..],
        ))
        .unwrap();
        assert_eq!(cases.len(), 166);
        for case in cases {
            let mode = if case.creative {
                Mode::Creative
            } else {
                Mode::Survival
            };
            let mut sim = Simulation::new(mode, 189);
            let mut bow = enchanted_bow(case.enchants);
            bow.damage = case.initial_damage;
            sim.inventory.slots[0] = Some(bow.clone());
            sim.inventory.slots[1] = (case.ammo > 0).then(|| Stack::new(262, case.ammo));
            assert_eq!(sim.start_bow(&bow), case.started, "{case:?}");
            for _ in 0..case.charge {
                sim.item_use.tick(Some(bow.identity), false);
            }
            sim.equipped.tick(
                Some(bow.clone()),
                case.started.then_some(case.charge as u32),
            );
            assert_eq!(sim.equipped.bow_fov_multiplier(0.), 1.);
            let expected_zoom = 1. + (case.fov - 1.) * 0.5;
            assert!(
                (sim.equipped.bow_fov_multiplier(1.) - expected_zoom).abs() < 0.000001,
                "{case:?}"
            );
            if case.remove_ammo {
                sim.inventory.slots[1] = None;
            }
            let mut out = Outcome::default();
            let shot = sim.release_bow(glam::DVec3::ZERO, &mut out);
            sim.equipped.tick(sim.inventory.held(), None);
            let recovered_zoom = expected_zoom + (1. - expected_zoom) * 0.5;
            assert!(
                (sim.equipped.bow_fov_multiplier(1.) - recovered_zoom).abs() < 0.000001,
                "{case:?}"
            );
            assert_eq!(shot.is_some(), case.fired, "{case:?}");
            assert_eq!(
                sim.inventory.slots[1].as_ref().map_or(0, |s| s.count),
                case.remaining_ammo,
                "{case:?}"
            );
            assert_eq!(
                sim.inventory.held().as_ref().map_or(0, |s| s.count),
                case.bow_count,
                "{case:?}"
            );
            assert_eq!(
                sim.inventory.held().as_ref().map_or(0, |s| s.damage),
                case.bow_damage,
                "{case:?}"
            );
            assert_eq!(
                sim.statistics
                    .get("stat.useItem.minecraft.bow")
                    .copied()
                    .unwrap_or(0),
                case.used_stat,
                "{case:?}"
            );
            assert_eq!(out.world_sounds.len(), case.sounds, "{case:?}");
            if let Some(shot) = shot {
                assert_eq!(Some(shot.damage), case.damage, "{case:?}");
                assert_eq!(Some(shot.critical), case.critical, "{case:?}");
                assert_eq!(Some(shot.punch), case.punch, "{case:?}");
                assert_eq!(Some(shot.fire_ticks), case.fire, "{case:?}");
                assert_eq!(Some(shot.pickup), case.pickup, "{case:?}");
                assert!(shot.speed >= 0.3 && shot.speed <= 3.);
            }
        }
    }

    #[test]
    fn replacing_the_drawn_bow_cancels_and_armor_slots_supply_no_arrows() {
        let mut sim = Simulation::new(Mode::Survival, 189);
        let bow = Stack::new(261, 1);
        sim.inventory.slots[0] = Some(bow.clone());
        sim.inventory.slots[39] = Some(Stack::new(262, 1));
        assert!(!sim.start_bow(&bow));
        sim.inventory.slots[12] = sim.inventory.slots[39].take();
        assert!(sim.start_bow(&bow));
        for _ in 0..20 {
            sim.item_use.tick(Some(bow.identity), false);
        }
        sim.inventory.slots[0] = Some(Stack::new(261, 1));
        let mut out = Outcome::default();
        assert!(sim.release_bow(glam::DVec3::ZERO, &mut out).is_none());
        sim.tick_item_use(false, &mut out);
        assert!(!sim.item_use.active());
        assert_eq!(sim.inventory.slots[12].as_ref().unwrap().count, 1);
        assert!(out.world_sounds.is_empty());
    }

    #[test]
    #[ignore = "requires installed item models and game resources"]
    fn holding_and_releasing_right_click_draws_then_fires_one_arrow() {
        let mut world = super::super::tests::flat_world();
        let mut sim = Simulation::new(Mode::Survival, 189);
        sim.mob_spawning = false;
        let mut player = crate::player::Player::new(glam::Vec3::new(8.5, 1., 5.5));
        player.set_rotation_degrees(0., 0.);
        let resources = crate::validation::resources();
        let settings = crate::settings::Settings::default();
        sim.inventory.slots[0] = Some(Stack::new(261, 1));
        sim.inventory.slots[1] = Some(Stack::new(262, 2));
        for _ in 0..20 {
            sim.tick(&mut world, &mut player, &resources, &settings, false, true);
            assert!(sim.projectiles.is_empty());
        }
        assert!(player.using_item);
        assert_eq!(sim.equipped.use_ticks, Some(20));
        assert!(sim.equipped.bow_fov_multiplier(1.) < 0.9);
        sim.tick(&mut world, &mut player, &resources, &settings, false, false);
        assert!(!player.using_item);
        assert_eq!(sim.projectiles.len(), 1);
        let super::super::projectiles::Payload::Arrow(shot) = sim.projectiles[0].payload else {
            panic!("bow must create an arrow");
        };
        assert_eq!(shot.speed, 3.);
        assert!(shot.critical);
        assert_eq!(sim.inventory.slots[1].as_ref().unwrap().count, 1);
        assert_eq!(sim.inventory.held().unwrap().damage, 1);
        sim.tick(&mut world, &mut player, &resources, &settings, false, false);
        assert_eq!(sim.projectiles.len(), 1);
        assert_eq!(sim.statistics["stat.useItem.minecraft.bow"], 1);
    }
}
