//! Living entity fire counters are measured in server ticks. Lava and burning
//! blocks cause contact damage separately from the periodic onFire damage.
use super::{command_entity_state::MobData, *};
use crate::vanilla::{atmosphere, item_nbt, lightmap, movement};
use glam::DVec3;

impl Simulation {
    pub(super) fn extinguish_clicked_fire(&mut self, world: &mut World, p: IVec3, face: u8, out: &mut Outcome) {
        // ItemInWorldManager.onBlockClicked handles this on C07 START in both
        // game modes. Fire itself is unpickable: the hit is its support face.
        let Some(side) = [IVec3::NEG_Y, IVec3::Y, IVec3::NEG_Z, IVec3::Z, IVec3::NEG_X, IVec3::X]
            .get(usize::from(face)).copied() else { return; };
        self.extinguish_fire_at(world, p + side, out);
    }

    pub(super) fn extinguish_fire_at(&mut self, world: &mut World, p: IVec3, out: &mut Outcome) {
        if world.get(p) >> 4 == 51 && world.set(p, 0) {
            out.changed.push(p);
            let mut random = JavaRandom::from_state(self.particle_random[2]);
            out.world_sounds.push((
                "random.fizz",
                p.as_dvec3() + DVec3::splat(0.5),
                0.5,
                2.6 + (random.next_float() - random.next_float()) * 0.8,
            ));
            self.particle_random[2] = random.state();
        }
    }
}

pub(super) fn set_fire(ticks: &mut i32, seconds: i32, protection: i32) {
    let mut duration = seconds * 20;
    if protection > 0 {
        duration -= (duration as f32 * (protection as f32 * 0.15)).floor() as i32;
    }
    *ticks = (*ticks).max(duration);
}

pub(super) fn mob_protection(data: Option<&MobData>) -> i32 {
    data.into_iter()
        .flat_map(|d| d.equipment.values())
        .flatten()
        .map(|s| s.enchantment_level(1))
        .max()
        .unwrap_or(0)
}

pub(super) fn fire_immune(kind: MobKind, data: Option<&MobData>) -> bool {
    matches!(kind, MobKind::Blaze | MobKind::Ghast | MobKind::MagmaCube | MobKind::PigZombie | MobKind::Wither | MobKind::EnderDragon) || kind == MobKind::Skeleton
        && item_nbt::number(data.and_then(|d| d.extra.get("SkeletonType"))) == 1
}

#[derive(Clone, Copy)]
pub(super) struct Climate {
    sky_loss: u8,
    overworld: bool,
    raining: bool,
}

impl Climate {
    pub(super) fn new(sim: &Simulation) -> Self {
        let (rain, thunder) = sim.weather.strengths(1.);
        Self {
            sky_loss: atmosphere::skylight_subtracted(sim.clock as i64, rain, rain * thunder),
            overworld: sim.dimension == 0,
            raining: sim.dimension == 0 && rain > 0.2,
        }
    }

    fn rain_reaches(self, world: &World, position: Vec3, height: f32) -> bool {
        self.raining
            && (world.rain_exposed(position.floor().as_ivec3())
                || world.rain_exposed((position + Vec3::Y * height).floor().as_ivec3()))
    }
}

#[derive(Default, Clone, Copy)]
struct Contact {
    water: bool,
    lava: bool,
    fire: bool,
    cactus: bool,
}

impl Contact {
    fn stationary(world: &World, mob: &Mob, scale: f32) -> Self {
        let (w, h) = mob.kind.size();
        let feet = mob.position.as_dvec3();
        let r = f64::from(w * scale * 0.5);
        let bounds = movement::Aabb {
            min: feet - DVec3::new(r, 0., r),
            max: feet + DVec3::new(r, f64::from(h * scale), r),
        };
        let inside = bounds.expand(DVec3::splat(-0.001));
        Self {
            water: movement::water_contact(world, bounds).0,
            lava: movement::touches_liquid(
                world,
                bounds.expand(DVec3::new(-0.1f32 as f64, -0.4f32 as f64, -0.1f32 as f64)),
                10,
                11,
            ),
            fire: movement::touches_liquid(world, inside, 10, 11)
                || movement::touches_liquid(world, inside, 51, 51),
            cactus: movement::touches_liquid(world, inside, 81, 81),
        }
    }
}

