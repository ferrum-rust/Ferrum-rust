use super::{Action, Button, Gui, Painter, Rect, Screen, Stats, GRAY, WHITE};
use crate::{gameplay::Simulation, resource_packs, settings::Settings};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Skin,
    Sounds,
    Chat,
    Snooper,
    Packs,
    Broadcast,
    LockDifficulty,
    IncompatiblePack,
}
impl Page {
    pub fn back(self) -> Screen {
        if self == Self::IncompatiblePack {
            Screen::OptionPage(Self::Packs)
        } else {
            Screen::Options
        }
    }
}
#[derive(Clone, Debug)]
pub enum Command {
    Slider(Slider),
    Toggle(Toggle),
    CycleDifficulty,
    LockDifficulty,
    CycleShader,
    OpenPackFolder,
    OpenStreamHelp,
    AddPack(String),
    ConfirmPack,
    RemovePack(usize),
    MovePack(usize, i32),
    ApplyPacks,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slider {
    Fov,
    Sound(u8),
    ChatOpacity,
    ChatScale,
    ChatFocused,
    ChatUnfocused,
    ChatWidth,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggle {
    Skin(u8),
    ChatVisibility,
    ChatColors,
    ChatLinks,
    ChatPrompt,
    ReducedDebug,
    Unicode,
    Realms,
    Snooper,
}
pub enum Effect {
    None,
    ReloadPacks(Vec<String>),
    CycleShader,
    CycleDifficulty,
    LockDifficulty,
    OpenPackFolder,
    OpenStreamHelp,
}
pub struct State {
    pub language_parent: Screen,
    pub stream_parent: Screen,
    pub difficulty_locked: bool,
    pub hardcore: bool,
    pub remote: bool,
    pub selected: Vec<String>,
    pub(crate) packs: Vec<resource_packs::Pack>,
    pending: Option<String>,
    pub(crate) error: String,
    scroll: [f32; 2],
    drag: Option<(usize, f32, f32)>,
    device_stats: std::collections::BTreeMap<String, String>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            language_parent: Screen::Options,
            stream_parent: Screen::Options,
            difficulty_locked: false,
            hardcore: false,
            remote: false,
            selected: Vec::new(),
            packs: Vec::new(),
            pending: None,
            error: String::new(),
            scroll: [0.; 2],
            drag: None,
            device_stats: Default::default(),
        }
    }
}
impl State {
    pub fn set_device(&mut self, renderer: &crate::renderer::Renderer) {
        self.device_stats = renderer.menu_device_stats();
        for (key, value) in system_memory() {
            self.device_stats.insert(key.into(), value.to_string());
        }
    }
    pub fn end_scroll(&mut self) {
        self.drag = None;
    }
    pub fn reset_scroll(&mut self) {
        self.scroll = [0.; 2];
        self.end_scroll();
    }
    pub fn open_packs(&mut self, current: &[String]) {
        self.error.clear();
        self.packs = resource_packs::discover().unwrap_or_else(|e| {
            self.error = e.to_string();
            Vec::new()
        });
        self.selected = current
            .iter()
            .filter(|n| self.packs.iter().any(|p| p.name == **n))
            .cloned()
            .collect();
        self.pending = None;
    }
}
#[cfg(windows)]
fn system_memory() -> Vec<(&'static str, u64)> {
    #[repr(C)]
    struct Memory {
        size: u32,
        load: u32,
        total: u64,
        available: u64,
        page: u64,
        available_page: u64,
        virtual_total: u64,
        available_virtual: u64,
        extended: u64,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalMemoryStatusEx(state: *mut Memory) -> i32;
    }
    // Win32 fills this size-prefixed structure without retaining its address.
    let mut state: Memory = unsafe { std::mem::zeroed() };
    state.size = std::mem::size_of::<Memory>() as u32;
    if unsafe { GlobalMemoryStatusEx(&mut state) } == 0 {
        return Vec::new();
    }
    vec![
        ("system_memory_available_bytes", state.available),
        ("system_memory_total_bytes", state.total),
    ]
}
#[cfg(not(windows))]
fn system_memory() -> Vec<(&'static str, u64)> {
    Vec::new()
}

