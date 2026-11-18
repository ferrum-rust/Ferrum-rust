use super::*;
use crate::{player::Player, vanilla::ray_trace::Kind, world::Chunk};
use serde::Deserialize;
use std::sync::{Arc, LazyLock};

#[derive(Deserialize)]
pub(super) struct Reference {
    placements: Vec<(u8, f32, bool, u16, u8)>,
    pub(super) boxes: Vec<Vec<[f32; 5]>>,
    pub(super) rgba: Vec<u8>,
}
pub(super) static ORIGINAL: LazyLock<Reference> = LazyLock::new(|| {
    serde_json::from_reader(flate2::read::GzDecoder::new(
        &include_bytes!("../tests/fixtures/vanilla189-banner.json.gz")[..],
    ))
    .unwrap()
});
pub(super) fn stack() -> Stack {
    let patterns = [("gra", 1), ("cre", 12), ("bo", 15)].map(|(p, c)| {
        DropNbt::Compound(Tag::from([
            ("Pattern".into(), DropNbt::String(p.into())),
            ("Color".into(), DropNbt::Int(c)),
        ]))
    });
    let tag = DropNbt::Compound(Tag::from([
        ("Base".into(), DropNbt::Short(11)),
        (
            "Patterns".into(),
            DropNbt::List {
                element: 10,
                values: patterns.to_vec(),
            },
        ),
    ]));
    let mut stack = Stack::new(425, 2);
    stack.meta = 4;
    stack.with_nbt(DropNbt::Compound(Tag::from([(
        "BlockEntityTag".into(),
        tag,
    )])))
}
fn world() -> World {
    World {
        chunks: [((0, 0), Arc::new(Chunk::empty()))].into(),
        spawn: Vec3::new(8.5, 63., 6.5),
        label: "Banner test".into(),
        save_path: Default::default(),
        edits: Default::default(),
        imported: false,
        light_revision: 0,
    }
}
#[test]
fn original_banner_faces_and_rotations_survive_restore_and_harvest() {
    let p = IVec3::new(8, 64, 8);
    let stack = stack();
    for &(face, yaw, accepted, state, count) in &ORIGINAL.placements {
        let mut w = world();
        w.set(p, 16);
        let mut s = Simulation::default();
        s.inventory.slots[0] = Some(stack.clone());
        let mut out = Outcome::default();
        let hit = Hit {
            kind: Kind::Block,
            position: p,
            point: p.as_dvec3(),
            face,
        };
        let mut player = Player::new(w.spawn);
        player.set_pose_f64(w.spawn.as_dvec3().to_array(), yaw, 0., true);
        s.place_held_block(&mut w, &player, &stack, hit, &mut out);
        let at = hit.adjacent();
        assert_eq!(
            (
                !out.changed.is_empty(),
                w.get(at),
                s.inventory.held().unwrap().count
            ),
            (accepted, state, count),
            "{face}/{yaw}"
        );
        if !accepted {
            continue;
        }
        let saved: Simulation = serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
        Arc::make_mut(w.chunks.get_mut(&(0, 0)).unwrap())
            .tiles
            .clear();
        saved.world_commands.restore_tiles(&mut w, &[(0, 0)]);
        let written = w.tile(at).and_then(item_nbt::compound).unwrap();
        assert_eq!(written["Base"], DropNbt::Int(11));
        assert_eq!(written["Patterns"], item_tag(&stack).unwrap()["Patterns"]);
        if face == 1 {
            s.destroy_picked_block(&mut w, at, &Default::default(), &mut out);
        } else {
            w.set(p, 0);
            out.changed.push(p);
            s.check_banner_support(&mut w, p, &mut out);
        }
        assert_eq!(w.get(at), 0);
        let drop = &s.drops[0].stack;
        assert_eq!((drop.id, drop.meta, drop.count), (425, 11, 1));
        assert_eq!(
            item_tag(drop).unwrap()["Patterns"],
            item_tag(&stack).unwrap()["Patterns"]
        );
    }
}
#[test]
#[ignore = "requires installed resources to run the normal singleplayer right-click path"]
fn right_click_places_banner_and_rejects_missing_support_or_obstructed_target() {
    let mut w = world();
    let p = IVec3::new(8, 64, 8);
    w.set(p, 16);
    let mut player = Player::new(w.spawn);
    player.set_pose_f64(w.spawn.as_dvec3().to_array(), 0., 0., true);
    let mut s = Simulation::default();
    s.mode = Mode::Creative;
    let banner = stack();
    s.inventory.slots[0] = Some(banner.clone());
    let result = s.tick(
        &mut w,
        &mut player,
        crate::validation::resources(),
        &Default::default(),
        false,
        true,
    );
    assert!(result.changed.contains(&(p - IVec3::Z)));
    assert_eq!(w.get(p - IVec3::Z), 177 << 4 | 2);
    assert_eq!(s.inventory.held().unwrap().count, 2);
    let hit = Hit {
        kind: Kind::Block,
        position: p,
        face: 1,
        point: p.as_dvec3(),
    };
    for (support, target, neighbor) in [(31 << 4, 0, 0), (16, 16, 0), (16, 0, 81 << 4)] {
        w.set(p, support);
        w.set(p + IVec3::Y, target);
        w.set(p + IVec3::Y + IVec3::X, neighbor);
        let mut out = Outcome::default();
        s.place_held_block(&mut w, &player, &banner, hit, &mut out);
        assert!(out.changed.is_empty());
    }
}
