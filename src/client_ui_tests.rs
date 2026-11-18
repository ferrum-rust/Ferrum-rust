use super::*;
use crate::keybindings::{Binding, ControlAction};
use clap::Parser;

fn layout(app: &mut App) {
    app.ui_frame.set_viewport([1600, 900], &app.settings);
    ui::paint(
        &mut app.ui_frame,
        &app.gui,
        &app.settings,
        &app.resources,
        &app.assets,
        app.game.as_ref().map(|g| &g.sim),
        &Stats::default(),
    );
}

#[test]
#[ignore = "loads installed assets and a disposable singleplayer game"]
fn in_world_options_draws_the_playing_hotbar_under_the_pause_gradient() {
    let (mut app, dir) = isolated_app();
    let mut options = app.options.clone();
    options.project_dir = dir.clone();
    let mut game = Game::load(options, &app.assets, &app.settings).unwrap();
    game.sim.inventory.slots[4] = Some(crate::inventory::Stack::new(322, 3));
    game.sim.inventory.selected = 4;
    let mut chunk = Chunk::empty();
    for x in 0..16 {
        for y in 62..75 {
            chunk.set(x, y, 12, if (x+y)%2 == 0 { 4 << 4 } else { 89 << 4 });
        }
    }
    game.world.chunks.clear();
    game.world.chunks.insert((0,0), Arc::new(chunk));
    game.player.set_pose_f64([8.5, 66., 6.5], 0., 0., true);
    let mut renderer = pollster::block_on(Renderer::new(None, &app.assets, &app.resources, &app.settings, None)).unwrap();
    renderer.upload((0,0), mesh::build_with_settings(&game.world, (0,0), &app.assets, &app.settings));
    let capture = |app: &App, renderer: &mut Renderer, name: &str| {
        let game = app.game.as_ref().unwrap();
        let path = dir.join(name);
        renderer.screenshot(&path, &RenderInput {
            selected_block: None, assets: &app.assets, resources: &app.resources, settings: &app.settings,
            gui: &app.gui, ui: &app.ui_frame, world: Some(&game.world), player: Some(&game.player),
            simulation: Some(&game.sim), alpha: 1., time: 0.,
        }).unwrap();
        image::open(path).unwrap().into_rgb8()
    };
    app.game = Some(game);
    app.open(Screen::Playing);
    layout(&mut app);
    let playing = capture(&app, &mut renderer, "playing.png");
    let hud = bytemuck::cast_slice::<_, u8>(&app.ui_frame.vertices).to_vec();
    assert!(app.ui_frame.vertices.len() >= 100);
    app.key(KeyCode::Escape, true, false).unwrap();
    layout(&mut app);
    assert_eq!(app.gui.screen, Screen::Pause);
    let start = hud.len() / std::mem::size_of::<ui::UiVertex>();
    let dim = bytemuck::cast_slice::<_, u8>(&app.ui_frame.vertices[start..start+6]).to_vec();
    let colors = app.ui_frame.vertices[start..start+6].iter().map(|v| v.color).collect::<Vec<_>>();
    assert!(colors.contains(&[16./255.,16./255.,16./255.,192./255.]));
    assert!(colors.contains(&[16./255.,16./255.,16./255.,208./255.]));
    click(&mut app, |action| matches!(action, Action::Screen(Screen::Options)));
    assert_eq!(app.gui.screen, Screen::Options);
    layout(&mut app);
    assert_eq!(bytemuck::cast_slice::<_, u8>(&app.ui_frame.vertices[..start]), hud);
    assert_eq!(bytemuck::cast_slice::<_, u8>(&app.ui_frame.vertices[start..start+6]), dim);
    let options = capture(&app, &mut renderer, "options.png");
    let mut sharp_edges = 0;
    // Test individual pixels across textured edges, and across the item and selection in the hotbar.
    for (xs, ys) in [(30..360, 250..480), (615..985, 862..896)] {
        for y in ys {
            let opacity = (192. + 16. * (y as f32 + 0.5) / playing.height() as f32) / 255.;
            for x in xs.clone() {
                let src = playing.get_pixel(x,y);
                let actual = options.get_pixel(x,y);
                if src.0[0].abs_diff(playing.get_pixel(x+1,y).0[0]) > 24 { sharp_edges += 1; }
                for (a,b) in src.0.into_iter().zip(actual.0) {
                    let expected = (a as f32 * (1.-opacity) + 16.*opacity).round() as u8;
                    assert!(b.abs_diff(expected) <= 2, "world or hotbar blurred at ({x},{y}): {b} vs {expected}");
                }
            }
        }
    }
    assert!(sharp_edges > 500, "fixture must include visible edges");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
#[ignore = "loads the installed 1.8.9 player skin and inventory resources"]
fn survival_inventory_turns_the_player_toward_the_previous_frame_cursor() {
    let (mut app, dir) = isolated_app();
    app.options.username = "Menu189".into();
    let mut options = app.options.clone();
    options.project_dir = dir.clone();
    app.game = Some(Game::load(options, &app.assets, &app.settings).unwrap());
    app.open(Screen::Playing);
    app.keyboard_input(PhysicalKey::Code(KeyCode::KeyE), true, false, None).unwrap();
    assert_eq!(app.gui.screen, Screen::Inventory);
    assert!(app.gui.creative.slim);
    let skin = app.resources.sprite("textures/entity/alex.png").layer;
    let player_vertices = |app: &App| app.ui_frame.vertices.iter()
        .filter(|v| v.layer == skin).map(|v| v.position).collect::<Vec<_>>();
    let output = std::env::var_os("FERRUM_INVENTORY_PREVIEW_CAPTURE").map(PathBuf::from);
    let mut renderer = output.as_ref().map(|_| {
        pollster::block_on(Renderer::new(None, &app.assets, &app.resources, &app.settings, None)).unwrap()
    });
    layout(&mut app);
    let mut last = player_vertices(&app);
    assert!(last.len() > 100, "expected a textured 3D player");
    for (name, cursor) in [("center",[363.,167.]), ("left",[203.,167.]), ("right",[523.,167.]),
                           ("up",[363.,47.]), ("down",[363.,287.])] {
        app.gui.cursor = cursor;
        layout(&mut app);
        assert_eq!(player_vertices(&app), last);
        layout(&mut app);
        let current = player_vertices(&app);
        assert_ne!(current, last, "cursor moved {name}");
        last = current;
        if let (Some(renderer), Some(output)) = (&mut renderer, &output) {
            let game = app.game.as_ref().unwrap();
            renderer.screenshot(&output.join(format!("{name}.png")), &RenderInput {
                selected_block: None,
                assets: &app.assets, resources: &app.resources, settings: &app.settings, gui: &app.gui,
                ui: &app.ui_frame, world: None, player: None, simulation: Some(&game.sim), alpha: 1., time: 0.,
            }).unwrap();
        }
    }
    app.open(Screen::Crafting);
    layout(&mut app);
    assert!(player_vertices(&app).is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
#[ignore = "loads local resources for the playing screen and chest interaction"]
fn torch_outline_matches_clicks_and_opening_the_chest_behind_it() {
    let (mut app, dir) = isolated_app();
    let mut options = app.options.clone();
    options.project_dir = dir.clone();
    let mut game = Game::load(options, &app.assets, &app.settings).unwrap();
    game.world.chunks.clear();
    game.world.chunks.insert((0, 0), Arc::new(Chunk::empty()));
    game.sim.mobs.clear();
    let torch = IVec3::new(8, 65, 8);
    let chest = torch + IVec3::Z;
    game.world.set(torch - IVec3::Y, 16);
    game.world.set(torch, 50 << 4 | 5);
    game.world.set(chest, 54 << 4 | 2);
    let y = 65.3 - f64::from(1.62_f32);
    game.player.set_pose_f64([8.5, y, 6.5], 0., 0., true);
    app.game = Some(game);
    app.open(Screen::Playing);
    let game = app.game.as_mut().unwrap();
    assert_eq!(game.selected_block(&app.gui, &game.player, 1.), Some(torch));
    game.player.set_pose_f64([8.8, y, 6.5], 0., 0., true);
    game.player.previous.x = 8.5;
    assert_eq!(game.selected_block(&app.gui, &game.player, 0.), Some(torch));
    assert_eq!(game.selected_block(&app.gui, &game.player, 1.), Some(chest));
    app.bound_input(Binding::Use, true).unwrap();
    assert_eq!((app.gui.screen, app.gui.container), (Screen::Chest, Some(chest)));
    app.open(Screen::Playing);
    let game = app.game.as_mut().unwrap();
    game.player.set_pose_f64([8.8, y, 4.5], 0., 0., true);
    assert_eq!(game.selected_block(&app.gui, &game.player, 1.), None);
    game.sim.mode = Mode::Creative;
    assert_eq!(game.selected_block(&app.gui, &game.player, 1.), Some(chest));
    app.gui.hide_hud = true;
    assert_eq!(game.selected_block(&app.gui, &game.player, 1.), None);
    app.gui.hide_hud = false;
    app.bound_input(Binding::Attack, true).unwrap();
    let game = app.game.as_mut().unwrap();
    game.sim.tick(&mut game.world, &mut game.player, &app.resources, &app.settings, true, false);
    assert_eq!(game.world.get(torch), 50 << 4 | 5);
    assert_eq!(game.world.get(chest), 0);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
#[ignore = "requires original local 1.8.9 resources"]
fn survival_right_click_places_a_door_then_opens_and_fills_a_hopper() {
    let (mut app, dir) = isolated_app();
    let mut options = app.options.clone();
    options.project_dir = dir.clone();
    let mut game = Game::load(options, &app.assets, &app.settings).unwrap();
    game.world.chunks.clear();
    game.world.chunks.insert((0, 0), Arc::new(Chunk::empty()));
    game.sim.mobs.clear();
    game.sim.mode = Mode::Survival;
    game.sim.inventory.selected = 0;
    game.sim.inventory.slots[0] = Some(crate::inventory::Stack::new(324, 1));
    let p = IVec3::new(8, 64, 8);
    game.world.set(p - IVec3::Y, 16);
    game.player.set_pose_f64([8.5, 64., 8.5], 0., 90., true);
    app.game = Some(game);
    app.open(Screen::Playing);
    app.bound_input(Binding::Use, true).unwrap();
    let game = app.game.as_mut().unwrap();
    game.sim.tick(&mut game.world, &mut game.player, &app.resources, &app.settings, false, app.right);
    assert_eq!((game.world.get(p),game.world.get(p + IVec3::Y)), (64 << 4 | 1,64 << 4 | 8));
    assert!(game.sim.inventory.held().is_none());
    let hopper = p + IVec3::X * 3;
    game.world.set(hopper,154 << 4);
    game.world.set(hopper - IVec3::Y,54 << 4 | 2);
    game.player.set_pose_f64([11.8,63.,6.5],0.,0.,true);
    app.bound_input(Binding::Use, false).unwrap();
    app.bound_input(Binding::Use, true).unwrap();
    assert_eq!((app.gui.screen, app.gui.container), (Screen::Chest, Some(hopper)));
    let game = app.game.as_mut().unwrap();
    game.sim.inventory.cursor = Some(crate::inventory::Stack::new(264, 3));
    app.window_click(SlotRef::Container(4), ClickAction::Pickup { right: false }).unwrap();
    let game = app.game.as_mut().unwrap();
    game.sim.inventory.slots[10] = Some(crate::inventory::Stack::new(260, 4));
    app.window_click(SlotRef::Player(10), ClickAction::QuickMove).unwrap();
    let game = app.game.as_mut().unwrap();
    assert_eq!(game.sim.container_view(&game.world, hopper).unwrap().slots[0], Some(crate::inventory::Stack::new(260, 4)));
    app.window_click(SlotRef::Container(0), ClickAction::QuickMove).unwrap();
    let game = app.game.as_mut().unwrap();
    game.sim.tick(&mut game.world, &mut game.player, &app.resources, &app.settings, false, false);
    let open = game.sim.container_view(&game.world,hopper).unwrap();
    assert_eq!((open.special,open.slots.len(),open.slots[4].as_ref().unwrap().count), (crate::container::Special::Hopper,5,2));
    assert_eq!(game.sim.container_view(&game.world,hopper-IVec3::Y).unwrap().slots[0], Some(crate::inventory::Stack::new(264,1)));
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
#[ignore = "requires original local 1.8.9 resources"]
fn chat_hud_and_spectator_wheel_preserve_real_slots() {
    let (mut app, dir) = isolated_app();
    let mut options = app.options.clone();
    options.project_dir = dir.clone();
    options.world_rules.allow_cheats = true;
    let mut game = Game::load(options, &app.assets, &app.settings).unwrap();
    game.sim.inventory.selected = 4;
    game.sim.inventory.slots[4] = Some(crate::inventory::Stack::new(326, 1));
    game.sim.inventory.slots[5] = Some(crate::inventory::Stack::new(1, 3));
    let initial = game.sim.inventory.slots.clone();
    app.game = Some(game);
    app.open(Screen::Playing);

    layout(&mut app);
    let hud_vertices = |app: &App| {
        let layers = ["textures/gui/widgets.png", "textures/gui/icons.png"]
            .map(|s| app.resources.sprite(s).layer);
        app.ui_frame.vertices.iter().filter(|v| layers.contains(&v.layer))
            .map(|v| (v.layer, v.uv, v.position)).collect::<Vec<_>>()
    };
    let original = hud_vertices(&app);
    assert!(original.len() > 12);
    let captures = std::env::var_os("FERRUM_CHAT_HUD_CAPTURE").map(PathBuf::from).unwrap_or_else(|| dir.clone());
    std::fs::create_dir_all(&captures).unwrap();
    let mut renderer = pollster::block_on(Renderer::new(None, &app.assets, &app.resources, &app.settings, None)).unwrap();
    for (pos, mesh) in app.game.as_mut().unwrap().uploads.drain(..) {
        renderer.upload(pos, mesh);
    }
    let render_hud = |renderer: &mut Renderer, app: &App, name: &str| {
        let game = app.game.as_ref().unwrap();
        let path = captures.join(name);
        renderer.screenshot(&path, &RenderInput {
            selected_block: None, assets: &app.assets, resources: &app.resources,
            settings: &app.settings, gui: &app.gui, ui: &app.ui_frame,
            world: Some(&game.world), player: Some(&game.player), simulation: Some(&game.sim), alpha: 1., time: 0.,
        }).unwrap();
        image::open(path).unwrap().to_rgba8()
    };
    let playing = render_hud(&mut renderer, &app, "playing.png");
    app.key(KeyCode::KeyT, true, false).unwrap();
    assert_eq!(app.gui.screen, Screen::Chat);
    app.text("/gamemode spectator");
    app.focus_lost();
    app.world_wheel(-1.);
    assert_eq!(app.game.as_ref().unwrap().sim.inventory.selected, 4);
    assert_eq!(app.gui.chat.input, "/gamemode spectator");
    layout(&mut app);
    assert_eq!(hud_vertices(&app), original);
    let chat = render_hud(&mut renderer, &app, "chat.png");
    let pixels = |img: &image::RgbaImage, x, y, w, h| image::imageops::crop_imm(img, x, y, w, h).to_image();
    // Scale 2. The top of the hotbar is fully visible. The text input overlays its lower edge.
    assert_eq!(pixels(&playing, 618, 856, 364, 16), pixels(&chat, 618, 856, 364, 16));
    assert_ne!(pixels(&playing, 4, 872, 320, 24), pixels(&chat, 4, 872, 320, 24));
    app.key(KeyCode::Enter, true, false).unwrap();
    assert_eq!(app.gui.screen, Screen::Playing);
    assert!(app.gui.spectator.spectator);
    layout(&mut app);
    assert!(hud_vertices(&app).is_empty());
    let spectator = render_hud(&mut renderer, &app, "spectator.png");
    assert_ne!(pixels(&playing, 618, 856, 364, 16), pixels(&spectator, 618, 856, 364, 16));

    let initial_speed = app.game.as_ref().unwrap().player.fly_speed;
    app.world_wheel(120.);
    assert_eq!(app.game.as_ref().unwrap().player.fly_speed, initial_speed + 0.005);
    for _ in 0..50 { app.world_wheel(1.); }
    assert_eq!(app.game.as_ref().unwrap().player.fly_speed, 0.2);
    for _ in 0..50 { app.world_wheel(-1.); }
    assert_eq!(app.game.as_ref().unwrap().player.fly_speed, 0.);
    app.settings.bindings.set(Binding::Pick, 0);
    app.mouse_button(MouseButton::Middle, true).unwrap();
    assert!(app.gui.spectator.active());
    app.world_wheel(-1.); // In an empty local world the first enabled entry is Close.
    assert_eq!(app.game.as_ref().unwrap().player.fly_speed, 0.);
    app.mouse_button(MouseButton::Middle, true).unwrap();
    assert!(!app.gui.spectator.active());
    app.key(KeyCode::Digit9, true, false).unwrap();
    app.key(KeyCode::KeyQ, true, false).unwrap();
    assert!(app.gui.spectator.active());
    app.key(KeyCode::KeyE, true, false).unwrap();
    assert_eq!(app.gui.screen, Screen::Inventory);
    app.window_click(SlotRef::Player(4), ClickAction::Hotbar(5)).unwrap();
    app.key(KeyCode::Escape, true, false).unwrap();
    let game = app.game.as_ref().unwrap();
    assert_eq!(game.sim.inventory.selected, 4);
    assert_eq!(game.sim.inventory.slots, initial);

    app.send_chat_message("/gamemode survival".into()).unwrap();
    assert!(!app.gui.spectator.active());
    app.world_wheel(-1.);
    assert_eq!(app.game.as_ref().unwrap().sim.inventory.selected, 5);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
#[ignore = "requires original local 1.8.9 resources"]
fn remote_spectator_wheel_selects_teleport_and_keeps_server_held_slot() {
    use crate::network::{Client, Command, Event, PlayerInfo, Pose};
    let (mut app, dir) = isolated_app();
    let mut options = app.options.clone();
    options.project_dir = dir.clone();
    let mut game = Game::load(options, &app.assets, &app.settings).unwrap();
    let mut renderer = pollster::block_on(Renderer::new(None, &app.assets, &app.resources, &app.settings, None)).unwrap();
    let (client, outgoing, events) = Client::test_transport();
    game.remote = Some(crate::remote::Remote::with_client(client));
    let [x,y,z] = game.player.precise_position();
    let target = [5_u8; 16];
    for event in [
        Event::GameState { reason: 3, value: 3. },
        Event::HeldSlot(2),
        Event::Position { pose: Pose { x,y,z, ..Default::default() }, correction_id: 1, relative: 0 },
        Event::PlayerList { action: 0, players: vec![PlayerInfo {
            uuid: target, name: Some("Target".into()), properties: vec![], mode: Some(0),
            latency: None, display_name: None,
        }] },
    ] { events.send(event).unwrap(); }
    let poll = |g: &mut Game, renderer: &mut Renderer| {
        g.remote.as_mut().unwrap().poll(&mut g.world, &mut g.player, &mut g.sim, &mut g.dirty, renderer).unwrap();
    };
    poll(&mut game, &mut renderer);
    assert!(game.player.is_spectator());
    app.game = Some(game);
    app.open(Screen::Playing);
    app.world_wheel(1.);
    let speed = app.game.as_ref().unwrap().player.fly_speed;
    assert_eq!(speed, 0.05_f32 + 0.005);
    app.key(KeyCode::Digit1, true, false).unwrap(); // open
    app.world_wheel(-1.); // highlight Teleport to player
    app.mouse_button(MouseButton::Middle, true).unwrap(); // enter group
    app.world_wheel(-1.); // highlight Target
    app.mouse_button(MouseButton::Middle, true).unwrap();
    assert!(matches!(outgoing.try_recv().unwrap(), Command::Spectate(uuid) if uuid == target));
    let game = app.game.as_mut().unwrap();
    game.remote.as_mut().unwrap().tick(&mut game.world, &mut game.player, &mut game.sim, &app.resources, false, false).unwrap();
    let commands = outgoing.try_iter().collect::<Vec<_>>();
    assert!(commands.iter().any(|c| matches!(c, Command::Movement {..})));
    assert!(!commands.iter().any(|c| matches!(c, Command::HeldSlot(_))));
    assert_eq!((game.sim.inventory.selected, game.player.fly_speed), (2, speed));
    events.send(Event::GameState { reason: 3, value: 0. }).unwrap();
    poll(game, &mut renderer);
    assert!(!game.player.is_spectator());
    assert_eq!(game.sim.inventory.selected, 2);
    app.world_wheel(-1.);
    let game = app.game.as_mut().unwrap();
    game.remote.as_mut().unwrap().tick(&mut game.world, &mut game.player, &mut game.sim, &app.resources, false, false).unwrap();
    assert!(outgoing.try_iter().any(|c| matches!(c, Command::HeldSlot(3))));
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
#[ignore = "requires original local 1.8.9 resources"]
fn chat_input_runs_local_commands_and_activates_server_suggestions() {
    let (mut app,dir)=isolated_app();
    let mut options=app.options.clone();options.project_dir=dir.clone();options.world_rules.allow_cheats=true;
    app.game=Some(Game::load(options,&app.assets,&app.settings).unwrap());
    app.open(Screen::Playing);
    app.open_chat(true);app.text("gamem");app.key(KeyCode::Tab,true,false).unwrap();
    assert_eq!(app.gui.chat.input,"/gamemode");
    app.text(" c");app.key(KeyCode::Enter,true,false).unwrap();
    let game=app.game.as_ref().unwrap();
    assert_eq!(game.sim.mode,Mode::Creative);assert!(game.player.allow_flying);
    app.open_chat(false);
    app.gui.chat.receive(r#"{"text":"밤으로","clickEvent":{"action":"suggest_command","value":"/time set night"}}"#,1,&app.resources,&app.settings.language);
    click(&mut app,|a|matches!(a,Action::ChatEvent(_)));
    assert_eq!(app.gui.chat.input,"/time set night");
    app.key(KeyCode::Enter,true,false).unwrap();
    assert_eq!(app.game.as_ref().unwrap().sim.clock,13000);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
#[ignore = "requires installed original resources"]
fn escape_opens_a_reachable_lan_world_and_grants_session_cheats() {
    use ui::pause::Command as Lan;
    let (mut app,dir)=isolated_app();
    let mut options=app.options.clone();
    options.project_dir=dir.clone();
    app.game=Some(Game::load(options,&app.assets,&app.settings).unwrap());
    app.open(Screen::Playing);
    app.key(KeyCode::Escape,true,false).unwrap();
    assert_eq!(app.gui.screen,Screen::Pause);
    click(&mut app,|a|matches!(a,Action::Screen(Screen::ShareLan)));
    click(&mut app,|a|matches!(a,Action::Lan(Lan::Mode)));
    click(&mut app,|a|matches!(a,Action::Lan(Lan::Mode)));
    click(&mut app,|a|matches!(a,Action::Lan(Lan::Cheats)));
    assert_eq!((app.gui.lan_form.mode,app.gui.lan_form.cheats),(1,true));
    click(&mut app,|a|matches!(a,Action::Lan(Lan::Start)));
    assert_eq!(app.gui.screen,Screen::Playing);
    let game=app.game.as_ref().unwrap();
    let host=game.sim.lan.as_ref().unwrap();
    let addr=format!("127.0.0.1:{}",host.port);
    let status=crate::network::status(&addr,Duration::from_secs(4)).unwrap();
    assert_eq!(status.json["version"]["protocol"],47);
    assert_eq!(status.json["description"]["text"],game.world.label);
    assert_eq!(game.sim.mode,Mode::Survival,"chosen mode applies to incoming guests");
    assert!(!game.options.world_rules.allow_cheats);
    app.open_chat(true);app.text("gamemode creative");app.key(KeyCode::Enter,true,false).unwrap();
    assert_eq!(app.game.as_ref().unwrap().sim.mode,Mode::Creative);
    app.key(KeyCode::Escape,false,false).unwrap();
    app.key(KeyCode::Escape,true,false).unwrap();
    layout(&mut app);
    assert!(app.ui_frame.buttons.iter().any(|b|matches!(b.action,Action::Screen(Screen::ShareLan))&&!b.enabled));
    drop(app.game.take());
    let since=std::time::Instant::now();
    while std::net::TcpStream::connect(&addr).is_ok() {
        assert!(since.elapsed()<Duration::from_secs(3),"listener outlived its world");
        std::thread::sleep(Duration::from_millis(35));
    }
    std::fs::remove_dir_all(dir).unwrap();
}
fn click(app: &mut App, matches: impl Fn(&Action) -> bool) {
    layout(app);
    let button = app
        .ui_frame
        .buttons
        .iter()
        .find(|b| b.enabled && matches(&b.action))
        .expect("enabled button")
        .rect;
    app.gui.cursor = [button.x + button.w / 2., button.y + button.h / 2.];
    app.mouse_button(MouseButton::Left, true).unwrap();
    app.mouse_button(MouseButton::Left, false).unwrap();
}
pub(super) fn isolated_app() -> (App, PathBuf) {
    let options = Options::parse_from([
        "ferrum189",
        "--fresh",
        "--generator",
        "1",
        "--render-distance",
        "2",
    ]);
    let jar = PathBuf::from(std::env::var_os("APPDATA").unwrap())
        .join(".minecraft/versions/1.8.9/1.8.9.jar");
    let assets = Arc::new(Assets::load(&jar).unwrap());
    let resources = Arc::new(Resources::load().unwrap());
    let settings = Settings {
        gui_scale: 2,
        language: "en_us".into(),
        ..Default::default()
    };
    let mut app = App::new(options, assets, resources, settings);
    static NEXT_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "ferrum-controls-{}-{}-{}",
        std::process::id(),
        timestamp(),
        NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    app.settings_path = dir.join("options.json");
    (app, dir)
}

#[test]
#[ignore = "requires original local 1.8.9 resources"]
fn rebind_in_menu_and_use_keyboard_and_mouse_in_world() {
    let (mut app, dir) = isolated_app();
    click(&mut app, |a| matches!(a, Action::Screen(Screen::Options)));
    click(&mut app, |a| matches!(a, Action::Screen(Screen::Controls)));
    // Original list is alphabetized by translated labels, scroll to the actual button.
    for action in [Binding::Forward, Binding::Use] {
        let mut found = false;
        for scroll in (0..=800).step_by(10) {
            app.gui.scroll = scroll;
            layout(&mut app);
            if app
                .ui_frame
                .buttons
                .iter()
                .any(|b| matches!(b.action,Action::Controls(ControlAction::Edit(k)) if k==action))
            {
                click(
                    &mut app,
                    |a| matches!(a,Action::Controls(ControlAction::Edit(k)) if *k==action),
                );
                found = true;
                break;
            }
        }
        assert!(found);
        assert_eq!(app.gui.editing_binding, Some(action));
        if action == Binding::Forward {
            app.key(KeyCode::ArrowUp, true, false).unwrap();
        } else {
            app.mouse_button(MouseButton::Back, true).unwrap();
            app.mouse_button(MouseButton::Back, false).unwrap();
        }
    }
    let saved: Settings =
        serde_json::from_slice(&std::fs::read(&app.settings_path).unwrap()).unwrap();
    assert_eq!(
        saved
            .bindings
            .action_for(crate::keybindings::mouse_code(MouseButton::Back)),
        Some(Binding::Use)
    );
    assert_eq!(
        saved
            .bindings
            .action_for(crate::keybindings::key_code(KeyCode::ArrowUp).unwrap()),
        Some(Binding::Forward)
    );
    app.game = Some(Game::load(app.options.clone(), &app.assets, &app.settings).unwrap());
    app.open(Screen::Playing);
    app.key(KeyCode::KeyW, true, false).unwrap();
    assert!(!app
        .game
        .as_ref()
        .unwrap()
        .player
        .keys
        .contains(&KeyCode::KeyW));
    app.key(KeyCode::ArrowUp, true, false).unwrap();
    assert!(app
        .game
        .as_ref()
        .unwrap()
        .player
        .keys
        .contains(&KeyCode::KeyW));
    app.key(KeyCode::ArrowUp, false, false).unwrap();
    assert!(!app
        .game
        .as_ref()
        .unwrap()
        .player
        .keys
        .contains(&KeyCode::KeyW));
    app.mouse_button(MouseButton::Back, true).unwrap();
    assert!(app.right);
    app.mouse_button(MouseButton::Back, false).unwrap();
    assert!(!app.right);
    let unknown = PhysicalKey::Unidentified(winit::keyboard::NativeKeyCode::Windows(0xfe));
    app.settings
        .bindings
        .set(Binding::Forward, i32::from('한' as u16) + 256);
    app.keyboard_input(unknown, true, false, Some("한"))
        .unwrap();
    assert!(app
        .game
        .as_ref()
        .unwrap()
        .player
        .keys
        .contains(&KeyCode::KeyW));
    app.keyboard_input(unknown, false, false, None).unwrap();
    assert!(!app
        .game
        .as_ref()
        .unwrap()
        .player
        .keys
        .contains(&KeyCode::KeyW));
    app.key(KeyCode::KeyE, true, false).unwrap();
    assert_eq!(app.gui.screen, Screen::Inventory);
    app.key(KeyCode::KeyE, false, false).unwrap();
    app.key(KeyCode::KeyE, true, false).unwrap();
    assert_eq!(app.gui.screen, Screen::Playing);
    app.key(KeyCode::KeyT, true, false).unwrap();
    app.text("안녕");
    app.key(KeyCode::Enter, true, false).unwrap();
    assert_eq!(app.gui.screen, Screen::Playing);
    app.open_chat(false);
    app.key(KeyCode::ArrowUp, true, false).unwrap();
    assert_eq!(app.gui.chat.input, "안녕");
    std::fs::remove_file(app.settings_path).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
#[ignore = "requires original local 1.8.9 resources"]
fn options_mouse_actions_save_values_and_restore_the_parent_screen() {
    use ui::options::{Command, Page, Slider, Toggle};
    let (mut app, dir) = isolated_app();
    click(&mut app, |a| {
        matches!(a, Action::Screen(Screen::Multiplayer))
    });
    click(&mut app, |a| {
        matches!(a, Action::Server(ui::multiplayer::Command::Direct))
    });
    app.key(KeyCode::F2, true, false).unwrap();
    assert!(app.screenshot_pending);
    app.key(KeyCode::F6, true, false).unwrap();
    assert_eq!(app.gui.screen, Screen::OptionPage(Page::Broadcast));
    click(&mut app, |a| {
        matches!(a, Action::Screen(Screen::Multiplayer))
    });
    assert!(!app.gui.multiplayer.at_list());
    click(&mut app, |a| {
        matches!(a, Action::Server(ui::multiplayer::Command::Cancel))
    });
    click(&mut app, |a| matches!(a, Action::Screen(Screen::Main)));
    click(&mut app, |a| matches!(a, Action::Screen(Screen::Languages)));
    assert_eq!(app.gui.back(), Screen::Main);
    app.key(KeyCode::Escape, true, false).unwrap();
    click(&mut app, |a| matches!(a, Action::Screen(Screen::Options)));
    click(&mut app, |a| {
        matches!(a, Action::Screen(Screen::OptionPage(Page::Sounds)))
    });
    // The center of the master slider is 50%. Drag beyond its right edge before release.
    click(&mut app, |a| {
        matches!(a, Action::Option(Command::Slider(Slider::Sound(0))))
    });
    assert_eq!(app.settings.volume, 0.5);
    app.mouse_button(MouseButton::Left, true).unwrap();
    app.gui.cursor[0] = app.ui_frame.size[0];
    app.drag_menu().unwrap();
    app.mouse_button(MouseButton::Left, false).unwrap();
    assert_eq!(app.settings.volume, 1.0);
    click(&mut app, |a| matches!(a, Action::Screen(Screen::Options)));
    click(&mut app, |a| {
        matches!(a, Action::Screen(Screen::OptionPage(Page::Skin)))
    });
    click(&mut app, |a| {
        matches!(a, Action::Option(Command::Toggle(Toggle::Skin(6))))
    });
    assert_eq!(app.settings.menu.skin_parts, 0x3f);
    click(&mut app, |a| matches!(a, Action::Screen(Screen::Options)));
    click(&mut app, |a| {
        matches!(a, Action::Screen(Screen::OptionPage(Page::Chat)))
    });
    click(&mut app, |a| {
        matches!(a, Action::Option(Command::Toggle(Toggle::ChatVisibility)))
    });
    click(&mut app, |a| {
        matches!(a, Action::Option(Command::Toggle(Toggle::ReducedDebug)))
    });
    let saved: Settings =
        serde_json::from_slice(&std::fs::read(&app.settings_path).unwrap()).unwrap();
    assert_eq!(saved.volume, 1.0);
    assert_eq!(saved.menu.skin_parts, 0x3f);
    assert_eq!(saved.menu.chat_visibility, 1);
    assert!(saved.menu.reduced_debug);
    app.key(KeyCode::Escape, true, false).unwrap();
    assert_eq!(app.gui.screen, Screen::Main);
    std::fs::remove_file(app.settings_path).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn odd_sized_windows_map_each_physical_pixel_like_gui_screen() {
    let size = [1281, 721];
    for y in 0..721 {
        for x in [0, 1, 319, 639, 640, 1279, 1280] {
            assert_eq!(
                input::menu_cursor([f64::from(x) + 0.5, f64::from(y) + 0.5], size, 2.),
                [
                    (x * 641 / 1281) as f32,
                    (360 - (720 - y) * 361 / 721) as f32
                ]
            );
        }
    }
}

#[test]
#[ignore = "requires installed original resources and the native chunk provider"]
fn world_menu_create_load_rename_recreate_and_delete_in_an_isolated_profile() {
    use ui::create_world::Command as C;
    use ui::world_select::Command as W;
    let (mut app, dir) = isolated_app();
    app.library = crate::world_library::Library::isolated(dir.clone());
    app.settings.render_distance = 2;
    click(&mut app, |a| matches!(a, Action::Screen(Screen::Worlds)));
    assert!(app.gui.worlds.is_empty());
    click(&mut app, |a| matches!(a, Action::Screen(Screen::Create)));
    app.keyboard_input(PhysicalKey::Code(KeyCode::ControlLeft), true, false, None)
        .unwrap();
    app.key(KeyCode::KeyA, true, false).unwrap();
    app.key(KeyCode::ControlLeft, false, false).unwrap();
    app.text("UI World");
    click(&mut app, |a| matches!(a, Action::WorldForm(C::More)));
    click(&mut app, |a| matches!(a, Action::WorldForm(C::CycleType))); // Default -> Superflat
    click(&mut app, |a| matches!(a, Action::WorldForm(C::Bonus)));
    click(&mut app, |a| matches!(a, Action::WorldForm(C::Cheats)));
    click(&mut app, |a| matches!(a, Action::CreateWorld));
    assert_eq!(app.gui.screen, Screen::Loading);
    let mut game = app
        .loading
        .take()
        .unwrap()
        .recv_timeout(Duration::from_secs(90))
        .unwrap()
        .unwrap();
    assert!(game.sim.flat_world);
    assert!(game.options.world_rules.allow_cheats);
    assert!(game
        .sim
        .containers
        .values()
        .any(|c| c.slots.iter().any(Option::is_some)));
    assert!(game.world.save_path.starts_with(&dir));
    game.save().unwrap();
    assert!(game.world.save_path.is_file());
    app.action(Action::Screen(Screen::Worlds), false).unwrap();
    assert_eq!(app.gui.worlds[0].name, "UI World");
    fn select(app: &mut App) {
        layout(app);
        app.gui.cursor = [app.ui_frame.size[0] / 2., 42.];
        app.mouse_button(MouseButton::Left, true).unwrap();
        app.mouse_button(MouseButton::Left, false).unwrap();
        assert_eq!(app.gui.selected_world, Some(0));
    }
    select(&mut app);
    click(&mut app, |a| matches!(a, Action::Worlds(W::Rename)));
    app.key(KeyCode::ControlLeft, true, false).unwrap();
    app.key(KeyCode::KeyA, true, false).unwrap();
    app.key(KeyCode::ControlLeft, false, false).unwrap();
    app.text("Renamed");
    app.key(KeyCode::Enter, true, false).unwrap();
    assert_eq!(app.gui.worlds[0].name, "Renamed");
    select(&mut app);
    click(&mut app, |a| matches!(a, Action::Worlds(W::Recreate)));
    assert_eq!(app.gui.create.world_type.name(), "flat");
    assert!(app.gui.create.cheats());
    assert!(app.gui.create.name().contains("Renamed"));
    click(&mut app, |a| matches!(a, Action::Screen(Screen::Worlds)));
    select(&mut app);
    click(&mut app, |a| matches!(a, Action::Worlds(W::Delete)));
    click(&mut app, |a| matches!(a, Action::Screen(Screen::Worlds)));
    assert_eq!(app.gui.worlds.len(), 1);
    select(&mut app);
    click(&mut app, |a| matches!(a, Action::Worlds(W::Delete)));
    click(&mut app, |a| matches!(a, Action::Worlds(W::ConfirmDelete)));
    assert!(app.gui.worlds.is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
#[ignore = "requires installed 1.8.9 Korean resources and JAR"]
fn normal_creative_inventory_tabs_search_hotbar_drag_and_discard() {
    use crate::ui::creative::{Hit,PLAYER,SEARCH};
    let (mut app,dir)=isolated_app();app.settings.language="ko_kr".into();
    let mut opts=app.options.clone();opts.creative=true;opts.project_dir=dir.clone();
    app.game=Some(Game::load(opts,&app.assets,&app.settings).unwrap());
    let bag=&app.game.as_ref().unwrap().sim.inventory;
    assert!(bag.slots.iter().chain(&bag.crafting).all(Option::is_none));
    assert!(bag.cursor.is_none());
    app.open(Screen::Playing);
    app.bound_input(Binding::Inventory,true).unwrap();assert_eq!(app.gui.screen,Screen::Creative);
    let cell=|app:&mut App,i:usize|{
        layout(app);let r=app.ui_frame.buttons.iter().filter(|b|matches!(b.action,Action::Creative(Hit::Cell(_)))).nth(i).unwrap().rect;
        app.gui.cursor=[r.x+8.,r.y+8.];
    };
    let press=|app:&mut App,button|{app.mouse_button(button,true).unwrap();app.mouse_button(button,false).unwrap();};
    cell(&mut app,0);press(&mut app,MouseButton::Left);press(&mut app,MouseButton::Left);press(&mut app,MouseButton::Right);
    assert_eq!(app.game.as_ref().unwrap().sim.inventory.cursor,Some(Stack::new(1,1)));
    cell(&mut app,1);press(&mut app,MouseButton::Left);assert!(app.game.as_ref().unwrap().sim.inventory.cursor.is_none());
    let typekey=|app:&mut App,key:KeyCode,s:&str|{
        app.keyboard_input(PhysicalKey::Code(key),true,false,Some(s)).unwrap();
        app.keyboard_input(PhysicalKey::Code(key),false,false,None).unwrap();
    };
    typekey(&mut app,KeyCode::KeyT,"t");assert_eq!(app.gui.creative.tab,SEARCH);assert!(app.gui.creative.search.value.is_empty());
    app.text("다이아몬드");assert_eq!(app.gui.creative.list().len(),13);
    cell(&mut app,2);typekey(&mut app,KeyCode::Digit2,"2");
    assert_eq!(app.game.as_ref().unwrap().sim.inventory.slots[1],Some(Stack::new(264,64)));
    typekey(&mut app,KeyCode::KeyE,"e");assert_eq!(app.gui.screen,Screen::Creative);assert_eq!(app.gui.creative.search.value,"e");
    click(&mut app,|a|matches!(a,Action::Creative(Hit::Tab(PLAYER))));
    click(&mut app,|a|matches!(a,Action::Slot(1)));assert_eq!(app.game.as_ref().unwrap().sim.inventory.cursor,Some(Stack::new(264,64)));
    let hover=|app:&mut App,index|{layout(app);let r=app.ui_frame.buttons.iter().find(|b|matches!(b.action,Action::Slot(n) if n==index)).unwrap().rect;app.gui.cursor=[r.x+3.,r.y+3.];};
    hover(&mut app,9);app.mouse_button(MouseButton::Left,true).unwrap();
    hover(&mut app,10);let slot=app.hovered_slot().unwrap();app.inventory_drag.as_mut().unwrap().slots.push(slot);
    app.mouse_button(MouseButton::Left,false).unwrap();
    for slot in [9,10]{assert_eq!(app.game.as_ref().unwrap().sim.inventory.slots[slot],Some(Stack::new(264,32)));}
    app.keys.insert(KeyCode::ShiftLeft);click(&mut app,|a|matches!(a,Action::Creative(Hit::Bin)));
    assert!(app.game.as_ref().unwrap().sim.inventory.slots.iter().all(Option::is_none));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
#[ignore = "requires installed 1.8.9 resources"]
fn gameplay_q_control_q_and_inventory_q_use_the_real_input_route() {
    let (mut app,dir)=isolated_app();
    let mut options=app.options.clone();options.project_dir=dir.clone();options.creative=false;
    app.game=Some(Game::load(options,&app.assets,&app.settings).unwrap());
    app.open(Screen::Playing);
    let key=|app:&mut App,k,pressed|app.keyboard_input(PhysicalKey::Code(k),pressed,false,None).unwrap();
    let tap=|app:&mut App,k|{key(app,k,true);key(app,k,false);};
    {
        let g=app.game.as_mut().unwrap();g.sim.local_name="Player".into();g.sim.drops.clear();
        g.sim.inventory.selected=3;g.sim.inventory.slots[3]=Some(Stack::new(264,17));
        g.player.set_pose_f64([100.125,70.25,-10.375],113.25,60.,false);
    }
    tap(&mut app,KeyCode::KeyQ);
    key(&mut app,KeyCode::ControlLeft,true);tap(&mut app,KeyCode::KeyQ);key(&mut app,KeyCode::ControlLeft,false);
    tap(&mut app,KeyCode::KeyQ);
    let sim=&app.game.as_ref().unwrap().sim;
    assert_eq!(sim.drops.iter().map(|d|d.stack.count).collect::<Vec<_>>(),[1,16]);
    assert_eq!(sim.statistics["stat.drop"],2);
    assert!(sim.drops.iter().all(|d|d.thrower.as_deref()==Some("Player") && d.pickup_delay==40));
    assert!(sim.inventory.slots[3].is_none());
    assert_ne!(sim.drops[0].velocity,sim.drops[1].velocity);
    let sim=&mut app.game.as_mut().unwrap().sim;
    sim.inventory.slots[9]=Some(Stack::new(4,9));
    tap(&mut app,KeyCode::KeyE);assert_eq!(app.gui.screen,Screen::Inventory);
    layout(&mut app);
    let rect=app.ui_frame.buttons.iter().find(|b|matches!(b.action,Action::Slot(9))).unwrap().rect;
    app.gui.cursor=[rect.x+3.,rect.y+3.];
    tap(&mut app,KeyCode::KeyQ);
    key(&mut app,KeyCode::ControlRight,true);tap(&mut app,KeyCode::KeyQ);key(&mut app,KeyCode::ControlRight,false);
    let sim=&app.game.as_ref().unwrap().sim;
    assert_eq!(sim.drops.iter().map(|d|d.stack.count).collect::<Vec<_>>(),[1,16,1,8]);
    assert!(sim.drops[2..].iter().all(|d|d.thrower.is_none()&&d.pickup_delay==40));
    assert_eq!(sim.statistics["stat.drop"],2);
    tap(&mut app,KeyCode::KeyE);assert_eq!(app.gui.screen,Screen::Playing);
    let sim=&mut app.game.as_mut().unwrap().sim;
    sim.inventory.slots[3]=Some(Stack::new(264,1));sim.commands.player_mode=Some(3);
    tap(&mut app,KeyCode::KeyQ);
    let sim=&app.game.as_ref().unwrap().sim;
    assert_eq!(sim.drops.len(),4);assert!(sim.inventory.slots[3].is_some());
    std::fs::remove_dir_all(dir).unwrap();
}
