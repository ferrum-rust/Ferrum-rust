use super::*;
use std::{collections::VecDeque, time::Instant};

#[path = "chat_components.rs"]
pub mod components;
use components::Run;

pub struct Message {
    runs: Vec<Run>,
    pub system: bool,
    received: Instant,
    completion: bool,
    layout: std::cell::RefCell<Option<LayoutCache>>,
}
#[derive(Default)]
pub struct State {
    pub input: String,
    pub caret: usize,
    pub anchor: usize,
    pub scroll: usize,
    pub url: String,
    entries: VecDeque<Message>,
    sent: Vec<String>,
    history: usize,
    draft: String,
    pub tab_names: Vec<String>,
    pub tab_open: bool,
    actionbar: Option<(String, Instant)>,
    waiting: bool,
    choices: Vec<String>,
    choice: usize,
    scroll_limit: std::cell::Cell<usize>,
    opened: Option<Instant>,
}
impl State {
    pub fn open(&mut self, command: bool) {
        self.input = if command { "/".into() } else { String::new() };
        self.end();
        self.scroll = 0;
        self.history = self.sent.len();
        self.draft.clear();
        self.opened = Some(Instant::now());
        self.cancel_completion();
    }
    fn end(&mut self) {
        self.caret = self.input.len();
        self.anchor = self.caret;
    }
    pub fn push(&mut self, text: String, system: bool) {
        if self.entries.len() == 100 {
            self.entries.pop_front();
        }
        self.entries.push_back(Message {
            runs: vec![Run::literal(text)],
            system,
            received: Instant::now(),
            completion: false,
            layout: Default::default(),
        });
        if self.scroll > 0 {
            self.scroll += 1;
        }
    }
    pub fn submit(&mut self) -> String {
        let message = self.input.trim().to_owned();
        if !message.is_empty() && self.sent.last() != Some(&message) {
            self.sent.push(message.clone());
        }
        self.input.clear();
        self.end();
        self.scroll = 0;
        message
    }
    pub fn recall(&mut self, by: isize) {
        let next = self.history.saturating_add_signed(by).min(self.sent.len());
        if self.history == self.sent.len() {
            self.draft.clone_from(&self.input);
        }
        self.history = next;
        self.input
            .clone_from(self.sent.get(next).unwrap_or(&self.draft));
        self.end();
    }
    pub fn selected(&self) -> &str {
        &self.input[self.caret.min(self.anchor)..self.caret.max(self.anchor)]
    }
    pub fn insert(&mut self, text: &str) {
        let a = self.caret.min(self.anchor);
        let b = self.caret.max(self.anchor);
        let mut remaining = 100usize.saturating_sub(
            self.input[..a].encode_utf16().count() + self.input[b..].encode_utf16().count(),
        );
        let value: String = text
            .chars()
            .filter(|c| *c >= ' ' && *c != '\u{7f}' && *c != '§')
            .take_while(|c| {
                if c.len_utf16() > remaining {
                    return false;
                }
                remaining -= c.len_utf16();
                true
            })
            .collect();
        self.input.replace_range(a..b, &value);
        self.caret = a + value.len();
        self.anchor = self.caret;
    }
    pub fn move_by(&mut self, right: bool, shift: bool, word: bool) {
        let bytes = &self.input;
        if right {
            let tail = &bytes[self.caret..];
            let distance = if word {
                tail.find(' ')
                    .map(|at| at + tail[at..].len() - tail[at..].trim_start_matches(' ').len())
                    .unwrap_or(tail.len())
            } else {
                tail.chars().next().map_or(0, char::len_utf8)
            };
            self.caret += distance;
        } else {
            let head = &bytes[..self.caret];
            self.caret = if word {
                head.trim_end_matches(' ').rfind(' ').map_or(0, |p| p + 1)
            } else {
                head.char_indices().next_back().map_or(0, |p| p.0)
            };
        }
        if !shift {
            self.anchor = self.caret;
        }
    }
    pub fn delete(&mut self, right: bool, word: bool) {
        if self.anchor == self.caret {
            self.move_by(right, true, word);
        }
        self.insert("");
    }
    pub fn wheel(&mut self, direction: isize) {
        self.scroll = self
            .scroll
            .saturating_add_signed(direction)
            .min(self.scroll_limit.get());
    }
    pub fn receive(&mut self, json: &str, position: u8, r: &Resources, language: &str) {
        let runs = components::parse(json, |key| r.tr(language, key));
        if position == 2 {
            self.actionbar = Some((components::formatted(&runs), Instant::now()));
        } else {
            self.push(String::new(), position != 0);
            self.entries.back_mut().unwrap().runs = runs;
        }
    }
    pub fn cancel_completion(&mut self) {
        self.waiting = false;
        self.choices.clear();
        self.choice = 0;
    }
    fn replace_word(&mut self, text: &str) {
        self.anchor = self.input[..self.caret].rfind(' ').map_or(0, |i| i + 1);
        self.insert(text);
    }
    pub fn complete(&mut self) -> Option<String> {
        if self.choices.is_empty() {
            self.waiting = self.caret > 0;
            return self.waiting.then(|| self.input[..self.caret].to_owned());
        }
        if self.choices.len() > 1 {
            self.entries.retain(|msg| !msg.completion);
            self.push(self.choices.join(", "), true);
            self.entries.back_mut().unwrap().completion = true;
        }
        let choice = self.choices[self.choice].clone();
        self.replace_word(&choice);
        self.choice = (self.choice + 1) % self.choices.len();
        None
    }
    pub fn complete_with(&mut self, mut choices: Vec<String>) {
        if !std::mem::take(&mut self.waiting) {
            return;
        }
        let Some(first) = choices.first() else { return };
        let mut prefix = first.as_str();
        for candidate in &choices[1..] {
            let len = prefix
                .chars()
                .zip(candidate.chars())
                .take_while(|(a, b)| a == b)
                .map(|(ch, _)| ch.len_utf8())
                .sum();
            prefix = &prefix[..len];
        }
        let start = self.input[..self.caret].rfind(' ').map_or(0, |i| i + 1);
        if !prefix.is_empty() && !prefix.eq_ignore_ascii_case(&self.input[start..]) {
            self.replace_word(prefix);
        } else {
            choices.retain(|s| !s.is_empty());
            self.choices = choices;
            self.choice = 0;
            if !self.choices.is_empty() {
                self.complete();
            }
        }
    }
}
fn url_at(word: &str) -> Option<String> {
    let trimmed = word.trim_matches(|c: char| matches!(c, '<' | '>' | '(' | ')' | ',' | ';' | '"'));
    ((trimmed.starts_with("https://") || trimmed.starts_with("http://"))
        && !trimmed.contains(char::is_whitespace))
    .then(|| trimmed.to_owned())
}
pub fn plain(text: &str) -> String {
    let mut chars = text.chars();
    let mut out = String::new();
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn completion_cycles_utf8_replaces_previous_candidates_and_ignores_stale_replies() {
        let mut chat = State::default();
        chat.open(true);
        chat.insert("msg 알");
        assert_eq!(chat.complete().as_deref(), Some("/msg 알"));
        chat.complete_with(vec!["알파".into(), "알프".into()]);
        assert_eq!(chat.input, "/msg 알파");
        assert!(chat.complete().is_none());
        assert_eq!(chat.input, "/msg 알프");
        assert_eq!(chat.entries.len(), 1);
        chat.cancel_completion();
        chat.open(true);
        chat.insert("ga");
        assert_eq!(chat.complete().as_deref(), Some("/ga"));
        chat.complete_with(vec!["/gamemode".into(), "/gamerule".into()]);
        assert_eq!(chat.input, "/game");
        assert_eq!(chat.complete().as_deref(), Some("/game"));
        chat.cancel_completion();
        chat.insert("x");
        chat.complete_with(vec!["/gamerule".into()]);
        assert_eq!(chat.input, "/gamex");
        assert_eq!(chat.submit(), "/gamex");
        chat.open(false);
        chat.insert("초안");
        chat.recall(-1);
        assert_eq!(chat.input, "/gamex");
        chat.recall(1);
        assert_eq!(chat.input, "초안");
    }
}
pub(super) fn confirm(p: &mut Painter<'_>, gui: &Gui) {
    let [w, h] = p.frame.size;
    p.rect(0., 0., w, h, [0., 0., 0., 0.6]);
    p.center(w / 2., 70., &p.tr("chat.link.confirm"), WHITE);
    for (i, line) in super::options::wrapped(p, &gui.chat.url, w - 50.)
        .iter()
        .enumerate()
    {
        p.center(w / 2., 90. + i as f32 * 9., line, WHITE);
    }
    p.center(w / 2., 110., &p.tr("chat.link.warning"), [1., 0.8, 0.8, 1.]);
    for (i, (key, choice)) in [
        ("gui.yes", Action::ChatLinkChoice(true)),
        ("chat.copy", Action::CopyChatLink),
        ("gui.no", Action::ChatLinkChoice(false)),
    ]
    .into_iter()
    .enumerate()
    {
        p.button(
            Rect {
                x: (w / 2.).floor() - 155. + i as f32 * 105.,
                y: (h / 6.).floor() + 96.,
                w: 100.,
                h: 20.,
            },
            &p.tr(key),
            choice,
            true,
        );
    }
}
struct Part {
    text: String,
    x: f32,
    width: f32,
    run: usize,
}
#[derive(Default)]
struct Line {
    parts: Vec<Part>,
    width: f32,
}

