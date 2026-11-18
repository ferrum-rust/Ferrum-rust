use super::{Action, Button, Painter, Rect};
use std::{cell::Cell, ops::Range};
use winit::keyboard::KeyCode;

#[derive(Clone, Default, Debug)]
pub(crate) struct TextField {
    pub value: String,
    cursor: usize,
    anchor: usize,
    scroll: Cell<usize>,
    selection_changed: Cell<bool>,
}

impl TextField {
    pub fn new(value: &str) -> Self {
        Self {
            value: value.into(),
            cursor: value.len(),
            anchor: value.len(),
            scroll: Cell::new(0),
            selection_changed: Cell::new(true),
        }
    }
    /// Loading preexisting text applies the UTF-16 cap, but does not filter it
    /// like typed/pasted characters (GuiTextField.setText vs. writeText).
    pub fn with_limit(value: &str, limit: usize) -> Self {
        let mut remaining = limit;
        let prefix = value
            .char_indices()
            .find_map(|(i, c)| {
                if c.len_utf16() > remaining {
                    Some(i)
                } else {
                    remaining -= c.len_utf16();
                    None
                }
            })
            .unwrap_or(value.len());
        Self::new(&value[..prefix])
    }
    fn range(&self) -> Range<usize> {
        self.cursor.min(self.anchor)..self.cursor.max(self.anchor)
    }
    pub fn set_cursor(&mut self, index: usize, shift: bool) {
        self.cursor = self.value.floor_char_boundary(index.min(self.value.len()));
        self.selection_changed.set(true);
        if !shift {
            self.anchor = self.cursor;
        }
    }
    pub fn insert(&mut self, text: &str, limit: usize) {
        let range = self.range();
        let remaining = self.value[..range.start].encode_utf16().count()
            + self.value[range.end..].encode_utf16().count();
        let mut space = limit.saturating_sub(remaining);
        let mut typed = String::new();
        for c in text
            .chars()
            .filter(|&c| c != '§' && c >= ' ' && c != '\u{7f}')
        {
            if c.len_utf16() > space {
                break;
            }
            typed.push(c);
            space -= c.len_utf16();
        }
        let position = range.start + typed.len();
        self.value.replace_range(range, &typed);
        self.set_cursor(position, false);
    }
    fn offset(&self, right: bool, word: bool) -> usize {
        if right {
            let rest = &self.value[self.cursor..];
            if word {
                self.cursor
                    + rest.find(' ').map_or(rest.len(), |i| {
                        i + rest[i..].bytes().take_while(|b| *b == b' ').count()
                    })
            } else {
                self.cursor + rest.chars().next().map_or(0, char::len_utf8)
            }
        } else {
            let prefix = &self.value[..self.cursor];
            if word {
                prefix.trim_end_matches(' ').rfind(' ').map_or(0, |i| i + 1)
            } else {
                self.cursor - prefix.chars().next_back().map_or(0, char::len_utf8)
            }
        }
    }
    pub fn key(&mut self, key: KeyCode, shift: bool, control: bool, limit: usize) {
        match key {
            KeyCode::KeyA if control => {
                self.anchor = 0;
                self.cursor = self.value.len();
            }
            KeyCode::KeyC | KeyCode::KeyX if control => {
                crate::clipboard::set(&self.value[self.range()]);
                if key == KeyCode::KeyX {
                    self.insert("", limit);
                }
            }
            KeyCode::KeyV if control => self.insert(&crate::clipboard::get(), limit),
            KeyCode::ArrowLeft | KeyCode::ArrowRight => {
                self.set_cursor(self.offset(key == KeyCode::ArrowRight, control), shift)
            }
            KeyCode::Home => self.set_cursor(0, shift),
            KeyCode::End => self.set_cursor(self.value.len(), shift),
            KeyCode::Delete | KeyCode::Backspace => {
                if self.cursor == self.anchor {
                    self.cursor = self.offset(key == KeyCode::Delete, control);
                }
                self.insert("", limit);
            }
            _ => {}
        }
    }
    pub(super) fn draw(
        &self,
        p: &mut Painter<'_>,
        rect: Rect,
        focused: bool,
        time: f32,
        limit: usize,
        at: impl Fn(usize) -> Action,
    ) {
        self.paint(p, rect, focused, time, limit, true, at);
    }
    pub(super) fn draw_plain(
        &self,
        p: &mut Painter<'_>,
        rect: Rect,
        limit: usize,
        at: impl Fn(usize) -> Action,
    ) {
        // GuiContainerCreative never increments searchField.cursorCounter.
        self.paint(p, rect, true, 0., limit, false, at);
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn paint(
        &self,
        p: &mut Painter<'_>,
        rect: Rect,
        focused: bool,
        time: f32,
        limit: usize,
        background: bool,
        at: impl Fn(usize) -> Action,
    ) {
        let y = rect.y;
        let baseline = if background {
            y + ((rect.h - 8.) / 2.).floor()
        } else {
            y
        };
        let hit = |p: &mut Painter<'_>, rect: Rect, action| {
            p.frame.buttons.push(Button {
                rect,
                enabled: true,
                action,
            });
        };
        if background {
            p.rect(
                rect.x - 1.,
                y - 1.,
                rect.w + 2.,
                rect.h + 2.,
                [160. / 255., 160. / 255., 160. / 255., 1.],
            );
            p.rect(rect.x, y, rect.w, rect.h, [0., 0., 0., 1.]);
        }
        let field = self;
        let available = if background { rect.w - 8. } else { rect.w };
        let fit = |text: &str| {
            let mut width = 0.;
            text.chars()
                .take_while(|&ch| {
                    width += p.glyph_width(ch);
                    width <= available
                })
                .map(char::len_utf8)
                .sum::<usize>()
        };
        let mut start = field
            .value
            .floor_char_boundary(field.scroll.get().min(field.value.len()));
        if field.selection_changed.replace(false) {
            let end = start + fit(&field.value[start..]);
            if field.cursor < start {
                start = field.cursor;
            } else if field.cursor > end {
                // GuiTextField advances by the excess character count, measured
                // from the previous viewport (important for unequal glyph widths).
                let excess = field.value[end..field.cursor].chars().count();
                start = field.value[start..]
                    .char_indices()
                    .nth(excess)
                    .map_or(field.value.len(), |(n, _)| start + n);
            }
            field.scroll.set(start);
        }
        let end = start + fit(&field.value[start..]);
        let x = if background { rect.x + 4. } else { rect.x };
        let text_color = if background {
            [224. / 255., 224. / 255., 224. / 255., 1.]
        } else {
            [1.; 4]
        };
        let prefix_end = field.cursor.clamp(start, end);
        // Unicode draw advances can differ from getStringWidth. GuiTextField
        // positions the suffix and caret using drawStringWithShadow's return.
        let mut caret_x = x;
        if start != end {
            caret_x = p.text_end(x, baseline, &field.value[start..prefix_end], text_color);
        }
        let bar = field.cursor < field.value.len() || field.value.encode_utf16().count() >= limit;
        if bar {
            caret_x -= 1.;
        }
        if prefix_end < end {
            p.text(caret_x, baseline, &field.value[prefix_end..end], text_color);
        }
        if focused {
            let selection = field.range();
            if !selection.is_empty() {
                let from = selection.start.clamp(start, end);
                let to = selection.end.clamp(start, end);
                p.rect(
                    x + p.width(&field.value[start..from]),
                    baseline - 1.,
                    p.width(&field.value[from..to]),
                    10.,
                    [0., 0.25, 1., 0.55],
                );
            }
            if ((time * 20.) as u32 / 6).is_multiple_of(2) {
                if bar {
                    p.rect(
                        caret_x,
                        baseline - 1.,
                        1.,
                        11.,
                        [208. / 255., 208. / 255., 208. / 255., 1.],
                    );
                } else {
                    p.text(caret_x, baseline, "_", text_color);
                }
            }
        }
        let mut click = start;
        let mut dx = x;
        let mut left = rect.x;
        for ch in field.value[start..end].chars() {
            dx += p.glyph_width(ch);
            hit(
                p,
                Rect {
                    x: left,
                    y,
                    w: dx - left,
                    h: rect.h,
                },
                at(click),
            );
            click += ch.len_utf8();
            left = dx;
        }
        hit(
            p,
            Rect {
                x: left,
                y,
                w: rect.x + rect.w - left,
                h: rect.h,
            },
            at(click),
        );
    }
}
