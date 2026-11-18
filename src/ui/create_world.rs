use super::{
    text_field::TextField, Action, Mode, Painter, Rect, Resources, Screen, WorldEntry, WHITE,
};

#[path = "world_customizer.rs"]
mod custom;
#[path = "world_presets.rs"]
mod presets;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GameMode {
    #[default]
    Survival,
    Hardcore,
    Creative,
}
impl GameMode {
    pub fn mode(self) -> Mode {
        if self == Self::Creative {
            Mode::Creative
        } else {
            Mode::Survival
        }
    }
    pub fn hardcore(self) -> bool {
        self == Self::Hardcore
    }
    fn key(self) -> &'static str {
        match self {
            Self::Survival => "survival",
            Self::Hardcore => "hardcore",
            Self::Creative => "creative",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WorldType {
    #[default]
    Default,
    Flat,
    LargeBiomes,
    Amplified,
    Customized,
    Debug,
}
impl WorldType {
    const ALL: [Self; 6] = [
        Self::Default,
        Self::Flat,
        Self::LargeBiomes,
        Self::Amplified,
        Self::Customized,
        Self::Debug,
    ];
    pub fn id(self) -> u8 {
        self as u8
    }
    pub fn name(self) -> &'static str {
        [
            "default",
            "flat",
            "largeBiomes",
            "amplified",
            "customized",
            "debug_all_block_states",
        ][self.id() as usize]
    }
    fn parse(s: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|t| t.name().eq_ignore_ascii_case(s))
            .unwrap_or_default()
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Page {
    #[default]
    Basic,
    More,
    Flat,
    FlatPresets,
    Custom,
    CustomPresets,
}
#[derive(Clone, Debug)]
pub enum Command {
    More,
    CycleMode,
    CycleType,
    Structures,
    Cheats,
    Bonus,
    Cursor(usize),
    Customize,
    Presets,
    Select(usize),
    RemoveLayer,
    Done,
    Cancel,
    Custom(custom::Command),
}
pub struct Form {
    name: TextField,
    seed: TextField,
    pub game_mode: GameMode,
    pub world_type: WorldType,
    pub generator_options: String,
    pub structures: bool,
    allow_cheats: bool,
    cheats_set: bool,
    bonus_chest: bool,
    page: Page,
    focused: bool,
    draft: String,
    share: TextField,
    selected: Option<usize>,
    scroll: f32,
    scroll_drag: Option<(f32, f32, f32)>,
    custom: custom::Editor,
}
impl Default for Form {
    fn default() -> Self {
        Self {
            name: TextField::new("New World"),
            seed: Default::default(),
            game_mode: GameMode::Survival,
            world_type: WorldType::Default,
            generator_options: String::new(),
            structures: true,
            allow_cheats: false,
            cheats_set: false,
            bonus_chest: false,
            page: Page::Basic,
            focused: true,
            draft: String::new(),
            share: Default::default(),
            selected: None,
            scroll: 0.,
            scroll_drag: None,
            custom: Default::default(),
        }
    }
}
impl Form {
    pub fn new(r: &Resources, lang: &str) -> Self {
        Self {
            name: TextField::new(&r.tr(lang, "selectWorld.newWorld")),
            ..Default::default()
        }
    }
    pub fn recreate(entry: &WorldEntry, r: &Resources, lang: &str) -> Self {
        let name = r
            .tr(lang, "selectWorld.newWorld.copyOf")
            .replace("%s", &entry.name)
            .replace("%1$s", &entry.name);
        let mut name_field = TextField::default();
        name_field.insert(&name, 32);
        Self {
            name: name_field,
            seed: TextField::new(&(entry.seed as i64).to_string()),
            game_mode: if entry.rules.hardcore {
                GameMode::Hardcore
            } else if entry.mode == Mode::Creative {
                GameMode::Creative
            } else {
                GameMode::Survival
            },
            world_type: WorldType::parse(&entry.rules.world_type),
            generator_options: entry.rules.generator_options.clone(),
            structures: entry.rules.structures,
            allow_cheats: entry.rules.allow_cheats,
            ..Default::default()
        }
    }
    pub fn name(&self) -> &str {
        &self.name.value
    }
    pub fn seed(&self) -> &str {
        &self.seed.value
    }
    pub fn cheats(&self) -> bool {
        self.allow_cheats && !self.game_mode.hardcore()
    }
    pub fn bonus(&self) -> bool {
        self.bonus_chest && !self.game_mode.hardcore()
    }
    fn page(&mut self, page: Page) {
        self.page = page;
        self.scroll = 0.;
        self.scroll_drag = None;
        self.selected = None;
        self.focused = false;
    }
    fn field(&mut self) -> Option<(&mut TextField, usize)> {
        if !self.focused {
            return None;
        }
        match self.page {
            Page::Basic => Some((&mut self.name, 32)),
            Page::More => Some((&mut self.seed, 32)),
            Page::FlatPresets => Some((&mut self.share, 1230)),
            Page::CustomPresets => Some((&mut self.share, 2000)),
            _ => None,
        }
    }
    pub fn text(&mut self, text: &str) {
        if self.page == Page::Custom {
            self.custom.text(text);
        } else if let Some((field, limit)) = self.field() {
            field.insert(text, limit);
        }
    }
    pub fn key(
        &mut self,
        key: winit::keyboard::KeyCode,
        shift: bool,
        ctrl: bool,
    ) -> Option<Action> {
        use winit::keyboard::KeyCode as K;
        if matches!(self.page, Page::Basic | Page::More) && matches!(key, K::Enter | K::NumpadEnter)
        {
            return (!self.name().is_empty()).then_some(Action::CreateWorld);
        }
        if key == K::Escape && !matches!(self.page, Page::Basic | Page::More) {
            return Some(Action::Screen(Screen::Main));
        }
        if self.page == Page::Custom {
            self.custom.key(key, shift, ctrl);
        } else if let Some((field, limit)) = self.field() {
            field.key(key, shift, ctrl, limit);
        }
        None
    }
    fn list(&self, size: [f32; 2]) -> Option<(f32, f32, f32, usize, f32)> {
        let [w, h] = size.map(f32::ceil);
        let (top, bottom, row, count, bar) = match self.page {
            Page::Flat => (
                43.,
                h - 60.,
                24.,
                presets::layer_count(&self.draft),
                (w / 2.).floor() + 124.,
            ),
            Page::FlatPresets => (
                80.,
                h - 37.,
                24.,
                presets::data().flat.len(),
                (w / 2.).floor() + 124.,
            ),
            Page::CustomPresets => (
                80.,
                h - 32.,
                38.,
                presets::data().customized.len(),
                (w / 2.).floor() + 124.,
            ),
            Page::Custom => (
                32.,
                h - 32.,
                25.,
                self.custom.rows(),
                (w / 2.).floor() + 156.,
            ),
            _ => return None,
        };
        Some((top, bottom, row, count, bar))
    }
    pub fn wheel(&mut self, delta: f32, size: [f32; 2]) {
        if let Some((top, bottom, row, count, _)) = self.list(size) {
            self.scroll = (self.scroll - delta.signum() * (row / 2.).floor())
                .clamp(0., (count as f32 * row + 4. - (bottom - top)).max(0.));
        }
    }
    pub fn pointer_down(&mut self, cursor: [f32; 2], size: [f32; 2]) {
        self.focused = false;
        if self.page == Page::Custom {
            self.custom.focus = None;
        }
        if let Some((top, bottom, row, count, bar)) = self.list(size) {
            let total = count as f32 * row;
            let visible = bottom - top;
            let max = total + 4. - visible;
            if max > 0.
                && cursor[1] >= top
                && cursor[1] <= bottom
                && (bar..bar + 6.).contains(&cursor[0])
            {
                let thumb = (visible * visible / total)
                    .floor()
                    .max(32.)
                    .min(visible - 8.);
                self.scroll_drag = Some((cursor[1], self.scroll, max / (visible - thumb).max(1.)));
            }
        }
    }
    pub fn drag(&mut self, cursor: [f32; 2], size: [f32; 2]) {
        if let Some((initial, scroll, factor)) = self.scroll_drag {
            if let Some((top, bottom, row, count, _)) = self.list(size) {
                self.scroll = (scroll + (cursor[1] - initial) * factor)
                    .clamp(0., (count as f32 * row + 4. - (bottom - top)).max(0.));
            }
        }
        if self.page == Page::Custom {
            self.custom.drag(cursor[0]);
        }
    }
    pub fn release(&mut self) {
        self.scroll_drag = None;
        self.custom.release();
    }
    pub fn apply(&mut self, cmd: Command, shift: bool, cursor: [f32; 2]) {
        match cmd {
            Command::More => self.page(if self.page == Page::Basic {
                Page::More
            } else {
                Page::Basic
            }),
            Command::CycleMode if self.world_type != WorldType::Debug => {
                self.game_mode = match self.game_mode {
                    GameMode::Survival => GameMode::Hardcore,
                    GameMode::Hardcore => GameMode::Creative,
                    GameMode::Creative => GameMode::Survival,
                };
                if !self.cheats_set {
                    self.allow_cheats = self.game_mode == GameMode::Creative;
                }
            }
            Command::CycleMode => {}
            Command::CycleType => {
                let index = self.world_type.id() as usize + 1;
                self.world_type = if index == 5 && !shift {
                    WorldType::Default
                } else {
                    WorldType::ALL[index % 6]
                };
                self.generator_options.clear();
            }
            Command::Structures => self.structures = !self.structures,
            Command::Cheats => {
                self.cheats_set = true;
                self.allow_cheats = !self.allow_cheats;
            }
            Command::Bonus => self.bonus_chest = !self.bonus_chest,
            Command::Cursor(i) => {
                self.focused = true;
                if let Some((field, _)) = self.field() {
                    field.set_cursor(i, false);
                }
            }
            Command::Customize => {
                if self.world_type == WorldType::Flat {
                    self.draft = crate::world_types::normalize_flat(&self.generator_options);
                    self.page(Page::Flat);
                } else {
                    self.custom = custom::Editor::open(&self.generator_options);
                    self.page(Page::Custom);
                }
            }
            Command::Presets => {
                let flat = self.page == Page::Flat;
                self.share = TextField::new(&if flat {
                    self.draft.clone()
                } else {
                    self.custom.export()
                });
                self.page(if flat {
                    Page::FlatPresets
                } else {
                    Page::CustomPresets
                });
            }
            Command::Select(i) => {
                self.selected = Some(i);
                if self.page == Page::FlatPresets {
                    if let Some(p) = presets::data().flat.get(i) {
                        self.share = TextField::new(&p.settings);
                    }
                }
                if self.page == Page::CustomPresets {
                    if let Some(p) = presets::data().customized.get(i) {
                        self.share = TextField::new(&p.settings);
                    }
                }
            }
            Command::RemoveLayer => {
                if let Some(i) = self.selected {
                    presets::remove_layer(&mut self.draft, i);
                    self.selected = self
                        .selected
                        .filter(|&v| v < presets::layer_count(&self.draft));
                }
            }
            Command::Done => match self.page {
                Page::Flat => {
                    self.generator_options = self.draft.clone();
                    self.page(Page::More);
                }
                Page::Custom => {
                    self.generator_options = self.custom.export();
                    self.page(Page::More);
                }
                Page::FlatPresets => {
                    self.draft = crate::world_types::normalize_flat(&self.share.value);
                    self.page(Page::Flat);
                }
                Page::CustomPresets => {
                    self.custom = custom::Editor::open(&self.share.value);
                    self.page(Page::Custom);
                }
                _ => {}
            },
            Command::Cancel => self.page(match self.page {
                Page::FlatPresets => Page::Flat,
                Page::CustomPresets => Page::Custom,
                _ => Page::More,
            }),
            Command::Custom(cmd) => {
                if self.custom.apply(cmd, cursor[0]) {
                    self.scroll = 0.;
                }
            }
        }
    }
    pub fn capture(&mut self, screen: &str, _resources: &Resources, _lang: &str) {
        self.game_mode = if screen.contains("hardcore") {
            GameMode::Hardcore
        } else if screen.contains("creative") {
            self.allow_cheats = true;
            GameMode::Creative
        } else {
            self.game_mode
        };
        if screen.ends_with("more") {
            self.page(Page::More);
            return;
        }
        let world_type = match screen {
            "create-flat" | "flat" | "flat-presets" => WorldType::Flat,
            "create-customized" | "customized" | "customized-2" | "customized-3"
            | "customized-4" | "customized-presets" => WorldType::Customized,
            "create-large-biomes" => WorldType::LargeBiomes,
            "create-amplified" => WorldType::Amplified,
            _ => return,
        };
        self.world_type = world_type;
        self.page(Page::More);
        if screen.starts_with("create-") {
            return;
        }
        self.apply(Command::Customize, false, [0.; 2]);
        if screen.ends_with("presets") {
            self.apply(Command::Presets, false, [0.; 2]);
        } else if let Some(n) = screen
            .strip_prefix("customized-")
            .and_then(|n| n.parse::<usize>().ok())
        {
            self.custom.page = n.saturating_sub(1).min(3);
        }
    }
}

fn rect(x: f32, y: f32, w: f32) -> Rect {
    Rect { x, y, w, h: 20. }
}
fn button(p: &mut Painter<'_>, r: Rect, label: &str, cmd: Command, enabled: bool) {
    p.button(r, label, Action::WorldForm(cmd), enabled);
}
fn caption(p: &Painter<'_>, key: &str, enabled: bool) -> String {
    format!(
        "{} {}",
        p.tr(key),
        p.tr(if enabled { "options.on" } else { "options.off" })
    )
}
pub(super) fn draw(p: &mut Painter<'_>, form: &Form, entries: &[WorldEntry], time: f32) {
    let [w, h] = p.frame.size.map(f32::ceil);
    let cx = (w / 2.).floor();
    p.background();
    if form.page == Page::Custom {
        custom::draw(p, form, time);
        return;
    }
    if !matches!(form.page, Page::Basic | Page::More) {
        presets::draw(p, form, time);
        return;
    }
    p.center(cx, 20., &p.tr("selectWorld.create"), WHITE);
    let left = cx - 100.;
    let gray = [160. / 255., 160. / 255., 160. / 255., 1.];
    if form.page == Page::More {
        p.text(left, 47., &p.tr("selectWorld.enterSeed"), gray);
        p.text(left, 85., &p.tr("selectWorld.seedInfo"), gray);
        form.seed
            .draw(p, rect(left, 60., 200.), form.focused, time, 32, |i| {
                Action::WorldForm(Command::Cursor(i))
            });
        if !matches!(form.world_type, WorldType::Debug | WorldType::Customized) {
            p.text(cx - 150., 122., &p.tr("selectWorld.mapFeatures.info"), gray);
            button(
                p,
                rect(cx - 155., 100., 150.),
                &caption(p, "selectWorld.mapFeatures", form.structures),
                Command::Structures,
                true,
            );
        }
        if form.world_type != WorldType::Debug {
            p.text(
                cx - 150.,
                172.,
                &p.tr("selectWorld.allowCommands.info"),
                gray,
            );
            button(
                p,
                rect(cx - 155., 151., 150.),
                &caption(p, "selectWorld.allowCommands", form.cheats()),
                Command::Cheats,
                !form.game_mode.hardcore(),
            );
            button(
                p,
                rect(cx + 5., 151., 150.),
                &caption(p, "selectWorld.bonusItems", form.bonus()),
                Command::Bonus,
                !form.game_mode.hardcore(),
            );
        }
        let typ = p.tr(&format!("generator.{}", form.world_type.name()));
        button(
            p,
            rect(cx + 5., 100., 150.),
            &format!("{} {typ}", p.tr("selectWorld.mapType")),
            Command::CycleType,
            true,
        );
        if matches!(form.world_type, WorldType::Customized | WorldType::Flat) {
            button(
                p,
                rect(cx + 5., 120., 150.),
                &p.tr("selectWorld.customizeType"),
                Command::Customize,
                true,
            );
        }
        if form.world_type == WorldType::Amplified {
            for (i, line) in super::options::wrapped(p, &p.tr("generator.amplified.info"), 150.)
                .iter()
                .enumerate()
            {
                p.draw_text_run(cx + 7., 122. + i as f32 * 9., line, gray, false);
            }
        }
    } else {
        p.text(left, 47., &p.tr("selectWorld.enterName"), gray);
        let folder = crate::world_library::folder_name(form.name(), entries);
        p.text(
            left,
            85.,
            &format!("{} {folder}", p.tr("selectWorld.resultFolder")),
            gray,
        );
        form.name
            .draw(p, rect(left, 60., 200.), form.focused, time, 32, |i| {
                Action::WorldForm(Command::Cursor(i))
            });
        let mode = if form.world_type == WorldType::Debug {
            "spectator"
        } else {
            form.game_mode.key()
        };
        let key = format!("selectWorld.gameMode.{mode}");
        let label = format!("{}: {}", p.tr("selectWorld.gameMode"), p.tr(&key));
        button(
            p,
            rect(cx - 75., 115., 150.),
            &label,
            Command::CycleMode,
            form.world_type != WorldType::Debug,
        );
        p.text(left, 137., &p.tr(&format!("{key}.line1")), gray);
        p.text(left, 149., &p.tr(&format!("{key}.line2")), gray);
    }
    let more = if form.page == Page::Basic {
        "selectWorld.moreWorldOptions"
    } else {
        "gui.done"
    };
    button(
        p,
        rect(cx - 75., 187., 150.),
        &p.tr(more),
        Command::More,
        true,
    );
    p.button(
        rect(cx - 155., h - 28., 150.),
        &p.tr("selectWorld.create"),
        Action::CreateWorld,
        !form.name().is_empty(),
    );
    p.button(
        rect(cx + 5., h - 28., 150.),
        &p.tr("gui.cancel"),
        Action::Screen(Screen::Worlds),
        true,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;
    use crate::ui::{paint, Frame, Gui, Stats};
    use crate::validation::{assets, resources};
    use winit::keyboard::KeyCode;

    #[test]
    #[ignore = "uses the installed original game language resources"]
    fn installed_189_create_pages_match_button_geometry_states_and_korean_captions() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-world-menus.json"
        ))
        .unwrap();
        let settings = Settings {
            language: "ko_kr".into(),
            ..Default::default()
        };
        for (name, original) in fixture.as_object().unwrap() {
            if name.starts_with("world") {
                continue;
            }
            let mut form = Form::new(resources(), &settings.language);
            form.capture(name, resources(), &settings.language);
            let gui = Gui {
                create: form,
                screen: Screen::Create,
                ..Default::default()
            };
            let mut frame = Frame {
                size: [800., 450.],
                captions: Some(Vec::new()),
                ..Default::default()
            };
            paint(
                &mut frame,
                &gui,
                &settings,
                resources(),
                assets(),
                None,
                &Stats::default(),
            );
            for row in original["buttons"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|r| r[6] == true)
            {
                let coords: [i32; 4] = std::array::from_fn(|i| row[i + 1].as_i64().unwrap() as i32);
                let found = frame
                    .buttons
                    .iter()
                    .find(|b| [b.rect.x, b.rect.y, b.rect.w, b.rect.h].map(|v| v as i32) == coords)
                    .unwrap_or_else(|| panic!("{name} missing original button {row}"));
                assert_eq!(
                    found.enabled,
                    row[5].as_bool().unwrap(),
                    "{name} {coords:?}"
                );
                let label = frame
                    .captions
                    .as_ref()
                    .unwrap()
                    .iter()
                    .find(|(r, _)| [r.x, r.y, r.w, r.h].map(|v| v as i32) == coords)
                    .unwrap();
                assert_eq!(label.1, row[7].as_str().unwrap(), "{name} {coords:?}");
            }
        }
    }
    #[test]
    fn editing_and_cancel_follow_original_create_world_flow() {
        let mut form = Form::default();
        form.key(KeyCode::KeyA, false, true);
        form.text("테스트");
        assert_eq!(form.name(), "테스트");
        let click = |f: &mut Form, c| f.apply(c, false, [0.; 2]);
        click(&mut form, Command::CycleMode);
        assert!(form.game_mode.hardcore());
        click(&mut form, Command::CycleMode);
        assert!(form.cheats());
        assert_eq!(form.game_mode.mode(), Mode::Creative);
        click(&mut form, Command::More);
        click(&mut form, Command::Cheats);
        click(&mut form, Command::Cursor(0));
        form.text("-42");
        assert_eq!(form.seed(), "-42");
        assert!(!form.cheats());
        click(&mut form, Command::CycleType);
        click(&mut form, Command::Customize);
        click(&mut form, Command::Presets);
        click(&mut form, Command::Select(2));
        click(&mut form, Command::Done);
        click(&mut form, Command::Select(0));
        click(&mut form, Command::RemoveLayer);
        click(&mut form, Command::Done);
        assert!(form.generator_options.contains("5*minecraft:sand"));
        assert!(!form.generator_options.contains("90*minecraft:water"));
        let applied = form.generator_options.clone();
        click(&mut form, Command::Customize);
        click(&mut form, Command::Select(0));
        click(&mut form, Command::RemoveLayer);
        click(&mut form, Command::Cancel);
        assert_eq!(form.generator_options, applied);
        click(&mut form, Command::CycleType);
        click(&mut form, Command::CycleType);
        click(&mut form, Command::CycleType);
        assert_eq!(form.world_type, WorldType::Customized);
        click(&mut form, Command::Customize);
        form.apply(
            Command::Custom(custom::Command::Slide(0, 0.)),
            false,
            [146., 0.],
        );
        form.release();
        click(&mut form, Command::Done);
        let value: serde_json::Value = serde_json::from_str(&form.generator_options).unwrap();
        assert_eq!(value["seaLevel"], 255);
    }
}
