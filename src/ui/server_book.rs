use super::*;
use crate::vanilla::{drops::DropNbt as N, item_nbt};
#[derive(Default)]
pub(crate) struct Book {
    pub pages: Vec<String>,
    pub page: usize,
    pub unsigned: bool,
    pub signing: bool,
    pub title: String,
    pub author: String,
    modified: bool,
}
/// PlayerControllerMP.onPlayerRightClick calls the block before using the held book.
/// This is the client-side return value; several containers return true even when
/// the server subsequently declines to open them.
pub(crate) fn block_consumes(
    world: &crate::world::World,
    pos: glam::IVec3,
    mode: u8,
    sneaking: bool,
) -> bool {
    if mode == 3 {
        return true;
    }
    if sneaking {
        return false;
    }
    let state = world.get(pos);
    match state >> 4 {
        64 | 193..=197 if state & 8 != 0 => world.get(pos - glam::IVec3::Y) >> 4 == state >> 4,
        84 => state & 1 != 0,
        93 | 94 | 149..=151 | 178 => mode != 2,
        137 => mode == 1,
        23
        | 25
        | 26
        | 54
        | 58
        | 61..=64
        | 68
        | 69
        | 71
        | 77
        | 85
        | 92
        | 96
        | 107
        | 113
        | 116..=118
        | 122
        | 130
        | 138
        | 143
        | 145
        | 146
        | 154
        | 158
        | 167
        | 183..=197 => true,
        _ => false,
    }
}

