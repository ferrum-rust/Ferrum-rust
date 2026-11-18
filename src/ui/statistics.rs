use super::{options, Action, Button, Gui, Painter, Rect, Screen, WHITE};
use crate::inventory::Stack;
use std::{collections::HashMap, sync::OnceLock, time::Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tab {
    #[default]
    General,
    Blocks,
    Items,
    Mobs,
}
#[derive(Clone, Copy, Debug)]
pub enum Command {
    Open,
    Tab(Tab),
    Sort(usize),
}
#[derive(Default)]
pub struct State {
    pub values: HashMap<String, i32>,
    pub waiting: bool,
    pub revision: u64,
    pub tab: Tab,
    sort: Option<(usize, bool)>,
    pub scroll: i32,
    opened: Option<Instant>,
}
struct Row {
    tab: Tab,
    name: &'static str,
    keys: Vec<&'static str>,
    id: u16,
}
fn catalog() -> &'static [Row] {
    static ROWS: OnceLock<Vec<Row>> = OnceLock::new();
    ROWS.get_or_init(|| {
        include_str!("statistics.tsv")
            .lines()
            .map(|line| {
                let mut s = line.split('\t');
                let tab = match s.next().unwrap() {
                    "g" => Tab::General,
                    "b" => Tab::Blocks,
                    "i" => Tab::Items,
                    _ => Tab::Mobs,
                };
                let name = s.next().unwrap();
                Row {
                    tab,
                    name,
                    keys: s.collect(),
                    id: name.parse().unwrap_or(0),
                }
            })
            .collect()
    })
}
impl State {
    pub fn open(&mut self, values: &HashMap<String, i32>, remote_revision: Option<u64>) {
        self.values.clone_from(values);
        self.waiting = remote_revision.is_some();
        self.revision = remote_revision.unwrap_or(0);
        self.tab = Tab::General;
        self.sort = None;
        self.scroll = 0;
        self.opened = Some(Instant::now());
    }
    pub fn update(&mut self, revision: u64, values: &HashMap<String, i32>) {
        if self.revision != revision {
            self.values.clone_from(values);
            self.revision = revision;
            self.waiting = false;
        }
    }
    pub fn apply(&mut self, cmd: Command) {
        match cmd {
            Command::Tab(tab) => {
                self.tab = tab;
                self.sort = None;
                self.scroll = 0;
            }
            Command::Sort(i) => {
                self.sort = match self.sort {
                    Some((column, false)) if column == i => None,
                    Some((column, true)) if column == i => Some((i, false)),
                    _ => Some((i, true)),
                };
            }
            Command::Open => {}
        }
    }
    fn rows(&self, tab: Tab) -> Vec<&'static Row> {
        let mut rows: Vec<_> = catalog()
            .iter()
            .filter(|r| {
                r.tab == tab
                    && (tab == Tab::General
                        || r.keys
                            .iter()
                            .any(|k| self.values.get(*k).copied().unwrap_or(0) > 0))
            })
            .collect();
        if matches!(tab, Tab::Blocks | Tab::Items) {
            if let Some((col, desc)) = self.sort {
                rows.sort_by(|a, b| {
                    let value = |r: &Row| {
                        r.keys
                            .get(col)
                            .filter(|k| **k != "-")
                            .map(|k| self.values.get(*k).copied().unwrap_or(0))
                    };
                    let (x, y) = (value(a), value(b));
                    match (x, y) {
                        (Some(x), Some(y)) => {
                            if desc {
                                y.cmp(&x)
                            } else {
                                x.cmp(&y)
                            }
                        }
                        _ => y.is_some().cmp(&x.is_some()),
                    }
                    .then_with(|| a.id.cmp(&b.id))
                });
            }
        }
        rows
    }
    fn height(&self) -> i32 {
        match self.tab {
            Tab::General => 10,
            Tab::Mobs => 36,
            _ => 20,
        }
    }
    fn header(&self) -> i32 {
        if matches!(self.tab, Tab::Items | Tab::Blocks) {
            20
        } else {
            0
        }
    }
    pub fn wheel(&mut self, delta: f32, height: f32) {
        let content = self.rows(self.tab).len() as i32 * self.height() + self.header();
        let max = (content - (height as i32 - 100)).max(0);
        self.scroll = (self.scroll - delta.signum() as i32 * self.height() / 2).clamp(0, max);
    }
}
fn number(v: i32) -> String {
    let digits = v.unsigned_abs().to_string();
    let mut out = if v < 0 { "-".into() } else { String::new() };
    for (i, c) in digits.bytes().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c as char);
    }
    out
}
fn value(v: i32, kind: &str) -> String {
    let n = f64::from(v);
    match kind {
        "time" => {
            let sec = n / 20.;
            for (scale, unit) in [(31536000., "y"), (86400., "d"), (3600., "h"), (60., "m")] {
                if sec / scale > 0.5 {
                    return format!("{:.2} {unit}", sec / scale);
                }
            }
            if sec.fract() == 0. {
                format!("{sec:.1} s")
            } else {
                format!("{sec} s")
            }
        }
        "distance" if n > 50000. => format!("{:.2} km", n / 100000.),
        "distance" if n > 50. => format!("{:.2} m", n / 100.),
        "distance" => format!("{v} cm"),
        "decimal" => format!("{:.2}", n * 0.1),
        _ => number(v),
    }
}
fn translated(p: &Painter<'_>, key: &str, args: &[serde_json::Value]) -> String {
    let json = serde_json::json!({"translate":key,"with":args}).to_string();
    super::chat::components::parse(&json, |key| p.tr(key))
        .into_iter()
        .map(|r| r.text)
        .collect()
}
pub(super) fn draw(p: &mut Painter<'_>, gui: &Gui) {
    let [w, h] = p.frame.size;
    let cx = (w / 2.).floor();
    let bottom = h - 64.;
    let s = &gui.statistics;
    if s.waiting {
        p.rect(0., 0., w, h, [0., 0., 0., 0.65]);
        p.center(
            cx,
            (h / 2.).floor(),
            &p.tr("multiplayer.downloadingStats"),
            WHITE,
        );
        let phase = s
            .opened
            .map_or(0, |t| (t.elapsed().as_millis() / 150 % 7) as usize);
        p.center(
            cx,
            (h / 2.).floor() + 18.,
            [
                "O o o", "o O o", "o o O", "o o o", "o o O", "o O o", "O o o",
            ][phase],
            WHITE,
        );
        return;
    }
    let rows = s.rows(s.tab);
    let step = s.height() as f32;
    let scroll = s.scroll as f32;
    let x = cx - 108.;
    options::dirt(
        p,
        Rect {
            x: 0.,
            y: 32.,
            w,
            h: bottom - 32.,
        },
        [0., scroll],
        32. / 255.,
    );
    let stat_icon = |p: &mut Painter<'_>, x, y, u, v| {
        p.image(
            "textures/gui/container/stats_icons.png",
            Rect {
                x,
                y,
                w: 18.,
                h: 18.,
            },
            [u, v, 18., 18.],
            WHITE,
        )
    };
    let header_y = 36. - scroll;
    if s.header() > 0 && header_y + 18. >= 32. {
        for i in 0..3 {
            let right = x + 115. + 50. * i as f32;
            stat_icon(p, right - 18., header_y + 1., 0., 18.);
            let icons = if s.tab == Tab::Blocks {
                [18., 36., 54.]
            } else {
                [72., 18., 36.]
            };
            stat_icon(p, right - 18., header_y + 1., icons[i], 18.);
            p.frame.buttons.push(Button {
                rect: Rect {
                    x: right - 36.,
                    y: header_y.max(32.),
                    w: 36.,
                    h: 18.,
                },
                action: Action::Statistics(Command::Sort(i)),
                enabled: true,
            });
            if let Some((col, desc)) = s.sort {
                if col == i {
                    stat_icon(
                        p,
                        right - 36.,
                        header_y + 1.,
                        if desc { 18. } else { 36. },
                        0.,
                    );
                }
            }
        }
    }
    for (i, r) in rows.iter().enumerate() {
        let y = 36. - scroll + s.header() as f32 + step * i as f32;
        if y + step < 32. || y > bottom {
            continue;
        }
        let col = if i % 2 == 0 {
            WHITE
        } else {
            [144. / 255., 144. / 255., 144. / 255., 1.]
        };
        let count = |key: &str| s.values.get(key).copied().unwrap_or(0);
        match r.tab {
            Tab::General => {
                p.text(x + 2., y + 1., &p.tr(r.name), col);
                let v = value(count(r.name), r.keys[0]);
                p.text(x + 215. - p.width(&v), y + 1., &v, col);
            }
            Tab::Blocks | Tab::Items => {
                stat_icon(p, x + 41., y + 1., 0., 0.);
                p.item(Stack::new(r.id, 1), x + 42., y + 2., false);
                for (n, key) in r.keys.iter().enumerate() {
                    let t = if *key == "-" {
                        "-".into()
                    } else {
                        number(count(key))
                    };
                    p.text(x + 115. + 50. * n as f32 - p.width(&t), y + 5., &t, col);
                }
            }
            Tab::Mobs => {
                let mob = p.tr(&format!("entity.{}.name", r.name));
                p.text(x - 8., y + 1., &mob, WHITE);
                for (n, key) in r.keys.iter().enumerate() {
                    let v = count(key);
                    let label = if n == 0 {
                        "stat.entityKills"
                    } else {
                        "stat.entityKilledBy"
                    };
                    let phrase = if v == 0 {
                        translated(p, &format!("{label}.none"), &[serde_json::json!(mob)])
                    } else if n == 0 {
                        translated(p, label, &[serde_json::json!(v), serde_json::json!(mob)])
                    } else {
                        translated(p, label, &[serde_json::json!(mob), serde_json::json!(v)])
                    };
                    let gray = if v == 0 { 96. } else { 144. } / 255.;
                    p.text(
                        x + 2.,
                        y + 10. + n as f32 * 9.,
                        &phrase,
                        [gray, gray, gray, 1.],
                    );
                }
            }
        }
    }
    options::mask_outside(p, 0., w, 32., bottom);
    options::scrollbar(
        p,
        cx + 124.,
        32.,
        bottom,
        rows.len() as f32 * step + s.header() as f32,
        scroll,
    );
    p.center(cx, 20., &p.tr("gui.stats"), WHITE);
    for (n, (tab, key)) in [
        (Tab::General, "stat.generalButton"),
        (Tab::Blocks, "stat.blocksButton"),
        (Tab::Items, "stat.itemsButton"),
        (Tab::Mobs, "stat.mobsButton"),
    ]
    .into_iter()
    .enumerate()
    {
        p.button(
            Rect {
                x: cx - 160. + n as f32 * 80.,
                y: h - 52.,
                w: 80.,
                h: 20.,
            },
            &p.tr(key),
            Action::Statistics(Command::Tab(tab)),
            !s.rows(tab).is_empty(),
        );
    }
    p.button(
        Rect {
            x: cx + 4.,
            y: h - 28.,
            w: 150.,
            h: 20.,
        },
        &p.tr("gui.done"),
        Action::Screen(Screen::Pause),
        true,
    );
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn empty_s37_finishes_request_and_registered_rows_use_jar_units() {
        let mut s=State::default();let mut values=HashMap::new();
        s.open(&values,Some(3));s.update(3,&values);assert!(s.waiting);
        s.update(4,&values);assert!(!s.waiting);assert!(s.rows(Tab::Items).is_empty());
        values.insert("stat.useItem.minecraft.diamond_pickaxe".into(),11);
        values.insert("stat.mineBlock.minecraft.stone".into(),20);
        values.insert("stat.killEntity.Cow".into(),3);
        s.update(5,&values);
        assert!(s.rows(Tab::Items).iter().any(|r|r.id==278));
        assert!(s.rows(Tab::Blocks).iter().any(|r|r.id==1));
        assert!(s.rows(Tab::Mobs).iter().any(|r|r.name=="Cow"));
        assert_eq!(value(600,"time"),"30.0 s");assert_eq!(value(1200,"time"),"1.00 m");
        assert_eq!(value(50000,"distance"),"500.00 m");assert_eq!(value(100000,"distance"),"1.00 km");
        assert_eq!(value(-1234567,"number"),"-1,234,567");
    }
}
