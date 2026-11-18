use super::*;
use crate::{inventory::Stack, vanilla::movement::Aabb};
use serde_json::Value;

#[test]
fn fuse_motion_exposure_and_ordered_blast_blocks_match_installed_jar() {
    let data: Value = serde_json::from_reader(flate2::read::GzDecoder::new(
        &include_bytes!("../tests/fixtures/vanilla189-tnt.json.gz")[..],
    ))
    .unwrap();
    let mut world = super::super::tests::flat_world();
    for x in 0..16 {
        for z in 0..16 {
            world.set(IVec3::new(x, 5, z), 4 << 4);
        }
    }
    let mut t = tnt::Primed::new(
        DVec3::new(8.5, 6., 8.5),
        DVec3::new(0.0125, f64::from(0.2f32), -0.013),
        80,
        false,
    );
    for row in data["motion"].as_array().unwrap() {
        let blast = t.tick(&world);
        for (i, value) in t
            .position
            .to_array()
            .into_iter()
            .chain(t.velocity.to_array())
            .enumerate()
        {
            assert_eq!(
                value,
                row[i].as_f64().unwrap(),
                "tick {} coordinate {i}",
                80 - t.fuse
            );
        }
        assert_eq!(t.fuse, i32::try_from(row[6].as_i64().unwrap()).unwrap());
        assert_eq!(blast.is_some(), row[7].as_bool().unwrap());
    }
    for (i, row) in data["blasts"].as_array().unwrap().iter().enumerate() {
        let state = (row[0].as_u64().unwrap() as u16) << 4;
        for x in 4..13 {
            for z in 4..13 {
                world.set(IVec3::new(x, 17, z), state);
                world.set(IVec3::new(x, 16, z), 1 << 4);
                world.set(IVec3::new(x, 18, z), if x == 9 { state } else { 0 });
            }
        }
        let center = DVec3::new(8.25, 18.06125, 8.5);
        let positions = tnt::footprint(&world, center, 4., &mut JavaRandom::new(4189 + i as i64));
        let expected: Vec<[i32; 3]> = serde_json::from_value(row[1].clone()).unwrap();
        let got: Vec<_> = positions.iter().map(|p| p.to_array()).collect();
        assert_eq!(
            got.len(),
            expected.len(),
            "footprint size for {}",
            state >> 4
        );
        assert!(
            got.iter().zip(&expected).all(|(a, b)| a == b),
            "affected block order for {}",
            state >> 4
        );
        assert_eq!(
            tnt::exposure(
                &world,
                center,
                Aabb {
                    min: DVec3::new(10., 17., 8.),
                    max: DVec3::new(10.6, 18.8, 8.6)
                }
            ),
            row[2].as_f64().unwrap() as f32
        );
    }
}

#[test]
#[ignore = "uses the installed Minecraft resources for a full singleplayer tick"]
fn real_right_click_redstone_save_and_chain_detonate_in_singleplayer() {
    let resources = crate::resources::Resources::load().unwrap();
    let settings = Settings::default();
    let mut world = super::super::tests::flat_world();
    let p = IVec3::new(8, 6, 10);
    let mut player = Player::new(glam::Vec3::new(8.5, 5., 8.5));
    player.set_rotation_degrees(0., 0.);
    world.set(p, 46 << 4);
    for x in 3..14 {
        for z in 3..16 {
            world.set(IVec3::new(x, 5, z), 7 << 4);
        }
    }
    let mut sim = Simulation {
        mob_spawning: false,
        ..Default::default()
    };
    sim.inventory.slots[0] = Some(Stack::new(259, 1));
    let out = sim.tick(&mut world, &mut player, &resources, &settings, false, true);
    assert!(out
        .world_sounds
        .iter()
        .any(|&(clip, _, vol, pitch)| clip == "random.fuse" && vol == 1. && pitch == 1.));
    assert_eq!(sim.inventory.held().unwrap().damage, 1);
    assert_eq!(world.get(p), 0);
    assert_eq!(sim.primed_tnt.len(), 1);
    let adjacent = p + IVec3::X;
    world.set(adjacent, 46 << 4);
    world.set(p + IVec3::Y, 20 << 4);
    world.set(p + IVec3::Z, 49 << 4);
    for _ in 0..25 {
        sim.tick_tnt(&mut world, &mut player, &settings, &mut Default::default());
    }
    let mut resumed: Simulation =
        serde_json::from_slice(&serde_json::to_vec(&sim).unwrap()).unwrap();
    assert_eq!(resumed.primed_tnt[0].fuse, sim.primed_tnt[0].fuse);
    let remaining = resumed.primed_tnt[0].fuse;
    for _ in 0..=remaining {
        resumed.tick_tnt(&mut world, &mut player, &settings, &mut Default::default());
    }
    assert_eq!(world.get(adjacent), 0);
    assert_eq!(world.get(p + IVec3::Y), 0);
    assert_eq!(world.get(p + IVec3::Z), 49 << 4);
    assert_eq!(world.get(p - IVec3::Y), 7 << 4);
    assert!(resumed.health < 20.);
    assert_eq!(resumed.primed_tnt.len(), 1);
    assert!((9..=28).contains(&resumed.primed_tnt[0].fuse));
    for _ in 0..30 {
        resumed.tick_tnt(&mut world, &mut player, &settings, &mut Default::default());
    }
    assert!(resumed.primed_tnt.is_empty());
    world.set(p, 46 << 4);
    world.set(p + IVec3::X, 152 << 4);
    resumed.update_circuits(
        &mut world,
        &mut player,
        vec![p + IVec3::X],
        &mut Default::default(),
    );
    assert_eq!(world.get(p), 0);
    assert_eq!(resumed.primed_tnt[0].fuse, 80);
}