struct LayoutCache {
    key: (u32, bool, bool, usize),
    lines: Vec<Line>,
}
impl Message {
    fn wrapped(&self, p: &Painter<'_>, width: f32, colors: bool) -> std::cell::Ref<'_, Vec<Line>> {
        let key = (
            width.to_bits(),
            colors,
            p.s.menu.force_unicode || p.s.locale_unicode,
            p.r as *const Resources as usize,
        );
        if self.layout.borrow().as_ref().is_none_or(|c| c.key != key) {
            self.layout.replace(Some(LayoutCache {
                key,
                lines: layout(p, &self.runs, width, colors),
            }));
        }
        std::cell::Ref::map(self.layout.borrow(), |c| &c.as_ref().unwrap().lines)
    }
}
fn layout(p: &Painter<'_>, runs: &[Run], width: f32, colors: bool) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut line = Line::default();
    for (run_index, run) in runs.iter().enumerate() {
        let mut format = if colors {
            run.format.clone()
        } else {
            String::new()
        };
        for (i, paragraph) in run.text.split('\n').enumerate() {
            if i > 0 {
                lines.push(std::mem::take(&mut line));
            }
            for word in paragraph.split_inclusive(' ') {
                let word_width = p.width(&format!(
                    "{format}{}",
                    if colors { word.to_owned() } else { plain(word) }
                ));
                if line.width > 0. && line.width + word_width > width && word_width <= width {
                    lines.push(std::mem::take(&mut line));
                }
                let mut text = format.clone();
                let mut chars = word.chars();
                let mut segment_width = 0.;
                while let Some(c) = chars.next() {
                    if c == '§' {
                        if let Some(code) = chars.next() {
                            if colors {
                                if code.is_ascii_hexdigit() || code.eq_ignore_ascii_case(&'r') {
                                    format.clear();
                                }
                                if !code.eq_ignore_ascii_case(&'r')
                                    && !format.contains(&format!("§{code}"))
                                {
                                    format.push('§');
                                    format.push(code);
                                }
                                text.push('§');
                                text.push(code);
                            }
                            continue;
                        }
                    }
                    let advance = p.glyph_width(c)
                        + f32::from(format.contains("§l") || format.contains("§L"));
                    if line.width + segment_width + advance > width
                        && (line.width + segment_width) > 0.
                    {
                        if segment_width > 0. {
                            line.parts.push(Part {
                                text: std::mem::take(&mut text),
                                x: line.width,
                                width: segment_width,
                                run: run_index,
                            });
                        }
                        lines.push(std::mem::take(&mut line));
                        text = format.clone();
                        segment_width = 0.;
                    }
                    text.push(c);
                    segment_width += advance;
                }
                if segment_width > 0. {
                    line.parts.push(Part {
                        text,
                        x: line.width,
                        width: segment_width,
                        run: run_index,
                    });
                    line.width += segment_width;
                }
            }
        }
    }
    lines.push(line);
    lines
}
pub(super) fn draw(p: &mut Painter<'_>, gui: &Gui) {
    let open = matches!(gui.screen, Screen::Chat | Screen::Sleeping);
    let [w, h] = p.frame.size;
    let settings = &p.s.menu;
    let mut hover = None;
    if settings.chat_visibility < 2 {
        let scale = settings.chat_scale.max(0.01);
        let width = ((settings.chat_width * 280. + 40.).floor() / scale).ceil();
        let height = if open {
            settings.chat_height_focused
        } else {
            settings.chat_height_unfocused
        };
        let count = ((height * 160. + 20.).floor() / 9.) as usize;
        let mut messages = Vec::new();
        let mut line_count = 0;
        for message in gui.chat.entries.iter().rev() {
            if settings.chat_visibility == 1 && !message.system {
                continue;
            }
            let age = message.received.elapsed().as_secs_f32();
            if !open && age >= 10. {
                continue;
            }
            let opacity = (settings.chat_opacity * 0.9 + 0.1)
                * if open {
                    1.
                } else {
                    ((10. - age).clamp(0., 1.)).powi(2)
                };
            let lines = message.wrapped(p, width, settings.chat_colors);
            line_count += lines.len();
            messages.push((message, lines, opacity));
            if line_count >= 100 {
                break;
            }
        }
        let start = p.frame.vertices.len();
        gui.chat
            .scroll_limit
            .set(line_count.min(100).saturating_sub(count));
        let offset = if open {
            gui.chat.scroll.min(gui.chat.scroll_limit.get())
        } else {
            0
        };
        let visible = messages.iter().flat_map(|(message, lines, alpha)| {
            lines.iter().rev().map(move |line| (*message, line, alpha))
        });
        for (idx, (message, line, alpha)) in visible.take(100).skip(offset).take(count).enumerate()
        {
            let y = -(idx as f32) * 9.;
            p.rect(-2., y - 9., width + 6., 9., [0., 0., 0., alpha * 0.5]);
            for part in &line.parts {
                p.text(part.x, y - 8., &part.text, [1., 1., 1., *alpha]);
                if !open {
                    continue;
                }
                let rect = Rect {
                    x: 2. + part.x * scale,
                    y: h - 40. + (y - 9.) * scale,
                    w: part.width * scale,
                    h: 9. * scale,
                };
                if rect.contains(gui.cursor) {
                    hover = message.runs[part.run].events.hover.as_deref();
                }
                let mut events = message.runs[part.run].events.clone();
                if events.click.is_none() && settings.chat_links {
                    if let Some(link) = url_at(plain(&part.text).trim()) {
                        events.click = Some(components::Click::Url(link.into()));
                    }
                }
                if events.click.is_some() || events.insertion.is_some() {
                    p.frame.buttons.push(Button {
                        rect,
                        action: Action::ChatEvent(events),
                        enabled: true,
                    });
                }
            }
        }
        for vertex in &mut p.frame.vertices[start..] {
            vertex.position[0] = (vertex.position[0] + 1.) * scale - 1. + 4. / w;
            vertex.position[1] = (vertex.position[1] - 1.) * scale + 1. - 2. * (h - 40.) / h;
        }
    }
    if let Some((message, received)) = &gui.chat.actionbar {
        let remaining = 60. - received.elapsed().as_secs_f32() * 20.;
        if remaining * 255. / 20. > 8. {
            p.text_pass(
                (w / 2.).floor() - (p.width(message) / 2.).floor(),
                h - 72.,
                message,
                [1., 1., 1., (remaining / 20.).min(1.)],
            );
        }
    }
    if open {
        p.rect(2., h - 14., w - 4., 12., [0., 0., 0., 0.5]);
        let mut start = 0;
        while p.width(&gui.chat.input[start..gui.chat.caret]) > w - 12. {
            start += gui.chat.input[start..]
                .chars()
                .next()
                .map_or(0, char::len_utf8);
        }
        let selected_x = 4. + p.width(&gui.chat.input[start..gui.chat.caret]);
        if gui.chat.anchor != gui.chat.caret {
            let end = 4. + p.width(&gui.chat.input[start..gui.chat.anchor.max(start)]);
            p.rect(
                end.min(selected_x),
                h - 12.,
                (end - selected_x).abs(),
                10.,
                [0.1, 0.2, 0.8, 0.8],
            );
        }
        let text = &gui.chat.input;
        let mut end = start;
        let mut x = 4.;
        let mut left = x;
        for c in text[start..].chars() {
            x += p.glyph_width(c);
            if x > w {
                break;
            }
            p.frame.buttons.push(Button {
                rect: Rect {
                    x: left,
                    y: h - 12.,
                    w: x - left,
                    h: 12.,
                },
                action: Action::ChatCursor(end),
                enabled: true,
            });
            left = x;
            end += c.len_utf8();
        }
        p.frame.buttons.push(Button {
            rect: Rect {
                x: left,
                y: h - 12.,
                w: w - left,
                h: 12.,
            },
            action: Action::ChatCursor(end),
            enabled: true,
        });
        let cursor = gui.chat.caret.clamp(start, end);
        let color = [224. / 255., 224. / 255., 224. / 255., 1.];
        let bar = gui.chat.caret < text.len() || text.encode_utf16().count() >= 100;
        let mut caret = 4.;
        if start != end {
            caret = p.text_end(4., h - 12., &text[start..cursor], color);
        }
        if bar {
            caret -= 1.;
        }
        if cursor < end {
            p.text(caret, h - 12., &text[cursor..end], color);
        }
        let ticks = gui
            .chat
            .opened
            .map_or(0, |t| (t.elapsed().as_secs_f32() * 20.) as u32);
        if (ticks / 6).is_multiple_of(2) {
            if bar {
                p.rect(
                    caret,
                    h - 13.,
                    1.,
                    11.,
                    [208. / 255., 208. / 255., 208. / 255., 1.],
                );
            } else {
                p.text(caret, h - 12., "_", color);
            }
        }
    }
    if let Some(text) = hover {
        let lines = text.split('\n').collect::<Vec<_>>();
        let width = lines.iter().map(|s| p.width(s)).fold(0., f32::max);
        let height = if lines.len() > 1 {
            lines.len() as f32 * 10.
        } else {
            8.
        };
        let x = if gui.cursor[0] + 16. + width > w {
            (gui.cursor[0] - width - 16.).max(4.)
        } else {
            gui.cursor[0] + 12.
        };
        let y = (gui.cursor[1] - 12.).min(h - height - 6.).max(4.);
        p.rect(
            x - 3.,
            y - 4.,
            width + 6.,
            height + 8.,
            [16. / 255., 0., 16. / 255., 240. / 255.],
        );
        for (i, text) in lines.iter().enumerate() {
            p.text(
                x,
                y + (i * 10) as f32 + if i > 0 { 2. } else { 0. },
                text,
                WHITE,
            );
        }
    }
    if gui.chat.tab_open && !gui.option_state.remote && !gui.chat.tab_names.is_empty() && !open {
        let width = gui
            .chat
            .tab_names
            .iter()
            .map(|n| p.width(n))
            .fold(0., f32::max)
            + 18.;
        for (i, name) in gui.chat.tab_names.iter().take(20).enumerate() {
            let y = 10. + i as f32 * 9.;
            p.rect((w - width) * 0.5, y, width, 8., [0., 0., 0., 0.55]);
            p.text((w - width) * 0.5 + 2., y, name, WHITE);
        }
    }
}
