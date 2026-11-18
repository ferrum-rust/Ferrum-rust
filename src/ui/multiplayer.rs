//! Saved servers and ephemeral vanilla LAN advertisements.
#[path = "lan_discovery.rs"]
mod lan_discovery;
use super::{text_field::TextField, Action, Painter, Rect, Screen, WHITE};
use crate::resources::Resources;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::mpsc::Receiver,
    time::{Duration, Instant},
};
use winit::event::MouseButton;
use winit::keyboard::KeyCode;

const GRAY: [f32; 4] = [160. / 255., 160. / 255., 160. / 255., 1.];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Entry {
    name: String,
    address: String,
    #[serde(default)]
    packs: ResourceMode,
    #[serde(default)]
    sample: Option<u8>,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum ResourceMode {
    Enabled,
    Disabled,
    #[default]
    Prompt,
}
impl ResourceMode {
    fn next(self) -> Self {
        match self {
            Self::Enabled => Self::Disabled,
            Self::Disabled => Self::Prompt,
            Self::Prompt => Self::Enabled,
        }
    }
    fn key(self) -> &'static str {
        match self {
            Self::Enabled => "addServer.resourcePack.enabled",
            Self::Disabled => "addServer.resourcePack.disabled",
            Self::Prompt => "addServer.resourcePack.prompt",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Saved {
    entries: Vec<Entry>,
    #[serde(default)]
    last_address: String,
}
impl Default for Saved {
    fn default() -> Self {
        #[cfg(test)]
        let entries = [
            ("Ferrum Survival", "survival.example.invalid"),
            ("Ferrum Creative", "creative.example.invalid"),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (name, address))| Entry {
            name: name.into(),
            address: address.into(),
            sample: Some(i as u8),
            packs: Default::default(),
        })
        .collect();
        #[cfg(not(test))]
        let entries = Vec::new();
        Self {
            entries,
            last_address: String::new(),
        }
    }
}

#[derive(Clone, Debug)]
enum PingState {
    #[cfg_attr(test, allow(dead_code))]
    Pending,
    Ready {
        motd: [String; 2],
        players: String,
        ping_level: i32,
        incompatible: Option<String>,
        tooltip: String,
    },
    Failed(String),
}

type PingMessage = (
    u64,
    String,
    std::result::Result<crate::network::ServerStatus, String>,
);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    List,
    Add,
    Edit(usize),
    Direct,
    Delete(usize),
    Preview,
    Error,
}
#[derive(Clone, Debug)]
pub enum Command {
    Select(usize),
    Join(usize),
    Move(usize, i32),
    Navigate(i32, bool, f32),
    Add,
    Edit,
    Direct,
    Delete,
    Confirm,
    Cancel,
    Refresh,
    SetField(usize, usize),
    NextField,
    PackMode,
    Key(KeyCode, bool, bool),
}
// GuiScreenAddServer validates just the hostname (before the first colon),
// and accepts a single root dot, an empty host, and a trailing root label.
fn acceptable_address(address: &str) -> bool {
    let host = address.split(':').next().unwrap_or_default();
    let separator = |ch| matches!(ch, '.' | '\u{3002}' | '\u{ff0e}' | '\u{ff61}');
    if host.is_empty() || (host.chars().count() == 1 && host.starts_with(separator)) {
        return true;
    }
    let host = host.strip_suffix(separator).unwrap_or(host);
    host.split(separator).all(|label| {
        !label.is_empty()
            && if label.is_ascii() {
                label.len() <= 63
            } else {
                acceptable_unicode_label(label)
            }
    })
}

#[cfg(windows)]
fn acceptable_unicode_label(label: &str) -> bool {
    #[link(name = "Normaliz")]
    unsafe extern "system" {
        fn IdnToAscii(
            flags: u32,
            input: *const u16,
            input_len: i32,
            output: *mut u16,
            capacity: i32,
        ) -> i32;
    }
    let input: Vec<u16> = label.encode_utf16().collect();
    let len = unsafe {
        IdnToAscii(
            0,
            input.as_ptr(),
            input.len() as i32,
            std::ptr::null_mut(),
            0,
        )
    };
    (1..=63).contains(&len)
}

#[cfg(not(windows))]
fn acceptable_unicode_label(label: &str) -> bool {
    label.chars().count() <= 63
}

pub struct Browser {
    path: PathBuf,
    saved: Saved,
    discovery: lan_discovery::Discovery,
    page: Page,
    selected: Option<usize>,
    fields: [TextField; 2],
    focus: Option<usize>,
    packs: ResourceMode,
    preview: String,
    pub(crate) connect_to: Option<String>,
    error: String,
    scroll: f32,
    dragging: Option<(f32, f32, bool)>,
    clicked: Option<(usize, Instant)>,
    refreshed: Option<Instant>,
    pings: HashMap<String, PingState>,
    ping_rx: Option<Receiver<PingMessage>>,
    ping_gen: u64,
}
impl Default for Browser {
    fn default() -> Self {
        Self {
            path: PathBuf::from("servers.json"),
            saved: Default::default(),
            discovery: Default::default(),
            page: Page::List,
            selected: None,
            fields: Default::default(),
            focus: None,
            packs: Default::default(),
            preview: String::new(),
            connect_to: None,
            error: String::new(),
            scroll: 0.,
            dragging: None,
            clicked: None,
            refreshed: None,
            pings: HashMap::new(),
            ping_rx: None,
            ping_gen: 0,
        }
    }
}
impl Browser {
    pub fn capture(&mut self, name: &str, r: &Resources, lang: &str) -> Result<()> {
        self.refreshed = None;
        match name {
            "multiplayer-selected" => self.selected = Some(0),
            "add-server" => self.apply(Command::Add, r, lang)?,
            "direct-connect" => self.apply(Command::Direct, r, lang)?,
            "edit-server" | "delete-server" => {
                self.selected = Some(0);
                self.apply(
                    if name == "edit-server" {
                        Command::Edit
                    } else {
                        Command::Delete
                    },
                    r,
                    lang,
                )?;
            }
            _ => {}
        }
        Ok(())
    }
    fn load(path: &Path) -> Result<Saved> {
        match std::fs::read(path) {
            Ok(raw) => {
                #[cfg_attr(test, allow(unused_mut))]
                let mut saved: Saved =
                    serde_json::from_slice(&raw).context("Could not read the local server list")?;
                #[cfg(not(test))]
                saved
                    .entries
                    .retain(|e| !(e.sample.is_some() && e.address.ends_with(".invalid")));
                Ok(saved)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Saved::default()),
            Err(e) => Err(e.into()),
        }
    }
    pub fn enter(&mut self) -> Result<()> {
        self.scroll = 0.;
        self.clicked = None;
        self.selected = None;
        self.dragging = None;
        self.refreshed = Some(Instant::now());
        match Self::load(&self.path) {
            Ok(saved) => {
                self.saved = saved;
                self.page = Page::List;
                self.discovery.start();
                self.start_pings();
            }
            Err(e) => {
                self.page = Page::Error;
                self.error = e.to_string();
            }
        }
        Ok(())
    }
    pub fn leave(&mut self) {
        self.discovery.stop();
        self.ping_rx = None;
    }
    fn start_pings(&mut self) {
        #[cfg(test)]
        {
            return;
        }
        #[cfg(not(test))]
        {
            self.ping_gen = self.ping_gen.wrapping_add(1);
            let gen = self.ping_gen;
            let (tx, rx) = std::sync::mpsc::channel();
            self.ping_rx = Some(rx);
            self.pings.clear();
            for entry in &self.saved.entries {
                let addr = entry.address.trim().to_owned();
                if addr.is_empty()
                    || addr.ends_with(".invalid")
                    || self.pings.contains_key(&addr)
                {
                    continue;
                }
                self.pings.insert(addr.clone(), PingState::Pending);
                let tx = tx.clone();
                let _ = std::thread::Builder::new()
                    .name("ferrum-server-ping".into())
                    .spawn(move || {
                        let res = crate::network::status(&addr, Duration::from_secs(4))
                            .map_err(|e| format!("{e:#}"));
                        let _ = tx.send((gen, addr, res));
                    });
            }
        }
    }
    pub fn poll_lan(&mut self) {
        if self.at_list() {
            self.discovery.poll();
            if let Some(rx) = &self.ping_rx {
                while let Ok((gen, addr, result)) = rx.try_recv() {
                    if gen != self.ping_gen {
                        continue;
                    }
                    let state = match result {
                        Ok(status) => parse_server_status(status),
                        Err(err) => PingState::Failed(clean_disconnect_reason(&err)),
                    };
                    self.pings.insert(addr, state);
                }
            }
        }
    }
    fn row_count(&self) -> usize {
        self.saved.entries.len() + 1 + self.discovery.servers.len()
    }
    fn selectable(&self, row: usize) -> bool {
        row < self.row_count() && row != self.saved.entries.len()
    }
    fn commit(&mut self, saved: Saved) -> Result<()> {
        crate::world::atomic_json(&self.path, &saved)?;
        self.saved = saved;
        Ok(())
    }
    pub fn at_list(&self) -> bool {
        self.page == Page::List
    }
    fn form(&self) -> bool {
        matches!(self.page, Page::Add | Page::Edit(_) | Page::Direct)
    }
    fn valid(&self) -> bool {
        let ip = &self.fields[1].value;
        !ip.is_empty()
            && ip.chars().any(|c| c != ':')
            && (self.page == Page::Direct || !self.fields[0].value.is_empty())
    }
    pub fn key(&self, key: KeyCode, shift: bool, ctrl: bool, height: f32) -> Option<Command> {
        if key == KeyCode::Escape {
            // The two original address forms override keyTyped without calling
            // GuiScreen, as does the disconnected screen used for our preview.
            return (self.form() || self.page == Page::Preview)
                .then_some(Command::Key(key, shift, ctrl));
        }
        if self.at_list() {
            match key {
                KeyCode::F5 => Some(Command::Refresh),
                KeyCode::Enter | KeyCode::NumpadEnter => self.selected.map(Command::Join),
                KeyCode::ArrowUp | KeyCode::ArrowDown => {
                    self.selected?;
                    let delta = if key == KeyCode::ArrowUp { -1 } else { 1 };
                    Some(Command::Navigate(delta, shift, height))
                }
                _ => None,
            }
        } else if self.form() {
            Some(match key {
                KeyCode::Tab if self.page != Page::Direct => Command::NextField,
                KeyCode::Enter | KeyCode::NumpadEnter => Command::Confirm,
                _ => Command::Key(key, shift, ctrl),
            })
        } else {
            None
        }
    }
    pub fn text(&mut self, s: &str) {
        if self.form() {
            self.edit(|field, limit| field.insert(s, limit));
        }
    }
    fn edit(&mut self, change: impl FnOnce(&mut TextField, usize)) {
        let Some(focus) = self.focus else { return };
        let limit = if focus == 0 { 32 } else { 128 };
        let validate = focus == 1 && self.page != Page::Direct;
        let field = &mut self.fields[focus];
        let previous = validate.then(|| field.clone());
        change(field, limit);
        if let Some(previous) = previous {
            if previous.value != field.value && !acceptable_address(&field.value) {
                *field = previous;
            }
        }
    }
    fn max_scroll(&self, height: f32) -> f32 {
        (self.row_count() as f32 * 36. - (height.ceil() - 96. - 4.)).max(0.)
    }
    pub fn wheel(&mut self, delta: f32, height: f32) {
        if self.at_list() {
            self.scroll = (self.scroll - delta.signum() * 18.).clamp(0., self.max_scroll(height));
        }
    }
    pub fn pointer_down(
        &mut self,
        cursor: [f32; 2],
        size: [f32; 2],
        button: MouseButton,
        shift: bool,
    ) -> Option<Command> {
        let size = size.map(f32::ceil);
        let [x, y] = cursor;
        let cx = (size[0] / 2.).floor();
        if self.form() {
            self.focus = [0, 1].into_iter().find(|&i| {
                if self.page == Page::Direct && i == 0 {
                    return false;
                }
                let field_y = if self.page == Page::Direct {
                    116.
                } else {
                    [66., 106.][i]
                };
                box_at(cx - 100., field_y, 200., 20.).contains(cursor)
            });
            return None;
        }
        if self.at_list() && (32.0..size[1] - 64.).contains(&y) {
            let bar = cx + 154.;
            self.scroll = self.scroll.min(self.max_scroll(size[1]));
            if button == MouseButton::Left {
                self.dragging = Some((y, self.scroll, (bar..bar + 6.).contains(&x)));
            }
            let content_y = y.floor() - 36. + self.scroll.floor();
            if (cx - 152. ..=cx + 152.).contains(&x.floor()) && content_y >= 0. {
                let i = (content_y / 36.) as usize;
                let count = self.saved.entries.len();
                if i < count {
                    let dx = x.floor() - (cx - 150.);
                    let dy = content_y % 36.;
                    let action = if dx > 16. && dx < 32. {
                        Command::Join(i)
                    } else if dx < 16. && dy < 16. && i > 0 {
                        Command::Move(i, if shift { -(i as i32) } else { -1 })
                    } else if dx < 16. && dy > 16. && i + 1 < count {
                        Command::Move(i, if shift { (count - 1 - i) as i32 } else { 1 })
                    } else {
                        return Some(Command::Select(i));
                    };
                    self.dragging = None;
                    return Some(action);
                }
                if self.selectable(i) {
                    return Some(Command::Select(i));
                }
            }
        }
        None
    }
    pub fn drag(&mut self, cursor: [f32; 2], size: [f32; 2]) {
        if let Some((origin, scroll, bar)) = self.dragging {
            let (track, thumb) = self.scrollbar(size[1]);
            let max = self.max_scroll(size[1]);
            let ratio = if bar {
                max / (track - thumb).max(1.)
            } else {
                -1.
            };
            self.scroll = (scroll + (cursor[1] - origin) * ratio).clamp(0., max);
            if (cursor[1] - origin).abs() > 3. {
                self.clicked = None;
            }
        }
    }
    pub fn release(&mut self) {
        self.dragging = None;
    }
    fn scrollbar(&self, height: f32) -> (f32, f32) {
        let track = (height.ceil() - 96.).max(1.);
        let thumb = (track * track / (self.row_count() as f32 * 36.))
            .floor()
            .clamp(32f32.min(track), (track - 8.).max(32f32.min(track)));
        (track, thumb)
    }
    pub(crate) fn connection_error(&mut self, reason: String) {
        self.page = Page::Error;
        self.error = clean_disconnect_reason(&reason);
    }
    pub(crate) fn pack_mode(&self,address:&str)->ResourceMode{
        self.saved.entries.iter().find(|e|e.address==address).map_or(ResourceMode::Prompt,|e|e.packs)
    }
    fn join(&mut self,i:usize){
        if let Some(e) = self.saved.entries.get(i) {
            if e.sample.is_some() && e.address.ends_with(".invalid") {
                self.show_preview(e.address.clone());
            } else {
                self.connect_to = Some(e.address.trim().to_owned());
            }
        } else if let Some(j) = i.checked_sub(self.saved.entries.len()+1) {
            self.connect_to = self.discovery.servers.get(j).map(|e| e.address.clone());
        }
        self.clicked = None;
    }
    fn show_preview(&mut self, address: String) {
        self.preview = address;
        self.page = Page::Preview;
        self.clicked = None;
        self.dragging = None;
    }
    pub fn apply(&mut self, cmd: Command, r: &Resources, lang: &str) -> Result<()> {
        if let Err(e) = self.dispatch(cmd, &r.tr(lang, "selectServer.defaultName")) {
            self.error = e.to_string();
            self.page = Page::Error;
        }
        Ok(())
    }
    fn dispatch(&mut self, cmd: Command, default_name: &str) -> Result<()> {
        match cmd {
            Command::Refresh => return self.enter(),
            Command::Select(i) if self.at_list() && self.selectable(i) => {
                let now = Instant::now();
                if self.clicked.is_some_and(|(j, t)| {
                    j == i && now.duration_since(t) < Duration::from_millis(250)
                }) {
                    self.join(i);
                } else {
                    self.selected = Some(i);
                    self.clicked = Some((i, now));
                }
            }
            Command::Join(i) if self.at_list() && self.selectable(i) => {
                self.selected = Some(i);
                self.join(i)
            }
            Command::Move(i, delta) if self.at_list() && i < self.saved.entries.len() => {
                let j = i as i32 + delta;
                if j >= 0 && (j as usize) < self.saved.entries.len() {
                    let mut saved = self.saved.clone();
                    saved.entries.swap(i, j as usize);
                    self.commit(saved)?;
                    if self.selected == Some(i) {
                        self.selected = Some(j as usize);
                    }
                    self.clicked = None;
                }
            }
            Command::Add | Command::Edit | Command::Direct if self.at_list() => {
                let (page, name, address, packs) = match cmd {
                    Command::Add => (Page::Add, default_name, "", ResourceMode::Prompt),
                    Command::Direct => (
                        Page::Direct,
                        "",
                        self.saved.last_address.as_str(),
                        ResourceMode::Prompt,
                    ),
                    _ => {
                        let Some(i) = self.selected.filter(|i| *i < self.saved.entries.len()) else {
                            return Ok(());
                        };
                        let e = &self.saved.entries[i];
                        (Page::Edit(i), e.name.as_str(), e.address.as_str(), e.packs)
                    }
                };
                self.focus = Some(usize::from(page == Page::Direct));
                self.fields = [TextField::new(name), TextField::new(address)];
                self.packs = packs;
                self.page = page;
            }
            Command::Delete if self.at_list() => {
                if let Some(i) = self.selected.filter(|i| *i < self.saved.entries.len()) {
                    self.page = Page::Delete(i);
                }
            }
            Command::Confirm => match self.page {
                Page::Delete(i) => {
                    let mut saved = self.saved.clone();
                    saved.entries.remove(i);
                    self.commit(saved)?;
                    self.selected = None;
                    self.page = Page::List;
                }
                Page::Direct if self.valid() => {
                    self.store_direct()?;
                    self.connect_to=Some(self.fields[1].value.clone());self.page=Page::List;
                }
                Page::Add | Page::Edit(_) if self.valid() => {
                    let mut saved = self.saved.clone();
                    let entry = Entry {
                        name: self.fields[0].value.clone(),
                        address: self.fields[1].value.clone(),
                        packs: self.packs,
                        sample: None,
                    };
                    if let Page::Edit(i) = self.page {
                        let sample = saved.entries[i]
                            .sample
                            .filter(|_| saved.entries[i].address == entry.address);
                        saved.entries[i] = Entry { sample, ..entry };
                    } else {
                        saved.entries.push(entry);
                        self.selected = None;
                    }
                    self.commit(saved)?;
                    self.page = Page::List;
                    self.start_pings();
                }
                _ => {}
            },
            Command::Cancel => {
                if self.page == Page::Direct {
                    self.store_direct()?;
                }
                if self.page == Page::Error {
                    return self.enter();
                }
                self.page = Page::List;
                self.clicked = None;
            }
            Command::PackMode => self.packs = self.packs.next(),
            Command::NextField if matches!(self.page, Page::Add | Page::Edit(_)) => {
                self.focus = Some(usize::from(self.focus == Some(0)));
            }
            Command::SetField(i, pos) if self.form() && i < 2 => {
                self.focus = Some(i);
                self.fields[i].set_cursor(pos, false);
            }
            Command::Key(key, shift, ctrl) if self.form() => {
                self.edit(|field, limit| field.key(key, shift, ctrl, limit));
            }
            Command::Navigate(delta, reorder, height) if self.at_list() => {
                if let Some(i) = self.selected {
                    let mut dest = i as i32 + delta;
                    if !reorder && dest == self.saved.entries.len() as i32 {
                        dest += delta; // The scanning status row cannot be joined.
                    }
                    let exists = dest >= 0 && self.selectable(dest as usize);
                    if exists && reorder {
                        self.dispatch(Command::Move(i, delta), default_name)?;
                    } else if !reorder {
                        self.selected = exists.then_some(dest as usize);
                    }
                    if exists || !reorder && delta > 0 {
                        self.scroll =
                            (self.scroll + 36. * delta as f32).clamp(0., self.max_scroll(height));
                    }
                    self.clicked = None;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn store_direct(&mut self) -> Result<()> {
        if self.saved.last_address != self.fields[1].value {
            let mut saved = self.saved.clone();
            saved.last_address.clone_from(&self.fields[1].value);
            self.commit(saved)?;
        }
        Ok(())
    }
}

fn box_at(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect { x, y, w, h }
}
fn button(p: &mut Painter<'_>, rect: Rect, key: &str, cmd: Command, enabled: bool) {
    p.button(rect, &p.tr(key), Action::Server(cmd), enabled);
}
fn tile_band(p: &mut Painter<'_>, top: f32, bottom: f32, offset: f32, color: f32) {
    let w = p.frame.size[0].ceil();
    let mut y = top;
    while y < bottom {
        let v = (y + offset).rem_euclid(32.);
        let h = (32. - v).min(bottom - y);
        for x in (0..w.ceil() as usize).step_by(32) {
            p.image(
                "textures/gui/options_background.png",
                box_at(x as f32, y, 32., h),
                [0., v / 2., 16., h / 2.],
                [color, color, color, 1.],
            );
        }
        y += h;
    }
}
fn edit_field(p: &mut Painter<'_>, browser: &Browser, i: usize, y: f32, time: f32) {
    let rect = box_at((p.frame.size[0].ceil() / 2.).floor() - 100., y, 200., 20.);
    browser.fields[i].draw(p, rect, browser.focus == Some(i), time, if i == 0 { 32 } else { 128 }, |pos| Action::Server(Command::SetField(i,pos)));
}
fn clipped(rect: Rect, top: f32, bottom: f32) -> Rect {
    let y = rect.y.max(top);
    Rect {
        y,
        h: (rect.y + rect.h).min(bottom) - y,
        ..rect
    }
}
fn simple_text<'a>(p: &Painter<'_>, width: f32, text: &'a str) -> &'a str {
    let mut end = 0;
    let mut used = 0.;
    for ch in text.chars() {
        used += p.glyph_width(ch);
        if used > width {
            break;
        }
        end += ch.len_utf8();
    }
    &text[..end]
}
fn localized<'a>(p: &Painter<'_>, en: &'a str, ko: &'a str) -> &'a str {
    if p.s.language == "ko_kr" {
        ko
    } else {
        en
    }
}

fn color_code_for_name(name: &str) -> Option<char> {
    Some(match name {
        "black" => '0',
        "dark_blue" => '1',
        "dark_green" => '2',
        "dark_aqua" => '3',
        "dark_red" => '4',
        "dark_purple" => '5',
        "gold" => '6',
        "gray" => '7',
        "dark_gray" => '8',
        "blue" => '9',
        "green" => 'a',
        "aqua" => 'b',
        "red" => 'c',
        "light_purple" => 'd',
        "yellow" => 'e',
        "white" => 'f',
        _ => return None,
    })
}

fn chat_json_to_legacy(value: &serde_json::Value, inherited: Option<char>, out: &mut String) {
    match value {
        serde_json::Value::String(s) => {
            if let Some(c) = inherited {
                out.push('§');
                out.push(c);
            }
            out.push_str(s);
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                chat_json_to_legacy(item, inherited, out);
            }
        }
        serde_json::Value::Object(obj) => {
            let color = obj
                .get("color")
                .and_then(|v| v.as_str())
                .and_then(color_code_for_name)
                .or(inherited);
            if let Some(text) = obj.get("text").and_then(|v| v.as_str()) {
                if !text.is_empty() {
                    if let Some(c) = color {
                        out.push('§');
                        out.push(c);
                    }
                    out.push_str(text);
                }
            } else if let Some(tr) = obj.get("translate").and_then(|v| v.as_str()) {
                if let Some(c) = color {
                    out.push('§');
                    out.push(c);
                }
                out.push_str(tr);
            }
            if let Some(extra) = obj.get("extra") {
                chat_json_to_legacy(extra, color, out);
            }
        }
        _ => {}
    }
}

fn strip_section_codes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '§' {
            let _ = chars.next();
        } else {
            out.push(ch);
        }
    }
    out
}

