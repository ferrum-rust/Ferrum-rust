use super::{button, presets, rect, Action, Form, Painter, Rect, TextField, WHITE};
use crate::vanilla::random::JavaRandom;
use serde_json::Value;
use std::sync::OnceLock;
#[path = "world_customizer_controls.rs"]
mod controls;
use controls::PAGES;

#[derive(Clone, Copy)]
enum Control {
    Empty,
    Label(&'static str, bool),
    Slider(&'static str, &'static str, f32, f32),
    Toggle(&'static str, &'static str),
    Field(&'static str),
}
#[derive(Clone, Debug)]
pub enum Command {
    Page(usize),
    Toggle(usize),
    Slide(usize, f32),
    Cursor(usize, usize),
    Randomize,
    Defaults,
    Confirm(bool),
}
fn defaults() -> &'static Value {
    static BASE: OnceLock<Value> = OnceLock::new();
    BASE.get_or_init(|| {
        serde_json::from_str(&presets::data().defaults).expect("original factory defaults")
    })
}
pub(super) struct Editor {
    pub page: usize,
    values: Value,
    pub focus: Option<usize>,
    field: TextField,
    dragging: Option<(usize, f32)>,
    confirming: bool,
}
impl Default for Editor {
    fn default() -> Self {
        Self::open("")
    }
}
impl Editor {
    pub fn open(json: &str) -> Self {
        let mut values = defaults().clone();
        if let Ok(Value::Object(input)) = serde_json::from_str(json) {
            for (key, value) in input {
                if let Some(slot) = values.get_mut(&key) {
                    if slot.is_boolean() && value.is_boolean()
                        || slot.is_number() && value.is_number()
                    {
                        *slot = value;
                    }
                }
            }
        }
        Self {
            page: 0,
            values,
            focus: None,
            field: Default::default(),
            dragging: None,
            confirming: false,
        }
    }
    pub fn export(&self) -> String {
        // The share field displays Factory's declaration order, including when edited.
        let fields = &presets::data().defaults;
        let output = fields[1..fields.len() - 1]
            .split(',')
            .map(|pair| {
                let (quoted, value) = pair.split_once(':').unwrap();
                let key = quoted.trim_matches('"');
                if self.values[key] == defaults()[key] {
                    format!("{quoted}:{value}")
                } else {
                    format!("{quoted}:{}", self.values[key])
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("{{{output}}}")
    }
    pub fn rows(&self) -> usize {
        PAGES[self.page].len().div_ceil(2)
    }
    fn value(&self, k: &str) -> f32 {
        self.values[k].as_f64().unwrap_or_default() as f32
    }
    fn set(&mut self, k: &str, v: f32) {
        self.values[k] = if defaults()[k].is_i64() {
            Value::from(v as i32)
        } else {
            Value::from(v)
        };
    }
    fn edit(&mut self, change: impl FnOnce(&mut TextField)) {
        let Some(index) = self.focus else { return };
        let Control::Field(key) = PAGES[self.page][index] else {
            return;
        };
        let mut next = self.field.clone();
        change(&mut next);
        let v = if next.value.is_empty() {
            0.
        } else if let Ok(v) = next.value.parse::<f32>() {
            v
        } else {
            return;
        };
        if !v.is_finite() || v < 0. {
            return;
        }
        let bounds = PAGES[2]
            .iter()
            .find_map(|&control| match control {
                Control::Slider(k, _, lo, hi) if k == key => Some((lo, hi)),
                _ => None,
            })
            .unwrap();
        let clamped = v.clamp(bounds.0, bounds.1);
        self.set(key, clamped);
        self.field = if v != clamped && v != 0. {
            TextField::new(&format!("{clamped:.3}"))
        } else {
            next
        };
    }
    pub fn text(&mut self, s: &str) {
        self.edit(|f| f.insert(s, 32));
    }
    pub fn key(&mut self, key: winit::keyboard::KeyCode, shift: bool, ctrl: bool) {
        use winit::keyboard::KeyCode;
        if self.page != 3 || self.confirming {
            return;
        }
        let Some(current) = self.focus else { return };
        if matches!(key, KeyCode::ArrowDown | KeyCode::ArrowUp) {
            if let Ok(value) = self.field.value.parse::<f32>() {
                let step = match (shift, ctrl) {
                    (true, true) => 0.01,
                    (true, false) => 0.1,
                    (false, true) => 10.,
                    (false, false) => 1.,
                };
                let value = if key == KeyCode::ArrowUp {
                    value + step
                } else {
                    value - step
                };
                self.edit(|f| *f = TextField::new(&format!("{value:.3}")));
            }
            return;
        }
        if key == KeyCode::Tab {
            let next = if shift {
                (current + 30) % 32
            } else {
                (current + 2) % 32
            };
            self.focus_field(next, usize::MAX);
            return;
        }
        self.edit(|f| f.key(key, shift, ctrl, 32));
    }
    fn focus_field(&mut self, i: usize, idx: usize) {
        let Control::Field(k) = PAGES[self.page][i] else {
            return;
        };
        if self.focus != Some(i) {
            self.field = TextField::new(&format!("{:.3}", self.value(k)));
        }
        self.focus = Some(i);
        self.field.set_cursor(idx, false);
    }
    pub fn release(&mut self) {
        self.dragging = None
    }
    pub fn drag(&mut self, x: f32) {
        if let Some((i, left)) = self.dragging {
            if let Control::Slider(k, _, lo, hi) = PAGES[self.page][i] {
                self.set(k, lo + ((x - left - 4.) / 142.).clamp(0., 1.) * (hi - lo));
            }
        }
    }
    pub fn apply(&mut self, cmd: Command, x: f32) -> bool {
        match cmd {
            Command::Page(i) => {
                self.page = i.min(3);
                self.focus = None;
                self.release();
                return true;
            }
            Command::Toggle(i) => {
                if let Control::Toggle(k, _) = PAGES[self.page][i] {
                    self.values[k] = (!self.values[k].as_bool().unwrap_or_default()).into();
                }
            }
            Command::Slide(i, left) => {
                self.dragging = Some((i, left));
                self.drag(x);
            }
            Command::Cursor(i, idx) => self.focus_field(i, idx),
            Command::Randomize => {
                let mut rng = JavaRandom::new(JavaRandom::independent_seed());
                for &control in PAGES[self.page] {
                    match control {
                        Control::Slider(k, _, lo, hi) => {
                            let t = (self.value(k) - lo) / (hi - lo);
                            let t = (t * (0.75 + rng.next_float() * 0.5)
                                + (rng.next_float() * 0.1 - 0.05))
                                .clamp(0., 1.);
                            self.set(k, lo + t * (hi - lo));
                        }
                        Control::Toggle(k, _) => self.values[k] = rng.next_boolean().into(),
                        _ => {}
                    }
                }
            }
            Command::Defaults => self.confirming = true,
            Command::Confirm(yes) => {
                self.confirming = false;
                if yes {
                    self.values = defaults().clone();
                    self.focus = None;
                    self.field = Default::default();
                    self.page = 0;
                    return true;
                }
            }
        }
        false
    }
}
fn act(cmd: Command) -> super::Command {
    super::Command::Custom(cmd)
}
fn slider(p: &mut Painter<'_>, r: Rect, label: String, v: f32, cmd: Command) {
    let path = "textures/gui/widgets.png";
    p.image(path, Rect { w: 75., ..r }, [0., 46., 75., 20.], WHITE);
    p.image(
        path,
        Rect {
            x: r.x + 75.,
            w: 75.,
            ..r
        },
        [125., 46., 75., 20.],
        WHITE,
    );
    let x = r.x + (v.clamp(0., 1.) * 142.).floor();
    p.image(path, rect(x, r.y, 4.), [0., 66., 4., 20.], WHITE);
    p.image(path, rect(x + 4., r.y, 4.), [196., 66., 4., 20.], WHITE);
    let gray = 224. / 255.;
    p.center(
        r.x + 75.,
        r.y + 6.,
        &label,
        if r.contains(p.mouse) {
            [1., 1., 160. / 255., 1.]
        } else {
            [gray, gray, gray, 1.]
        },
    );
    p.frame.buttons.push(super::super::Button {
        rect: r,
        action: Action::WorldForm(act(cmd)),
        enabled: true,
    });
}
pub(super) fn draw(p: &mut Painter<'_>, form: &Form, time: f32) {
    let ed = &form.custom;
    let page = ed.page;
    let [w, h] = p.frame.size.map(f32::ceil);
    let cx = (w / 2.).floor();
    presets::rows(
        p,
        form,
        |p, row, _, y| {
            for col in 0..2 {
                let i = row * 2 + col;
                let Some(control) = PAGES[page].get(i) else {
                    continue;
                };
                let x = cx - 155. + col as f32 * 160.;
                let r = rect(x, y, 150.);
                match *control {
                    Control::Empty => {}
                    Control::Label(key, colon) => {
                        let center =
                            if col == 0 && matches!(PAGES[page].get(i + 1), Some(Control::Empty)) {
                                cx
                            } else {
                                x + 75.
                            };
                        let label = if colon {
                            format!("{}:", p.tr(key))
                        } else {
                            p.tr(key)
                        };
                        p.center(center, y + 5., &label, WHITE);
                    }
                    Control::Toggle(k, key) => {
                        let yn = if ed.values[k] == true {
                            "gui.yes"
                        } else {
                            "gui.no"
                        };
                        button(
                            p,
                            r,
                            &format!("{}: {}", p.tr(key), p.tr(yn)),
                            act(Command::Toggle(i)),
                            true,
                        );
                    }
                    Control::Slider(k, key, lo, hi) => {
                        let position = (ed.value(k) - lo) / (hi - lo);
                        let v = lo + (hi - lo) * position;
                        let label = if k == "fixedBiome" {
                            if v < 0. {
                                p.tr("gui.all")
                            } else {
                                let id = v as i32;
                                crate::vanilla::biome::get_biome(if id >= 8 { id + 2 } else { id })
                                    .map_or("?", |b| b.name)
                                    .into()
                            }
                        } else if page == 2 {
                            format!("{v:.3}")
                        } else {
                            format!("{}", v as i32)
                        };
                        slider(
                            p,
                            r,
                            format!("{}: {}", p.tr(key), label),
                            position,
                            Command::Slide(i, x),
                        );
                    }
                    Control::Field(k) => {
                        let focused = ed.focus == Some(i);
                        let base = TextField::new(&format!("{:.3}", ed.value(k)));
                        let field = if focused { &ed.field } else { &base };
                        field.draw(p, r, focused, time, 32, |n| {
                            Action::WorldForm(act(Command::Cursor(i, n)))
                        });
                    }
                }
            }
        },
        false,
    );
    p.center(cx, 2., &p.tr("options.customizeTitle"), WHITE);
    let indicator = p
        .tr("book.pageIndicator")
        .replacen("%s", &(page + 1).to_string(), 1)
        .replacen("%s", "4", 1)
        .replace("%1$s", &(page + 1).to_string())
        .replace("%2$s", "4");
    p.center(cx, 12., &indicator, WHITE);
    p.center(
        cx,
        22.,
        &p.tr(&format!("createWorld.customize.custom.page{page}")),
        WHITE,
    );
    button(
        p,
        rect(20., 5., 80.),
        &p.tr("createWorld.customize.custom.prev"),
        act(Command::Page(page.saturating_sub(1))),
        page > 0,
    );
    button(
        p,
        rect(w - 100., 5., 80.),
        &p.tr("createWorld.customize.custom.next"),
        act(Command::Page(page + 1)),
        page < 3,
    );
    button(
        p,
        rect(cx - 187., h - 27., 90.),
        &p.tr("createWorld.customize.custom.defaults"),
        act(Command::Defaults),
        ed.values != *defaults(),
    );
    button(
        p,
        rect(cx - 92., h - 27., 90.),
        &p.tr("createWorld.customize.custom.randomize"),
        act(Command::Randomize),
        page < 3,
    );
    button(
        p,
        rect(cx + 3., h - 27., 90.),
        &p.tr("createWorld.customize.custom.presets"),
        super::Command::Presets,
        true,
    );
    button(
        p,
        rect(cx + 98., h - 27., 90.),
        &p.tr("gui.done"),
        super::Command::Done,
        true,
    );
    if ed.confirming {
        p.frame.buttons.clear();
        p.rect(0., 0., w, h, [0., 0., 0., 0.5]);
        let box_rect = Rect {
            x: cx - 90.,
            y: 100.,
            w: 180.,
            h: 85.,
        };
        p.rect(
            cx - 91.,
            99.,
            182.,
            87.,
            [160. / 255., 160. / 255., 160. / 255., 1.],
        );
        super::super::options::dirt(p, box_rect, [-box_rect.x, -100.], 64. / 255.);
        for (dy, key) in [
            (105., "confirmTitle"),
            (125., "confirm1"),
            (135., "confirm2"),
        ] {
            p.center(
                cx,
                dy,
                &p.tr(&format!("createWorld.customize.custom.{key}")),
                WHITE,
            );
        }
        button(
            p,
            rect(cx - 55., 160., 50.),
            &p.tr("gui.yes"),
            act(Command::Confirm(true)),
            true,
        );
        button(
            p,
            rect(cx + 5., 160., 50.),
            &p.tr("gui.no"),
            act(Command::Confirm(false)),
            true,
        );
    }
}