struct ScrollList {
    top: f32,
    bottom: f32,
    left: f32,
    right: f32,
    bar_x: f32,
    row: f32,
    total: f32,
    index: usize,
}
impl ScrollList {
    fn max(&self) -> f32 {
        (self.total - (self.bottom - self.top)).max(0.)
    }
    fn thumb(&self) -> f32 {
        let h = self.bottom - self.top;
        (h * h / (self.total - 4.).max(1.))
            .floor()
            .clamp(32f32.min(h), h)
            .min((h - 8.).max(1.))
    }
}
fn list(
    gui: &Gui,
    resources: &crate::resources::Resources,
    viewport: [f32; 2],
    index: usize,
) -> Option<ScrollList> {
    let [w, h] = viewport.map(f32::ceil);
    let cx = (w / 2.).floor();
    match gui.screen {
        Screen::Languages => Some(ScrollList {
            top: 32.,
            bottom: h - 61.,
            left: 0.,
            right: w,
            bar_x: cx + 124.,
            row: 18.,
            total: resources.languages.len() as f32 * 18. + 4.,
            index: 0,
        }),
        Screen::OptionPage(Page::Snooper) => Some(ScrollList {
            top: 80.,
            bottom: h - 40.,
            left: 0.,
            right: w,
            bar_x: w - 10.,
            row: 10.,
            total: (gui.option_state.device_stats.len() + 13) as f32 * 10. + 4.,
            index: 0,
        }),
        Screen::OptionPage(Page::Packs) => {
            let left = cx - 204. + index as f32 * 208.;
            let rows = if index == 0 {
                gui.option_state
                    .packs
                    .len()
                    .saturating_sub(gui.option_state.selected.len())
            } else {
                gui.option_state.selected.len() + 1
            };
            Some(ScrollList {
                top: 32.,
                bottom: h - 51.,
                left,
                right: left + 200.,
                bar_x: left + 194.,
                row: 36.,
                total: rows as f32 * 36. + 17.,
                index,
            })
        }
        _ => None,
    }
}
fn hovered_list(
    gui: &Gui,
    resources: &crate::resources::Resources,
    size: [f32; 2],
) -> Option<ScrollList> {
    let index = usize::from(
        gui.screen == Screen::OptionPage(Page::Packs) && gui.cursor[0] >= size[0] * 0.5,
    );
    list(gui, resources, size, index).filter(|l| {
        gui.cursor[0] >= l.left
            && gui.cursor[0] < l.right
            && gui.cursor[1] >= l.top
            && gui.cursor[1] < l.bottom
    })
}
pub fn wheel(
    gui: &mut Gui,
    resources: &crate::resources::Resources,
    size: [f32; 2],
    delta: f32,
) -> bool {
    let Some(l) = hovered_list(gui, resources, size) else {
        return false;
    };
    gui.option_state.scroll[l.index] = (gui.option_state.scroll[l.index]
        - delta.signum() * (l.row * 0.5).floor())
    .clamp(0., l.max());
    true
}
pub fn begin_scroll(
    gui: &mut Gui,
    resources: &crate::resources::Resources,
    size: [f32; 2],
) -> bool {
    let Some(l) = hovered_list(gui, resources, size) else {
        return false;
    };
    let handle = gui.cursor[0] >= l.bar_x && gui.cursor[0] < l.bar_x + 6.;
    let multiplier = if handle {
        l.max() / (l.bottom - l.top - l.thumb()).max(1.)
    } else {
        -1.
    };
    gui.option_state.drag = Some((l.index, gui.cursor[1], multiplier));
    handle
}
pub fn drag_scroll(gui: &mut Gui, resources: &crate::resources::Resources, size: [f32; 2]) -> bool {
    let Some((idx, last_y, speed)) = gui.option_state.drag else {
        return false;
    };
    let Some(l) = list(gui, resources, size, idx) else {
        return false;
    };
    gui.option_state.scroll[idx] =
        (gui.option_state.scroll[idx] + (gui.cursor[1] - last_y) * speed).clamp(0., l.max());
    gui.option_state.drag = Some((idx, gui.cursor[1], speed));
    true
}
const SOUND: [&str; 9] = [
    "master", "music", "record", "weather", "block", "hostile", "neutral", "player", "ambient",
];
const PARTS: [&str; 7] = [
    "cape",
    "jacket",
    "left_sleeve",
    "right_sleeve",
    "left_pants_leg",
    "right_pants_leg",
    "hat",
];
const LEVELS: [&str; 4] = [
    "options.difficulty.peaceful",
    "options.difficulty.easy",
    "options.difficulty.normal",
    "options.difficulty.hard",
];
impl Slider {
    pub fn set(self, settings: &mut Settings, rect: Rect, mouse_x: f32) {
        let v = ((mouse_x - rect.x - 4.) / (rect.w - 8.)).clamp(0., 1.);
        match self {
            Self::Fov => settings.fov = (30. + 80. * v).round(),
            Self::Sound(0) => settings.volume = v,
            Self::Sound(i) => {
                if let Some(s) = settings.menu.sound_levels.get_mut(i as usize - 1) {
                    *s = v;
                }
            }
            Self::ChatOpacity => settings.menu.chat_opacity = v,
            Self::ChatScale => settings.menu.chat_scale = v,
            Self::ChatFocused => settings.menu.chat_height_focused = v,
            Self::ChatUnfocused => settings.menu.chat_height_unfocused = v,
            Self::ChatWidth => settings.menu.chat_width = v,
        }
    }
    fn value(self, s: &Settings) -> f32 {
        match self {
            Self::Fov => (s.fov - 30.) / 80.,
            Self::Sound(0) => s.volume,
            Self::Sound(i) => s.menu.sound_levels[i as usize - 1],
            Self::ChatOpacity => s.menu.chat_opacity,
            Self::ChatScale => s.menu.chat_scale,
            Self::ChatFocused => s.menu.chat_height_focused,
            Self::ChatUnfocused => s.menu.chat_height_unfocused,
            Self::ChatWidth => s.menu.chat_width,
        }
    }
    fn label(self, p: &Painter<'_>) -> String {
        let v = self.value(p.s);
        let (key, label) = match self {
            Self::Fov => (
                "options.fov",
                if p.s.fov == 70. {
                    p.tr("options.fov.min")
                } else if p.s.fov == 110. {
                    p.tr("options.fov.max")
                } else {
                    format!("{}", p.s.fov as i32)
                },
            ),
            Self::Sound(i) => {
                return format!(
                    "{}: {}",
                    p.tr(&format!("soundCategory.{}", SOUND[i as usize])),
                    if v == 0. {
                        p.tr("options.off")
                    } else {
                        format!("{}%", (v * 100.) as i32)
                    }
                )
            }
            Self::ChatOpacity => (
                "options.chat.opacity",
                format!("{}%", (v * 90. + 10.) as i32),
            ),
            Self::ChatScale => (
                "options.chat.scale",
                if v == 0. {
                    p.tr("options.off")
                } else {
                    format!("{}%", (v * 100.) as i32)
                },
            ),
            Self::ChatFocused => (
                "options.chat.height.focused",
                format!("{}px", (v * 160. + 20.) as i32),
            ),
            Self::ChatUnfocused => (
                "options.chat.height.unfocused",
                format!("{}px", (v * 160. + 20.) as i32),
            ),
            Self::ChatWidth => (
                "options.chat.width",
                format!("{}px", (v * 280. + 40.) as i32),
            ),
        };
        format!("{}: {}", p.tr(key), label)
    }
}
pub fn activate(command: Command, gui: &mut Gui, s: &mut Settings) -> Effect {
    let m = &mut s.menu;
    match command {
        Command::Toggle(t) => match t {
            Toggle::Skin(i) => m.skin_parts ^= 1 << i,
            Toggle::ChatVisibility => m.chat_visibility = (m.chat_visibility + 1) % 3,
            Toggle::ChatColors => m.chat_colors = !m.chat_colors,
            Toggle::ChatLinks => m.chat_links = !m.chat_links,
            Toggle::ChatPrompt => m.chat_links_prompt = !m.chat_links_prompt,
            Toggle::ReducedDebug => m.reduced_debug = !m.reduced_debug,
            Toggle::Unicode => m.force_unicode = !m.force_unicode,
            Toggle::Realms => m.realms_notifications = !m.realms_notifications,
            Toggle::Snooper => m.snooper = !m.snooper,
        },
        Command::CycleDifficulty => return Effect::CycleDifficulty,
        Command::LockDifficulty => return Effect::LockDifficulty,
        Command::CycleShader => return Effect::CycleShader,
        Command::OpenPackFolder => return Effect::OpenPackFolder,
        Command::OpenStreamHelp => return Effect::OpenStreamHelp,
        Command::AddPack(name) => {
            let Some(pack) = gui.option_state.packs.iter().find(|p| p.name == name) else {
                return Effect::None;
            };
            if !gui.option_state.selected.contains(&name) {
                if pack.format == 1 {
                    gui.option_state.selected.insert(0, name);
                } else {
                    gui.option_state.pending = Some(name);
                    gui.open(Screen::OptionPage(Page::IncompatiblePack));
                }
            }
        }
        Command::ConfirmPack => {
            if let Some(name) = gui.option_state.pending.take() {
                gui.option_state.selected.insert(0, name);
            }
            gui.open(Screen::OptionPage(Page::Packs));
        }
        Command::RemovePack(i) => {
            if i < gui.option_state.selected.len() {
                gui.option_state.selected.remove(i);
            }
        }
        Command::MovePack(i, d) => {
            if let Some(j) = i.checked_add_signed(d as isize).filter(|j| {
                *j < gui.option_state.selected.len() && i < gui.option_state.selected.len()
            }) {
                gui.option_state.selected.swap(i, j);
            }
        }
        Command::ApplyPacks => {
            if gui.option_state.selected != m.resource_packs {
                return Effect::ReloadPacks(gui.option_state.selected.clone());
            }
            gui.open(Screen::Options);
        }
        Command::Slider(_) => {}
    }
    Effect::None
}
fn rect(x: f32, y: f32, w: f32) -> Rect {
    Rect { x, y, w, h: 20. }
}
fn cell(cx: f32, top: f32, i: usize) -> Rect {
    rect(
        cx - 155. + (i % 2) as f32 * 160.,
        top + (i / 2) as f32 * 24.,
        150.,
    )
}
fn done(p: &mut Painter<'_>, cx: f32, y: f32, screen: Screen) {
    p.button(
        rect(cx - 100., y, 200.),
        &p.tr("gui.done"),
        Action::Screen(screen),
        true,
    );
}
fn toggle(p: &mut Painter<'_>, r: Rect, t: Toggle) {
    let m = &p.s.menu;
    let (key, val) = match t {
        Toggle::Skin(i) => (
            format!("options.modelPart.{}", PARTS[i as usize]),
            m.skin_parts & (1 << i) != 0,
        ),
        Toggle::ChatVisibility => {
            let label = format!(
                "{}: {}",
                p.tr("options.chat.visibility"),
                p.tr([
                    "options.chat.visibility.full",
                    "options.chat.visibility.system",
                    "options.chat.visibility.hidden"
                ][m.chat_visibility.min(2) as usize])
            );
            p.button(r, &label, Action::Option(Command::Toggle(t)), true);
            return;
        }
        Toggle::ChatColors => ("options.chat.color".into(), m.chat_colors),
        Toggle::ChatLinks => ("options.chat.links".into(), m.chat_links),
        Toggle::ChatPrompt => ("options.chat.links.prompt".into(), m.chat_links_prompt),
        Toggle::ReducedDebug => ("options.reducedDebugInfo".into(), m.reduced_debug),
        Toggle::Unicode => ("options.forceUnicodeFont".into(), m.force_unicode),
        Toggle::Realms => ("options.realmsNotifications".into(), m.realms_notifications),
        Toggle::Snooper => ("options.snooper".into(), m.snooper),
    };
    let label = format!(
        "{}: {}",
        p.tr(&key),
        p.tr(if val { "options.on" } else { "options.off" })
    );
    p.button(r, &label, Action::Option(Command::Toggle(t)), true);
}
fn slider(p: &mut Painter<'_>, r: Rect, s: Slider) {
    let half = (r.w / 2.).floor();
    let image = "textures/gui/widgets.png";
    p.image(image, Rect { w: half, ..r }, [0., 46., half, 20.], WHITE);
    p.image(
        image,
        Rect {
            x: r.x + half,
            w: r.w - half,
            ..r
        },
        [200. - (r.w - half), 46., r.w - half, 20.],
        WHITE,
    );
    let x = r.x + (s.value(p.s) * (r.w - 8.)).floor();
    p.image(image, rect(x, r.y, 4.), [0., 66., 4., 20.], WHITE);
    p.image(image, rect(x + 4., r.y, 4.), [196., 66., 4., 20.], WHITE);
    let label = s.label(p);
    #[cfg(test)]
    if let Some(labels) = &mut p.frame.captions {
        labels.push((r, label.clone()));
    }
    p.center(
        r.x + half,
        r.y + 6.,
        &label,
        if r.contains(p.mouse) {
            [1., 1., 160. / 255., 1.]
        } else {
            [224. / 255., 224. / 255., 224. / 255., 1.]
        },
    );
    p.frame.buttons.push(Button {
        rect: r,
        action: Action::Option(Command::Slider(s)),
        enabled: true,
    });
}
fn title(p: &mut Painter<'_>, cx: f32, y: f32, key: &str) {
    p.center(cx, y, &p.tr(key), WHITE);
}
pub(super) fn wrapped(p: &Painter<'_>, text: &str, width: f32) -> Vec<String> {
    let mut result = Vec::new();
    for paragraph in text.lines() {
        let mut line = String::new();
        for ch in paragraph.chars() {
            line.push(ch);
            if p.width(&line) > width && line.len() > ch.len_utf8() {
                let split = line
                    .rfind(' ')
                    .filter(|i| *i > 0)
                    .unwrap_or(line.len() - ch.len_utf8());
                result.push(line[..split].to_owned());
                line = line[split..].trim_start().to_owned();
            }
        }
        result.push(line);
    }
    result
}
pub(super) fn dirt(p: &mut Painter<'_>, area: Rect, origin: [f32; 2], value: f32) {
    let tex = "textures/gui/options_background.png";
    let sprite = p.r.sprite(tex);
    let right = area.x + area.w;
    let bottom = area.y + area.h;
    let mut y = area.y;
    while y < bottom {
        let v = (y + origin[1]).rem_euclid(32.);
        let h = (32. - v).min(bottom - y);
        let mut x = area.x;
        while x < right {
            let u = (x + origin[0]).rem_euclid(32.);
            let w = (32. - u).min(right - x);
            p.sprite(
                sprite,
                Rect { x, y, w, h },
                [u * 0.5, v * 0.5, w * 0.5, h * 0.5],
                [value, value, value, 1.],
            );
            x += w;
        }
        y += h;
    }
}
fn list_edges(p: &mut Painter<'_>, x: f32, w: f32, top: f32, bottom: f32) {
    for (y, reverse) in [(top, false), (bottom - 4., true)] {
        let from = p.frame.vertices.len();
        p.rect(x, y, w, 4., [0., 0., 0., 1.]);
        for (i, v) in p.frame.vertices[from..].iter_mut().enumerate() {
            let upper = matches!(i, 0 | 3 | 5);
            v.color[3] = f32::from(upper != reverse);
        }
    }
}
pub(super) fn mask_outside(p: &mut Painter<'_>, x: f32, w: f32, top: f32, bottom: f32) {
    dirt(
        p,
        Rect {
            x,
            y: 0.,
            w,
            h: top,
        },
        [-x, 0.],
        64. / 255.,
    );
    dirt(
        p,
        Rect {
            x,
            y: bottom,
            w,
            h: p.frame.size[1] - bottom,
        },
        [-x, 0.],
        64. / 255.,
    );
    list_edges(p, x, w, top, bottom);
}
pub(super) fn scrollbar(p: &mut Painter<'_>, x: f32, top: f32, bottom: f32, content: f32, scroll: f32) {
    let height = bottom - top;
    let total = content + 4.;
    if total <= height || height <= 0. {
        return;
    }
    let thumb = (height * height / content)
        .floor()
        .clamp(32f32.min(height), height)
        .min((height - 8.).max(1.));
    let y = top
        + ((height - thumb) * scroll / (total - height))
            .floor()
            .clamp(0., height - thumb);
    p.rect(x, top, 6., height, [0., 0., 0., 1.]);
    p.rect(x, y, 6., thumb, [0.5, 0.5, 0.5, 1.]);
    p.rect(x, y, 5., thumb - 1., [0.75, 0.75, 0.75, 1.]);
}
fn draw_snooper(p: &mut Painter<'_>, gui: &Gui, stats: &Stats<'_>) {
    let [w, h] = p.frame.size.map(f32::ceil);
    let cx = (w / 2.).floor();
    let mut values = gui.option_state.device_stats.clone();
    let live = [
        (
            "cpu_cores",
            std::thread::available_parallelism()
                .map(usize::from)
                .unwrap_or(1)
                .to_string(),
        ),
        (
            "current_action",
            if gui.options_parent == Screen::Pause {
                "singleplayer"
            } else {
                "idle"
            }
            .into(),
        ),
        (
            "display_type",
            if p.s.fullscreen {
                "fullscreen"
            } else {
                "windowed"
            }
            .into(),
        ),
        (
            "endianness",
            if cfg!(target_endian = "little") {
                "little"
            } else {
                "big"
            }
            .into(),
        ),
        ("fps", format!("{}", stats.fps as u32)),
        ("run_time", format!("{}", stats.time as u64)),
        ("language", p.s.language.clone()),
        ("os_architecture", std::env::consts::ARCH.into()),
        ("os_name", std::env::consts::OS.into()),
        ("render_distance", p.s.render_distance.to_string()),
        ("resource_packs", p.s.menu.resource_packs.len().to_string()),
        ("version", "1.8.9".into()),
        ("vsync_enabled", p.s.vsync.to_string()),
    ];
    values.extend(live.into_iter().map(|(k, v)| (k.to_owned(), v)));
    let bottom = h - 40.;
    let offset = gui.option_state.scroll[0]
        .min((values.len() as f32 * 10. + 4. - (bottom - 80.)).max(0.))
        .floor();
    dirt(
        p,
        Rect {
            x: 0.,
            y: 80.,
            w,
            h: bottom - 80.,
        },
        [0., offset],
        32. / 255.,
    );
    for (i, (key, val)) in values.iter().enumerate() {
        let y = 84. + 10. * i as f32 - offset;
        if y + 8. <= 80. || y >= bottom {
            continue;
        }
        let mut width = 0.;
        let end = val.char_indices().find_map(|(index, c)| {
            width += p.glyph_width(c);
            (width > w - 220.).then_some(index)
        }).unwrap_or(val.len());
        p.text_pass(10., y, key, WHITE);
        p.text_pass(230., y, &val[..end], WHITE);
    }
    mask_outside(p, 0., w, 80., bottom);
    title(p, cx, 8., "options.snooper.title");
    for (i, line) in wrapped(p, &p.tr("options.snooper.desc"), w - 30.)
        .into_iter()
        .enumerate()
    {
        p.center(cx, 22. + 9. * i as f32, &line, [0.5, 0.5, 0.5, 1.]);
    }
    scrollbar(p, w - 10., 80., bottom, values.len() as f32 * 10., offset);
    toggle(p, rect(cx - 152., h - 30., 150.), Toggle::Snooper);
    p.button(
        rect(cx + 2., h - 30., 150.),
        &p.tr("gui.done"),
        Action::Screen(Screen::Options),
        true,
    );
}
fn draw_packs(p: &mut Painter<'_>, gui: &Gui) {
    let [w, h] = p.frame.size.map(f32::ceil);
    let cx = (w / 2.).floor();
    let bottom = h - 51.;
    let state = &gui.option_state;
    let available: Vec<_> = state
        .packs
        .iter()
        .filter(|pack| !state.selected.contains(&pack.name))
        .map(Some)
        .collect();
    let mut selected: Vec<_> = state
        .selected
        .iter()
        .filter_map(|name| state.packs.iter().find(|pack| pack.name == *name))
        .map(Some)
        .collect();
    selected.push(None);
    for (column, rows) in [available, selected].into_iter().enumerate() {
        let x = cx - 204. + column as f32 * 208.;
        let key = if column == 0 {
            "resourcePack.available.title"
        } else {
            "resourcePack.selected.title"
        };
        let offset = gui.option_state.scroll[column]
            .min((rows.len() as f32 * 36. + 17. - (bottom - 32.)).max(0.))
            .floor();
        dirt(
            p,
            Rect {
                x,
                y: 32.,
                w: 200.,
                h: bottom - 32.,
            },
            [0., offset],
            32. / 255.,
        );
        let y = (36. - offset).min(35.);
        if y + 9. > 32. {
            let label = format!("§n§l{}", p.tr(key));
            p.text_pass(
                x + 102. - (p.width(&label) * 0.5).floor(),
                y,
                &label,
                WHITE,
            );
        }
        let num = rows.len();
        for (i, pack) in rows.into_iter().enumerate() {
            let y = 49. + i as f32 * 36. - offset;
            if y + 32. <= 32. || y >= bottom {
                continue;
            }
            let icon = Rect {
                x: x + 2.,
                y,
                w: 32.,
                h: 32.,
            };
            let hovering = Rect { w: 192., ..icon }.contains(p.mouse);
            let name = pack.map_or("Default", |v| v.name.as_str());
            let incompatible = pack.is_some_and(|v| v.format != 1);
            if incompatible {
                p.rect(x + 1., y - 1., 190., 34., [0.46, 0.16, 0., 1.]);
            }
            let icon_key = crate::resource_packs::icon_key(pack.map(|v| v.name.as_str()));
            let sprite =
                p.r.sprites
                    .get(&icon_key)
                    .copied()
                    .unwrap_or_else(|| p.r.sprite(&crate::resource_packs::icon_key(None)));
            p.sprite(
                sprite,
                icon,
                [0., 0., sprite.width as f32, sprite.height as f32],
                WHITE,
            );
            let mut label = name.to_owned();
            let mut desc = pack.map_or_else(
                || "The default look of Minecraft".into(),
                |v| v.description.clone(),
            );
            if let Some(pack) = pack {
                if hovering {
                    if incompatible {
                        label = p.tr("resourcePack.incompatible");
                        desc = p.tr(if pack.format < 1 {
                            "resourcePack.incompatible.old"
                        } else {
                            "resourcePack.incompatible.new"
                        });
                    }
                    p.rect(icon.x, y, 32., 32., [0.06, 0.06, 0.06, 0.63]);
                    let arrows = if column == 0 {
                        vec![(0., icon, Command::AddPack(pack.name.clone()))]
                    } else {
                        let mut v = vec![(32., Rect { w: 16., ..icon }, Command::RemovePack(i))];
                        if i > 0 {
                            v.push((
                                96.,
                                Rect {
                                    x: icon.x + 16.,
                                    w: 16.,
                                    h: 16.,
                                    ..icon
                                },
                                Command::MovePack(i, -1),
                            ));
                        }
                        if i + 2 < num {
                            v.push((
                                64.,
                                Rect {
                                    x: icon.x + 16.,
                                    y: y + 16.,
                                    w: 16.,
                                    h: 16.,
                                },
                                Command::MovePack(i, 1),
                            ));
                        }
                        v
                    };
                    for (u, hit, command) in arrows {
                        p.image(
                            "textures/gui/resource_packs.png",
                            icon,
                            [u, if hit.contains(p.mouse) { 32. } else { 0. }, 32., 32.],
                            WHITE,
                        );
                        p.frame.buttons.push(Button {
                            rect: hit,
                            action: Action::Option(command),
                            enabled: true,
                        });
                    }
                }
            }
            if p.width(&label) > 157. {
                while !label.is_empty() && p.width(&label) + p.width("...") > 157. {
                    label.pop();
                }
                label.push_str("...");
            }
            p.text(icon.x + 34., y + 1., &label, WHITE);
            for (j, line) in wrapped(p, &desc, 157.).iter().take(2).enumerate() {
                p.text(
                    icon.x + 34.,
                    y + 12. + 10. * j as f32,
                    line,
                    [0.5, 0.5, 0.5, 1.],
                );
            }
        }
        mask_outside(p, x, 200., 32., bottom);
        scrollbar(p, x + 194., 32., bottom, num as f32 * 36. + 13., offset);
    }
    title(p, cx, 16., "resourcePack.title");
    p.button(
        rect(cx - 154., h - 48., 150.),
        &p.tr("resourcePack.openFolder"),
        Action::Option(Command::OpenPackFolder),
        true,
    );
    p.button(
        rect(cx + 4., h - 48., 150.),
        &p.tr("gui.done"),
        Action::Option(Command::ApplyPacks),
        true,
    );
    p.center(
        cx - 77.,
        h - 26.,
        &p.tr("resourcePack.folderInfo"),
        [0.5, 0.5, 0.5, 1.],
    );
    if !state.error.is_empty() {
        p.center(cx, h - 14., &state.error, [1., 0.3, 0.3, 1.]);
    }
}