fn clean_disconnect_reason(raw: &str) -> String {
    let mut text = raw.trim().to_owned();
    for marker in ["Login rejected: ", "Server disconnected: "] {
        if let Some(idx) = text.find(marker) {
            let prefix = &text[..idx + marker.len()];
            let rest = text[idx + marker.len()..].trim();
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(rest) {
                let mut parsed = String::new();
                chat_json_to_legacy(&val, None, &mut parsed);
                let plain = strip_section_codes(&parsed);
                if !plain.trim().is_empty() {
                    text = format!("{prefix}{}", plain.trim());
                    break;
                }
            }
        }
    }
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
        let mut parsed = String::new();
        chat_json_to_legacy(&val, None, &mut parsed);
        let plain = strip_section_codes(&parsed);
        if !plain.trim().is_empty() {
            return plain.trim().to_owned();
        }
    }
    strip_section_codes(&text)
}

fn parse_server_status(status: crate::network::ServerStatus) -> PingState {
    let ms = status.latency.as_millis().min(u32::MAX as u128) as u32;
    let ping_level = match ms {
        0..=149 => 0,
        150..=299 => 1,
        300..=599 => 2,
        600..=999 => 3,
        _ => 4,
    };
    let players = match (
        status.json["players"]["online"].as_i64(),
        status.json["players"]["max"].as_i64(),
    ) {
        (Some(online), Some(max)) => format!("{online}/{max}"),
        _ => String::new(),
    };
    let incompatible = match status.json["version"]["protocol"].as_i64() {
        Some(proto) if proto != i64::from(crate::protocol::PROTOCOL_VERSION) => status.json
            ["version"]["name"]
            .as_str()
            .map(strip_section_codes)
            .filter(|s| !s.is_empty()),
        _ => None,
    };
    let mut legacy_motd = String::new();
    chat_json_to_legacy(&status.json["description"], None, &mut legacy_motd);
    let legacy_motd = legacy_motd.replace('\r', "");
    let mut iter = legacy_motd.split('\n');
    let line1 = iter.next().unwrap_or_default().trim_end().to_owned();
    let line2 = iter.next().unwrap_or_default().trim_end().to_owned();
    let tooltip = if incompatible.is_some() {
        format!("호환되지 않는 프로토콜 ({ms} ms)")
    } else {
        format!("{ms} ms")
    };
    PingState::Ready {
        motd: [line1, line2],
        players,
        ping_level,
        incompatible,
        tooltip,
    }
}

