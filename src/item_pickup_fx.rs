//! EntityPickupFX retains the collected entity's last visual state for three client ticks.
use super::{xp_orb::Orb, Drop, Simulation};
use crate::player::Player;
use glam::DVec3;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Collector {
    Local,
    Actor {
        id: i32,
        previous: DVec3,
        feet: DVec3,
    },
}

#[derive(Clone, Debug)]
pub(crate) enum Visual {
    Item(Drop),
    Orb(Orb),
}

#[derive(Clone, Debug)]
pub(crate) struct Effect {
    pub(crate) visual: Visual,
    collector: Collector,
    age: u8,
}

impl Effect {
    pub(crate) fn source(&self) -> DVec3 {
        match &self.visual {
            Visual::Item(item) => item.position,
            Visual::Orb(orb) => orb.position,
        }
    }

    pub(crate) fn position(&self, sim: &Simulation, player: &Player, partial: f32) -> DVec3 {
        let target = match self.collector {
            Collector::Local => {
                player.precise_eye(partial) - DVec3::Y * f64::from(player.eye_height())
            }
            Collector::Actor { id, previous, feet } => {
                let scene = &sim.server_scene;
                let pose = scene
                    .actors
                    .get(&id)
                    .map(|a| (a.previous, a.feet))
                    .or_else(|| scene.mobs.get(&id).map(|a| (a.previous, a.feet)))
                    .or_else(|| {
                        sim.mobs
                            .iter()
                            .find(|a| a.id == u64::from(id as u32))
                            .map(|a| (a.previous, a.position))
                    });
                let (last, current) =
                    pose.map_or((previous, feet), |(a, b)| (a.as_dvec3(), b.as_dvec3()));
                last + (current - last) * f64::from(partial)
            }
        };
        let fraction = (f32::from(self.age) + partial) / 3.;
        let start = self.source();
        start + (target + DVec3::Y * 0.5 - start) * f64::from(fraction * fraction)
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Effects(pub(crate) Vec<Effect>);

impl Effects {
    pub(crate) fn item_count(&mut self, item: &Drop, original_count: u8, collector: Collector) {
        let mut snapshot = item.clone();
        snapshot.stack.count = original_count;
        self.0.push(Effect {
            visual: Visual::Item(snapshot),
            collector,
            age: 0,
        });
    }

    pub(crate) fn item(&mut self, item: &Drop, collector: Collector) {
        self.item_count(item, item.stack.count, collector);
    }

    pub(crate) fn orb(&mut self, orb: &Orb, collector: Collector) {
        self.0.push(Effect {
            visual: Visual::Orb(orb.clone()),
            collector,
            age: 0,
        });
    }

    pub(crate) fn tick(&mut self) {
        self.0.retain_mut(|effect| {
            effect.age += 1;
            effect.age < 3
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{inventory::Stack, models::server::Actor, vanilla::random::JavaRandom};

    #[test]
    fn original_pickup_fx_render_manager_trajectory_and_three_tick_lifetime() {
        type RenderCall = ([f64; 3], f32, f32, i32, u8);
        type Sample = (u8, f32, [f64; 3], [f64; 3], RenderCall);
        let (source, camera, frames, dead, layer): (
            [f64; 3],
            [f64; 3],
            Vec<Sample>,
            [bool; 4],
            u8,
        ) = serde_json::from_str(include_str!("../tests/fixtures/vanilla189-pickup-fx.json"))
            .unwrap();
        assert_eq!(layer, 3);
        let source = DVec3::from_array(source);
        let camera = DVec3::from_array(camera);
        let mut item = Drop::new(1, source, Stack::new(1, 0), &mut JavaRandom::new(0), 0);
        item.age = 41;
        item.yaw = 125.5;
        item.hover_start = 0.7;
        let mut sim = Simulation::default();
        // Local authoritative pickup has already consumed the stack when the effect is emitted.
        sim.pickup_fx.item_count(&item, 8, Collector::Local);
        sim.pickup_fx.item_count(
            &item,
            8,
            Collector::Actor {
                id: 5,
                previous: DVec3::ZERO,
                feet: DVec3::ZERO,
            },
        );
        let mut player = Player::new(glam::Vec3::ZERO);
        for (age, partial, previous, current, (expected, yaw, bob_partial, item_age, count)) in
            frames
        {
            if sim.pickup_fx.0[0].age < age {
                sim.pickup_fx.tick();
            }
            player.set_position_f64(current);
            player.previous = DVec3::from_array(previous).as_vec3();
            sim.server_scene.actors.insert(
                5,
                Actor {
                    previous: player.previous,
                    gait: Default::default(),
                    feet: player.position,
                    yaw: 0.,
                    pitch: 0.,
                    skin: 127,
                    alex: false,
                    held: None,
                    hurt: false,
                    bed: None,
                    swing: 0.,
                    ..Default::default()
                },
            );
            assert!(!dead[age as usize]);
            for effect in &sim.pickup_fx.0 {
                let actual = (effect.position(&sim, &player, partial) - camera).as_vec3();
                assert_eq!(actual, DVec3::from_array(expected).as_vec3());
                let Visual::Item(snapshot) = &effect.visual else {
                    panic!("item effect")
                };
                assert_eq!(
                    (snapshot.yaw, partial, snapshot.age, snapshot.stack.count),
                    (yaw, bob_partial, item_age, count)
                );
            }
        }
        sim.pickup_fx.tick();
        assert_eq!(sim.pickup_fx.0.is_empty(), dead[3]);
    }
}

#[cfg(test)]
#[test]
#[ignore = "requires locally imported 1.8.9 item models"]
fn collected_stack_mesh_matches_original_pickup_render_arguments_in_world_renderer() {
    use crate::{
        gameplay::Mode,
        inventory::Stack,
        mesh::Frustum,
        models::{self, ModelContext},
        settings::Settings,
        vanilla::random::JavaRandom,
        world::World,
    };
    use glam::{Mat4, Vec3};
    type Sample = (u8, f32, [f64; 3], [f64; 3], ([f64; 3], f32, f32, i32, u8));
    let (source, camera, calls, _, _): ([f64; 3], [f64; 3], Vec<Sample>, [bool; 4], u8) =
        serde_json::from_str(include_str!("../tests/fixtures/vanilla189-pickup-fx.json")).unwrap();
    let world = World {
        chunks: Default::default(),
        spawn: Vec3::ZERO,
        label: String::new(),
        save_path: Default::default(),
        edits: Default::default(),
        imported: false,
        light_revision: 0,
    };
    let mut settings = Settings::default();
    settings.entity_culling = false;
    let eye = Vec3::new(10., 84., 25.);
    let frustum = Frustum::new(
        Mat4::perspective_rh(1.4, 16. / 9., 0.05, 128.)
            * Mat4::look_at_rh(eye, Vec3::new(10., 81., 9.), Vec3::Y),
    );
    let draw = |sim: &Simulation, p: &Player, a: f32| {
        models::build(ModelContext {
            sim,
            player: p,
            world: &world,
            assets: crate::validation::assets(),
            resources: crate::validation::resources(),
            settings: &settings,
            frustum: &frustum,
            viewport_height: 1080,
            eye,
            alpha: a,
            perspective: 0,
            render_hand: false,
            hitboxes: false,
        })
        .opaque
    };
    let mut sim = Simulation::new(Mode::Survival, 189);
    let mut item = Drop::new(
        90,
        DVec3::from_array(source),
        Stack::new(1, 8),
        &mut JavaRandom::new(189),
        0,
    );
    item.age = 41;
    item.yaw = 125.5;
    item.hover_start = 0.7;
    sim.pickup_fx.item(&item, Collector::Local);
    let mut normal = Simulation::default();
    let mut player = Player::new(Vec3::ZERO);
    for (age, partial, previous, current, (xyz, _, model_partial, _, _)) in calls {
        if sim.pickup_fx.0[0].age < age {
            sim.pickup_fx.tick();
        }
        player.set_position_f64(current);
        player.previous = DVec3::from_array(previous).as_vec3();
        item.position = DVec3::from_array(xyz) + DVec3::from_array(camera);
        item.previous = item.position;
        normal.drops.clear();
        normal.drops.push(item.clone());
        let original = draw(&normal, &player, model_partial);
        let pickup = draw(&sim, &player, partial);
        assert!(
            original.vertices.len() >= 48,
            "expected two stone block copies"
        );
        assert_eq!(original.indices, pickup.indices);
        for (a, b) in original.vertices.iter().zip(&pickup.vertices) {
            assert!(Vec3::from_array(a.position).abs_diff_eq(Vec3::from_array(b.position), 2e-5));
            assert_eq!((a.uv, a.layer, a.color), (b.uv, b.layer, b.color));
        }
    }
}
