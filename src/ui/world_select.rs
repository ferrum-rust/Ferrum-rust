use super::{
    options, text_field::TextField, Action, Gui, Painter, Rect, Screen, WorldEntry, WHITE,
};
use std::{
    cell::RefCell,
    collections::HashMap,
    time::{Duration, Instant},
};
use winit::{event::MouseButton, keyboard::KeyCode};
#[path = "world_date.rs"]
mod date;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Rename,
    Delete,
    Recreate,
    SaveName,
    ConfirmDelete,
    NameCursor(usize),
}

pub struct State {
    scroll: f32,
    drag: Option<(f32, f32, f32)>,
    clicked: Option<(usize, Instant)>,
    name: TextField,
    focused: bool,
    opened: Instant,
    locale: String,
    reference: bool,
    dates: RefCell<HashMap<i64, String>>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            scroll: 0.,
            drag: None,
            clicked: None,
            name: TextField::default(),
            focused: false,
            opened: Instant::now(),
            locale: date::system_locale(),
            reference: false,
            dates: Default::default(),
        }
    }
}
fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect { x, y, w, h }
}
fn max_scroll(height: f32, count: usize) -> f32 {
    (count as f32 * 36. + 4. - (height.ceil() - 96.)).max(0.)
}
impl State {
    pub fn reset(&mut self) {
        self.scroll = 0.;
        self.clicked = None;
        self.drag = None;
        self.dates.get_mut().clear();
    }
    pub fn reference_locale(&mut self) {
        self.locale = "ko-kr".into();
        self.reference = true;
        self.dates.get_mut().clear();
    }
    pub fn begin_rename(&mut self, name: &str) {
        self.name = TextField::with_limit(name, 32);
        self.focused = true;
        self.opened = Instant::now();
    }
    pub fn name(&self) -> &str {
        self.name.value.trim_matches(|c: char| c <= ' ')
    }
    pub fn set_cursor(&mut self, index: usize) {
        self.name.set_cursor(index, false);
        self.focused = true;
    }
    pub fn text(&mut self, text: &str) {
        if self.focused {
            self.name.insert(text, 32);
        }
    }
    pub fn key(&mut self, screen: Screen, key: KeyCode, shift: bool, ctrl: bool) -> Option<Action> {
        if screen != Screen::RenameWorld {
            return None;
        }
        if self.focused {
            self.name.key(key, shift, ctrl, 32);
        }
        if matches!(key, KeyCode::Enter | KeyCode::NumpadEnter) && !self.name().is_empty() {
            Some(Action::Worlds(Command::SaveName))
        } else {
            None
        }
    }
    pub fn pointer_down(
        &mut self,
        screen: Screen,
        cursor: [f32; 2],
        size: [f32; 2],
        count: usize,
        button: MouseButton,
    ) -> Option<Action> {
        let [w, h] = size.map(f32::ceil);
        let cx = (w / 2.).floor();
        if screen == Screen::RenameWorld {
            self.focused = rect(cx - 100., 60., 200., 20.).contains(cursor);
            return None;
        }
        let [x, y] = cursor;
        if screen != Screen::Worlds || button != MouseButton::Left || !(32. ..=h - 64.).contains(&y)
        {
            return None;
        }
        let inside_bar = (cx + 124. ..=cx + 130.).contains(&x);
        let height = h - 96.;
        let max = max_scroll(h, count);
        let ratio = if inside_bar && max > 0. {
            let thumb = (height * height / (count as f32 * 36.))
                .floor()
                .clamp(32., (height - 8.).max(32.));
            max / (height - thumb).max(1.)
        } else {
            -1.
        };
        self.drag = Some((y, self.scroll, ratio));
        let relative = y - 36. + self.scroll.floor();
        if !(cx - 110. ..=cx + 110.).contains(&x) || relative < 0. {
            return None;
        }
        let row = (relative / 36.) as usize;
        if row >= count {
            return None;
        }
        let now = Instant::now();
        let double = self
            .clicked
            .is_some_and(|(i, t)| i == row && now.duration_since(t) < Duration::from_millis(250));
        self.clicked = Some((row, now));
        Some(if double {
            Action::PlayWorld
        } else {
            Action::SelectWorld(row)
        })
    }
    pub fn wheel(&mut self, y: f32, height: f32, count: usize) {
        self.scroll = (self.scroll - y.signum() * 18.).clamp(0., max_scroll(height, count));
    }
    pub fn drag(&mut self, y: f32, height: f32, count: usize) {
        if let Some((start, scroll, ratio)) = self.drag {
            self.scroll = (scroll + (y - start) * ratio).clamp(0., max_scroll(height, count));
        }
    }
    pub fn release(&mut self) {
        self.drag = None;
    }
}