fn mc_section_color(code: char, default: [f32; 4]) -> Option<[f32; 4]> {
    let rgb: [u8; 3] = match code.to_ascii_lowercase() {
        '0' => [0x00, 0x00, 0x00],
        '1' => [0x00, 0x00, 0xAA],
        '2' => [0x00, 0xAA, 0x00],
        '3' => [0x00, 0xAA, 0xAA],
        '4' => [0xAA, 0x00, 0x00],
        '5' => [0xAA, 0x00, 0xAA],
        '6' => [0xFF, 0xAA, 0x00],
        '7' => [0xAA, 0xAA, 0xAA],
        '8' => [0x55, 0x55, 0x55],
        '9' => [0x55, 0x55, 0xFF],
        'a' => [0x55, 0xFF, 0x55],
        'b' => [0x55, 0xFF, 0xFF],
        'c' => [0xFF, 0x55, 0x55],
        'd' => [0xFF, 0x55, 0xFF],
        'e' => [0xFF, 0xFF, 0x55],
        'f' => [0xFF, 0xFF, 0xFF],
        'r' => return Some(default),
        'k' | 'l' | 'm' | 'n' | 'o' => return None,
        _ => return None,
    };
    Some([
        f32::from(rgb[0]) / 255.,
        f32::from(rgb[1]) / 255.,
        f32::from(rgb[2]) / 255.,
        1.0,
    ])
}