#[derive(Clone, Copy)]
enum Damage {
    OnFire,
    Lava,
    InFire,
    Cactus,
}
impl Damage {
    fn source(self) -> HitSource {
        HitSource {
            fire: !matches!(self, Self::Cactus),
            unblockable: matches!(self, Self::OnFire),
            ..Default::default()
        }
    }
    fn amount(self) -> f32 {
        if matches!(self, Self::Lava) {
            4.
        } else {
            1.
        }
    }
    fn death(self) -> &'static str {
        match self {
            Self::Lava => "death.attack.lava",
            Self::InFire => "death.attack.inFire",
            Self::OnFire => "death.attack.onFire",
            Self::Cactus => "death.attack.cactus",
        }
    }
}

struct FireTick {
    tracked: bool,
    ongoing: bool,
    lava: bool,
    contact: bool,
    fizz: bool,
}
impl FireTick {
    fn begin(ticks: &mut i32, env: Contact, wet: bool, protection: i32) -> Self {
        if env.water {
            *ticks = 0;
        }
        let ongoing = *ticks > 0 && *ticks % 20 == 0;
        if *ticks > 0 {
            *ticks -= 1;
        }
        if env.lava {
            set_fire(ticks, 15, protection);
        }
        // Entity.onEntityUpdate writes watcher 0 before living wet/creative
        // extinguishing and before the subsequent moveEntity contact callback.
        let tracked = *ticks > 0;
        if wet {
            *ticks = 0;
        }
        Self {
            tracked,
            ongoing,
            lava: env.lava,
            contact: env.fire,
            fizz: false,
        }
    }

    fn finish(&mut self, ticks: &mut i32, wet: bool, protection: i32, fire_resistance: i32) {
        if self.contact {
            if !wet {
                *ticks += 1;
                if *ticks == 0 {
                    set_fire(ticks, 8, protection);
                }
            }
        } else if *ticks <= 0 {
            *ticks = -fire_resistance;
        }
        if wet && *ticks > 0 {
            *ticks = -fire_resistance;
            self.fizz = true;
        }
    }

    fn damage(&self, cactus: bool) -> impl Iterator<Item = Damage> {
        [
            (self.ongoing, Damage::OnFire),
            (self.lava, Damage::Lava),
            (self.contact, Damage::InFire),
            (cactus, Damage::Cactus),
        ]
        .into_iter()
        .filter_map(|(enabled, cause)| enabled.then_some(cause))
    }
}

impl Simulation {
    pub(crate) fn player_burning(&self) -> bool {
        self.fire_flag
    }

    pub(super) fn tick_player_fire(&mut self, world: &World, player: &Player) {
        let env = Contact {
            water: player.in_water,
            lava: player.in_lava(),
            fire: player.touching_fire(),
            cactus: false, // player landing/contact is dispatched by tick_local
        };
        let wet = env.water
            || Climate::new(self).rain_reaches(
                world,
                player.position,
                if player.sleeping { 0.2 } else { 1.8 },
            );
        let protection = self.inventory.armor_enchantment(1);
        let mut tick = FireTick::begin(&mut self.fire_ticks, env, wet, protection);
        self.fire_flag = tick.tracked;
        tick.finish(&mut self.fire_ticks, wet, protection, 20);
        if self.mode == Mode::Creative && self.fire_ticks > 0 {
            self.fire_ticks = 0;
        }
        for cause in tick.damage(false) {
            if self.hurt_from(cause.amount(), cause.source()) && self.health <= 0. {
                self.death_message_key = Some(cause.death());
            }
        }
    }
}

