//! Concrete creatures registered by EntityList in Java Edition 1.8.9.
use serde::{Deserialize, Serialize};

macro_rules! mob_registry {
    ($( $kind:ident => ($id:literal, $name:literal, $hp:literal, $w:literal, $h:literal) ),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
        #[repr(u8)]
        pub enum MobKind { $( $kind = $id ),+ }
        impl MobKind {
            pub const ALL: &'static [Self] = &[$(Self::$kind),+];
            pub fn from_id(id: u8) -> Option<Self> {
                match id { $( $id => Some(Self::$kind), )+ _ => None }
            }
            pub fn from_name(name: &str) -> Option<Self> {
                match name { $( $name => Some(Self::$kind), )+ _ => None }
            }
            pub fn name(self) -> &'static str {
                match self { $( Self::$kind => $name, )+ }
            }
            pub fn health(self) -> f32 {
                match self { $( Self::$kind => $hp, )+ }
            }
            pub fn size(self) -> (f32, f32) {
                match self { $( Self::$kind => ($w, $h), )+ }
            }
        }
    };
}

// Protocol ID, entity registry name, base maximum health, width and height.
// The first nine variants retain their legacy serde indices for existing saves.
mob_registry! {
    Cow => (92, "Cow", 10., 0.9, 1.3),
    Pig => (90, "Pig", 10., 0.9, 0.9),
    Sheep => (91, "Sheep", 8., 0.9, 1.3),
    Chicken => (93, "Chicken", 4., 0.4, 0.7),
    Zombie => (54, "Zombie", 20., 0.6, 1.95),
    Skeleton => (51, "Skeleton", 20., 0.6, 1.8),
    Creeper => (50, "Creeper", 20., 0.6, 1.8),
    Spider => (52, "Spider", 16., 1.4, 0.9),
    Villager => (120, "Villager", 20., 0.6, 1.8),
    Giant => (53, "Giant", 100., 3.6000001, 10.799999),
    Slime => (55, "Slime", 1., 0.51000005, 0.51000005),
    Ghast => (56, "Ghast", 10., 4., 4.),
    PigZombie => (57, "PigZombie", 20., 0.6, 1.95),
    Enderman => (58, "Enderman", 40., 0.6, 2.9),
    CaveSpider => (59, "CaveSpider", 12., 0.7, 0.5),
    Silverfish => (60, "Silverfish", 8., 0.4, 0.3),
    Blaze => (61, "Blaze", 20., 0.6, 1.8),
    MagmaCube => (62, "LavaSlime", 1., 0.51000005, 0.51000005),
    EnderDragon => (63, "EnderDragon", 200., 16., 8.),
    Wither => (64, "WitherBoss", 300., 0.9, 3.5),
    Bat => (65, "Bat", 6., 0.5, 0.9),
    Witch => (66, "Witch", 26., 0.6, 1.95),
    Endermite => (67, "Endermite", 8., 0.4, 0.3),
    Guardian => (68, "Guardian", 30., 0.85, 0.85),
    Squid => (94, "Squid", 10., 0.95, 0.95),
    Wolf => (95, "Wolf", 8., 0.6, 0.8),
    Mooshroom => (96, "MushroomCow", 10., 0.9, 1.3),
    SnowGolem => (97, "SnowMan", 4., 0.7, 1.9),
    Ocelot => (98, "Ozelot", 10., 0.6, 0.7),
    IronGolem => (99, "VillagerGolem", 100., 1.4, 2.9),
    Horse => (100, "EntityHorse", 53., 1.4, 1.6),
    Rabbit => (101, "Rabbit", 10., 0.6, 0.7),
}
impl MobKind {
    pub fn vanilla_id(self) -> u8 {
        self as u8
    }
    pub fn hostile(self) -> bool {
        matches!(self.vanilla_id(), 50..=64 | 66..=68)
    }
    pub fn has_egg(self) -> bool {
        !matches!(
            self,
            Self::Giant | Self::EnderDragon | Self::Wither | Self::SnowGolem | Self::IronGolem
        )
    }
    pub fn texture(self) -> &'static str {
        match self {
            Self::Villager => "textures/entity/villager/farmer.png",
            _ => crate::remote_mobs::Appearance::default().texture(self.vanilla_id()),
        }
    }

    pub fn ageable(self) -> bool {
        matches!(self.vanilla_id(), 90..=93 | 95 | 96 | 98 | 100 | 101 | 120)
    }
    pub fn adult_size(self, data: Option<&super::command_entity_state::MobData>) -> (f32, f32) {
        use crate::vanilla::item_nbt::number;
        let field = |key| number(data.and_then(|d| d.extra.get(key)));
        match self {
            Self::Slime | Self::MagmaCube => {
                let width = 0.51000005 * (field("Size").clamp(0, 126) + 1) as f32;
                (width, width)
            }
            Self::Guardian if field("Elder") != 0 => (1.9975, 1.9975),
            Self::Skeleton if field("SkeletonType") == 1 => (0.72, 2.535),
            _ => self.size(),
        }
    }
    pub fn max_health(self, data: Option<&super::command_entity_state::MobData>) -> f32 {
        use crate::vanilla::{
            drops::DropNbt as Tag,
            item_nbt::{compound, number},
        };
        let Some(data) = data else {
            return self.health();
        };
        if let Some(Tag::List { values, .. }) = data.extra.get("Attributes") {
            for attribute in values.iter().filter_map(compound) {
                if matches!(attribute.get("Name"), Some(Tag::String(s)) if s == "generic.maxHealth")
                {
                    let base = match attribute.get("Base") {
                        Some(Tag::Double(n)) => *n,
                        Some(Tag::Float(n)) => f64::from(*n),
                        Some(Tag::Long(n)) => *n as f64,
                        v @ Some(Tag::Byte(_) | Tag::Short(_) | Tag::Int(_)) => {
                            f64::from(number(v))
                        }
                        _ => continue,
                    };
                    if base.is_finite() {
                        return base.clamp(0., 1024.) as f32;
                    }
                }
            }
        }
        match self {
            Self::Slime | Self::MagmaCube => {
                (number(data.extra.get("Size")).clamp(0, 126) + 1).pow(2) as f32
            }
            Self::Guardian if number(data.extra.get("Elder")) != 0 => 80.,
            Self::Wolf if matches!(data.extra.get("OwnerUUID"), Some(Tag::String(id)) if !id.is_empty()) => {
                20.
            }
            _ => self.health(),
        }
    }
}