impl Book {
    pub fn from_stack(stack: &Stack) -> Option<Self> {
        if !matches!(stack.id, 386 | 387) {
            return None;
        }
        let unsigned = stack.id == 386;
        let m = stack.nbt.as_deref().and_then(item_nbt::compound);
        let mut pages = match m.and_then(|m| m.get("pages")) {
            Some(N::List { element: 8, values }) => values
                .iter()
                .filter_map(|v| {
                    if let N::String(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .collect(),
            _ => Vec::new(),
        };
        if pages.is_empty() {
            pages.push(String::new())
        }
        let valid = m.is_some_and(|m| {
            matches!(m.get("author"), Some(N::String(_)))
                && matches!(m.get("title"),Some(N::String(s)) if s.encode_utf16().count()<=32)
        });
        if !unsigned && !valid {
            pages = vec![
                serde_json::json!({"text":"* Invalid book tag *","color":"dark_red"}).to_string(),
            ];
        }
        Some(Self {
            unsigned,
            pages,
            ..Default::default()
        })
    }
    pub fn page_by(&mut self, delta: i32) {
        if delta > 0 && self.unsigned && self.page + 1 == self.pages.len() && self.pages.len() < 50
        {
            self.pages.push(String::new());
            self.modified = true;
        }
        self.page = self
            .page
            .saturating_add_signed(delta as isize)
            .min(self.pages.len().saturating_sub(1));
    }
    pub fn erase(&mut self) {
        if !self.unsigned {
            return;
        }
        let s = if self.signing {
            &mut self.title
        } else {
            &mut self.pages[self.page]
        };
        if s.pop().is_some() {
            self.modified = true
        }
    }
    pub fn insert(&mut self, input: &str, r: &Resources, s: &Settings) {
        if !self.unsigned {
            return;
        }
        if self.signing {
            for c in input
                .chars()
                .filter(|&c| c >= ' ' && c != '§' && c != '\u{7f}')
            {
                if self.title.encode_utf16().count() + c.len_utf16() <= 16 {
                    self.title.push(c);
                    self.modified = true
                }
            }
            return;
        }
        let mut proposed = self.pages[self.page].clone();
        proposed.extend(
            input
                .chars()
                .filter(|&c| c == '\n' || c >= ' ' && c != '§' && c != '\u{7f}'),
        );
        if proposed.encode_utf16().count() >= 256 {
            return;
        }
        let shown = format!("{proposed}_");
        let mut lines = 0;
        for paragraph in shown.split('\n') {
            let mut rest = paragraph;
            loop {
                lines += 1;
                let mut width = 0.;
                let mut space = None;
                let mut end = rest.len();
                for (i, c) in rest.char_indices() {
                    width += super::font::character_width(r, s, c);
                    if width > 118. {
                        end = space.map_or(i, |pos| pos + 1);
                        break;
                    }
                    if c == ' ' {
                        space = Some(i)
                    }
                }
                if end == rest.len() || end == 0 {
                    break;
                }
                rest = &rest[end..];
            }
        }
        if lines * 9 <= 128 && proposed != self.pages[self.page] {
            self.pages[self.page] = proposed;
            self.modified = true;
        }
    }
    pub fn finish(&self, held: &Stack, signed: bool) -> Option<Stack> {
        if !self.unsigned
            || !self.modified
            || held.id != 386
            || signed && self.title.trim().is_empty()
        {
            return None;
        }
        let mut s = held.clone();
        let mut tags = s
            .nbt
            .as_deref()
            .and_then(item_nbt::compound)
            .cloned()
            .unwrap_or_default();
        let mut pages = self.pages.clone();
        while pages.len() > 1 && pages.last().is_some_and(String::is_empty) {
            pages.pop();
        }
        if signed {
            s.id = 387;
            tags.insert("title".into(), N::String(self.title.trim().into()));
            tags.insert("author".into(), N::String(self.author.clone()));
            for page in &mut pages {
                *page = serde_json::Value::String(page.clone()).to_string();
            }
        }
        tags.insert(
            "pages".into(),
            N::List {
                element: 8,
                values: pages.into_iter().map(N::String).collect(),
            },
        );
        s.nbt = Some(std::sync::Arc::new(N::Compound(tags)));
        Some(s)
    }
}
pub(super) fn draw(p: &mut Painter<'_>, book: &Book) {
    let [w, h] = p.frame.size;
    let cx = (w / 2.).floor();
    let x = cx - 96.;
    let black = [0., 0., 0., 1.];
    p.rect(0., 0., w, h, [0., 0., 0., 0.6]);
    p.image(
        "textures/gui/book.png",
        Rect {
            x,
            y: 2.,
            w: 192.,
            h: 192.,
        },
        [0., 0., 192., 192.],
        WHITE,
    );
    if book.signing {
        for (text, y) in [
            (p.tr("book.editTitle"), 34.),
            (format!("{}_", book.title), 50.),
            (p.tr("book.byAuthor").replace("%s", &book.author), 60.),
        ] {
            p.text_pass(x + 94. - p.width(&text) / 2., y, &text, black);
        }
        for (i, line) in options::wrapped(p, &p.tr("book.finalizeWarning"), 116.)
            .iter()
            .enumerate()
        {
            p.text_pass(x + 36., 82. + i as f32 * 9., line, black)
        }
        p.button(
            Rect {
                x: cx - 100.,
                y: 196.,
                w: 98.,
                h: 20.,
            },
            &p.tr("book.finalizeButton"),
            Action::BookFinish(true),
            !book.title.trim().is_empty(),
        );
        p.button(
            Rect {
                x: cx + 2.,
                y: 196.,
                w: 98.,
                h: 20.,
            },
            &p.tr("gui.cancel"),
            Action::BookSigning(false),
            true,
        );
        return;
    }
    let total = book.pages.len().max(1);
    let mut label = p.tr("book.pageIndicator");
    for (i, val) in [(1, (book.page + 1).to_string()), (2, total.to_string())] {
        label = label
            .replace(&format!("%{i}$s"), &val)
            .replacen("%s", &val, 1);
    }
    p.text_pass(x + 148. - p.width(&label), 18., &label, black);
    if let Some(raw) = book.pages.get(book.page) {
        let content = if book.unsigned {
            format!("{raw}_")
        } else {
            chat::components::formatted(&chat::components::parse(raw, |key| p.tr(key)))
        };
        for (i, text) in options::wrapped(p, &content, 116.)
            .iter()
            .take(14)
            .enumerate()
        {
            p.text_pass(x + 36., 34. + 9. * i as f32, text, black);
        }
    }
    for (delta, xx, v, enabled) in [
        (-1, x + 38., 205., book.page > 0),
        (
            1,
            x + 120.,
            192.,
            book.page + 1 < total || book.unsigned && total < 50,
        ),
    ] {
        if enabled {
            let rect = Rect {
                x: xx,
                y: 156.,
                w: 23.,
                h: 13.,
            };
            p.image(
                "textures/gui/book.png",
                rect,
                [if rect.contains(p.mouse) { 23. } else { 0. }, v, 23., 13.],
                WHITE,
            );
            p.frame.buttons.push(Button {
                rect,
                action: Action::BookPage(delta),
                enabled: true,
            });
        }
    }
    if book.unsigned {
        p.button(
            Rect {
                x: cx - 100.,
                y: 196.,
                w: 98.,
                h: 20.,
            },
            &p.tr("book.signButton"),
            Action::BookSigning(true),
            true,
        );
        p.button(
            Rect {
                x: cx + 2.,
                y: 196.,
                w: 98.,
                h: 20.,
            },
            &p.tr("gui.done"),
            Action::BookFinish(false),
            true,
        );
    } else {
        p.button(
            Rect {
                x: cx - 100.,
                y: 196.,
                w: 200.,
                h: 20.,
            },
            &p.tr("gui.done"),
            Action::Screen(Screen::Playing),
            true,
        );
    }
}