fn draw_motd_line(p: &mut Painter<'_>, x: f32, y: f32, max_width: f32, text: &str, default_color: [f32; 4]) {
    let mut cursor_x = x;
    let mut color = default_color;
    let mut seg = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '§' {
            if let Some(code) = chars.next() {
                if !seg.is_empty() {
                    p.text_pass(cursor_x, y, &seg, color);
                    cursor_x += p.width(&seg);
                    seg.clear();
                }
                if let Some(next_color) = mc_section_color(code, default_color) {
                    color = next_color;
                }
                continue;
            }
        }
        let w = p.glyph_width(ch);
        if (cursor_x - x) + p.width(&seg) + w > max_width {
            break;
        }
        seg.push(ch);
    }
    if !seg.is_empty() {
        p.text_pass(cursor_x, y, &seg, color);
    }
}

fn wrap_lines(p: &Painter<'_>, text: &str, max_width: f32, max_lines: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for raw_line in text.split('\n') {
        let mut current = String::new();
        for ch in raw_line.chars() {
            let candidate_w = p.width(&current) + p.glyph_width(ch);
            if !current.is_empty() && candidate_w > max_width {
                lines.push(std::mem::take(&mut current));
                if lines.len() >= max_lines {
                    return lines;
                }
            }
            current.push(ch);
        }
        lines.push(current);
        if lines.len() >= max_lines {
            return lines;
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn list(p: &mut Painter<'_>, browser: &Browser, time: f32) {
    let [w, h] = p.frame.size.map(f32::ceil);
    let cx = (w / 2.).floor();
    let bottom = h - 64.;
    let scroll = browser.scroll.min(browser.max_scroll(h)).floor();
    tile_band(p, 32., bottom, scroll, 32. / 255.);
    let x = cx - 150.;
    let mut tip: Option<String> = None;
    let status_color = [170. / 255., 170. / 255., 170. / 255., 1.];
    let dim_color = [128. / 255., 128. / 255., 128. / 255., 1.];
    let err_color = [170. / 255., 0., 0., 1.];
    for (i, entry) in browser.saved.entries.iter().enumerate() {
        let y = 36. + 36. * i as f32 - scroll;
        if y + 34. < 32. || y - 2. > bottom {
            continue;
        }
        let row = clipped(box_at(cx - 152., y, 305., 36.), 32., bottom);
        if browser.selected == Some(i) {
            p.rect(cx - 152., y - 2., 304., 36., [0.5, 0.5, 0.5, 1.]);
            p.rect(cx - 151., y - 1., 302., 34., [0., 0., 0., 1.]);
        }
        let icon = box_at(x, y, 32., 32.);
        let sprite = p.r.sprite("textures/misc/unknown_server.png");
        p.sprite(
            sprite,
            icon,
            [0., 0., sprite.width as f32, sprite.height as f32],
            WHITE,
        );
        let live_state = browser.pings.get(entry.address.trim());
        let tx = x + 35.;
        let (pinging, ping_level, right_label, right_color, motd_lines, motd_color, hover_tip) =
            if let Some(sample) = entry.sample.filter(|_| entry.address.ends_with(".invalid")) {
                let pinging = browser
                    .refreshed
                    .is_some_and(|t| t.elapsed() < Duration::from_millis(400));
                let count = if sample == 0 { "3/20" } else { "0/16" };
                let motd = if pinging {
                    ""
                } else {
                    localized(p, "Local server preview", "로컬 서버 미리보기")
                };
                let tip_str = if pinging {
                    "..."
                } else if sample == 0 {
                    "42ms"
                } else {
                    "180ms"
                };
                (
                    pinging,
                    i32::from(sample),
                    count.to_owned(),
                    dim_color,
                    [motd.to_owned(), String::new()],
                    dim_color,
                    tip_str.to_owned(),
                )
            } else {
                match live_state {
                    None | Some(PingState::Pending) => {
                        let msg = localized(p, "Pinging...", "서버 상태 확인 중...");
                        (
                            true,
                            0,
                            String::new(),
                            dim_color,
                            [msg.to_owned(), String::new()],
                            dim_color,
                            "...".to_owned(),
                        )
                    }
                    Some(PingState::Ready {
                        motd,
                        players,
                        ping_level,
                        incompatible,
                        tooltip,
                    }) => {
                        if let Some(ver) = incompatible {
                            (
                                false,
                                5,
                                ver.clone(),
                                err_color,
                                motd.clone(),
                                status_color,
                                tooltip.clone(),
                            )
                        } else {
                            (
                                false,
                                *ping_level,
                                players.clone(),
                                status_color,
                                motd.clone(),
                                status_color,
                                tooltip.clone(),
                            )
                        }
                    }
                    Some(PingState::Failed(err)) => {
                        let msg = localized(p, "Can't connect to server", "서버에 연결할 수 없습니다");
                        (
                            false,
                            5,
                            String::new(),
                            err_color,
                            [msg.to_owned(), entry.address.clone()],
                            err_color,
                            if err.is_empty() {
                                "(no connection)".to_owned()
                            } else {
                                simple_text(p, 240., err).to_owned()
                            },
                        )
                    }
                }
            };

        p.text_pass(
            tx,
            y + 1.,
            simple_text(p, (250. - p.width(&right_label)).max(80.), &entry.name),
            WHITE,
        );
        if !motd_lines[0].is_empty() {
            draw_motd_line(p, tx, y + 12., 264., &motd_lines[0], motd_color);
        }
        if !motd_lines[1].is_empty() {
            let line2_color = if ping_level == 5 && right_label.is_empty() {
                dim_color
            } else {
                motd_color
            };
            draw_motd_line(p, tx, y + 21., 264., &motd_lines[1], line2_color);
        }
        if !pinging && !right_label.is_empty() {
            p.text_pass(x + 288. - p.width(&right_label), y + 1., &right_label, right_color);
        }
        let ping_box = box_at(x + 290., y, 10., 8.);
        let animation = ((time * 10.) as usize + i * 2) % 8;
        let level = if pinging {
            animation.min(8 - animation) as f32
        } else {
            ping_level as f32
        };
        p.image(
            "textures/gui/icons.png",
            ping_box,
            [if pinging { 10. } else { 0. }, 176. + level * 8., 10., 8.],
            WHITE,
        );
        if row.contains(p.mouse) || p.s.touchscreen {
            p.rect(x, y, 32., 32., [0.56, 0.56, 0.56, 0.63]);
            let areas = [
                (box_at(x + 16., y, 16., 32.), 0., true),
                (box_at(x, y, 16., 16.), 96., i > 0),
                (
                    box_at(x, y + 16., 16., 16.),
                    64.,
                    i + 1 < browser.saved.entries.len(),
                ),
            ];
            for (area, u, enabled) in areas {
                if enabled {
                    p.image(
                        "textures/gui/server_selection.png",
                        icon,
                        [u, if area.contains(p.mouse) { 32. } else { 0. }, 32., 32.],
                        WHITE,
                    );
                }
            }
            if ping_box.contains(p.mouse) {
                tip = Some(hover_tip);
            }
        }
    }
    let y = 36. + 36. * browser.saved.entries.len() as f32 - scroll + 12.;
    if y + 20. >= 32. && y < bottom {
        let title = p.tr("lanServer.scanning");
        let dots = ["O o o", "o O o", "o o O", "o O o"][((time / 0.3) as usize) % 4];
        p.text_pass(cx - (p.width(&title) / 2.).floor(), y, &title, WHITE);
        p.text_pass(
            cx - (p.width(dots) / 2.).floor(),
            y + 9.,
            dots,
            [128. / 255., 128. / 255., 128. / 255., 1.],
        );
    }
    for (j, server) in browser.discovery.servers.iter().enumerate() {
        let row = browser.saved.entries.len() + 1 + j;
        let y = 36. + 36. * row as f32 - scroll;
        if y + 34. < 32. || y - 2. > bottom {
            continue;
        }
        if browser.selected == Some(row) {
            p.rect(cx - 152., y - 2., 304., 36., [0.5, 0.5, 0.5, 1.]);
            p.rect(cx - 151., y - 1., 302., 34., [0., 0., 0., 1.]);
        }
        p.text_pass(x + 35., y + 1., &p.tr("lanServer.title"), WHITE);
        p.text_pass(x + 35., y + 12., &server.motd, [128./255., 128./255., 128./255., 1.]);
        let address = if p.s.menu.hide_server_address { p.tr("selectServer.hiddenAddress") } else { server.address.clone() };
        p.text_pass(x + 35., y + 23., &address, [48./255., 48./255., 48./255., 1.]);
    }
    tile_band(p, 0., 32., 0., 64. / 255.);
    tile_band(p, bottom, h, 0., 64. / 255.);
    super::main_menu::gradient(p, box_at(0., 32., w, 4.), [0., 0., 0., 1.], [0.; 4]);
    super::main_menu::gradient(p, box_at(0., bottom - 4., w, 4.), [0.; 4], [0., 0., 0., 1.]);
    let max = browser.max_scroll(h);
    if max > 0. {
        let (track, thumb) = browser.scrollbar(h);
        let x = cx + 154.;
        let y = 32. + (scroll * (track - thumb) / max).floor();
        p.rect(x, 32., 6., track, [0., 0., 0., 1.]);
        p.rect(x, y, 6., thumb, [0.5, 0.5, 0.5, 1.]);
        p.rect(x, y, 5., thumb - 1., [0.75, 0.75, 0.75, 1.]);
    }
    p.center(cx, 20., &p.tr("multiplayer.title"), WHITE);
    let has_token = std::env::var_os("FERRUM189_MC_ACCESS_TOKEN")
        .is_some_and(|v| !v.is_empty());
    if let Ok(name) = std::env::var("FERRUM189_MC_USERNAME") {
        let name = name.trim();
        if !name.is_empty() && has_token {
            p.text(
                6.,
                6.,
                &format!("Microsoft 정품: {name}"),
                [0.45, 0.95, 0.68, 1.0],
            );
        }
    } else if !cfg!(test) {
        p.text(
            6.,
            6.,
            localized(p, "Offline Mode", "오프라인 모드 (클라이언트.exe에서 MS 로그인)"),
            [1.0, 0.78, 0.35, 1.0],
        );
    }
    let selected = browser.selected;
    button(
        p,
        box_at(cx - 154., h - 52., 100., 20.),
        "selectServer.select",
        Command::Join(selected.unwrap_or(0)),
        selected.is_some(),
    );
    button(
        p,
        box_at(cx - 50., h - 52., 100., 20.),
        "selectServer.direct",
        Command::Direct,
        true,
    );
    button(
        p,
        box_at(cx + 54., h - 52., 100., 20.),
        "selectServer.add",
        Command::Add,
        true,
    );
    button(
        p,
        box_at(cx - 154., h - 28., 70., 20.),
        "selectServer.edit",
        Command::Edit,
        selected.is_some_and(|row| row < browser.saved.entries.len()),
    );
    button(
        p,
        box_at(cx - 74., h - 28., 70., 20.),
        "selectServer.delete",
        Command::Delete,
        selected.is_some_and(|row| row < browser.saved.entries.len()),
    );
    button(
        p,
        box_at(cx + 4., h - 28., 70., 20.),
        "selectServer.refresh",
        Command::Refresh,
        true,
    );
    p.button(
        box_at(cx + 80., h - 28., 75., 20.),
        &p.tr("gui.cancel"),
        Action::Screen(Screen::Main),
        true,
    );
    if let Some(value) = tip.as_deref() {
        let width = p.width(value);
        let x = (p.mouse[0] + 12.).min(w - width - 6.);
        let y = p.mouse[1] - 12.;
        p.rect(x - 3., y - 3., width + 6., 15., [0.06, 0., 0.10, 0.94]);
        p.text(x, y, value, WHITE);
    }
}
pub(super) fn draw(p: &mut Painter<'_>, browser: &Browser, time: f32) {
    let [w, h] = p.frame.size.map(f32::ceil);
    let cx = (w / 2.).floor();
    let quarter = (h / 4.).floor();
    if browser.at_list() {
        list(p, browser, time);
        return;
    }
    p.background();
    match browser.page {
        Page::Add | Page::Edit(_) => {
            p.center(cx, 17., &p.tr("addServer.title"), WHITE);
            p.text(cx - 100., 53., &p.tr("addServer.enterName"), GRAY);
            p.text(cx - 100., 94., &p.tr("addServer.enterIp"), GRAY);
            edit_field(p, browser, 0, 66., time);
            edit_field(p, browser, 1, 106., time);
            let packs = format!(
                "{}: {}",
                p.tr("addServer.resourcePack"),
                p.tr(browser.packs.key())
            );
            p.button(
                box_at(cx - 100., quarter + 72., 200., 20.),
                &packs,
                Action::Server(Command::PackMode),
                true,
            );
            button(
                p,
                box_at(cx - 100., quarter + 114., 200., 20.),
                "addServer.add",
                Command::Confirm,
                browser.valid(),
            );
            button(
                p,
                box_at(cx - 100., quarter + 138., 200., 20.),
                "gui.cancel",
                Command::Cancel,
                true,
            );
        }
        Page::Direct => {
            p.center(cx, 20., &p.tr("selectServer.direct"), WHITE);
            p.text(cx - 100., 100., &p.tr("addServer.enterIp"), GRAY);
            edit_field(p, browser, 1, 116., time);
            button(
                p,
                box_at(cx - 100., quarter + 108., 200., 20.),
                "selectServer.select",
                Command::Confirm,
                browser.valid(),
            );
            button(
                p,
                box_at(cx - 100., quarter + 132., 200., 20.),
                "gui.cancel",
                Command::Cancel,
                true,
            );
        }
        Page::Delete(i) => {
            p.center(cx, 70., &p.tr("selectServer.deleteQuestion"), WHITE);
            p.center(
                cx,
                90.,
                &format!(
                    "'{}' {}",
                    browser.saved.entries[i].name,
                    p.tr("selectServer.deleteWarning")
                ),
                WHITE,
            );
            button(
                p,
                box_at(cx - 155., (h / 6.).floor() + 96., 150., 20.),
                "selectServer.deleteButton",
                Command::Confirm,
                true,
            );
            button(
                p,
                box_at(cx + 5., (h / 6.).floor() + 96., 150., 20.),
                "gui.cancel",
                Command::Cancel,
                true,
            );
        }
        Page::Preview => {
            p.center(
                cx,
                h / 2. - 27.,
                localized(p, "Local server preview", "로컬 서버 미리보기"),
                GRAY,
            );
            p.center(cx, h / 2. - 9., &browser.preview, WHITE);
            p.center(
                cx,
                h / 2.,
                localized(
                    p,
                    "Use Direct Connect to join a server.",
                    "실제 서버는 직접 연결로 접속하세요.",
                ),
                WHITE,
            );
            button(
                p,
                box_at(cx - 100., h / 2. + 27., 200., 20.),
                "gui.toMenu",
                Command::Cancel,
                true,
            );
        }
        Page::Error => {
            let lines = wrap_lines(p, &browser.error, (w - 48.).max(220.), 6);
            let total_h = lines.len() as f32 * 11.;
            let top_y = (h / 2. - total_h * 0.5).max(48.);
            p.center(cx, top_y - 18., &p.tr("connect.failed"), GRAY);
            for (idx, line) in lines.iter().enumerate() {
                p.center(cx, top_y + idx as f32 * 11., line, WHITE);
            }
            let btn_y = (top_y + total_h + 14.).max(h / 2. + 26.).min(h - 28.);
            p.button(
                box_at(cx - 100., btn_y, 200., 20.),
                &p.tr("gui.back"),
                Action::Server(Command::Cancel),
                true,
            );
        }
        Page::List => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_add_edit_move_direct_and_delete_use_only_local_data() {
        let root = std::env::temp_dir().join(format!("ferrum-browser-{}", std::process::id()));
        let mut b = Browser {
            path: root.join("servers.json"),
            ..Default::default()
        };
        b.enter().unwrap();
        assert_eq!(b.saved.entries.len(), 2);
        b.dispatch(Command::Add, "Server").unwrap();
        assert!(!b.valid());
        b.dispatch(Command::Key(KeyCode::KeyA, false, true), "")
            .unwrap();
        b.text("마을");
        b.dispatch(Command::NextField, "").unwrap();
        b.text("example.invalid:25565");
        b.dispatch(Command::PackMode, "").unwrap();
        b.dispatch(Command::Confirm, "").unwrap();
        let entry = &b.saved.entries[2];
        assert_eq!(
            (&entry.name[..], entry.packs),
            ("마을", ResourceMode::Enabled)
        );
        b.dispatch(Command::Select(2), "").unwrap();
        b.dispatch(Command::Edit, "").unwrap();
        b.fields[0] = TextField::new("Cancel change");
        b.dispatch(Command::Cancel, "").unwrap();
        assert_eq!(b.saved.entries[2].name, "마을");
        b.dispatch(Command::Move(2, -1), "").unwrap();
        b.dispatch(Command::Edit, "").unwrap();
        b.fields[0] = TextField::new("Edited");
        b.dispatch(Command::Confirm, "").unwrap();
        b.enter().unwrap();
        assert_eq!(b.saved.entries[1].name, "Edited");
        b.dispatch(Command::Select(1), "").unwrap();
        b.dispatch(Command::Select(1), "").unwrap();
        assert_eq!(b.connect_to.take().as_deref(),Some(b.saved.entries[1].address.as_str()));
        b.dispatch(Command::Cancel, "").unwrap();
        b.dispatch(Command::Delete, "").unwrap();
        b.dispatch(Command::Confirm, "").unwrap();
        assert_eq!(b.saved.entries.len(), 2);
        b.dispatch(Command::Direct, "").unwrap();
        b.text("[::1]:25565");
        b.dispatch(Command::Confirm, "").unwrap();
        assert_eq!(b.connect_to.take().as_deref(),Some("[::1]:25565"));
        b.enter().unwrap();
        b.dispatch(Command::Direct, "").unwrap();
        assert_eq!(b.fields[1].value, "[::1]:25565");
        std::fs::remove_file(&b.path).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn server_text_fields_preserve_unicode_selection_and_java_length_limit() {
        let mut field = TextField::new("가A");
        field.key(KeyCode::ArrowLeft, false, false, 4);
        field.key(KeyCode::ArrowLeft, true, false, 4);
        field.insert("🎮XY", 4);
        assert_eq!(field.value, "🎮XA");
        field.key(KeyCode::Home, false, false, 4);
        field.key(KeyCode::Delete, false, false, 4);
        assert_eq!(field.value, "XA");
    }
    #[test]
    fn udp_advertisements_join_through_rows_enter_and_double_click_without_persistence() {
        use std::net::UdpSocket;
        let mut b = Browser::default();
        let before = serde_json::to_vec(&b.saved).unwrap();
        let addr = lan_discovery::loopback(&mut b.discovery);
        let sender = UdpSocket::bind(("127.0.0.1", 0)).unwrap();
        for message in [
            "[MOTD]A[/MOTD][AD]12345[/AD]",
            "[MOTD]repeat[/MOTD][AD]12345[/AD]",
            "[MOTD]malformed[/MOTD][/MOTD][AD]1[/AD]",
            "[MOTD]bad port[/MOTD][AD]invalid[/AD]",
            "[MOTD]다른 월드[/MOTD][AD]45678[/AD]",
        ] {
            sender.send_to(message.as_bytes(), addr).unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while b.discovery.servers.len() < 2 && Instant::now() < deadline {
            b.poll_lan();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(b.discovery.servers.len(), 2);
        assert_eq!(b.discovery.servers[0].motd, "A");
        assert_eq!(b.discovery.servers[1].motd, "다른 월드");
        let last_saved = b.saved.entries.len() - 1;
        let scan = last_saved + 1;
        let lan = scan + 1;
        let size = [800., 450.];
        let row = |i| [400., 40. + 36. * i as f32];
        assert!(b.pointer_down(row(scan), size, MouseButton::Left, false).is_none());
        b.dispatch(Command::Select(last_saved), "").unwrap();
        b.dispatch(b.key(KeyCode::ArrowDown, false, false, size[1]).unwrap(), "").unwrap();
        assert_eq!(b.selected, Some(lan));
        b.dispatch(b.key(KeyCode::Enter, false, false, size[1]).unwrap(), "").unwrap();
        assert_eq!(b.connect_to.take().as_deref(), Some("127.0.0.1:12345"));
        b.dispatch(Command::Edit, "").unwrap();
        b.dispatch(Command::Delete, "").unwrap();
        assert!(b.at_list());
        for _ in 0..2 {
            let command = b.pointer_down(row(lan + 1), size, MouseButton::Left, false).unwrap();
            b.dispatch(command, "").unwrap();
        }
        assert_eq!(b.connect_to.take().as_deref(), Some("127.0.0.1:45678"));
        assert_eq!(before, serde_json::to_vec(&b.saved).unwrap());
    }
    #[test]
    fn mouse_and_keyboard_follow_scrolled_rows_focus_and_original_reordering_rules() {
        let dir = std::env::temp_dir().join(format!("ferrum-browser-input-{}", std::process::id()));
        let mut b = Browser {
            path: dir.join("servers.json"),
            ..Default::default()
        };
        let size = [799.5, 239.5];
        while b.saved.entries.len() < 12 {
            b.saved.entries.push(b.saved.entries[0].clone());
        }
        let selected = b.pointer_down([620., 36.], size, MouseButton::Right, false);
        assert!(selected.is_none());
        let cmd = b
            .pointer_down([400., 36.], size, MouseButton::Right, false)
            .unwrap();
        b.dispatch(cmd, "").unwrap();
        assert_eq!(b.selected, Some(0));
        let cmd = b.key(KeyCode::ArrowUp, false, false, size[1]).unwrap();
        b.dispatch(cmd, "").unwrap();
        assert_eq!(b.selected, None);
        b.dispatch(Command::Select(1), "").unwrap();
        let cmd = b.key(KeyCode::ArrowDown, true, false, size[1]).unwrap();
        b.dispatch(cmd, "").unwrap();
        assert_eq!((b.selected, b.scroll), (Some(2), 36.));
        let cmd = b
            .pointer_down([252., 36. + 36. + 22.], size, MouseButton::Left, true)
            .unwrap();
        b.dispatch(cmd, "").unwrap();
        assert_eq!(b.selected, Some(11));
        assert_eq!(b.saved.entries[11].name, "Ferrum Creative");
        b.dispatch(Command::Add, "Server").unwrap();
        let esc = b.key(KeyCode::Escape, false, false, size[1]).unwrap();
        b.dispatch(esc, "").unwrap();
        assert_eq!(b.page, Page::Add);
        b.pointer_down([12., 22.], size, MouseButton::Left, false);
        b.text("ignored");
        assert_eq!(b.fields[0].value, "Server");
        b.pointer_down([320., 112.], size, MouseButton::Right, false);
        b.text("example.invalid");
        assert_eq!(b.fields[1].value, "example.invalid");
        b.dispatch(Command::Cancel, "").unwrap();
        assert!(b.key(KeyCode::Escape, false, false, size[1]).is_none());
        b.dispatch(Command::Direct, "").unwrap();
        for key in [KeyCode::Tab, KeyCode::Escape] {
            let cmd = b.key(key, false, false, size[1]).unwrap();
            b.dispatch(cmd, "").unwrap();
            assert_eq!((b.page, b.focus), (Page::Direct, Some(1)));
        }
        std::fs::remove_file(&b.path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn adding_a_server_checks_the_hostname_on_each_edit_while_direct_connect_keeps_input() {
        let mut b = Browser::default();
        b.dispatch(Command::Add, "Server").unwrap();
        b.dispatch(Command::NextField, "").unwrap();
        b.text(&"a".repeat(64));
        assert!(b.fields[1].value.is_empty());
        b.text("마을。example.");
        b.text(".invalid");
        assert_eq!(b.fields[1].value, "마을。example.");
        b.text("invalid:25565");
        assert!(b.valid());
        b.dispatch(Command::Cancel, "").unwrap();
        b.dispatch(Command::Direct, "").unwrap();
        b.text("broken..example");
        assert_eq!(b.fields[1].value, "broken..example");
        assert!(b.valid());
    }
    #[test]
    #[ignore = "requires locally imported Minecraft resources"]
    fn original_title_and_server_widget_bounds_and_enabled_states_match() {
        let original: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-menu-layouts.json"
        ))
        .unwrap();
        let settings = crate::settings::Settings {
            gui_scale: 2,
            language: "ko_kr".into(),
            ..Default::default()
        };
        let r = crate::validation::resources();
        for name in [
            "main",
            "multiplayer",
            "multiplayer-selected",
            "add-server",
            "edit-server",
            "direct-connect",
            "delete-server",
        ] {
            let mut gui = super::super::Gui {
                screen: if name == "main" {
                    Screen::Main
                } else {
                    Screen::Multiplayer
                },
                ..Default::default()
            };
            gui.multiplayer
                .capture(name, r, &settings.language)
                .unwrap();
            let frame = super::super::build(
                &gui,
                &settings,
                r,
                crate::validation::assets(),
                [1600, 900],
                None,
                &Default::default(),
            );
            let mut actual = frame
                .buttons
                .iter()
                .filter(|b| {
                    !matches!(
                        b.action,
                        Action::Server(Command::SetField(..) | Command::Select(_))
                    )
                })
                .map(|b| {
                    (
                        [
                            b.rect.x as i32,
                            b.rect.y as i32,
                            b.rect.w as i32,
                            b.rect.h as i32,
                        ],
                        b.enabled,
                    )
                })
                .collect::<Vec<_>>();
            let mut expected = original[name]["buttons"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|b| b[6].as_bool() == Some(true))
                .map(|b| {
                    (
                        [1, 2, 3, 4].map(|i| b[i].as_i64().unwrap() as i32),
                        b[5].as_bool().unwrap(),
                    )
                })
                .collect::<Vec<_>>();
            actual.sort_unstable();
            expected.sort_unstable();
            assert_eq!(actual, expected, "original initGui buttons: {name}");
        }
    }
}