impl super::Simulation {
    pub(super) fn spawn_egg_mob(
        &mut self,
        item: &crate::inventory::Stack,
        kind: MobKind,
        position: glam::Vec3,
        child: bool,
    ) -> u64 {
        use crate::vanilla::{drops::DropNbt as Tag, item_nbt::compound};
        let yaw = if child {
            std::f32::consts::PI
        } else {
            let mut rng = self.block_ticks.world_random();
            let yaw = (180. + rng.next_float() * 360.).to_radians();
            self.block_ticks.set_world_random(&rng);
            yaw
        };
        let id = self.spawn_registered(kind, position, yaw);
        if child {
            self.mob_brains.entry(id).or_default().growing_age = -24000;
        }
        if let Some(Tag::String(name)) = item
            .nbt
            .as_deref()
            .and_then(compound)
            .and_then(|fields| fields.get("display"))
            .and_then(compound)
            .and_then(|display| display.get("Name"))
        {
            self.command_entities
                .mobs
                .entry(id)
                .or_default()
                .name
                .clone_from(name);
            if let Some(trader) = self.trades.get_mut(&id) {
                trader.custom_name = Some(name.clone());
            }
        }
        id
    }

    pub(super) fn spawn_egg_child(
        &mut self,
        item: &crate::inventory::Stack,
        kind: MobKind,
        position: glam::Vec3,
        parent_id: u64,
    ) -> u64 {
        use crate::vanilla::{drops::DropNbt as Tag, item_nbt::number};
        let parent_extra = self
            .command_entities
            .mobs
            .get(&parent_id)
            .map(|d| d.extra.clone());
        let parent_prof = self.trades.get(&parent_id).map(|t| t.profession).or_else(|| {
            parent_extra
                .as_ref()
                .and_then(|e| e.get("Profession"))
                .map(|t| (number(Some(t)).clamp(0, 4)) as u8)
        });
        let child_id = self.spawn_egg_mob(item, kind, position, true);
        let child_data = self.command_entities.mobs.entry(child_id).or_default();
        match kind {
            MobKind::Sheep => {
                let color = parent_extra
                    .as_ref()
                    .map_or(0, |e| number(e.get("Color")) & 15);
                child_data
                    .extra
                    .insert("Color".into(), Tag::Byte(color as i8));
                child_data.extra.insert("Sheared".into(), Tag::Byte(0));
            }
            MobKind::Horse => {
                if let Some(extra) = &parent_extra {
                    let horse_type = number(extra.get("Type"));
                    let variant = number(extra.get("Variant"));
                    child_data
                        .extra
                        .insert("Type".into(), Tag::Int(horse_type));
                    child_data
                        .extra
                        .insert("Variant".into(), Tag::Int(variant));
                }
            }
            MobKind::Rabbit => {
                if let Some(extra) = &parent_extra {
                    let rabbit_type = number(extra.get("RabbitType"));
                    child_data
                        .extra
                        .insert("RabbitType".into(), Tag::Int(rabbit_type));
                }
            }
            MobKind::Villager => {
                if let Some(prof) = parent_prof {
                    let custom_name = self
                        .trades
                        .get(&child_id)
                        .and_then(|t| t.custom_name.clone());
                    let mut trader = crate::villagers::Trader::spawned(prof);
                    trader.custom_name = custom_name;
                    self.trades.insert(child_id, trader);
                    child_data
                        .extra
                        .insert("Profession".into(), Tag::Int(i32::from(prof)));
                }
            }
            _ => {}
        }
        child_id
    }

