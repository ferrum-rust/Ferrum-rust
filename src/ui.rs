use crate::{
    assets::Assets,
    gameplay::{Mode, Simulation},
    inventory::{recipe, Stack},
    resources::{Resources, Sprite},
    settings::{Setting, Settings},
};
use bytemuck::{Pod, Zeroable};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
#[path = "chat_ui.rs"]
pub mod chat;
#[path = "options_ui.rs"]
pub mod options;
#[path = "ui/sodium_video.rs"]
pub mod sodium_video;
#[path="ui_font.rs"]
mod font;
pub(crate) use font::world_nametag;
pub(crate) use font::world_sign_line;
#[path = "controls_ui.rs"]
mod controls_ui;
#[path = "ui/main_menu.rs"]
mod main_menu;
pub(crate) use main_menu::local_date;
#[path = "ui/create_world.rs"]
pub mod create_world;
#[path="ui/remote_hud.rs"]
mod remote_hud;
#[path = "ui/multiplayer.rs"]
pub mod multiplayer;
pub(crate) mod text_field;
pub(crate) mod world_select;
pub(crate) mod sign;
pub(crate) mod command_block;
pub(crate) mod statistics;
pub(crate) mod pause;
pub(crate) mod achievements;
pub(crate) mod spectator;
pub(crate) mod utility_windows;
pub(crate) mod server_book;
pub(crate) mod special_windows;
pub(crate) mod creative;
mod creative_preview;
pub(crate) use creative_preview::offline_uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Statistics,
    Achievements,
    ShareLan,
    Book,
    ServerPack,
    Sign,
    CommandBlock,
    Chat,
    ConfirmChatLink,
    Main,
    Multiplayer,
    Realms,
    Worlds,
    RenameWorld,
    DeleteWorld,
    Create,
    Options,
    OptionPage(options::Page),
    Video,
    Languages,
    Controls,
    Playing,
    Pause,
    Inventory,
    Crafting,
    Chest,
    Furnace,
    Trading,
    Sleeping,
    Creative,
    Death,
    Loading,
}
#[derive(Clone, Debug)]
pub enum Action {
    Lan(pause::Command),
    Statistics(statistics::Command),
    BookPage(i32),
    BookSigning(bool),
    BookFinish(bool),
    Enchant(u8),
    AnvilCursor(usize),
    BeaconSelect(bool,i16),
    BeaconDone,
    ServerPack(bool),
    FinishSign,
    SaveCommandBlock,
    ToggleCommandOutput,
    CommandCursor(usize),
    CommandOutput,
    ChatEvent(chat::components::Interaction),
    ChatCursor(usize),
    ChatLinkChoice(bool),
    CopyChatLink,
    Option(options::Command),
    Controls(crate::keybindings::ControlAction),
    Screen(Screen),
    Server(multiplayer::Command),
    WorldForm(create_world::Command),
    Quit,
    Resume,
    SaveQuit,
    SelectWorld(usize),
    Worlds(world_select::Command),
    PlayWorld,
    CreateWorld,
    Setting(Setting),
    Language(String),
    Slot(usize),
    ContainerSlot(usize),
    Craft,
    SelectTrade(i32),
    Creative(creative::Hit),
    Respawn,
    LeaveBed,
}
#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
impl Rect {
    pub fn contains(&self, p: [f32; 2]) -> bool {
        p[0] >= self.x && p[0] < self.x + self.w && p[1] >= self.y && p[1] < self.y + self.h
    }
}
#[derive(Clone)]
pub struct Button {
    pub rect: Rect,
    pub action: Action,
    pub enabled: bool,
}
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct UiVertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
    pub color: [f32; 4],
    pub layer: u32,
    pub glint: [f32;4],
}
#[derive(Default)]
pub struct Frame {
    #[cfg(test)]
    pub captions: Option<Vec<(Rect,String)>>,
    pub vertices: Vec<UiVertex>,
    pub buttons: Vec<Button>,
    pub panorama: bool,
    pub panorama_time: f32,
    pub size: [f32; 2],
}
impl Frame {
    pub fn set_viewport(&mut self, resolution: [u32; 2], settings: &Settings) {
        let scale = settings.scale(resolution[0], resolution[1]);
        self.size = resolution.map(|axis| axis as f32 / scale);
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorldEntry {
    #[serde(default)]
    pub folder: String,
    #[serde(default)]
    pub last_played: i64,
    #[serde(default)]
    pub game_type_id: u8,
    #[serde(default)]
    pub requires_conversion: bool,
    #[serde(default)]
    pub rules: crate::world_library::Rules,
    #[serde(skip)]
    pub descriptor: Option<PathBuf>,
    pub name: String,
    pub seed: u64,
    pub mode: Mode,
    pub save_id: Option<String>,
    pub path: Option<PathBuf>,
    #[serde(default = "legacy_generator")]
    pub generator: u8,
    #[serde(default)]
    pub village: bool,
}
fn legacy_generator() -> u8 {
    1
}
#[cfg(test)]
#[test]
fn old_world_entries_retain_original_generator() {
    let entry: WorldEntry = serde_json::from_str(
        r#"{"name":"Legacy","seed":189,"mode":"Survival","save_id":"old","path":null}"#,
    )
    .unwrap();
    assert_eq!(entry.generator, 1);
    assert!(!entry.village);
}
pub struct Gui {
    pub(crate) boss_bar:crate::models::boss_bar::Bar,
    pub(crate) creative: creative::State,
    pub spectator:spectator::Menu,
    pub statistics:statistics::State,
    pub lan_form:pause::Form,
    pub achievements:achievements::State,
    pub(crate) book:server_book::Book,
    pub(crate) enchanting:Option<[i16;3]>,
    pub(crate) special:special_windows::Special,
    pub(crate) sign:sign::Editor,
    pub(crate) command_block:command_block::Editor,
    pub create: create_world::Form,
    pub chat: chat::State,
    pub option_state: options::State,
    pub video: sodium_video::State,
    pub editing_binding: Option<crate::keybindings::Binding>,
    pub screen: Screen,
    pub multiplayer: multiplayer::Browser,
    pub(crate) title: main_menu::Title,
    pub options_parent: Screen,
    pub cursor: [f32; 2],
    pub(crate) inventory_mouse: std::cell::Cell<[f32; 2]>,
    pub scroll: i32,
    pub selected_world: Option<usize>,
    pub world_list: world_select::State,
    pub worlds: Vec<WorldEntry>,
    pub status: String,
    pub loading_progress: Option<u8>,
    pub debug: bool,
    pub hide_hud: bool,
    pub advanced: bool,
    pub hitboxes: bool,
    pub perspective: u8,
    pub container: Option<glam::IVec3>,
    pub container_view: Option<crate::container::Container>,
    pub container_title:Option<String>,
    pub trader: Option<u64>,
    pub trade_index: usize,
    pub horse_jump: Option<f32>,
    pub mount_health: Option<(f32, f32)>,
}
impl Default for Gui {
    fn default() -> Self {
        Self {
            boss_bar:Default::default(),
            creative: Default::default(),
            sign:Default::default(),
            command_block:Default::default(),
            statistics:Default::default(),
            lan_form:Default::default(),
            achievements:Default::default(),
            spectator:Default::default(),
            book:Default::default(),enchanting:None,special:Default::default(),
            chat: Default::default(),
            create: Default::default(),
            option_state: Default::default(),
            video: Default::default(),
            screen: Screen::Main,
            editing_binding: None,
            multiplayer: Default::default(),
            title: Default::default(),
            options_parent: Screen::Main,
            cursor: [0.0; 2],
            inventory_mouse: Default::default(),
            scroll: 0,
            selected_world: None,
            world_list: Default::default(),
            worlds: Vec::new(),
            status: String::new(),
            loading_progress: None,
            debug: false,
            hide_hud: false,
            advanced: false,
            hitboxes: false,
            perspective: 0,
            container: None,
            container_view: None,
            container_title:None,
            trader: None,
            trade_index: 0,
            horse_jump: None,
            mount_health: None,
        }
    }
}
impl Gui {
    pub fn open(&mut self, screen: Screen) {
        if screen == Screen::Inventory && self.screen != screen {
            self.inventory_mouse.set([0.; 2]);
        }
        if screen == Screen::ShareLan && screen != self.screen {self.lan_form=Default::default();}
        if screen == Screen::Worlds {
            self.selected_world = None;
            self.world_list.reset();
        }
        if screen == Screen::OptionPage(options::Page::Broadcast) && self.screen != screen {
            self.option_state.stream_parent = self.screen;
        }
        if screen != self.screen {
            self.option_state.reset_scroll();
            if screen == Screen::Video || self.screen == Screen::Video {
                self.video.reset_view();
            }
        }
        if screen == Screen::Languages && self.screen != Screen::Languages {
            self.option_state.language_parent = if self.screen == Screen::Main {Screen::Main} else {Screen::Options};
        }
        if screen == Screen::Main {
            self.debug = false;
            self.chat = Default::default();
            self.spectator = Default::default();
            if self.screen != Screen::Main { self.title = Default::default(); }
            self.options_parent = Screen::Main;
        }
        if screen == Screen::Options {
            match self.screen {
                Screen::Pause | Screen::Playing => self.options_parent = Screen::Pause,
                Screen::Main => self.options_parent = Screen::Main,
                _ => {}
            }
        }
        self.screen = screen;
        self.scroll = 0;
        self.editing_binding = None;
    }
    pub fn back(&self) -> Screen {
        match self.screen {
            Screen::Chat|Screen::Book => Screen::Playing,
            Screen::ConfirmChatLink => Screen::Chat,
            Screen::Statistics|Screen::Achievements|Screen::ShareLan=>Screen::Pause,
            Screen::Realms | Screen::Multiplayer => Screen::Main,
            Screen::Languages => self.option_state.language_parent,
            Screen::OptionPage(options::Page::Broadcast) => self.option_state.stream_parent,
            Screen::OptionPage(page) => page.back(),
            Screen::Video | Screen::Controls => Screen::Options,
            Screen::Options => self.options_parent,
            Screen::RenameWorld | Screen::Create => Screen::Worlds,
            Screen::Inventory
            | Screen::Crafting
            | Screen::Creative
            | Screen::Chest
            | Screen::Furnace
            | Screen::Trading
            | Screen::Pause => Screen::Playing,
            _ => Screen::Main,
        }
    }
}
#[derive(Default)]
pub struct Stats<'a> {
    pub fps: f32,
    pub frame_ms: f32,
    pub chunks: usize,
    pub total_chunks: usize,
    pub distant_tiles: (usize, usize),
    pub triangles: usize,
    pub entities: usize,
    pub total_entities: usize,
    pub culled: usize,
    pub position: [f64; 3],
    pub yaw: f32,
    pub spawn: [f32;3],
    pub pitch: f32,
    pub gpu: &'a str,
    pub time: f32,
    pub portal_time: f32,
}
struct Painter<'a> {
    frame: &'a mut Frame,
    r: &'a Resources,
    a: &'a Assets,
    s: &'a Settings,
    mouse: [f32; 2],
    video_buttons: bool,
    time: f32,
    indicators:[f64;2],
    bidi: bool,
}
const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const GRAY: [f32; 4] = [0.63, 0.63, 0.63, 1.0];
impl Painter<'_> {
    fn quad(&mut self, rect: Rect, uv: [f32; 4], layer: u32, color: [f32; 4]) {
        let p = [
            [rect.x, rect.y],
            [rect.x, rect.y + rect.h],
            [rect.x + rect.w, rect.y + rect.h],
            [rect.x + rect.w, rect.y],
        ];
        let t = [
            [uv[0], uv[1]],
            [uv[0], uv[1] + uv[3]],
            [uv[0] + uv[2], uv[1] + uv[3]],
            [uv[0] + uv[2], uv[1]],
        ];
        self.polygon(p, t, layer, color);
    }
    fn polygon(&mut self, p: [[f32; 2]; 4], uv: [[f32; 2]; 4], layer: u32, color: [f32; 4]) {
        for i in [0, 1, 2, 0, 2, 3] {
            self.frame.vertices.push(UiVertex {
                position: [
                    p[i][0] / self.frame.size[0] * 2.0 - 1.0,
                    1.0 - p[i][1] / self.frame.size[1] * 2.0,
                ],
                uv: uv[i],
                color,
                layer,
                glint:[0.;4],
            });
        }
    }
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
        self.quad(Rect { x, y, w, h }, [-1.0, -1.0, 0.0, 0.0], 0, color);
    }
    fn image(&mut self, name: &str, rect: Rect, source: [f32; 4], color: [f32; 4]) {
        let sprite = self.r.sprite(name);
        self.sprite(sprite, rect, source, color);
    }
    fn sprite(&mut self, sprite: Sprite, rect: Rect, source: [f32; 4], color: [f32; 4]) {
        self.quad(rect, source.map(|v| v / 256.0), sprite.layer, color);
    }
    fn text(&mut self, x: f32, y: f32, text: &str, color: [f32; 4]) {
        self.text_end(x, y, text, color);
    }
    fn text_end(&mut self, x: f32, y: f32, text: &str, color: [f32; 4]) -> f32 {
        let visual = self.bidi.then(|| font::visual_order(text));
        let text = visual.as_deref().unwrap_or(text);
        let shadow = self.draw_text_run(x+1., y+1., text, color, true);
        shadow.max(self.draw_text_run(x, y, text, color, false))
    }
    fn center(&mut self, x: f32, y: f32, text: &str, color: [f32; 4]) {
        self.text(x - (self.width(text) * 0.5).floor(), y, text, color);
    }
    fn tr(&self, key: &str) -> String {
        self.r.tr(&self.s.language, key)
    }
    fn button(&mut self, rect: Rect, label: &str, action: Action, enabled: bool) {
        #[cfg(test)]
        if let Some(labels)=&mut self.frame.captions {labels.push((rect,label.to_owned()));}
        let hover = enabled && rect.contains(self.mouse);
        let v = if !enabled {
            46.0
        } else if hover {
            86.0
        } else {
            66.0
        };
        let half = if self.video_buttons { rect.w/2. } else { (rect.w/2.).floor() };
        self.image(
            "textures/gui/widgets.png",
            Rect { w: half, ..rect },
            [0.0, v, half, rect.h],
            WHITE,
        );
        self.image(
            "textures/gui/widgets.png",
            Rect {
                x: rect.x + half,
                w: half,
                ..rect
            },
            [200.0 - half, v, half, rect.h],
            WHITE,
        );
        self.center(
            rect.x + half,
            rect.y + ((rect.h - 8.) / 2.).floor(),
            label,
            if !enabled {
                let gray=if self.video_buttons {0.4} else {160./255.};
                [gray,gray,gray,1.]
            } else if hover {
                [1.0, 1.0, if self.video_buttons {0.63} else {160./255.}, 1.0]
            } else {
                let gray=if self.video_buttons {1.} else {224./255.};
                [gray,gray,gray,1.]
            },
        );
        self.frame.buttons.push(Button {
            rect,
            action,
            enabled,
        });
    }
    fn item(&mut self, item: Stack, x: f32, y: f32, count: bool) {
        if !creative::draw_icon(self,&item,x,y) {
            let start = self.frame.vertices.len();
            if item.id < 256 {
                let state = item.state();
                if let Some(faces) = self
                    .a
                    .block_models
                    .carried(state)
                    .filter(|f| !f.is_empty())
                {
                    let cam_dir = glam::Vec3::new(0.61237244, 0.5, 0.61237244);
                    let tint_rgb = match state >> 4 {
                        2 | 31 | 175 => *self
                            .a
                            .biome_colors
                            .grass
                            .get((127 << 8) | 127)
                            .unwrap_or(&0xFFFFFF),
                        18 if (state & 3) == 1 => 0x619961,
                        18 if (state & 3) == 2 => 8431445,
                        18 | 106 | 161 => *self
                            .a
                            .biome_colors
                            .foliage
                            .get((127 << 8) | 127)
                            .unwrap_or(&0x48B518),
                        111 => 2129968,
                        _ => 0xFFFFFF,
                    };
                    let mut visible: Vec<(
                        f32,
                        &crate::assets::block_models::BakedFace,
                        glam::Vec3,
                    )> = Vec::with_capacity(faces.len());
                    for f in faces {
                        let p0 = f.vertices[0].position;
                        let p1 = f.vertices[1].position;
                        let p2 = f.vertices[2].position;
                        let p3 = f.vertices[3].position;
                        let mut normal = (p1 - p0).cross(p2 - p0).normalize_or_zero();
                        if normal.length_squared() < 1e-6 {
                            normal =
                                crate::vanilla::ambient_occlusion::DIRECTIONS[f.facing].as_vec3();
                        }
                        if normal.dot(cam_dir) <= 1e-4 {
                            continue;
                        }
                        let centroid = (p0 + p1 + p2 + p3) * 0.25;
                        visible.push((centroid.dot(cam_dir), f, normal));
                    }
                    visible.sort_by(|a, b| {
                        a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal)
                    });
                    for (_, f, n) in visible {
                        let pts2d = f.vertices.map(|v| {
                            let p = v.position - glam::Vec3::splat(0.5);
                            [
                                x + 8.0 + 7.0710678 * (p.x - p.z),
                                y + 8.0 + 3.5355339 * (p.x + p.z) - 8.660254 * p.y,
                            ]
                        });
                        let uv = f.vertices.map(|v| v.uv);
                        let shade = if f.shade == 255 && f.facing != 1 {
                            1.0
                        } else {
                            let ny2 = n.y * n.y;
                            let y_light = if n.y >= 0.0 { 1.0 } else { 0.5 };
                            (ny2 * y_light
                                + (n.z * n.z) * (2607.0 / 4096.0)
                                + (n.x * n.x) * (1781.0 / 4096.0))
                                .clamp(0.0, 1.0)
                        };
                        let color = if f.tinted && tint_rgb != 0xFFFFFF {
                            [
                                (((tint_rgb >> 16) & 255) as f32 / 255.0) * shade,
                                (((tint_rgb >> 8) & 255) as f32 / 255.0) * shade,
                                ((tint_rgb & 255) as f32 / 255.0) * shade,
                                1.0,
                            ]
                        } else {
                            [shade, shade, shade, 1.0]
                        };
                        self.polygon(pts2d, uv, f.layer | 65536, color);
                    }
                } else {
                    let top = self.a.texture(state, 1, true) | 65536;
                    let left = self.a.texture(state, 2, false) | 65536;
                    let right = self.a.texture(state, 0, true) | 65536;
                    let uv = [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]];
                    self.polygon(
                        [
                            [x + 61731.0 / 4096.0, y + 15032.0 / 4096.0],
                            [x + 32768.0 / 4096.0, y + 550.0 / 4096.0],
                            [x + 3805.0 / 4096.0, y + 15032.0 / 4096.0],
                            [x + 32768.0 / 4096.0, y + 29513.0 / 4096.0],
                        ],
                        uv,
                        top,
                        WHITE,
                    );
                    self.polygon(
                        [
                            [x + 3805.0 / 4096.0, y + 15032.0 / 4096.0],
                            [x + 3805.0 / 4096.0, y + 50504.0 / 4096.0],
                            [x + 32768.0 / 4096.0, y + 64986.0 / 4096.0],
                            [x + 32768.0 / 4096.0, y + 29513.0 / 4096.0],
                        ],
                        uv,
                        left,
                        [2607.0 / 4096.0, 2607.0 / 4096.0, 2607.0 / 4096.0, 1.0],
                    );
                    self.polygon(
                        [
                            [x + 32768.0 / 4096.0, y + 29513.0 / 4096.0],
                            [x + 32768.0 / 4096.0, y + 64986.0 / 4096.0],
                            [x + 61731.0 / 4096.0, y + 50504.0 / 4096.0],
                            [x + 61731.0 / 4096.0, y + 15032.0 / 4096.0],
                        ],
                        uv,
                        right,
                        [1781.0 / 4096.0, 1781.0 / 4096.0, 1781.0 / 4096.0, 1.0],
                    );
                }
            } else {
                let sprite = self.r.item_sprite(item.id, item.meta);
                self.sprite(
                    sprite,
                    Rect {
                        x,
                        y,
                        w: 16.0,
                        h: 16.0,
                    },
                    [0.0, 0.0, sprite.width as f32, sprite.height as f32],
                    WHITE,
                );
            }
            if creative::world_glint(&item) {
                let [rx, ry, rw, rh] = if item.id >= 256 {
                    self.r
                        .item_textures
                        .get(&item.id)
                        .and_then(|s| s.strip_prefix("textures/")?.strip_suffix(".png"))
                        .and_then(creative::atlas_region)
                } else {
                    None
                }
                .unwrap_or([0.0, 0.0, 1.0 / 32.0, 1.0 / 32.0]);
                let glint = self.r.sprite("textures/misc/enchanted_item_glint.png");
                let corners = [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]];
                for quad in self.frame.vertices[start..].chunks_exact_mut(6) {
                    for (vertex, j) in quad.iter_mut().zip([0, 1, 2, 0, 2, 3]) {
                        let u = rx + rw * corners[j][0];
                        let v = ry + rh * corners[j][1];
                        for (k, angle, phase) in [
                            (0, -50f32, (self.time % 3.0) / 3.0),
                            (2, 10f32, -(self.time % 4.873) / 4.873),
                        ] {
                            let (sin, cos) = angle.to_radians().sin_cos();
                            vertex.glint[k] = 8.0 * (u * cos - v * sin) + phase;
                            vertex.glint[k + 1] = 8.0 * (u * sin + v * cos);
                        }
                        vertex.layer |= glint.layer << 17;
                    }
                }
            }
        }
        if count && item.count > 1 {
            let text = item.count.to_string();
            self.text(x + 17.0 - self.width(&text), y + 9.0, &text, WHITE);
        }
        let durability=crate::vanilla::item_entity::facts(item.id).map_or(0,|f|f.max_damage);
        if durability>0 && item.damage>0 && !item.unbreakable(){
            let fraction=1.-f64::from(item.damage)/f64::from(durability);
            let bar=(13.*fraction).round() as f32;
            let green=(255.*fraction).round().clamp(0.,255.) as u8;
            let red=255-green;
            self.rect(x+2.,y+13.,13.,2.,[0.,0.,0.,1.]);
            self.rect(x+2.,y+13.,12.,1.,[f32::from(red/4)/255.,64./255.,0.,1.]);
            self.rect(x+2.,y+13.,bar.max(0.),1.,[f32::from(red)/255.,f32::from(green)/255.,0.,1.]);
        }
    }
    fn slot(&mut self, index: usize, item: Option<Stack>, x: f32, y: f32) {
        let rect = Rect {
            x,
            y,
            w: 16.0,
            h: 16.0,
        };
        if let Some(s) = item {
            self.item(s, x, y, true);
        }
        if rect.contains(self.mouse) {
            self.rect(x, y, 16.0, 16.0, [1.0, 1.0, 1.0, 0.3]);
        }
        self.frame.buttons.push(Button {
            rect,
            action: Action::Slot(index),
            enabled: true,
        });
    }
    fn background(&mut self) {
        let [w, h] = self.frame.size;
        let tint = if self.video_buttons { 0.25 } else { 64. / 255. };
        for y in (0..h.ceil() as usize).step_by(32) {
            for x in (0..w.ceil() as usize).step_by(32) {
                self.image(
                    "textures/gui/options_background.png",
                    Rect {
                        x: x as f32,
                        y: y as f32,
                        w: 32.0,
                        h: 32.0,
                    },
                    [0.0, 0.0, 16.0, 16.0],
                    [tint, tint, tint, 1.0],
                );
            }
        }
    }
}
#[allow(dead_code)]
fn setting_label(p: &Painter<'_>, setting: Setting) -> String {
    let s = p.s;
    let on = |v: bool| p.tr(if v { "options.on" } else { "options.off" });
    let (key, value) =
        match setting {
            Setting::Graphics => (
                "options.graphics",
                p.tr(if s.fancy {
                    "options.graphics.fancy"
                } else {
                    "options.graphics.fast"
                }),
            ),
            Setting::Distance => ("options.renderDistance", if s.render_distance > s.full_detail_distance() {
                format!("{} (LOD)", s.render_distance)
            } else { format!("{}", s.render_distance) }),
            Setting::Lighting => (
                "options.ao",
                p.tr(["options.ao.off", "options.ao.min", "options.ao.max"]
                    [s.smooth_lighting as usize]),
            ),
            Setting::Fps => (
                "options.framerateLimit",
                if s.max_fps == 0 {
                    p.tr("options.framerateLimit.max")
                } else {
                    s.max_fps.to_string()
                },
            ),
            Setting::Anaglyph => ("options.anaglyph", on(s.anaglyph)),
            Setting::Bobbing => ("options.viewBobbing", on(s.bobbing)),
            Setting::GuiScale => (
                "options.guiScale",
                if s.gui_scale == 0 {
                    p.tr("options.guiScale.auto")
                } else {
                    s.gui_scale.to_string()
                },
            ),
            Setting::Brightness => ("options.gamma", format!("{}%", (s.gamma * 100.0) as u32)),
            Setting::Clouds => (
                "options.renderClouds",
                p.tr([
                    "options.off",
                    "options.graphics.fast",
                    "options.graphics.fancy",
                ][s.clouds as usize]),
            ),
            Setting::Particles => (
                "options.particles",
                p.tr([
                    "options.particles.all",
                    "options.particles.decreased",
                    "options.particles.minimal",
                ][s.particles as usize]),
            ),
            Setting::Fullscreen => ("options.fullscreen", on(s.fullscreen)),
            Setting::Vsync => ("options.vsync", on(s.vsync)),
            Setting::Mipmaps => ("options.mipmapLevels", s.mipmaps.to_string()),
            Setting::Alternatives => ("options.blockAlternatives", on(s.alternate_blocks)),
            Setting::Shadows => ("options.entityShadows", on(s.entity_shadows)),
            Setting::Culling => ("Entity Culling", on(s.entity_culling)),
            Setting::EntityDistance => ("Entity Distance", format!("{} m", s.entity_distance)),
            Setting::Uploads => ("Chunk Uploads / Frame", s.chunk_uploads.to_string()),
            Setting::Greedy => ("Greedy Meshing", on(s.greedy_meshing)),
            Setting::Fov => ("options.fov", format!("{:.0}", s.fov)),
            Setting::Sensitivity => (
                "options.sensitivity",
                format!("{:.0}%", s.sensitivity * 100.0),
            ),
            Setting::Volume => ("options.sound", format!("{:.0}%", s.volume * 100.0)),
            Setting::Difficulty => (
                "options.difficulty",
                p.tr([
                    "options.difficulty.peaceful",
                    "options.difficulty.easy",
                    "options.difficulty.normal",
                    "options.difficulty.hard",
                ][s.difficulty as usize]),
            ),
        };
    format!("{}: {}", p.tr(key), value)
}