fn display_name(p: &Painter<'_>, entry: &WorldEntry, i: usize) -> String {
    if entry.name.is_empty() {
        format!("{} {}", p.tr("selectWorld.world"), i + 1)
    } else {
        entry.name.clone()
    }
}
fn detail(p: &Painter<'_>, e: &WorldEntry) -> String {
    if e.requires_conversion {
        return format!("{} ", p.tr("selectWorld.conversion"));
    }
    let mut name = if e.rules.hardcore {
        format!("§4{}§r", p.tr("gameMode.hardcore"))
    } else {
        p.tr([
            "gameMode.survival",
            "gameMode.creative",
            "gameMode.adventure",
            "gameMode.spectator",
        ]
        .get(e.game_type_id as usize)
        .unwrap_or(&"gameMode.survival"))
    };
    if e.rules.allow_cheats {
        name.push_str(", ");
        name.push_str(&p.tr("selectWorld.cheats"));
    }
    name
}
pub(super) fn draw(p: &mut Painter<'_>, gui: &Gui) {
    let [w, h] = p.frame.size.map(f32::ceil);
    let cx = (w / 2.).floor();
    let selected = gui
        .selected_world
        .and_then(|i| gui.worlds.get(i).map(|e| (i, e)));
    let state = &gui.world_list;
    match gui.screen {
        Screen::RenameWorld => {
            p.background();
            p.center(cx, 20., &p.tr("selectWorld.renameTitle"), WHITE);
            p.text(
                cx - 100.,
                47.,
                &p.tr("selectWorld.enterName"),
                [160. / 255., 160. / 255., 160. / 255., 1.],
            );
            let time = if state.reference {
                0.
            } else {
                state.opened.elapsed().as_secs_f32()
            };
            state.name.draw(
                p,
                rect(cx - 100., 60., 200., 20.),
                state.focused,
                time,
                32,
                |i| Action::Worlds(Command::NameCursor(i)),
            );
            let y = (h / 4.).floor() + 108.;
            p.button(
                rect(cx - 100., y, 200., 20.),
                &p.tr("selectWorld.renameButton"),
                Action::Worlds(Command::SaveName),
                !state.name().is_empty(),
            );
            p.button(
                rect(cx - 100., y + 24., 200., 20.),
                &p.tr("gui.cancel"),
                Action::Screen(Screen::Worlds),
                true,
            );
        }
        Screen::DeleteWorld => {
            p.background();
            p.center(cx, 70., &p.tr("selectWorld.deleteQuestion"), WHITE);
            if let Some((i, e)) = selected {
                let warning = format!(
                    "'{}' {}",
                    display_name(p, e, i),
                    p.tr("selectWorld.deleteWarning")
                );
                for (n, line) in options::wrapped(p, &warning, w - 50.).iter().enumerate() {
                    p.center(cx, 90. + 9. * n as f32, line, WHITE);
                }
            }
            let y = (h / 6.).floor() + 96.;
            p.button(
                rect(cx - 155., y, 150., 20.),
                &p.tr("selectWorld.deleteButton"),
                Action::Worlds(Command::ConfirmDelete),
                selected.is_some(),
            );
            p.button(
                rect(cx + 5., y, 150., 20.),
                &p.tr("gui.cancel"),
                Action::Screen(Screen::Worlds),
                true,
            );
        }
        Screen::Worlds => {
            let bottom = h - 64.;
            let scroll = state.scroll.min(max_scroll(h, gui.worlds.len())).floor();
            options::dirt(p, rect(0., 32., w, bottom - 32.), [0., scroll], 32. / 255.);
            for (i, e) in gui
                .worlds
                .iter()
                .enumerate()
                .skip((scroll as usize).saturating_sub(36) / 36)
            {
                let y = 36. + i as f32 * 36. - scroll;
                if y > bottom + 2. {
                    break;
                }
                if gui.selected_world == Some(i) {
                    p.rect(
                        cx - 110.,
                        y - 2.,
                        220.,
                        36.,
                        [128. / 255., 128. / 255., 128. / 255., 1.],
                    );
                    p.rect(cx - 109., y - 1., 218., 34., [0., 0., 0., 1.]);
                }
                let x = cx - 106.;
                p.text(x, y + 1., &display_name(p, e, i), WHITE);
                let mut dates = state.dates.borrow_mut();
                let last = dates
                    .entry(e.last_played)
                    .or_insert_with(|| date::format(e.last_played, &state.locale, state.reference));
                let gray = [128. / 255., 128. / 255., 128. / 255., 1.];
                p.text(x, y + 12., &format!("{} ({last})", e.folder), gray);
                p.text(x, y + 22., &detail(p, e), gray);
            }
            options::mask_outside(p, 0., w, 32., bottom);
            options::scrollbar(
                p,
                cx + 124.,
                32.,
                bottom,
                gui.worlds.len() as f32 * 36.,
                scroll,
            );
            p.center(cx, 20., &p.tr("selectWorld.title"), WHITE);
            let y = h - 52.;
            let selected = selected.is_some();
            for (box_, key, action, enabled) in [
                (
                    rect(cx - 154., y, 150., 20.),
                    "selectWorld.select",
                    Action::PlayWorld,
                    selected,
                ),
                (
                    rect(cx + 4., y, 150., 20.),
                    "selectWorld.create",
                    Action::Screen(Screen::Create),
                    true,
                ),
                (
                    rect(cx - 154., y + 24., 72., 20.),
                    "selectWorld.rename",
                    Action::Worlds(Command::Rename),
                    selected,
                ),
                (
                    rect(cx - 76., y + 24., 72., 20.),
                    "selectWorld.delete",
                    Action::Worlds(Command::Delete),
                    selected,
                ),
                (
                    rect(cx + 4., y + 24., 72., 20.),
                    "selectWorld.recreate",
                    Action::Worlds(Command::Recreate),
                    selected,
                ),
                (
                    rect(cx + 82., y + 24., 72., 20.),
                    "gui.cancel",
                    Action::Screen(Screen::Main),
                    true,
                ),
            ] {
                p.button(box_, &p.tr(key), action, enabled);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        settings::Settings,
        ui::{Frame, Stats},
        validation::{assets, resources},
    };

    #[test]
    #[cfg(windows)]
    fn java_short_dates_match_korean_reference_profile() {
        assert_eq!(
            date::format(1_700_000_000_000, "ko-kr", true),
            "23. 11. 15 오전 7:13"
        );
        assert_eq!(
            date::format(1_600_000_000_000, "ko-kr", true),
            "20. 9. 13 오후 9:26"
        );
    }

    #[test]
    fn renaming_retains_existing_formatting_and_limits_utf16_characters() {
        let mut s = State::default();
        s.begin_rename("§aMine");
        assert_eq!(s.name(), "§aMine");
        s.text("§!");
        assert_eq!(s.name(), "§aMine!");
        s.begin_rename(&format!("{}🎁", "a".repeat(31)));
        assert_eq!(s.name().len(), 31);
    }

    #[test]
    fn slot_hit_uses_scrolled_coordinates_and_double_clicks_the_same_save() {
        let mut state = State::default();
        let view = [400.5, 240.5];
        let click = |state: &mut State, x, y| {
            state.pointer_down(Screen::Worlds, [x, y], view, 20, MouseButton::Left)
        };
        assert!(click(&mut state, 200., 35.).is_none());
        assert!(matches!(
            click(&mut state, 200., 72.),
            Some(Action::SelectWorld(1))
        ));
        state.release();
        assert!(matches!(
            click(&mut state, 200., 72.),
            Some(Action::PlayWorld)
        ));
        state.release();
        state.wheel(-1., view[1], 20);
        assert!(matches!(
            click(&mut state, 200., 90.),
            Some(Action::SelectWorld(2))
        ));
        state.release();
        assert!(click(&mut state, 325., 70.).is_none());
        state.drag(200., view[1], 20);
        assert_eq!(state.scroll, max_scroll(view[1], 20));
        state.release();
        assert!(matches!(
            click(&mut state, 200., 169.),
            Some(Action::SelectWorld(19))
        ));
        state.reset();
        assert_eq!(state.scroll, 0.);
    }

    #[test]
    #[ignore = "requires local original Minecraft resources"]
    fn original_select_rename_and_delete_buttons_match_ko_kr() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-world-menus.json"
        ))
        .unwrap();
        let settings = Settings {
            language: "ko_kr".into(),
            ..Default::default()
        };
        for (name, screen, selected) in [
            ("worlds-empty", Screen::Worlds, false),
            ("worlds", Screen::Worlds, false),
            ("worlds-selected", Screen::Worlds, true),
            ("world-rename", Screen::RenameWorld, true),
            ("world-delete", Screen::DeleteWorld, true),
        ] {
            let mut gui = Gui {
                screen,
                ..Default::default()
            };
            if name != "worlds-empty" {
                let world: WorldEntry = serde_json::from_value(
                    serde_json::json!({"name":"서바이벌 월드","seed":189,"mode":"Survival",
                    "folder":"Survival 189","last_played":1_700_000_000_000i64}),
                )
                .unwrap();
                gui.worlds.push(world);
            }
            gui.selected_world = selected.then_some(0);
            gui.world_list.begin_rename("서바이벌 월드");
            gui.world_list.reference_locale();
            let mut frame = Frame {
                size: [800., 450.],
                captions: Some(vec![]),
                ..Default::default()
            };
            super::super::paint(
                &mut frame,
                &gui,
                &settings,
                resources(),
                assets(),
                None,
                &Stats::default(),
            );
            let expected = oracle[name]["buttons"].as_array().unwrap();
            let captions = frame.captions.as_ref().unwrap();
            assert_eq!(captions.len(), expected.len(), "{name}");
            for ((area, label), row) in captions.iter().zip(expected) {
                let r = [area.x, area.y, area.w, area.h].map(|x| x as i32);
                let actual: [i32; 4] = std::array::from_fn(|i| row[i + 1].as_i64().unwrap() as i32);
                assert_eq!(r, actual, "{name}");
                assert_eq!(label, row[7].as_str().unwrap(), "{name}");
                let button = frame
                    .buttons
                    .iter()
                    .find(|b| b.rect.x == area.x && b.rect.y == area.y && b.rect.w == area.w)
                    .unwrap();
                assert_eq!(button.enabled, row[5].as_bool().unwrap(), "{name}");
            }
        }
    }
}
