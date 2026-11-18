use super::*;
use crate::inventory::{ClickAction, SlotRef};

#[test]
#[ignore = "scripts/test_vanilla_loopback.py --plugin-probe uses a disposable original Java server"]
fn installed_189_plugin_chest_reject_resync_and_accept() {
    let port = std::env::var("MC189_LOOPBACK_PORT").expect("original server port");
    let settings = crate::settings::Settings {
        render_distance: 2,
        ..Default::default()
    };
    let client = Client::connect(
        &format!("127.0.0.1:{port}"),
        "PluginProbe",
        (&settings).into(),
    )
    .unwrap();
    let mut remote = Remote::with_client(client);
    let mut sim = Simulation::default();
    let mut player = Player::new(Vec3::new(0., 64., 0.));
    let mut world = World {
        chunks: Default::default(),
        spawn: Vec3::ZERO,
        label: String::new(),
        save_path: Default::default(),
        edits: Default::default(),
        imported: true,
        light_revision: 0,
    };
    let mut dirty = HashSet::new();
    let mut phase = 0;
    let named = |s: Option<&Stack>, id: u16, count: u8, label: &str| {
        s.is_some_and(|s| {
            s.id == id && s.count == count && crate::vanilla::anvil::custom_name(s) == Some(label)
        })
    };
    let started = std::time::Instant::now();
    loop {
        remote
            .poll_world(&mut world, &mut player, &mut sim, &mut dirty, None)
            .unwrap();
        let marker = |tag: &str| remote.chat_lines.iter().any(|(s, _)| s.contains(tag));
        let confirmed = marker("plugin-original-rejected-and-confirmed");
        if marker("plugin-original-next-click-accepted") {
            assert_eq!(phase, 2);
            assert!(named(sim.inventory.cursor.as_ref(), 339, 1, "§b다음 메뉴"));
            let w = remote.extra.window.as_ref().unwrap();
            assert!(w.container.slots[0].is_none());
            assert!(named(w.container.slots[1].as_ref(), 388, 3, "§a서버 선택"));
            assert!(!remote.inventory_blocked);
            return;
        }
        if let Some(w) = remote.extra.window.as_ref() {
            assert!(w.title.contains("§6서버 메뉴"), "{}", w.title);
            assert_eq!(w.screen(), crate::ui::Screen::Chest);
            if phase == 0 && named(w.container.slots[0].as_ref(), 264, 1, "§d선택하기") {
                assert!(named(w.container.slots[1].as_ref(), 339, 1, "§b다음 메뉴"));
                remote
                    .window_click(
                        SlotRef::Container(0),
                        ClickAction::Pickup { right: false },
                        &mut sim,
                    )
                    .unwrap();
                assert!(
                    named(sim.inventory.cursor.as_ref(), 264, 1, "§d선택하기"),
                    "native predicted cursor"
                );
                phase = 1;
            } else if phase == 1
                && confirmed
                && named(sim.inventory.cursor.as_ref(), 388, 3, "§a서버 선택")
            {
                assert!(!remote.inventory_blocked);
                assert!(w.slots[0].is_none());
                assert!(named(w.container.slots[1].as_ref(), 339, 1, "§b다음 메뉴"));
                let cursor = remote.cursor.as_ref().map(from_slot).transpose().unwrap();
                assert!(named(cursor.as_ref(), 388, 3, "§a서버 선택"));
                remote
                    .window_click(
                        SlotRef::Container(1),
                        ClickAction::Pickup { right: false },
                        &mut sim,
                    )
                    .unwrap();
                phase = 2;
            }
        }
        assert!(started.elapsed()<std::time::Duration::from_secs(38),"Plugin menu timed out: phase={phase} confirmed={confirmed} blocked={} notice={:?} lines={:?}",
            remote.inventory_blocked,remote.notice,remote.chat_lines);
        std::thread::sleep(std::time::Duration::from_millis(12));
    }
}
#[test]
fn confirm_only_rejections_for_inventory_or_live_window() {
    let (client, commands) = Client::window_test_transport();
    let mut remote = Remote::with_client(client);
    let mut sim = Simulation::default();
    // A delayed response for an unopened container (including an ignored horse GUI).
    remote.confirm_transaction(4, -3, false).unwrap();
    assert!(commands.try_recv().is_err());
    assert!(!remote.inventory_blocked);
    remote.open_window(4, "minecraft:chest".into(), "{}".into(), 9, &mut sim);
    remote.confirm_transaction(4, 123, true).unwrap();
    remote.confirm_transaction(7, 124, false).unwrap();
    assert!(commands.try_recv().is_err());
    assert!(!remote.inventory_blocked);
    // The personal inventory is valid even while a chest is open. Its rejection does not block the chest.
    remote.confirm_transaction(0, i16::MIN, false).unwrap();
    assert!(matches!(
        commands.try_recv().unwrap(),
        Command::ConfirmTransaction {
            window_id: 0,
            action: i16::MIN
        }
    ));
    assert!(!remote.inventory_blocked);
    remote.confirm_transaction(4, 55, false).unwrap();
    assert!(matches!(
        commands.try_recv().unwrap(),
        Command::ConfirmTransaction {
            window_id: 4,
            action: 55
        }
    ));
    assert!(remote.inventory_blocked);
    remote.reset_window(&mut sim).unwrap();
    remote.confirm_transaction(4, 56, false).unwrap();
    assert!(commands.try_recv().is_err());
}