pub fn build(
    gui: &Gui,
    settings: &Settings,
    resources: &Resources,
    assets: &Assets,
    resolution: [u32; 2],
    simulation: Option<&Simulation>,
    stats: &Stats,
) -> Frame {
    let mut frame = Frame::default();
    frame.set_viewport(resolution, settings);
    paint(&mut frame, gui, settings, resources, assets, simulation, stats);
    frame
}
pub fn paint(
    frame: &mut Frame,
    gui: &Gui,
    settings: &Settings,
    resources: &Resources,
    assets: &Assets,
    simulation: Option<&Simulation>,
    stats: &Stats,
) {
    frame.vertices.clear();
    #[cfg(test)]
    if let Some(captions)=&mut frame.captions {captions.clear();}
    frame.buttons.clear();
    frame.panorama = gui.screen == Screen::Main;
    frame.panorama_time = gui.title.elapsed();
    let [w, h] = frame.size;
    let mut p = Painter {
        frame,
        r: resources,
        a: assets,
        s: settings,
        mouse: gui.cursor,
        video_buttons: gui.screen==Screen::Video,
        time:stats.time,
        indicators:gui.creative.indicators(simulation,stats),
        bidi: resources.languages.iter().any(|lang| lang.code == settings.language && lang.bidirectional),
    };
    let cx = w / 2.0;
    let show_overlay = gui.screen != Screen::Loading && (!gui.hide_hud || gui.screen != Screen::Playing);
    if show_overlay
        && simulation.is_some_and(|sim| sim.effects.get(9).is_none())
        && stats.portal_time > 0.0
    {
        let mut f = stats.portal_time.clamp(0.0, 1.0);
        if f < 1.0 {
            f *= f;
            f *= f;
            f = f * 0.8 + 0.2;
        }
        let layer = p.a.texture(90 << 4, 2, true) | 65536;
        p.quad(
            Rect { x: 0.0, y: 0.0, w, h },
            [0.0, 0.0, 1.0, 1.0],
            layer,
            [1.0, 1.0, 1.0, f],
        );
    }
    if simulation.is_some(){if let Some(b)=gui.boss_bar.current(){
        let middle=(w as i32/2) as f32;let x=middle-91.;
        let draw=|p:&mut Painter<'_>,v:f32,width:f32|p.image("textures/gui/icons.png",Rect{x,y:12.,w:width,h:5.},[0.,v,width,5.],WHITE);
        draw(&mut p,74.,182.);draw(&mut p,74.,182.);
        if b.width()>0{draw(&mut p,79.,b.width() as f32);}
        let name=if b.name.is_empty(){p.tr(b.name_key())}else{b.name.to_string()};
        p.center(middle,2.,&name,WHITE);
    }}
    if let Some(sim) = simulation.filter(|_| show_overlay) {
        hud(&mut p, gui, sim, stats);
    }
    if let Some(sim)=simulation.filter(|s| s.sleep.timer>0) {
        let t=sim.sleep.timer as f32;
        let fraction=if t<=100. {t/100.} else {1.-(t-100.)/10.};
        p.rect(0.,0.,w,h,[16./255.,16./255.,32./255.,(fraction*220.).trunc()/255.]);
    }
    if gui.screen==Screen::Playing {
        if let Some((key,_))=simulation.and_then(|s|s.sleep_notice) {
            p.center(cx,h-62.,&p.tr(key),WHITE);
        }
    }
    if show_overlay {
        if let Some(sim) = simulation {
            remote_hud::draw(&mut p, gui, sim);
        }
    }
    if (simulation.is_some() && show_overlay) || matches!(gui.screen, Screen::Chat | Screen::Sleeping) {
        chat::draw(&mut p, gui);
    }
    match gui.screen {
        Screen::Book=>server_book::draw(&mut p,&gui.book),
        Screen::ServerPack=>{
            p.rect(0.,0.,w,h,[0.,0.,0.,0.65]);
            p.center(cx,70.,&p.tr("multiplayer.texturePrompt.line1"),WHITE);
            p.center(cx,90.,&p.tr("multiplayer.texturePrompt.line2"),WHITE);
            for (i,accept) in [true,false].into_iter().enumerate(){
                p.button(Rect{x:cx-155.+160.*i as f32,y:(h/6.).floor()+96.,w:150.,h:20.},&p.tr(if accept{"gui.yes"}else{"gui.no"}),Action::ServerPack(accept),true);
            }
        }
        Screen::Sign=>sign::draw(&mut p,&gui.sign,stats.time),
        Screen::CommandBlock=>command_block::draw(&mut p,&gui.command_block,stats.time),
        Screen::Chat => {}
        Screen::ConfirmChatLink => chat::confirm(&mut p, gui),
        Screen::Main => main_menu::draw(&mut p, &gui.title, stats.time),
        Screen::Multiplayer => multiplayer::draw(&mut p, &gui.multiplayer, stats.time),
        Screen::Realms => main_menu::realms(&mut p),
        Screen::Worlds | Screen::RenameWorld | Screen::DeleteWorld =>
            world_select::draw(&mut p, gui),
        Screen::Create => create_world::draw(&mut p, &gui.create, &gui.worlds, stats.time),
        Screen::Options | Screen::Languages | Screen::OptionPage(_) => options::draw(&mut p, gui, simulation, stats),
        Screen::Video => sodium_video::draw(&mut p, gui, simulation.is_some()),
        Screen::Controls => controls_ui::draw(&mut p, gui),
        Screen::Statistics=>statistics::draw(&mut p,gui),
        Screen::Pause => pause::draw(&mut p, gui, simulation),
        Screen::ShareLan => pause::settings(&mut p, &gui.lan_form),
        Screen::Achievements => achievements::draw(&mut p,gui),
        Screen::Inventory | Screen::Crafting => {
            if let Some(sim) = simulation {
                inventory_screen(&mut p, gui, sim);
            }
        }
        Screen::Chest | Screen::Furnace => {
            if let Some(sim) = simulation {
                container_screen(&mut p, gui, sim);
            }
        }
        Screen::Trading => {
            if let Some(sim) = simulation {
                trading_screen(&mut p, gui, sim);
            }
        }
        Screen::Creative => {
            if let Some(sim) = simulation {
                creative::draw(&mut p, gui, sim);
            }
        }
        Screen::Death => {
            p.rect(0.0, 0.0, w, h, [0.45, 0.0, 0.0, 0.65]);
            p.center(cx, 55.0, &p.tr("deathScreen.title"), WHITE);
            if let Some(sim) = simulation {
                let label = format!("{}: ", p.tr("deathScreen.score"));
                let score = sim.xp.score.to_string();
                let x = cx - (p.width(&label) + p.width(&score)) * 0.5;
                p.text(x, 100., &label, WHITE);
                p.text(x + p.width(&label), 100., &score, [1., 1., 0.33, 1.]);
            }
            p.button(
                Rect {
                    x: cx - 100.0,
                    y: h * 0.25 + 72.,
                    w: 200.0,
                    h: 20.0,
                },
                &p.tr("deathScreen.respawn"),
                Action::Respawn,
                true,
            );
            p.button(
                Rect {
                    x: cx - 100.0,
                    y: h * 0.25 + 96.,
                    w: 200.0,
                    h: 20.0,
                },
                &p.tr("deathScreen.titleScreen"),
                Action::SaveQuit,
                true,
            );
        }
        Screen::Loading => {
            p.background();
            let half_width = (w.ceil()/2.).floor();
            let half_height = (h.ceil()/2.).floor();
            p.center(half_width,half_height-20.,&p.tr("menu.loadingLevel"),WHITE);
            p.center(half_width,half_height+4.,&gui.status,WHITE);
            if let Some(progress)=gui.loading_progress {
                p.rect(half_width-50.,half_height+16.,100.,2.,[128./255.,128./255.,128./255.,1.]);
                p.rect(half_width-50.,half_height+16.,f32::from(progress),2.,[128./255.,1.,128./255.,1.]);
            }
        }
        Screen::Sleeping => {
            p.button(Rect{x:cx-100.,y:h-40.,w:200.,h:20.},&p.tr("multiplayer.stopSleeping"),Action::LeaveBed,true);
        }
        Screen::Playing => {}
    }
    if !gui.status.is_empty()
        && gui.screen != Screen::Loading
        && !matches!(
            gui.screen,
            Screen::Playing | Screen::Inventory | Screen::Crafting | Screen::Creative
        )
    {
        p.center(cx, h - 57.0, &gui.status, [1.0, 0.8, 0.4, 1.0]);
    }
}
fn hud(p: &mut Painter<'_>, gui: &Gui, sim: &Simulation, stats: &Stats) {
    if gui.spectator.spectator {gui.spectator.draw(p,&sim.presentation);}
    else {normal_hud(p,gui,sim);}
    if gui.debug {debug_hud(p,sim,stats);}
}
fn normal_hud(p: &mut Painter<'_>, gui: &Gui, sim: &Simulation) {
    let [w, h] = p.frame.size;
    let x = w / 2.0 - 91.0;
    let y = h - 22.0;
    let horse_jump = gui.horse_jump.or_else(|| sim.horse_jump_power());
    let mount_health = gui.mount_health.or_else(|| sim.mount_health());
    p.image(
        "textures/gui/widgets.png",
        Rect {
            x,
            y,
            w: 182.0,
            h: 22.0,
        },
        [0.0, 0.0, 182.0, 22.0],
        WHITE,
    );
    p.image(
        "textures/gui/widgets.png",
        Rect {
            x: x - 1.0 + sim.inventory.selected as f32 * 20.0,
            y: y - 1.0,
            w: 24.0,
            h: 22.0,
        },
        [0.0, 22.0, 24.0, 22.0],
        WHITE,
    );
    for i in 0..9 {
        if let Some(item) = sim.inventory.slots[i].clone() {
            p.item(item, x + 3.0 + i as f32 * 20.0, y + 3.0, true);
        }
    }
    if sim.mode == Mode::Survival {
        let absorption = sim.absorption.ceil().max(0.0) as i32;
        let max_hearts = (sim.max_health / 2.0).ceil().max(0.0) as i32;
        let absorption_hearts = (absorption + 1) / 2;
        let total_hearts = ((sim.max_health + absorption as f32) / 2.0).ceil().max(0.0) as i32;
        let rows = ((total_hearts + 9) / 10).max(1);
        let row_spacing = (10 - (rows - 2)).max(3) as f32;
        let armor_y = h - 39.0 - (rows - 1) as f32 * row_spacing - 10.0;
        let status = if sim.effects.get(19).is_some() {
            36.0
        } else if sim.effects.get(20).is_some() {
            72.0
        } else {
            0.0
        };
        let regen_index = if sim.effects.get(10).is_some() {
            (sim.total_ticks % ((max_hearts + absorption_hearts + 5).max(1) as u64)) as i32
        } else {
            -1
        };
        let mut rem_absorption = absorption;
        let health = sim.health.ceil() as i32;
        for i in (0..total_hearts).rev() {
            let xx = x + (i % 10) as f32 * 8.0;
            let mut yy = h - 39.0 - (i / 10) as f32 * row_spacing;
            if i == regen_index {
                yy -= 2.0;
            }
            p.image(
                "textures/gui/icons.png",
                Rect {
                    x: xx,
                    y: yy,
                    w: 9.0,
                    h: 9.0,
                },
                [16.0, 0.0, 9.0, 9.0],
                WHITE,
            );
            let texture = if rem_absorption > 0 {
                let half = rem_absorption == absorption && absorption % 2 == 1;
                rem_absorption -= 2;
                Some(if half { 169.0 } else { 160.0 })
            } else if i * 2 + 1 < health {
                Some(52.0 + status)
            } else if i * 2 + 1 == health {
                Some(61.0 + status)
            } else {
                None
            };
            if let Some(texture) = texture {
                p.image(
                    "textures/gui/icons.png",
                    Rect {
                        x: xx,
                        y: yy,
                        w: 9.0,
                        h: 9.0,
                    },
                    [texture, 0.0, 9.0, 9.0],
                    WHITE,
                );
            }
        }
        let mount_rows = if let Some((_, max_hp)) = mount_health {
            let max_hearts = ((max_hp * 0.5).ceil() as i32).clamp(0, 30);
            ((max_hearts + 9) / 10).max(1)
        } else {
            let hungry = sim.effects.get(17).is_some();
            for i in 0..10 {
                let yy = h - 39.0;
                let xx = x + 173.0 - i as f32 * 8.0;
                p.image(
                    "textures/gui/icons.png",
                    Rect {
                        x: xx,
                        y: yy,
                        w: 9.0,
                        h: 9.0,
                    },
                    [if hungry { 133.0 } else { 16.0 }, 27.0, 9.0, 9.0],
                    WHITE,
                );
                let food = sim.food - i * 2;
                if food > 0 {
                    p.image(
                        "textures/gui/icons.png",
                        Rect {
                            x: xx,
                            y: yy,
                            w: 9.0,
                            h: 9.0,
                        },
                        [
                            if food >= 2 { 52.0 } else { 61.0 } + if hungry { 36.0 } else { 0.0 },
                            27.0,
                            9.0,
                            9.0,
                        ],
                        WHITE,
                    );
                }
            }
            1
        };
        if sim.inventory.armor() > 0 {
            for i in 0..10 {
                let value = sim.inventory.armor() as i32 - i * 2;
                p.image(
                    "textures/gui/icons.png",
                    Rect {
                        x: x + i as f32 * 8.0,
                        y: armor_y,
                        w: 9.0,
                        h: 9.0,
                    },
                    [
                        if value >= 2 {
                            34.0
                        } else if value == 1 {
                            25.0
                        } else {
                            16.0
                        },
                        9.0,
                        9.0,
                        9.0,
                    ],
                    WHITE,
                );
            }
        }
        if sim.air < 300 {
            let air_y = h - 49.0 - (mount_rows - 1) as f32 * 10.0;
            for i in 0..(sim.air.max(0) as u32).div_ceil(30) {
                p.image(
                    "textures/gui/icons.png",
                    Rect {
                        x: x + 173.0 - i as f32 * 8.0,
                        y: air_y,
                        w: 9.0,
                        h: 9.0,
                    },
                    [16.0, 18.0, 9.0, 9.0],
                    WHITE,
                );
            }
        }
        if horse_jump.is_none() {
            p.image(
                "textures/gui/icons.png",
                Rect {
                    x,
                    y: h - 29.0,
                    w: 182.0,
                    h: 5.0,
                },
                [0.0, 64.0, 182.0, 5.0],
                WHITE,
            );
            let filled = (sim.xp.progress * 183.) as i32;
            if filled > 0 {
                p.image(
                    "textures/gui/icons.png",
                    Rect {
                        x,
                        y: h - 29.0,
                        w: filled as f32,
                        h: 5.0,
                    },
                    [0.0, 69.0, filled as f32, 5.0],
                    WHITE,
                );
            }
        }
        let level = sim.xp.level;
        if level > 0 {
            p.center(w / 2.0, h - 39.0, &level.to_string(), [0.5, 1.0, 0.1, 1.0]);
        }
    }
    if let Some((hp, max_hp)) = mount_health {
        let max_hearts = ((max_hp * 0.5).ceil() as i32).clamp(0, 30);
        let health = hp.ceil() as i32;
        for idx in 0..max_hearts {
            let row = idx / 10;
            let col = idx % 10;
            let xx = x + 173.0 - col as f32 * 8.0;
            let yy = h - 39.0 - row as f32 * 10.0;
            p.image(
                "textures/gui/icons.png",
                Rect {
                    x: xx,
                    y: yy,
                    w: 9.0,
                    h: 9.0,
                },
                [52.0, 9.0, 9.0, 9.0],
                WHITE,
            );
            let uv_x = if idx * 2 + 1 < health {
                Some(88.0)
            } else if idx * 2 + 1 == health {
                Some(97.0)
            } else {
                None
            };
            if let Some(uv_x) = uv_x {
                p.image(
                    "textures/gui/icons.png",
                    Rect {
                        x: xx,
                        y: yy,
                        w: 9.0,
                        h: 9.0,
                    },
                    [uv_x, 9.0, 9.0, 9.0],
                    WHITE,
                );
            }
        }
    }
    if let Some(charge) = horse_jump {
        p.image(
            "textures/gui/icons.png",
            Rect {
                x,
                y: h - 29.0,
                w: 182.0,
                h: 5.0,
            },
            [0.0, 84.0, 182.0, 5.0],
            WHITE,
        );
        let filled = (charge.clamp(0.0, 1.0) * 182.0) as i32;
        if filled > 0 {
            p.image(
                "textures/gui/icons.png",
                Rect {
                    x,
                    y: h - 29.0,
                    w: filled as f32,
                    h: 5.0,
                },
                [0.0, 89.0, filled as f32, 5.0],
                WHITE,
            );
        }
    }
    if let Some(item) = sim.inventory.held() {
        let name = creative::held_item_name(p, &item);
        p.center(w / 2.0, h - 60.0, &name, WHITE);
    }
    p.image(
        "textures/gui/icons.png",
        Rect {
            x: w / 2.0 - 8.0,
            y: h / 2.0 - 8.0,
            w: 16.0,
            h: 16.0,
        },
        [0.0, 0.0, 16.0, 16.0],
        WHITE,
    );
}
fn debug_hud(p:&mut Painter<'_>,sim:&Simulation,stats:&Stats) {
    {
        let lines = [
            format!(
                "Minecraft 1.8.9 (Ferrum {}/Vulkan)",
                env!("CARGO_PKG_VERSION")
            ),
            format!(
                "{:.0} fps ({:.2} ms)  C: {}/{}  T: {}",
                stats.fps, stats.frame_ms, stats.chunks, stats.total_chunks, stats.triangles
            ),
            format!(
                "E: {}/{}  Culled: {}  P: {}",
                stats.entities,
                stats.total_entities,
                stats.culled,
                sim.particles.len()
            ),
            if p.s.render_distance > 24 { format!("LOD: {}/{} tiles  Range: {}  Detail: {}",
                stats.distant_tiles.0, stats.distant_tiles.1, p.s.render_distance, p.s.full_detail_distance())
            } else { String::new() },
            format!(
                "XYZ: {:.3} / {:.3} / {:.3}",
                stats.position[0], stats.position[1], stats.position[2]
            ),
            format!(
                "Block: {} {} {}",
                stats.position[0].floor() as i32,
                stats.position[1].floor() as i32,
                stats.position[2].floor() as i32
            ),
            format!(
                "Facing: {:.1} / {:.1}",
                stats.yaw.to_degrees(),
                stats.pitch.to_degrees()
            ),
            format!(
                "Day {} / Time {} / {:?}",
                sim.clock / 24000,
                sim.clock % 24000,
                sim.mode
            ),
            stats.gpu.to_string(),
        ];
        let reduced = p.s.menu.reduced_debug || sim.server_reduced_debug;
        for (i, line) in lines.iter().enumerate().filter(|(i,_)| !reduced || *i < 4 || *i == 8) {
            if !line.is_empty() {
                p.rect(
                    1.0,
                    2.0 + i as f32 * 9.0,
                    p.width(line) + 2.0,
                    9.0,
                    [0.0, 0.0, 0.0, 0.5],
                );
                p.text(2.0, 3.0 + i as f32 * 9.0, line, WHITE);
            }
        }
    }
}
/// InventoryEffectRenderer re-centres the inventory and its hit area together.
pub(crate) fn inventory_left(view_width: f32, panel_width: f32, effects: bool) -> f32 {
    let width = view_width.ceil();
    if effects {
        160. + ((width - panel_width - 200.) / 2.).trunc()
    } else {
        ((width - panel_width) / 2.).trunc()
    }
}
fn inventory_screen(p: &mut Painter<'_>, gui: &Gui, sim: &Simulation) {
    let [w, h] = p.frame.size.map(f32::ceil);
    main_menu::gradient(p, Rect { x: 0., y: 0., w, h },
        [16. / 255., 16. / 255., 16. / 255., 192. / 255.],
        [16. / 255., 16. / 255., 16. / 255., 208. / 255.]);
    let crafting = gui.screen == Screen::Crafting;
    let effects = !crafting && sim.effects.ordered().next().is_some();
    let x = inventory_left(w, 176., effects);
    let y = ((h - 166.) / 2.).trunc();
    let tex = if crafting {
        "textures/gui/container/crafting_table.png"
    } else {
        "textures/gui/container/inventory.png"
    };
    p.image(
        tex,
        Rect {
            x,
            y,
            w: 176.0,
            h: 166.0,
        },
        [0.0, 0.0, 176.0, 166.0],
        WHITE,
    );
    if !crafting {
        // GuiInventory draws the player with the previous frame's mouse position.
        let mouse = gui.inventory_mouse.replace(p.mouse.map(f32::floor));
        let look = [x + 51. - mouse[0], y + 25. - mouse[1]];
        creative_preview::draw(p, x + 51., y + 75., 30., look, sim, gui.creative.slim);
        if effects {
            creative_preview::effects(p, x - 124., y, sim);
        }
    }
    let dark = [64. / 255., 64. / 255., 64. / 255., 1.];
    p.text_pass(
        x + if crafting { 28.0 } else { 86.0 },
        y + if crafting { 6. } else { 16. },
        &p.tr("container.crafting"),
        dark,
    );
    if crafting {
        p.text_pass(x + 8.0, y + 72.0, &p.tr("container.inventory"), dark);
    }
    for row in 0..3 {
        for col in 0..9 {
            let i = 9 + row * 9 + col;
            p.slot(
                i,
                sim.inventory.slots[i].clone(),
                x + 8.0 + col as f32 * 18.0,
                y + 84.0 + row as f32 * 18.0,
            );
        }
    }
    for i in 0..9 {
        p.slot(
            i,
            sim.inventory.slots[i].clone(),
            x + 8.0 + i as f32 * 18.0,
            y + 142.0,
        );
    }
    let width = if crafting { 3 } else { 2 };
    for row in 0..width {
        for col in 0..width {
            let i = row * width + col;
            p.slot(
                100 + i,
                sim.inventory.crafting[i].clone(),
                x + if crafting { 30.0 } else { 88.0 } + col as f32 * 18.0,
                y + if crafting { 17.0 } else { 26.0 } + row as f32 * 18.0,
            );
        }
    }
    let result = recipe(&sim.inventory.crafting, width).map(|r| r.output);
    let result_rect = Rect {
        x: x + if crafting { 124.0 } else { 144.0 },
        y: y + if crafting { 35.0 } else { 36.0 },
        w: 16.0,
        h: 16.0,
    };
    if let Some(item) = result.clone() {
        p.item(item, result_rect.x, result_rect.y, true);
    }
    p.frame.buttons.push(Button {
        rect: result_rect,
        action: Action::Craft,
        enabled: result.is_some(),
    });
    if !crafting {
        for (i, kind) in ["helmet", "chestplate", "leggings", "boots"].iter().enumerate() {
            let slot = 39 - i;
            let yy = y + 8. + i as f32 * 18.;
            if sim.inventory.slots[slot].is_none() {
                let icon = p.r.sprite(&format!("textures/items/empty_armor_slot_{kind}.png"));
                p.sprite(icon, Rect { x: x + 8., y: yy, w: 16., h: 16. },
                    [0., 0., icon.width as f32, icon.height as f32], WHITE);
            }
            p.slot(slot, sim.inventory.slots[slot].clone(), x + 8., yy);
        }
    }
    if sim.mode == Mode::Creative {
        p.button(
            Rect {
                x: x + 98.0,
                y: y - 24.0,
                w: 78.0,
                h: 20.0,
            },
            "Creative",
            Action::Screen(Screen::Creative),
            true,
        );
    }
    if let Some(item) = sim.inventory.cursor.clone() {
        p.item(item, p.mouse[0].floor() - 8.0, p.mouse[1].floor() - 8.0, true);
    } else {
        let hovered = p.frame.buttons.iter().find_map(|b| {
            if !b.rect.contains(p.mouse) { return None }
            match b.action {
                Action::Slot(i @ 0..=39) => sim.inventory.slots[i].as_ref(),
                Action::Slot(i @ 100..=108) => sim.inventory.crafting[i - 100].as_ref(),
                Action::Craft => result.as_ref(),
                _ => None,
            }
        });
        if let Some(item) = hovered {
            let lines = creative::item_tip(p, item, false, gui.advanced);
            creative::tooltip(p, &lines);
        }
    }
}
fn trading_screen(p: &mut Painter<'_>, gui: &Gui, sim: &Simulation) {
    let Some(trader) = gui.trader.and_then(|id| sim.trades.get(&id)) else {
        return;
    };
    let [w, h] = p.frame.size.map(f32::ceil);
    let x = ((w - 176.) / 2.).trunc();
    let y = ((h - 166.) / 2.).trunc();
    let dark = [64. / 255., 64. / 255., 64. / 255., 1.];
    main_menu::gradient(
        p,
        Rect { x: 0., y: 0., w, h },
        [16. / 255., 16. / 255., 16. / 255., 192. / 255.],
        [16. / 255., 16. / 255., 16. / 255., 208. / 255.],
    );
    p.image(
        "textures/gui/container/villager.png",
        Rect {
            x,
            y,
            w: 176.,
            h: 166.,
        },
        [0., 0., 176., 166.],
        WHITE,
    );
    let title = trader
        .custom_name
        .clone()
        .unwrap_or_else(|| p.tr(trader.title_key()));
    p.text_pass(
        x + 88. - (p.width(&title) / 2.).floor(),
        y + 6.,
        &title,
        dark,
    );
    let texture = "textures/gui/container/villager.png";
    let mut hovered: Option<Stack> = None;
    if let Some(offer) = trader.offers.get(gui.trade_index) {
        if !offer.supported || offer.uses >= offer.max_uses {
            for dy in [21., 51.] {
                p.image(
                    texture,
                    Rect {
                        x: x + 83.,
                        y: y + dy,
                        w: 28.,
                        h: 21.,
                    },
                    [212., 0., 28., 21.],
                    WHITE,
                );
            }
        }
        for (item, dx) in [
            (Some(&offer.cost), 36.),
            (offer.second.as_ref(), 62.),
            (Some(&offer.output), 120.),
        ] {
            if let Some(item) = item {
                let rect = Rect {
                    x: x + dx,
                    y: y + 24.,
                    w: 16.,
                    h: 16.,
                };
                p.item(item.clone(), rect.x, rect.y, true);
                if rect.contains(p.mouse) {
                    hovered = Some(item.clone());
                }
            }
        }
    }
    for (direction, dx, enabled) in [
        (-1, 17., gui.trade_index > 0),
        (1, 147., gui.trade_index + 1 < trader.offers.len()),
    ] {
        let rect = Rect {
            x: x + dx,
            y: y + 23.,
            w: 12.,
            h: 19.,
        };
        let u = 176.
            + if !enabled {
                24.
            } else if rect.contains(p.mouse) {
                12.
            } else {
                0.
            };
        p.image(
            texture,
            rect,
            [u, if direction < 0 { 19. } else { 0. }, 12., 19.],
            WHITE,
        );
        p.frame.buttons.push(Button {
            rect,
            action: Action::SelectTrade(direction),
            enabled,
        });
    }
    for (slot, dx) in [(0, 36.), (1, 62.), (2, 120.)] {
        let rect = Rect {
            x: x + dx,
            y: y + 53.,
            w: 16.,
            h: 16.,
        };
        let item = if slot == 2 {
            sim.merchant_window.output.as_ref()
        } else {
            sim.merchant_window.inputs()[slot].as_ref()
        };
        if let Some(item) = item {
            p.item(item.clone(), rect.x, rect.y, true);
        }
        if rect.contains(p.mouse) {
            p.rect(rect.x, rect.y, rect.w, rect.h, [1., 1., 1., 0.3]);
            hovered = item.cloned();
        }
        p.frame.buttons.push(Button {
            rect,
            action: Action::ContainerSlot(slot),
            enabled: true,
        });
    }
    p.text_pass(
        x + 8.,
        y + 72.,
        &p.tr("container.inventory"),
        dark,
    );
    for row in 0..3 {
        for col in 0..9 {
            let i = 9 + row * 9 + col;
            p.slot(
                i,
                sim.inventory.slots[i].clone(),
                x + 8. + col as f32 * 18.,
                y + 84. + row as f32 * 18.,
            );
        }
    }
    for i in 0..9 {
        p.slot(
            i,
            sim.inventory.slots[i].clone(),
            x + 8. + i as f32 * 18.,
            y + 142.,
        );
    }
    if let Some(item) = sim.inventory.cursor.clone() {
        p.item(item, p.mouse[0].floor() - 8., p.mouse[1].floor() - 8., true);
    } else {
        if hovered.is_none() {
            hovered = p.frame.buttons.iter().find_map(|button| {
                if !button.rect.contains(p.mouse) {
                    return None;
                }
                match button.action {
                    Action::Slot(i) => sim.inventory.slots.get(i).cloned().flatten(),
                    _ => None,
                }
            });
        }
        let unavailable = trader
            .offers
            .get(gui.trade_index)
            .is_some_and(|offer| offer.uses >= offer.max_uses || !offer.supported);
        let expired_hover = unavailable
            && [21., 51.].into_iter().any(|dy| {
                Rect {
                    x: x + 83.,
                    y: y + dy,
                    w: 28.,
                    h: 21.,
                }
                .contains(p.mouse)
            });
        let lines = if let Some(item) = hovered {
            creative::item_tip(p, &item, false, gui.advanced)
        } else if expired_hover {
            vec![p.tr("merchant.deprecated")]
        } else {
            vec![]
        };
        creative::tooltip(p, &lines);
    }
}
fn container_screen(p: &mut Painter<'_>, gui: &Gui, sim: &Simulation) {
    let Some(container) = gui.container_view.as_ref().or_else(|| {
        gui.container
            .and_then(|pos| sim.containers.get(&crate::gameplay::container_key(pos)))
    }) else {
        return;
    };
    if special_windows::draw(p,gui,sim,container){return}
    if utility_windows::draw(p,gui,sim,container){return}
    let [w, h] = p.frame.size.map(f32::ceil);
    let x = ((w - 176.0) / 2.0).trunc();
    let furnace = container.kind == crate::container::Kind::Furnace;
    let rows = if furnace {
        3
    } else {
        container.slots.len().div_ceil(9)
    };
    let enchant=gui.enchanting;
    let height = if furnace||enchant.is_some() {
        166.
    } else {
        114. + rows as f32 * 18.
    };
    let upper = if furnace||enchant.is_some() {
        71.
    } else {
        17. + rows as f32 * 18.
    };
    let y = ((h - height) / 2.0).trunc();
    let dark = [64. / 255., 64. / 255., 64. / 255., 1.0];
    main_menu::gradient(
        p,
        Rect { x: 0.0, y: 0.0, w, h },
        [16. / 255., 16. / 255., 16. / 255., 192. / 255.],
        [16. / 255., 16. / 255., 16. / 255., 208. / 255.],
    );
    if let Some(levels)=enchant{
        let tex="textures/gui/container/enchanting_table.png";
        p.image(tex,Rect{x,y,w:176.,h:166.},[0.,0.,176.,166.],WHITE);
        for (i,level) in levels.into_iter().enumerate(){
            let rect=Rect{x:x+60.,y:y+14.+i as f32*19.,w:108.,h:19.};
            let lapis=container.slots.get(1).and_then(Option::as_ref).filter(|s|s.id==351&&s.meta==4).map_or(0,|s|s.count);
            let enabled=level>0&&(sim.mode==Mode::Creative||(sim.xp.level>=i32::from(level)&&usize::from(lapis)>i));
            p.image(tex,rect,[0.,if !enabled{185.}else if rect.contains(p.mouse){204.}else{166.},108.,19.],WHITE);
            if level>0{
                p.image(tex,Rect{x:rect.x+1.,y:rect.y+1.,w:16.,h:16.},[16.*i as f32,if enabled{223.}else{239.},16.,16.],WHITE);
                let label=level.to_string();p.text(rect.x+104.-p.width(&label),rect.y+8.,&label,if enabled{[0.5,1.,0.125,1.]}else{GRAY});
            }
            p.frame.buttons.push(Button{rect,action:Action::Enchant(i as u8),enabled});
        }
    }else if furnace {
        p.image(
            "textures/gui/container/furnace.png",
            Rect {
                x,
                y,
                w: 176.0,
                h: 166.0,
            },
            [0.0, 0.0, 176.0, 166.0],
            WHITE,
        );
    } else {
        p.image(
            "textures/gui/container/generic_54.png",
            Rect {
                x,
                y,
                w: 176.0,
                h: upper,
            },
            [0.0, 0.0, 176.0, upper],
            WHITE,
        );
        p.image(
            "textures/gui/container/generic_54.png",
            Rect {
                x,
                y: y + upper,
                w: 176.0,
                h: 96.0,
            },
            [0.0, 126.0, 176.0, 96.0],
            WHITE,
        );
    }
    let title = gui.container_title.clone().unwrap_or_else(|| {
        p.tr(if enchant.is_some() {
            "container.enchant"
        } else if furnace {
            "container.furnace"
        } else if rows == 6 {
            "container.chestDouble"
        } else {
            "container.chest"
        })
    });
    let (title_x, title_y) = if enchant.is_some() {
        (x + 12.0, y + 5.0)
    } else if furnace {
        (x + 88.0 - (p.width(&title) / 2.0).floor(), y + 6.0)
    } else {
        (x + 8.0, y + 6.0)
    };
    p.text_pass(title_x, title_y, &title, dark);
    p.text_pass(
        x + 8.0,
        y + height - 94.0,
        &p.tr("container.inventory"),
        dark,
    );
    for (i, item) in container.slots.iter().enumerate() {
        let (xx, yy) = if enchant.is_some(){(15.+i as f32*20.,47.)}else if furnace {
            [(56.0, 17.0), (56.0, 53.0), (116.0, 35.0)][i]
        } else {
            (8.0 + (i % 9) as f32 * 18.0, 18.0 + (i / 9) as f32 * 18.0)
        };
        let rect = Rect {
            x: x + xx,
            y: y + yy,
            w: 16.0,
            h: 16.0,
        };
        if let Some(item) = item {
            p.item(item.clone(), rect.x, rect.y, true);
        }
        if rect.contains(p.mouse) {
            p.rect(rect.x, rect.y, 16.0, 16.0, [1.0, 1.0, 1.0, 0.3]);
        }
        p.frame.buttons.push(Button {
            rect,
            action: Action::ContainerSlot(i),
            enabled: true,
        });
    }
    if furnace {
        let flame = 13.0 * container.burn as f32 / container.burn_total.max(1) as f32;
        let progress = 24.0 * container.cook as f32 / 200.0;
        if flame > 0.0 {
            p.image(
                "textures/gui/container/furnace.png",
                Rect {
                    x: x + 56.0,
                    y: y + 49.0 - flame,
                    w: 14.0,
                    h: flame,
                },
                [176.0, 13.0 - flame, 14.0, flame],
                WHITE,
            );
        }
        p.image(
            "textures/gui/container/furnace.png",
            Rect {
                x: x + 79.0,
                y: y + 34.0,
                w: progress,
                h: 16.0,
            },
            [176.0, 14.0, progress, 16.0],
            WHITE,
        );
    }
    for row in 0..3 {
        for col in 0..9 {
            let i = 9 + row * 9 + col;
            p.slot(
                i,
                sim.inventory.slots[i].clone(),
                x + 8.0 + col as f32 * 18.0,
                y + upper + 13.0 + row as f32 * 18.0,
            );
        }
    }
    for i in 0..9 {
        p.slot(
            i,
            sim.inventory.slots[i].clone(),
            x + 8.0 + i as f32 * 18.0,
            y + upper + 71.0,
        );
    }
    if let Some(item) = sim.inventory.cursor.clone() {
        p.item(item, p.mouse[0].floor() - 8.0, p.mouse[1].floor() - 8.0, true);
    } else if let Some(item) = p.frame.buttons.iter().find_map(|b| {
        if !b.rect.contains(p.mouse) { return None }
        match b.action {
            Action::Slot(i) => sim.inventory.slots.get(i)?.as_ref(),
            Action::ContainerSlot(i) => container.slots.get(i)?.as_ref(),
            _ => None,
        }
    }) {
        let lines = creative::item_tip(p, item, false, gui.advanced);
        creative::tooltip(p, &lines);
    }
}