fn confirm(p: &mut Painter<'_>, gui: &Gui) {
    let [w, h] = p.frame.size.map(f32::ceil);
    let cx = (w / 2.).floor();
    let lock = gui.screen == Screen::OptionPage(Page::LockDifficulty);
    let (heading, question, yes, no) = if lock {
        (
            "difficulty.lock.title",
            p.tr("difficulty.lock.question")
                .replace("%1$s", &p.tr(LEVELS[p.s.difficulty.min(3) as usize])),
            Command::LockDifficulty,
            Screen::Options,
        )
    } else {
        let old = gui
            .option_state
            .packs
            .iter()
            .any(|p| Some(&p.name) == gui.option_state.pending.as_ref() && p.format < 1);
        (
            "resourcePack.incompatible.confirm.title",
            p.tr(if old {
                "resourcePack.incompatible.confirm.old"
            } else {
                "resourcePack.incompatible.confirm.new"
            }),
            Command::ConfirmPack,
            Screen::OptionPage(Page::Packs),
        )
    };
    title(p, cx, 70., heading);
    for (i, line) in wrapped(p, &question, w - 50.).iter().enumerate() {
        p.center(cx, 90. + i as f32 * 9., line, WHITE);
    }
    let y = (h / 6.).floor() + 96.;
    p.button(
        rect(cx - 155., y, 150.),
        &p.tr("gui.yes"),
        Action::Option(yes),
        true,
    );
    p.button(
        rect(cx + 5., y, 150.),
        &p.tr("gui.no"),
        Action::Screen(no),
        true,
    );
}
pub(super) fn draw(p: &mut Painter<'_>, gui: &Gui, sim: Option<&Simulation>, stats: &Stats<'_>) {
    let [w, h] = p.frame.size.map(f32::ceil);
    let cx = (w / 2.).floor();
    let sixth = (h / 6.).floor();
    if sim.is_none() {
        p.background();
    } else {
        super::pause::background(p);
    }
    match gui.screen {
        Screen::Options => {
            title(p, cx, 15., "options.title");
            slider(p, cell(cx, sixth - 12., 0), Slider::Fov);
            if sim.is_none() {
                toggle(p, cell(cx, sixth - 12., 1), Toggle::Realms);
            } else {
                let st = &gui.option_state;
                let lock = !st.hardcore && !st.remote;
                let r = cell(cx, sixth - 12., 1);
                let enabled = lock && !st.difficulty_locked;
                let label = format!(
                    "{}: {}",
                    p.tr("options.difficulty"),
                    p.tr(LEVELS[sim.and_then(|s| s.world_difficulty).unwrap_or(p.s.difficulty).min(3) as usize])
                );
                p.button(
                    Rect {
                        w: if lock { 130. } else { 150. },
                        ..r
                    },
                    &label,
                    Action::Option(Command::CycleDifficulty),
                    enabled,
                );
                if lock {
                    let r = rect(r.x + 130., r.y, 20.);
                    let u = if st.difficulty_locked { 0. } else { 20. };
                    let v = if !enabled {
                        186.
                    } else if r.contains(p.mouse) {
                        166.
                    } else {
                        146.
                    };
                    p.image("textures/gui/widgets.png", r, [u, v, 20., 20.], WHITE);
                    p.frame.buttons.push(Button {
                        rect: r,
                        action: Action::Screen(Screen::OptionPage(Page::LockDifficulty)),
                        enabled,
                    });
                }
            }
            let entries = [
                (
                    "options.skinCustomisation",
                    Action::Screen(Screen::OptionPage(Page::Skin)),
                ),
                (
                    "Super Secret Settings...",
                    Action::Option(Command::CycleShader),
                ),
                (
                    "options.sounds",
                    Action::Screen(Screen::OptionPage(Page::Sounds)),
                ),
                (
                    "options.stream",
                    Action::Screen(Screen::OptionPage(Page::Broadcast)),
                ),
                ("options.video", Action::Screen(Screen::Video)),
                ("options.controls", Action::Screen(Screen::Controls)),
                ("options.language", Action::Screen(Screen::Languages)),
                (
                    "options.chat.title",
                    Action::Screen(Screen::OptionPage(Page::Chat)),
                ),
                (
                    "options.resourcepack",
                    Action::Screen(Screen::OptionPage(Page::Packs)),
                ),
                (
                    "options.snooper.view",
                    Action::Screen(Screen::OptionPage(Page::Snooper)),
                ),
            ];
            for (i, (key, action)) in entries.into_iter().enumerate() {
                p.button(cell(cx, sixth + 42., i), &p.tr(key), action, true);
            }
            done(p, cx, sixth + 168., gui.options_parent);
        }
        Screen::OptionPage(Page::Skin) => {
            title(p, cx, 20., "options.skinCustomisation.title");
            for i in 0..PARTS.len() {
                toggle(p, cell(cx, sixth, i), Toggle::Skin(i as u8));
            }
            done(p, cx, sixth + 96., Screen::Options);
        }
        Screen::OptionPage(Page::Sounds) => {
            title(p, cx, 15., "options.sounds.title");
            slider(p, rect(cx - 155., sixth - 12., 310.), Slider::Sound(0));
            for i in 1..9 {
                slider(p, cell(cx, sixth + 12., i - 1), Slider::Sound(i as u8));
            }
            done(p, cx, sixth + 168., Screen::Options);
        }
        Screen::OptionPage(Page::Chat) => {
            title(p, cx, 20., "options.chat.title");
            for (i, t) in [
                (0, Toggle::ChatVisibility),
                (1, Toggle::ChatColors),
                (2, Toggle::ChatLinks),
                (4, Toggle::ChatPrompt),
                (9, Toggle::ReducedDebug),
            ] {
                toggle(p, cell(cx, sixth, i), t);
            }
            for (i, s) in [
                (3, Slider::ChatOpacity),
                (5, Slider::ChatScale),
                (6, Slider::ChatFocused),
                (7, Slider::ChatUnfocused),
                (8, Slider::ChatWidth),
            ] {
                slider(p, cell(cx, sixth, i), s);
            }
            done(p, cx, sixth + 120., Screen::Options);
        }
        Screen::Languages => {
            let bottom = h - 61.;
            let top = 32.;
            let row = 18.;
            let offset = gui.option_state.scroll[0]
                .min((p.r.languages.len() as f32 * row + 4. - (bottom - top)).max(0.))
                .floor();
            dirt(
                p,
                Rect {
                    x: 0.,
                    y: top,
                    w,
                    h: bottom - top,
                },
                [0., offset],
                32. / 255.,
            );
            for (i, lang) in p.r.languages.iter().enumerate() {
                let y = top + 4. + i as f32 * row - offset;
                if y + 18. <= top || y - 2. >= bottom {
                    continue;
                }
                let r = Rect {
                    x: cx - 110.,
                    y: y - 2.,
                    w: 220.,
                    h: 18.,
                };
                if lang.code == p.s.language {
                    p.rect(r.x, r.y, r.w, r.h, [0.5, 0.5, 0.5, 1.]);
                    p.rect(r.x + 1., r.y + 1., r.w - 2., r.h - 2., [0., 0., 0., 1.]);
                }
                let previous_bidi = p.bidi;
                p.bidi = true;
                p.center(
                    cx,
                    y + 1.,
                    &format!("{} ({})", lang.name, lang.region),
                    WHITE,
                );
                p.bidi = previous_bidi;
                p.frame.buttons.push(Button {
                    rect: Rect {
                        y: r.y.max(top),
                        h: ((r.y + r.h).min(bottom) - r.y.max(top)).max(0.),
                        ..r
                    },
                    action: Action::Language(lang.code.clone()),
                    enabled: true,
                });
            }
            mask_outside(p, 0., w, top, bottom);
            scrollbar(
                p,
                cx + 124.,
                top,
                bottom,
                p.r.languages.len() as f32 * row,
                offset,
            );
            title(p, cx, 16., "options.language");
            p.center(
                cx,
                h - 56.,
                &format!("({})", p.tr("options.languageWarning").replace("%%", "%")),
                [0.5, 0.5, 0.5, 1.],
            );
            toggle(p, rect(cx - 155., h - 38., 150.), Toggle::Unicode);
            p.button(
                rect(cx + 5., h - 38., 150.),
                &p.tr("gui.done"),
                Action::Screen(gui.option_state.language_parent),
                true,
            );
        }
        Screen::OptionPage(Page::Snooper) => draw_snooper(p, gui, stats),
        Screen::OptionPage(Page::Packs) => draw_packs(p, gui),
        Screen::OptionPage(Page::LockDifficulty | Page::IncompatiblePack) => confirm(p, gui),
        Screen::OptionPage(Page::Broadcast) => {
            let lines = wrapped(
                p,
                &p.tr("stream.unavailable.account_not_migrated"),
                w * 0.75,
            );
            let y = (h * 0.85 / 2. - lines.len() as f32 * 4.5).floor().max(50.);
            title(p, cx, y - 18., "stream.unavailable.title");
            for (i, line) in lines.iter().enumerate() {
                p.center(cx, y + i as f32 * 9., line, GRAY);
            }
            p.button(
                rect(cx - 155., h - 50., 150.),
                &p.tr("gui.cancel"),
                Action::Screen(gui.option_state.stream_parent),
                true,
            );
            p.button(
                rect(cx + 5., h - 50., 150.),
                &p.tr("stream.unavailable.account_not_migrated.okay"),
                Action::Option(Command::OpenStreamHelp),
                true,
            );
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{paint, Frame};
    use crate::validation::{assets, resources};
    fn screen(name: &str) -> Screen {
        match name {
            "options" => Screen::Options,
            "skin" => Screen::OptionPage(Page::Skin),
            "sounds" => Screen::OptionPage(Page::Sounds),
            "languages" => Screen::Languages,
            "chat" => Screen::OptionPage(Page::Chat),
            "snooper" => Screen::OptionPage(Page::Snooper),
            "packs" => Screen::OptionPage(Page::Packs),
            "broadcast" => Screen::OptionPage(Page::Broadcast),
            _ => unreachable!(),
        }
    }
    fn coords(r: Rect) -> [i32; 4] {
        [r.x as i32, r.y as i32, r.w as i32, r.h as i32]
    }
    fn compare(name: &str, size: [f32; 2], buttons: &serde_json::Value, translated: bool) {
        let mut s = Settings {
            language: "ko_kr".into(),
            volume: 0.,
            ..Default::default()
        };
        s.menu.snooper = false;
        s.menu.realms_notifications = false;
        s.locale_unicode = resources().unicode(&s.language);
        let gui = Gui {
            screen: screen(name),
            ..Default::default()
        };
        let mut frame = Frame {
            size,
            captions: Some(vec![]),
            ..Default::default()
        };
        paint(
            &mut frame,
            &gui,
            &s,
            resources(),
            assets(),
            None,
            &Stats::default(),
        );
        let labels = frame.captions.as_ref().unwrap();
        let original = buttons.as_array().unwrap();
        assert_eq!(labels.len(), original.len(), "{name} widgets {size:?}");
        for row in original {
            let xywh: [i32; 4] = std::array::from_fn(|i| row[i + 1].as_i64().unwrap() as i32);
            let (rect, label) = labels
                .iter()
                .find(|(r, _)| coords(*r) == xywh)
                .unwrap_or_else(|| panic!("{name} {size:?} missing {:?}", row));
            let button = frame
                .buttons
                .iter()
                .find(|b| coords(b.rect) == coords(*rect))
                .unwrap();
            assert_eq!(button.enabled, row[5].as_bool().unwrap());
            if translated {
                assert_eq!(
                    *label,
                    row[7].as_str().unwrap().replace("§r", ""),
                    "{name} {xywh:?}"
                );
            }
        }
    }
    #[test]
    #[ignore = "requires original local assets and fonts"]
    fn original_options_widgets_and_korean_labels_match() {
        let original: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/vanilla189-menu-layouts.json"
        ))
        .unwrap();
        for name in [
            "options",
            "skin",
            "sounds",
            "languages",
            "chat",
            "snooper",
            "packs",
            "broadcast",
        ] {
            let entry = &original[name];
            let size = std::array::from_fn(|i| entry["size"][i].as_f64().unwrap() as f32);
            compare(name, size, &entry["buttons"], true);
            compare(name, size.map(|v| v - 0.5), &entry["buttons"], true);
        }
        let sizes: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/vanilla189-options-layout.json"
        ))
        .unwrap();
        for case in sizes.as_array().unwrap() {
            compare(
                case[0].as_str().unwrap(),
                [
                    case[1].as_f64().unwrap() as f32,
                    case[2].as_f64().unwrap() as f32,
                ],
                &case[3],
                false,
            );
        }
    }
    #[test]
    fn sliders_clamp_and_settings_round_trip() {
        let mut s = Settings::default();
        let r = rect(10., 60., 150.);
        Slider::Fov.set(&mut s, r, -1000.);
        assert_eq!(s.fov, 30.);
        Slider::Fov.set(&mut s, r, 85.);
        assert_eq!(s.fov, 70.);
        Slider::ChatWidth.set(&mut s, r, 1000.);
        assert_eq!(s.menu.chat_width, 1.);
        Slider::Sound(3).set(&mut s, r, 85.);
        assert_eq!(s.menu.sound_levels[2], 0.5);
        let mut gui = Gui::default();
        for option in [
            Toggle::Skin(6),
            Toggle::ChatColors,
            Toggle::Unicode,
            Toggle::Realms,
            Toggle::ChatVisibility,
        ] {
            activate(Command::Toggle(option), &mut gui, &mut s);
        }
        let decoded: Settings = serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
        assert_eq!(decoded.menu.skin_parts, 63);
        assert_eq!(decoded.menu.chat_visibility, 1);
        assert!(!decoded.menu.chat_colors && !decoded.menu.realms_notifications);
        assert!(decoded.menu.force_unicode);
    }
    #[test]
    #[ignore = "requires local font and language tables"]
    fn language_list_scrollbar_and_parent_navigation() {
        let mut g = Gui::default();
        g.open(Screen::Languages);
        assert_eq!(g.back(), Screen::Main);
        let size = [800., 450.];
        g.cursor = [400., 100.];
        assert!(wheel(&mut g, resources(), size, -120.));
        assert_eq!(g.option_state.scroll[0], 9.);
        g.cursor = [525., 111.];
        assert!(begin_scroll(&mut g, resources(), size));
        g.cursor[1] = 220.;
        assert!(drag_scroll(&mut g, resources(), size));
        assert!(g.option_state.scroll[0] > 100.);
        g.option_state.end_scroll();
        g.open(Screen::Options);
        g.open(Screen::Languages);
        assert_eq!(g.back(), Screen::Options);
        assert_eq!(g.option_state.scroll[0], 0.);
    }
}