    pub fn mob_size(&self, mob: &super::Mob) -> (f32, f32) {
        let data = self.command_entities.mobs.get(&mob.id);
        let (w, h) = mob.kind.adult_size(data);
        if self.is_baby(mob.id) {
            (w * 0.5, h * 0.5)
        } else {
            (w, h)
        }
    }

    pub fn spawn_registered(&mut self, kind: MobKind, position: glam::Vec3, yaw: f32) -> u64 {
        self.spawn_registered_group(kind, position, yaw, &mut None)
    }

    pub fn spawn_registered_group(
        &mut self,
        kind: MobKind,
        position: glam::Vec3,
        yaw: f32,
        group: &mut Option<crate::vanilla::spawning::AnimalGroup>,
    ) -> u64 {
        use crate::vanilla::drops::DropNbt as Tag;
        use crate::vanilla::spawning::{initialize_animal, AnimalKind, EntityRandom};
        let id = self.spawn_registered_raw(kind, position, yaw);
        let mut rng = EntityRandom::new(super::random(&mut self.rng) as i64);
        let data = self.command_entities.mobs.entry(id).or_default();
        match kind {
            MobKind::Slime | MobKind::MagmaCube => {
                let size = 1 << rng.random.next_int(3);
                data.extra.insert("Size".into(), Tag::Int(size - 1));
            }
            MobKind::Sheep => {
                let mut world_rng = self.block_ticks.world_random();
                let color = crate::vanilla::spawning::random_sheep_color(&mut world_rng);
                self.block_ticks.set_world_random(&world_rng);
                data.extra.insert("Color".into(), Tag::Byte(color as i8));
            }
            MobKind::Rabbit | MobKind::Horse => {
                let animal = if kind == MobKind::Horse {
                    AnimalKind::Horse
                } else {
                    AnimalKind::Rabbit
                };
                let traits = initialize_animal(animal, &mut rng, group);
                self.mob_brains.entry(id).or_default().growing_age = traits.growing_age;
                if let Some(coat) = traits.rabbit_type {
                    data.extra
                        .insert("RabbitType".into(), Tag::Int(coat.into()));
                }
                if let Some(horse) = traits.horse {
                    data.extra
                        .insert("Type".into(), Tag::Int(horse.kind.into()));
                    data.extra
                        .insert("Variant".into(), Tag::Int(horse.variant.into()));
                    let values = [
                        ("generic.maxHealth", horse.max_health),
                        ("generic.movementSpeed", horse.movement_speed),
                        ("horse.jumpStrength", horse.jump_strength),
                    ]
                    .into_iter()
                    .map(|(key, value)| {
                        Tag::Compound(
                            [
                                ("Name".into(), Tag::String(key.into())),
                                ("Base".into(), Tag::Double(value)),
                            ]
                            .into(),
                        )
                    })
                    .collect();
                    data.extra.insert(
                        "Attributes".into(),
                        Tag::List {
                            element: 10,
                            values,
                        },
                    );
                }
            }
            MobKind::Villager => {
                self.trades.insert(
                    id,
                    crate::villagers::Trader::spawned(rng.random.next_int(5) as u8),
                );
            }
            MobKind::Zombie | MobKind::PigZombie => {
                let mut world_rng = self.block_ticks.world_random();
                if world_rng.next_float() < 0.05 {
                    data.extra.insert("IsBaby".into(), Tag::Byte(1));
                }
                if world_rng.next_float() < 0.05 && kind == MobKind::Zombie {
                    data.extra.insert("IsVillager".into(), Tag::Byte(1));
                }
                self.block_ticks.set_world_random(&world_rng);
                if kind == MobKind::PigZombie {
                    data.equipment
                        .insert(99, Some(crate::inventory::Stack::new(283, 1)));
                }
            }
            MobKind::Skeleton => {
                let wither = self.dimension == -1 && rng.random.next_int(5) > 0;
                if wither {
                    data.extra.insert("SkeletonType".into(), Tag::Byte(1));
                }
                data.equipment.insert(
                    99,
                    Some(crate::inventory::Stack::new(
                        if wither { 272 } else { 261 },
                        1,
                    )),
                );
            }
            _ => {}
        }
        self.mobs.last_mut().unwrap().health = kind.max_health(Some(data));
        id
    }

