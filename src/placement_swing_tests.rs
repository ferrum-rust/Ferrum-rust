//! Normal held-input checks for Minecraft.rightClickMouse's block-use animation.
use super::{Mode, Outcome, Simulation};
use crate::{inventory::Stack, player::Player, settings::Settings, world::World};
use glam::Vec3;

fn fixture(mode: Mode, item: u16) -> (Simulation, World, Player) {
    let mut simulation = Simulation::new(mode, 189);
    simulation.mob_spawning = false;
    let count = if matches!(item, 259 | 290 | 326) { 1 } else { 16 };
    simulation.inventory.slots[0] = Some(Stack::new(item, count));
    for _ in 0..4 {
        simulation.equipped.tick(simulation.inventory.held(), None);
    }
    let mut player = Player::new(Vec3::new(8.5, 1., 8.5));
    player.set_rotation_degrees(0., 45.);
    (simulation, super::tests::flat_world(), player)
}

fn input(sim: &mut Simulation, world: &mut World, player: &mut Player, right: bool) -> Outcome {
    sim.tick(
        world,
        player,
        crate::validation::resources(),
        &Settings::default(),
        false,
        right,
    )
}

fn swing(sim: &Simulation) -> f32 {
    // At a restart/wrap the interpolated endpoint is 1, identical to phase 0.
    sim.hand_swing.sample(1.).rem_euclid(1.)
}

#[test]
#[ignore = "requires locally imported Minecraft resources"]
fn right_click_placement_swings_and_lowers_in_both_modes_with_potion_timing() {
    for mode in [Mode::Survival, Mode::Creative] {
        for effect in [None, Some((3, 1, 4)), Some((4, 1, 10))] {
            let (mut sim, mut world, mut player) = fixture(mode, 1);
            let duration = if let Some((id, amplifier, ticks)) = effect {
                sim.effects
                    .load_effect(crate::vanilla::item_food::Effect::new(id, 200, amplifier))
                    .unwrap();
                ticks
            } else {
                6
            };
            let at = sim
                .pick(&world, &player)
                .unwrap()
                .block()
                .unwrap()
                .adjacent();
            let out = input(&mut sim, &mut world, &mut player, true);
            assert!(out.changed.contains(&at));
            assert_eq!(world.get(at), 1 << 4);
            assert_eq!(
                sim.inventory.held().unwrap().count,
                if mode == Mode::Survival { 15 } else { 16 }
            );
            assert!((sim.equipped.lowered(1.) - 0.6).abs() < 1e-6);
            assert_eq!(swing(&sim), 0.);
            for tick in 1..=duration {
                input(&mut sim, &mut world, &mut player, false);
                let expected = (tick % duration) as f32 / duration as f32;
                assert!(
                    (swing(&sim) - expected).abs() < 1e-6,
                    "tick {tick}/{duration}"
                );
            }
            assert_eq!(sim.equipped.lowered(1.), 0.);
        }
    }
}

#[test]
#[ignore = "requires locally imported Minecraft resources"]
fn held_right_click_repeats_placement_and_swing_every_four_ticks() {
    let (mut sim, mut world, mut player) = fixture(Mode::Survival, 1);
    for tick in 0..12 {
        // Aim at fresh ground each accepted click, so collision cannot reject it.
        player.set_rotation_degrees((tick / 4) as f32 * 90., 45.);
        let before = sim.inventory.held().unwrap().count;
        input(&mut sim, &mut world, &mut player, true);
        let consumed = before - sim.inventory.held().unwrap().count;
        assert_eq!(consumed, u8::from(tick % 4 == 0), "tick {tick}");
        assert!(
            (swing(&sim) - (tick % 4) as f32 / 6.).abs() < 1e-6,
            "tick {tick}"
        );
    }
    assert_eq!(sim.inventory.held().unwrap().count, 13);
}

#[test]
#[ignore = "requires locally imported Minecraft resources"]
fn placing_the_final_block_swings_and_unequips_without_consumption_reset() {
    let (mut sim, mut world, mut player) = fixture(Mode::Survival, 1);
    sim.inventory.slots[0].as_mut().unwrap().count = 1;
    let out = input(&mut sim, &mut world, &mut player, true);
    assert!(!out.changed.is_empty());
    assert!(sim.inventory.held().is_none());
    assert!((sim.equipped.lowered(1.) - 0.4).abs() < 1e-6);
    input(&mut sim, &mut world, &mut player, false);
    assert!((swing(&sim) - 1. / 6.).abs() < 1e-6);
}

