use super::*;
#[derive(Clone, Copy, Debug)]
pub enum Command {
    Mode,
    Cheats,
    Start,
}
#[derive(Clone, Copy, Default)]
pub struct Form {
    pub mode: u8,
    pub cheats: bool,
}
impl Form {
    pub fn cycle(&mut self) {
        self.mode = match self.mode {
            0 => 3,
            3 => 1,
            1 => 2,
            _ => 0,
        }
    }
}
pub(super) fn background(p: &mut Painter<'_>) {
    let [w, h] = p.frame.size.map(f32::ceil);
    main_menu::gradient(
        p,
        Rect { x: 0., y: 0., w, h },
        [16. / 255., 16. / 255., 16. / 255., 192. / 255.],
        [16. / 255., 16. / 255., 16. / 255., 208. / 255.],
    );
}
pub(super) fn draw(p: &mut Painter<'_>, gui: &Gui, sim: Option<&Simulation>) {
    background(p);
    let [w, h] = p.frame.size.map(f32::ceil);
    let middle = (w / 2.).floor();
    let row = (h / 4.).floor();
    p.center(middle, 40., &p.tr("menu.game"), WHITE);
    let remote = gui.option_state.remote;
    let available = !remote && sim.is_some_and(|s| s.lan.is_none());
    for (dx, dy, width, key, action, enabled) in [
        (
            -100.,
            104.,
            200.,
            if remote {
                "menu.disconnect"
            } else {
                "menu.returnToMenu"
            },
            Action::SaveQuit,
            true,
        ),
        (-100., 8., 200., "menu.returnToGame", Action::Resume, true),
        (
            -100.,
            80.,
            98.,
            "menu.options",
            Action::Screen(Screen::Options),
            true,
        ),
        (
            2.,
            80.,
            98.,
            "menu.shareToLan",
            Action::Screen(Screen::ShareLan),
            available,
        ),
        (
            -100.,
            32.,
            98.,
            "gui.achievements",
            Action::Screen(Screen::Achievements),
            true,
        ),
        (
            2.,
            32.,
            98.,
            "gui.stats",
            Action::Statistics(statistics::Command::Open),
            true,
        ),
    ] {
        p.button(
            Rect {
                x: middle + dx,
                y: row + dy,
                w: width,
                h: 20.,
            },
            &p.tr(key),
            action,
            enabled,
        );
    }
}
pub(super) fn settings(p: &mut Painter<'_>, form: &Form) {
    background(p);
    let [w, h] = p.frame.size.map(f32::ceil);
    let mid = (w / 2.).floor();
    p.center(mid, 50., &p.tr("lanServer.title"), WHITE);
    p.center(mid, 82., &p.tr("lanServer.otherPlayers"), WHITE);
    let mode = ["survival", "creative", "adventure", "spectator"][usize::from(form.mode)];
    let mode = format!(
        "{} {}",
        p.tr("selectWorld.gameMode"),
        p.tr(&format!("selectWorld.gameMode.{mode}"))
    );
    let cheats = format!(
        "{} {}",
        p.tr("selectWorld.allowCommands"),
        p.tr(if form.cheats {
            "options.on"
        } else {
            "options.off"
        })
    );
    for (x, y, label, action) in [
        (
            -155.,
            h - 28.,
            p.tr("lanServer.start"),
            Action::Lan(Command::Start),
        ),
        (
            5.,
            h - 28.,
            p.tr("gui.cancel"),
            Action::Screen(Screen::Pause),
        ),
        (-155., 100., mode, Action::Lan(Command::Mode)),
        (5., 100., cheats, Action::Lan(Command::Cheats)),
    ] {
        p.button(
            Rect {
                x: mid + x,
                y,
                w: 150.,
                h: 20.,
            },
            &label,
            action,
            true,
        );
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::collections::HashMap;
    #[derive(Deserialize)]
    struct Layout {
        size: [u32; 2],
        buttons: Vec<(u16, u16, u16, u16, u16, bool, bool, String)>,
    }
    #[test]
    #[ignore = "requires the installed Korean 1.8.9 assets"]
    fn original_pause_share_lan_and_achievement_menu_buttons() {
        let oracle: HashMap<String, Layout> = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-pause-menus.json"
        ))
        .unwrap();
        let mut s = Settings::default();
        s.gui_scale = 2;
        s.language = "ko_kr".into();
        let r = crate::validation::resources();
        let a = crate::validation::assets();
        s.locale_unicode = r.unicode(&s.language);
        let captures = std::env::var_os("FERRUM189_PAUSE_UI_CAPTURE").map(std::path::PathBuf::from);
        let mut renderer = captures.as_ref().map(|dir| {
            std::fs::create_dir_all(dir).unwrap();
            let mut renderer =
                pollster::block_on(crate::renderer::Renderer::new(None, a, r, &s, None)).unwrap();
            renderer.resize(1600, 900);
            renderer
        });
        let sim = Simulation::default();
        let mut gui = Gui::default();
        gui.achievements = achievements::State::new("Menu189");
        gui.hide_hud = true;
        for name in [
            "pause-remote",
            "pause",
            "share-lan",
            "share-lan-1",
            "share-lan-2",
            "share-lan-3",
            "share-lan-4",
            "achievements",
        ] {
            gui.option_state.remote = name == "pause-remote";
            let share = name.starts_with("share");
            gui.open(if share {
                Screen::ShareLan
            } else if name == "achievements" {
                Screen::Achievements
            } else {
                Screen::Pause
            });
            if share && name != "share-lan" {
                gui.lan_form.cycle();
                gui.lan_form.cheats = !gui.lan_form.cheats;
            }
            let mut f = Frame::default();
            f.captions = Some(Vec::new());
            f.set_viewport([1600, 900], &s);
            paint(&mut f, &gui, &s, r, a, Some(&sim), &Stats::default());
            if let (Some(dir), Some(renderer)) = (&captures, &mut renderer) {
                renderer
                    .screenshot(
                        &dir.join(format!("{name}.png")),
                        &crate::renderer::RenderInput {
                            selected_block: None,
                            assets: a,
                            resources: r,
                            settings: &s,
                            gui: &gui,
                            ui: &f,
                            world: None,
                            player: None,
                            simulation: None,
                            alpha: 1.,
                            time: 0.,
                        },
                    )
                    .unwrap();
            }
            let expected = &oracle[name];
            assert_eq!(f.size, expected.size.map(|v| v as f32));
            assert_eq!(f.buttons.len(), expected.buttons.len(), "{name}");
            for ((b, (_, text)), (id, x, y, w, h, enabled, visible, label)) in f
                .buttons
                .iter()
                .zip(f.captions.as_ref().unwrap())
                .zip(&expected.buttons)
            {
                let act = match b.action {
                    Action::SaveQuit => 1,
                    Action::Resume => 4,
                    Action::Screen(Screen::Options) => 0,
                    Action::Screen(Screen::Achievements) => 5,
                    Action::Screen(Screen::ShareLan) => 7,
                    Action::Statistics(statistics::Command::Open) => 6,
                    Action::Screen(Screen::Pause) if share => 102,
                    Action::Screen(Screen::Pause) => 1,
                    Action::Lan(Command::Start) => 101,
                    Action::Lan(Command::Mode) => 104,
                    Action::Lan(Command::Cheats) => 103,
                    _ => panic!(),
                };
                assert_eq!((act, b.enabled), (*id, *enabled), "{name}");
                assert!(*visible);
                assert_eq!(
                    [b.rect.x, b.rect.y, b.rect.w, b.rect.h],
                    [*x, *y, *w, *h].map(f32::from),
                    "{name} {id}"
                );
                assert_eq!(text, label, "{name} {id}");
            }
        }
    }
}