fn sunlight(
    world: &World,
    climate: Climate,
    mob: &Mob,
    brain: &mut MobBrain,
    data: Option<&mut MobData>,
    protection: i32,
) {
    if !climate.overworld
        || climate.sky_loss >= 4
        || !matches!(mob.kind, MobKind::Zombie | MobKind::Skeleton)
    {
        return;
    }
    if mob.kind == MobKind::Zombie
        && (brain.growing_age < 0
            || item_nbt::number(data.as_ref().and_then(|d| d.extra.get("IsBaby"))) != 0)
    {
        return;
    }
    let position = mob.previous;
    let eyes = (position + Vec3::Y * 1.74).floor().as_ivec3();
    let light = world.game_light(eyes, climate.sky_loss, true, true);
    let brightness = lightmap::brightness_table(0)[usize::from(light)];
    if brightness <= 0.5 {
        return;
    }
    let state = brain
        .entity_random
        .get_or_insert_with(|| JavaRandom::new(JavaRandom::independent_seed()).state());
    let mut random = JavaRandom::from_state(*state);
    let feet = (position + Vec3::Y * 0.5).floor().as_ivec3();
    if random.next_float() * 30. < (brightness - 0.4) * 2. && world.can_see_sky(feet) {
        if let Some(slot) = data
            .and_then(|d| d.equipment.get_mut(&103))
            .filter(|s| s.is_some())
        {
            let helmet = slot.as_mut().unwrap();
            let maximum = armor::maximum_damage(helmet.id);
            if maximum > 0 && !helmet.unbreakable() {
                helmet.damage = helmet.damage.saturating_add(random.next_int(2) as u16);
                if i32::from(helmet.damage) >= maximum {
                    *slot = None;
                }
            }
        } else {
            set_fire(&mut brain.fire_ticks, 8, protection);
        }
    }
    *state = random.state();
}