#[test]
#[ignore = "requires locally imported Minecraft resources"]
fn rejected_placement_and_air_use_do_not_start_block_swing() {
    for mode in [Mode::Survival, Mode::Creative] {
        for pitch in [-60., 90.] {
            let (mut sim, mut world, mut player) = fixture(mode, 1);
            // Air, then a solid block placed into the player's own feet.
            player.set_rotation_degrees(0., pitch);
            for _ in 0..12 {
                input(&mut sim, &mut world, &mut player, true);
                assert_eq!(swing(&sim), 0.);
                assert_eq!(sim.equipped.lowered(1.), 0.);
            }
            assert_eq!(sim.inventory.held().unwrap().count, 16);
        }
    }
    let (mut sim, mut world, mut player) = fixture(Mode::Survival, 332);
    player.set_rotation_degrees(0., -60.);
    input(&mut sim, &mut world, &mut player, true);
    input(&mut sim, &mut world, &mut player, false);
    assert_eq!(sim.inventory.held().unwrap().count, 15);
    assert_eq!(swing(&sim), 0.);
    assert_eq!(sim.equipped.lowered(1.), 0.);
}

#[test]
#[ignore = "requires locally imported Minecraft resources"]
fn successful_block_tools_swing_but_bucket_air_callback_does_not() {
    for mode in [Mode::Survival, Mode::Creative] {
        for (item, swings) in [(259, true), (290, true), (326, false)] {
            let (mut sim, mut world, mut player) = fixture(mode, item);
            let out = input(&mut sim, &mut world, &mut player, true);
            assert!(
                !out.changed.is_empty(),
                "item {item} must perform its action"
            );
            // Tool durability changes do not lower the held tool in Survival.
            if swings {
                let lowered = if mode == Mode::Creative { 0.6 } else { 0. };
                assert!((sim.equipped.lowered(1.) - lowered).abs() < 1e-6);
            }
            input(&mut sim, &mut world, &mut player, false);
            let expected = if swings { 1. / 6. } else { 0. };
            assert!((swing(&sim) - expected).abs() < 1e-6, "item {item}");
        }
    }
}

#[test]
#[ignore = "uses installed resources and Vulkan framebuffer readback"]
fn placed_block_hand_moves_in_rendered_frames_and_returns_to_rest() {
    use crate::{renderer::{RenderInput, Renderer}, ui::{Frame, Gui, Screen}};
    let assets = crate::validation::assets();
    let resources = crate::validation::resources();
    let settings = Settings { clouds: 0, entity_shadows: false, ..Default::default() };
    let (mut sim, mut world, mut player) = fixture(Mode::Creative, 1);
    sim.clock = 6000;
    let out = input(&mut sim, &mut world, &mut player, true);
    assert!(!out.changed.is_empty());
    let mut renderer = pollster::block_on(Renderer::new(None, assets, resources, &settings, None)).unwrap();
    let (width, height) = (320, 240);
    renderer.resize(width, height);
    for &position in world.chunks.keys() {
        renderer.upload(position, crate::mesh::build_with_settings(&world, position, assets, &settings));
    }
    for _ in 0..40 { renderer.tick_lighting(&world, &player, &sim, 3); }
    let gui = Gui { screen: Screen::Playing, ..Default::default() };
    let path = std::env::temp_dir().join(format!("ferrum-placement-swing-{}.png", std::process::id()));
    let mut sheet = image::RgbImage::new(width * 4, height * 2);
    for tick in 0..=6 {
        let mut resting = sim.clone();
        resting.hand_swing = Default::default();
        for _ in 0..8 { resting.equipped.tick(resting.inventory.held(), None); }
        let mut capture = |state: &Simulation| {
            renderer.screenshot(&path, &RenderInput {
                selected_block: None, assets, resources, settings: &settings, gui: &gui,
                ui: &Frame::default(), world: Some(&world), player: Some(&player),
                simulation: Some(state), alpha: 1., time: 0.,
            }).unwrap();
            image::open(&path).unwrap().to_rgb8()
        };
        let baseline = capture(&resting);
        let shown = capture(&sim);
        let hand_changes = shown.enumerate_pixels().filter(|(x, y, pixel)|
            *x > width / 2 && *y > height / 3 && **pixel != *baseline.get_pixel(*x, *y)
        ).count();
        if tick < 6 { assert!(hand_changes > 30, "placement tick {tick}: {hand_changes} hand pixels changed"); }
        else { assert_eq!(hand_changes, 0, "hand must return to resting pose after six ticks"); }
        image::imageops::replace(&mut sheet, &shown, i64::from(tick % 4 * width), i64::from(tick / 4 * height));
        input(&mut sim, &mut world, &mut player, false);
    }
    if let Some(path) = std::env::var_os("FERRUM_PLACEMENT_CAPTURE") { sheet.save(path).unwrap(); }
    std::fs::remove_file(path).unwrap();
}