#[cfg(test)]
#[test]
fn chest_screen_aligns_glyphs_without_shadow_and_renders_barrier_sprite() {
    let r = crate::validation::resources();
    let a = crate::validation::assets();
    let settings = Settings {
        gui_scale: 2,
        ..Default::default()
    };
    let mut container = crate::container::Container::new(crate::container::Kind::Chest);
    container.slots.resize(36, None);
    container.slots[31] = Some(Stack::new(166, 1));
    let mut gui = Gui {
        screen: Screen::Chest,
        container_title: Some("Bed Wars Practice".into()),
        container_view: Some(container),
        ..Default::default()
    };
    gui.cursor = [0.0, 0.0];
    let sim = Simulation::new(Mode::Survival, 189);
    // Physical [854, 482] at scale 2 yields odd logical size [427.0, 241.0].
    let frame = build(&gui, &settings, r, a, [854, 482], Some(&sim), &Stats::default());
    let chest_layer = r.sprite("textures/gui/container/generic_54.png").layer;
    let font_layer = r.sprite("textures/font/ascii.png").layer;
    let barrier_layer = r.sprite("textures/items/barrier.png").layer;
    assert_ne!(barrier_layer, 0);

    let logical = |v: &UiVertex| -> [f32; 2] {
        [
            (v.position[0] + 1.0) * 0.5 * frame.size[0],
            (1.0 - v.position[1]) * 0.5 * frame.size[1],
        ]
    };

    let chest_verts: Vec<_> = frame
        .vertices
        .iter()
        .filter(|v| (v.layer & 0xFFFF) == chest_layer)
        .collect();
    assert!(!chest_verts.is_empty());
    for v in &chest_verts {
        let [lx, ly] = logical(v);
        assert!(
            (lx - lx.round()).abs() < 1e-3 && (ly - ly.round()).abs() < 1e-3,
            "chest quad must sit on integer logical pixels: ({lx}, {ly})"
        );
    }

    let font_verts: Vec<_> = frame
        .vertices
        .iter()
        .filter(|v| (v.layer & 0xFFFF) == font_layer)
        .collect();
    assert!(!font_verts.is_empty());
    for chunk in font_verts.chunks_exact(6) {
        for v in chunk {
            assert!(
                (v.color[0] - 64.0 / 255.0).abs() < 1e-4,
                "chest title/inventory text must use 0x404040 without 0x101010 drop shadow, got {:?}",
                v.color
            );
        }
        // Vertex 0 of each glyph quad is its top-left origin (FontRenderer uses [w - 1.01, 7.99] size).
        let [lx, ly] = logical(chunk[0]);
        assert!(
            (lx - lx.round()).abs() < 1e-3 && (ly - ly.round()).abs() < 1e-3,
            "chest font glyph origin must sit on integer logical pixels: ({lx}, {ly})"
        );
    }

    let barrier_verts = frame
        .vertices
        .iter()
        .filter(|v| (v.layer & 0xFFFF) == barrier_layer && (v.layer & 65536) == 0)
        .count();
    assert_eq!(barrier_verts, 6, "barrier slot must emit one 2D sprite quad");
    let missing_block_verts = frame
        .vertices
        .iter()
        .filter(|v| v.layer == 65536)
        .count();
    assert_eq!(missing_block_verts, 0, "barrier must not fall back to missing block layer");
}