    pub(crate) fn spawn_registered_raw(
        &mut self,
        kind: MobKind,
        position: glam::Vec3,
        yaw: f32,
    ) -> u64 {
        self.next_id = self.next_id.wrapping_add(1);
        self.mobs.push(super::Mob {
            id: self.next_id,
            kind,
            position,
            previous: position,
            velocity: glam::Vec3::ZERO,
            yaw,
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
        self.next_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        gameplay::{Mode, Outcome, Simulation},
        inventory::Stack,
        player::Player,
    };
    use clap::Parser;
    use glam::{IVec3, Vec3};

    #[test]
    #[ignore = "uses the installed client command resources"]
    fn all_original_mobs_summon_by_registry_name_and_survive_reload() {
        let mut world = super::super::tests::flat_world();
        let mut sim = Simulation::new(Mode::Creative, 189);
        let mut options = crate::Options::parse_from(["ferrum189"]);
        options.world_rules.allow_cheats = true;
        let resources = crate::validation::resources();
        let mut player = Player::new(world.spawn);
        let completions = super::super::local_commands::completions(
            &mut sim,
            &mut player,
            &mut world,
            resources,
            &options,
            "/summon ",
        );
        let original = (50..=68).chain(90..=101).chain([120]).collect::<Vec<_>>();
        assert_eq!(original.len(), MobKind::ALL.len());
        for id in original {
            let kind = MobKind::from_id(id).unwrap();
            assert!(completions.iter().any(|s| s == kind.name()));
            let command = format!("/summon {} 5 2 5 {{NoAI:1b}}", kind.name());
            let reply = super::super::local_commands::execute(
                &mut sim,
                &mut player,
                &mut world,
                resources,
                &options,
                &command,
            );
            assert_eq!(
                reply.messages[0]["translate"], "commands.summon.success",
                "{command}"
            );
            assert_eq!(sim.mobs.last().unwrap().kind, kind);
            let selector = format!(
                "/entitydata @e[type={},c=1] {{CustomName:checked}}",
                kind.name()
            );
            let reply = super::super::local_commands::execute(
                &mut sim,
                &mut player,
                &mut world,
                resources,
                &options,
                &selector,
            );
            assert_eq!(
                reply.messages[0]["translate"], "commands.entitydata.success",
                "{selector}"
            );
        }
        let snapshot = serde_json::to_vec(&sim).unwrap();
        let restored: Simulation = serde_json::from_slice(&snapshot).unwrap();
        assert_eq!(restored.mobs.len(), 32);
        for (original, restored_mob) in sim.mobs.iter().zip(&restored.mobs) {
            assert_eq!(
                (original.kind, original.id),
                (restored_mob.kind, restored_mob.id)
            );
            assert_eq!(
                restored.command_entities.mobs[&restored_mob.id].name,
                "checked"
            );
        }
        for (name, data, health, width) in [
            ("Slime", "Size:3", 16., 0.51000005 * 4.),
            ("Guardian", "Elder:1b", 80., 1.9975),
            (
                "EntityHorse",
                "Type:1,Attributes:[{Name:generic.maxHealth,Base:25.0d}]",
                25.,
                1.4,
            ),
            (
                "Wolf",
                "OwnerUUID:00000000-0000-0000-0000-000000000001",
                20.,
                0.6,
            ),
        ] {
            let reply = super::super::local_commands::execute(
                &mut sim,
                &mut player,
                &mut world,
                resources,
                &options,
                &format!("/summon {name} 5 2 5 {{{data}}}"),
            );
            assert_eq!(
                reply.messages[0]["translate"], "commands.summon.success",
                "{name}"
            );
            let mob = sim.mobs.last().unwrap();
            assert_eq!((mob.health, sim.mob_size(mob).0), (health, width), "{name}");
        }
    }

    #[test]
    fn original_spawn_eggs_spawn_on_blocks_and_liquids_and_set_spawners() {
        let mut world = super::super::tests::flat_world();
        let mut sim = Simulation::new(Mode::Survival, 189);
        let floor = IVec3::new(5, 0, 5);
        let eggs: Vec<_> = MobKind::ALL.iter().filter(|kind| kind.has_egg()).collect();
        assert_eq!(eggs.len(), 27);
        for kind in eggs {
            let mut egg = Stack::new(383, 1);
            egg.meta = kind.vanilla_id().into();
            sim.inventory.slots[0] = Some(egg.clone());
            assert!(sim.use_item_on_block(
                &mut world,
                egg,
                (floor, floor + IVec3::Y),
                &mut Outcome::default()
            ));
            assert!(sim.inventory.held().is_none());
            let mob = sim.mobs.last().unwrap();
            assert_eq!((mob.kind, mob.position), (*kind, Vec3::new(5.5, 1., 5.5)));
        }
        let mut egg = Stack::new(383, 2);
        egg.meta = 94;
        sim.inventory.slots[0] = Some(egg.clone());
        let water = floor + IVec3::Y;
        world.set(water, 9 << 4);
        assert!(sim.use_item_on_block(
            &mut world,
            egg.clone(),
            (water, water + IVec3::Y),
            &mut Outcome::default()
        ));
        assert_eq!(sim.mobs.last().unwrap().position, Vec3::new(5.5, 1.5, 5.5));
        world.set(water, 52 << 4);
        assert!(sim.use_item_on_block(
            &mut world,
            egg,
            (water, water + IVec3::Y),
            &mut Outcome::default()
        ));
        let tile = crate::vanilla::item_nbt::compound(world.tile(water).unwrap()).unwrap();
        assert_eq!(
            tile["EntityId"],
            crate::vanilla::drops::DropNbt::String("Squid".into())
        );
        assert_eq!(sim.mobs.len(), 28);
    }

    #[test]
    fn rabbit_and_horse_groups_keep_parent_coats_and_native_horse_attributes() {
        use crate::vanilla::{
            drops::DropNbt as Tag,
            item_nbt::{compound, number},
        };
        let mut sim = Simulation::new(Mode::Survival, 189);
        let mut group = None;
        let first = sim.spawn_registered_group(MobKind::Rabbit, Vec3::ZERO, 0., &mut group);
        let second = sim.spawn_registered_group(MobKind::Rabbit, Vec3::ZERO, 0., &mut group);
        assert_eq!(
            sim.command_entities.mobs[&first].extra["RabbitType"],
            sim.command_entities.mobs[&second].extra["RabbitType"]
        );
        assert!(!sim.is_baby(first) && sim.is_baby(second));
        let mut group = None;
        let first = sim.spawn_registered_group(MobKind::Horse, Vec3::ZERO, 0., &mut group);
        let second = sim.spawn_registered_group(MobKind::Horse, Vec3::ZERO, 0., &mut group);
        let leader = &sim.command_entities.mobs[&first].extra;
        let follower = &sim.command_entities.mobs[&second].extra;
        assert_eq!(leader["Type"], follower["Type"]);
        assert_eq!(
            number(leader.get("Variant")) & 255,
            number(follower.get("Variant")) & 255
        );
        let Tag::List { values, .. } = &follower["Attributes"] else {
            panic!("missing horse attributes")
        };
        for name in [
            "generic.maxHealth",
            "generic.movementSpeed",
            "horse.jumpStrength",
        ] {
            assert!(values
                .iter()
                .filter_map(compound)
                .any(|f| matches!(f.get("Name"),Some(Tag::String(s)) if s==name)));
        }
    }
}