pub(super) fn tick_mob(
    world: &World,
    climate: Climate,
    mob: &mut Mob,
    brain: &mut MobBrain,
    mut data: Option<&mut MobData>,
    mut merchant_random: Option<&mut u64>,
    enchantment_state: &mut u64,
    out: &mut Outcome,
) {
    let scale = if brain.growing_age < 0 { 0.5 } else { 1. };
    let active_body = brain
        .movement
        .as_ref()
        .filter(|_| !data.as_ref().is_some_and(|d| d.no_ai));
    let env = active_body.map_or_else(
        || Contact::stationary(world, mob, scale),
        |m| Contact {
            water: m.body.in_water,
            lava: m.body.in_lava,
            fire: m.body.touching_fire,
            cactus: m.body.touching_cactus,
        },
    );
    let wet = env.water || climate.rain_reaches(world, mob.position, mob.kind.size().1 * scale);
    let protection = mob_protection(data.as_deref());
    let immune = fire_immune(mob.kind, data.as_deref());
    let mut tick = FireTick::begin(&mut brain.fire_ticks, env, wet, protection);
    brain.fire_flag = tick.tracked && !immune;
    if immune {
        brain.fire_ticks = 0;
    } else {
        sunlight(world, climate, mob, brain, data.as_deref_mut(), protection);
        tick.finish(&mut brain.fire_ticks, wet, protection, 1);
    }
    let silent = data.as_ref().is_some_and(|d| d.silent);
    if tick.fizz && !silent {
        let state = merchant_random.as_deref_mut().unwrap_or_else(|| {
            brain
                .entity_random
                .get_or_insert_with(|| JavaRandom::new(JavaRandom::independent_seed()).state())
        });
        let mut random = JavaRandom::from_state(*state);
        out.world_sounds.push((
            "random.fizz",
            mob.position.as_dvec3(),
            0.7,
            1.6 + (random.next_float() - random.next_float()) * 0.4,
        ));
        *state = random.state();
    }
    if data.as_ref().is_some_and(|d| d.invulnerable) {
        return;
    }
    for cause in tick.damage(env.cactus) {
        if immune && cause.source().fire {
            continue;
        }
        if melee::apply_mob_hit(
            mob,
            brain,
            data.as_deref_mut(),
            cause.amount(),
            cause.source(),
            enchantment_state,
        ) == Some(true)
        {
            melee::mob_hurt_feedback(
                mob,
                brain,
                merchant_random.as_deref_mut(),
                false,
                silent,
                out,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vanilla::drops::DropNbt;

    #[test]
    #[ignore = "requires installed resources for the regular input tick"]
    fn left_click_extinguishes_lighter_and_fire_charge_fire_through_the_support_face() {
        let resources = crate::validation::resources();
        let settings = Settings::default();
        for mode in [Mode::Survival, Mode::Creative] {
            for item_id in [259, 385] {
                let mut world = super::super::tests::flat_world();
                let mut sim = Simulation::new(mode, 189);
                sim.mob_spawning = false;
                sim.inventory.slots[0] = Some(Stack::new(item_id, 2));
                let mut player = Player::new(world.spawn);
                player.set_rotation_degrees(0., 45.);
                let support = sim.pick(&world, &player).unwrap().block().unwrap();
                assert_eq!(support.face, 1);
                let flame = support.adjacent();
                sim.tick(&mut world, &mut player, resources, &settings, false, true);
                assert_eq!(world.get(flame) >> 4, 51, "igniter {item_id}");
                let through_fire = sim.pick(&world, &player).unwrap().block().unwrap();
                assert_eq!(through_fire.position, support.position,
                    "fire has no selection collision; the ray must reach its support");
                let before = world.get(support.position);
                let out = sim.tick(&mut world, &mut player, resources, &settings, true, false);
                assert_eq!(world.get(flame), 0, "mode {mode:?}, igniter {item_id}");
                assert_eq!(world.get(support.position), before);
                assert!(out.changed.contains(&flame));
                let sounds: Vec<_> = out.world_sounds.iter().filter(|s| s.0 == "random.fizz").collect();
                assert_eq!(sounds.len(), 1, "local client/server extinguish must not duplicate sound");
                assert_eq!(sounds[0].2, 0.5);
                assert!((1.8..=3.4).contains(&sounds[0].3));
            }
        }
    }

    #[test]
    fn extinguishing_uses_only_the_clicked_adjacent_face() {
        for (face, side) in [IVec3::NEG_Y, IVec3::Y, IVec3::NEG_Z, IVec3::Z, IVec3::NEG_X, IVec3::X]
            .into_iter().enumerate() {
            let mut world = super::super::tests::flat_world();
            let mut sim = Simulation::default();
            let support = IVec3::new(8, 3, 8);
            world.set(support, 1 << 4);
            world.set(support + side, 51 << 4 | 7);
            let mut out = Outcome::default();
            sim.extinguish_clicked_fire(&mut world, support, face as u8, &mut out);
            assert_eq!(world.get(support), 1 << 4);
            assert_eq!(world.get(support + side), 0);
            assert_eq!(out.changed, [support + side]);
            // A stale duplicate action never clears a replacement block.
            world.set(support + side, 20 << 4);
            sim.extinguish_fire_at(&mut world, support + side, &mut out);
            assert_eq!(world.get(support + side), 20 << 4);
            assert_eq!(out.world_sounds.len(), 1);
        }
    }

    #[test]
    fn player_leaves_fire_and_lava_burning_and_extinguishes_in_water() {
        let mut world = super::super::tests::flat_world();
        let mut sim = Simulation::new(Mode::Survival, 189);
        let mut player = Player::new(world.spawn);
        let at = player.position.floor().as_ivec3();
        let step = |world: &World, sim: &mut Simulation, player: &mut Player| {
            player.tick_local(world, &mut sim.fall_distance);
            sim.tick_player_fire(world, player);
            sim.hurt_ticks = sim.hurt_ticks.saturating_sub(1);
        };
        step(&world, &mut sim, &mut player);
        world.set(at, 51 << 4);
        step(&world, &mut sim, &mut player);
        // Counts, health and metadata from the installed 1.8.9
        // EntityPlayerMP.onUpdate + onUpdateEntity after spawn immunity expires.
        assert_eq!((sim.fire_ticks, sim.health), (-19, 19.));
        for _ in 0..19 {
            step(&world, &mut sim, &mut player);
        }
        assert_eq!((sim.fire_ticks, sim.health), (160, 18.));
        assert!(!sim.player_burning());
        world.set(at, 0);
        for _ in 0..21 {
            step(&world, &mut sim, &mut player);
        }
        assert_eq!((sim.fire_ticks, sim.health), (139, 16.));
        assert!(sim.player_burning());
        world.set(at, 11 << 4);
        step(&world, &mut sim, &mut player);
        assert_eq!((sim.fire_ticks, sim.health), (301, 13.));
        world.set(at, 9 << 4);
        step(&world, &mut sim, &mut player);
        assert_eq!((sim.fire_ticks, sim.health), (-20, 13.));
        sim.mode = Mode::Creative;
        world.set(at, 11 << 4);
        step(&world, &mut sim, &mut player);
        assert_eq!((sim.fire_ticks, sim.health), (0, 13.));
        assert!(
            sim.player_burning(),
            "creative publishes its lava flag before extinguishing"
        );
        world.set(at, 0);
        step(&world, &mut sim, &mut player);
        assert!(!sim.player_burning());
    }

    #[test]
    fn sun_burns_exposed_adult_undead_but_respects_shade_helmets_and_variants() {
        let mut world = super::super::tests::flat_world();
        let mut player = Player::new(world.spawn);
        let mut sim = Simulation::new(Mode::Creative, 189);
        sim.mob_spawning = false;
        sim.world_difficulty = Some(2);
        sim.clock = 18000;
        for (i, kind) in [
            MobKind::Zombie,
            MobKind::Skeleton,
            MobKind::Zombie,
            MobKind::Skeleton,
            MobKind::Zombie,
            MobKind::Skeleton,
            MobKind::Sheep,
        ]
        .into_iter()
        .enumerate()
        {
            let position = Vec3::new(2.5 + i as f32, 1., 8.5);
            let id = i as u64;
            sim.mobs.push(Mob {
                id,
                kind,
                position,
                previous: position,
                velocity: Vec3::ZERO,
                yaw: 0.,
                health: kind.health(),
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
            sim.mob_brains.entry(id).or_default().entity_random =
                Some(JavaRandom::new(189).state());
            let data = sim.command_entities.mobs.entry(id).or_default();
            data.no_ai = true;
            match i {
                2 => {
                    world.set(position.floor().as_ivec3() + 3 * IVec3::Y, 1 << 4);
                }
                3 => {
                    data.equipment.insert(103, Some(Stack::new(86, 1)));
                }
                4 => {
                    data.extra.insert("IsBaby".into(), DropNbt::Byte(1));
                }
                5 => {
                    data.extra.insert("SkeletonType".into(), DropNbt::Byte(1));
                }
                _ => {}
            }
        }
        let advance = |sim: &mut Simulation, world: &mut World, player: &mut Player, count| {
            for _ in 0..count {
                sim.tick_entities(world, player, &Settings::default(), &mut Outcome::default());
            }
        };
        advance(&mut sim, &mut world, &mut player, 100);
        assert!(sim.mobs.iter().all(|m| m.health == m.kind.health()));
        // Sleep just outside the monsters' bed safety radius, then run the actual
        // world tick so that daylight and the first entity update come from the same clock.
        let head = IVec3::new(14, 1, 0);
        world.set(head, (26 << 4) | 8 | 2);
        world.set(head + IVec3::Z, (26 << 4) | 2);
        player.set_position_f64([14.5, 1., 0.5]);
        assert_eq!(sim.try_bed(&mut world, &mut player, head).0, crate::vanilla::sleep::Attempt::Asleep);
        for _ in 0..1101 {
            sim.tick_sleep(&mut world, &mut player, &mut Outcome::default());
            sim.tick_entities(&mut world, &mut player, &Settings::default(), &mut Outcome::default());
        }
        assert!(!player.sleeping && sim.clock > 24000 && sim.clock < 26000);
        assert!(sim
            .mobs
            .iter()
            .all(|m| m.id >= 2 && m.health == m.kind.health()));
        assert_eq!(sim.mobs.len(), 5);
    }

    #[test]
    fn fire_protection_shortens_new_burns_without_shortening_existing_fire() {
        let mut ticks = -1;
        set_fire(&mut ticks, 15, 4);
        assert_eq!(ticks, 120);
        set_fire(&mut ticks, 8, 4);
        assert_eq!(ticks, 120);
    }
}
