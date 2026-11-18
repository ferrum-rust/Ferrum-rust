//! Achievement graph, progress and pan/zoom from GuiAchievements and StatFileWriter.
use super::*;
use crate::vanilla::random::JavaRandom;
use std::{collections::HashMap, sync::LazyLock};
#[derive(Deserialize)]
struct Node(String, i32, i32, u16, u16, Option<String>, bool);
static NODES: LazyLock<Vec<Node>> =
    LazyLock::new(|| serde_json::from_str(include_str!("achievements189.json")).unwrap());
const IMAGE: &str = "textures/gui/achievement/achievement_background.png";

pub struct State {
    offset: [f32; 2],
    zoom: f32,
    dragging: Option<[f32; 2]>,
    seed: i32,
}
impl Default for State {
    fn default() -> Self {
        Self::new("Player")
    }
}
fn panel(size: [f32; 2]) -> [f32; 2] {
    [
        (size[0].ceil() - 256.).div_euclid(2.),
        (size[1].ceil() - 202.).div_euclid(2.),
    ]
}
impl State {
    pub fn new(name: &str) -> Self {
        let id = super::offline_uuid(name);
        let mut uuid = String::new();
        for (i, b) in id.iter().enumerate() {
            if matches!(i, 4 | 6 | 8 | 10) {
                uuid.push('-')
            }
            use std::fmt::Write;
            write!(uuid, "{b:02x}").unwrap();
        }
        let seed = uuid
            .bytes()
            .fold(0i32, |a, b| a.wrapping_mul(31).wrapping_add(i32::from(b)));
        Self {
            offset: [-82., -70.],
            zoom: 1.,
            dragging: None,
            seed,
        }
    }
    fn clamp(&mut self) {
        let (left, top, right, bottom) = NODES.iter().fold((0, 0, 0, 0), |(l, t, r, b), n| {
            (l.min(n.1), t.min(n.2), r.max(n.1), b.max(n.2))
        });
        self.offset[0] = self.offset[0].clamp((left * 24 - 112) as f32, (right * 24 - 78) as f32);
        self.offset[1] = self.offset[1].clamp((top * 24 - 112) as f32, (bottom * 24 - 78) as f32);
    }
    fn in_map(p: [f32; 2], size: [f32; 2]) -> bool {
        let [x, y] = panel(size);
        Rect {
            x: x + 8.,
            y: y + 17.,
            w: 224.,
            h: 155.,
        }
        .contains(p)
    }
    pub fn clicked(&mut self, down: bool, point: [f32; 2], size: [f32; 2]) {
        self.dragging = (down && Self::in_map(point, size)).then_some(point);
    }
    pub fn drag(&mut self, point: [f32; 2], size: [f32; 2]) {
        if let Some(last) = self.dragging.filter(|_| Self::in_map(point, size)) {
            for i in 0..2 {
                self.offset[i] -= (point[i] - last[i]) * self.zoom;
            }
            self.dragging = Some(point);
            self.clamp();
        }
    }
    pub fn wheel(&mut self, amount: f32) {
        let next = (self.zoom - amount.signum() * 0.25).clamp(1., 2.);
        self.offset[0] -= (next - self.zoom) * 128.;
        self.offset[1] -= (next - self.zoom) * 101.;
        self.zoom = next;
        self.clamp();
    }
}
fn unlocked(n: &Node, stats: &HashMap<String, i32>) -> bool {
    stats.get(&n.0).copied().unwrap_or(0) > 0
}
fn parent(n: &Node) -> Option<&'static Node> {
    let key = n.5.as_deref()?;
    NODES.iter().find(|o| o.0 == key)
}
fn depth(n: &Node, stats: &HashMap<String, i32>) -> usize {
    if unlocked(n, stats) {
        return 0;
    }
    std::iter::successors(parent(n), |v| parent(v))
        .take_while(|v| !unlocked(v, stats))
        .count()
}
fn color(v: i32) -> [f32; 4] {
    [
        ((v >> 16) & 255) as f32 / 255.,
        ((v >> 8) & 255) as f32 / 255.,
        (v & 255) as f32 / 255.,
        1.,
    ]
}
fn rec(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect { x, y, w, h }
}
fn engrave(p: &mut Painter<'_>, x: f32, y: f32, u: f32, v: f32, w: f32, h: f32, shade: f32) {
    p.image(
        IMAGE,
        rec(x, y, w, h),
        [u, v, w, h],
        [shade, shade, shade, 1.],
    );
}
fn backdrop(p: &mut Painter<'_>, state: &State, offset: [i32; 2]) {
    let [gx, gy] = offset.map(|v| (v + 288) >> 4);
    let [dx, dy] = offset.map(|v| (v + 288) % 16);
    let side = 16. / state.zoom;
    let mut row = 0;
    while row as f32 * side - (dy as f32) < 155. {
        let depth = gy + row;
        let shade = 0.6 - depth as f32 / 25. * 0.3;
        let mut col = 0;
        while col as f32 * side - (dx as f32) < 224. {
            let seed = state.seed.wrapping_add(gx + col).wrapping_add(depth * 16);
            let mut rng = JavaRandom::new(i64::from(seed));
            let ore = rng.next_int((1 + depth).max(1)) + depth / 2;
            let block = if ore > 37 || depth == 35 {
                7
            } else {
                match ore {
                    22 => {
                        if rng.next_int(2) == 0 {
                            56
                        } else {
                            73
                        }
                    }
                    10 => 15,
                    8 => 16,
                    5.. => 1,
                    1.. => 3,
                    _ => 12,
                }
            };
            p.quad(
                rec((col * 16 - dx) as f32, (row * 16 - dy) as f32, 16., 16.),
                [0., 0., 1., 1.],
                p.a.texture(block << 4, 1, true) | 65536,
                [shade, shade, shade, 1.],
            );
            col += 1;
        }
        row += 1;
    }
}
// Clip the graph's textured triangles before overlaying the achievement frame.
fn project(p: &mut Painter<'_>, start: usize, at: [f32; 2], zoom: f32) {
    let batch = p.frame.vertices.split_off(start);
    let [sw, sh] = p.frame.size;
    for tri in batch.chunks_exact(3) {
        let mut verts: Vec<UiVertex> = tri
            .iter()
            .map(|v| {
                let mut v = *v;
                v.position = [
                    (v.position[0] + 1.) * sw / 2.,
                    (1. - v.position[1]) * sh / 2.,
                ];
                v
            })
            .collect();
        for (axis, edge, high) in [
            (0, 0., false),
            (1, 0., false),
            (0, 224. * zoom, true),
            (1, 155. * zoom, true),
        ] {
            if verts.is_empty() {
                break;
            }
            let mut clipped = vec![];
            let mut prev = *verts.last().unwrap();
            for current in verts {
                let inside = |v: UiVertex| {
                    if high {
                        v.position[axis] <= edge
                    } else {
                        v.position[axis] >= edge
                    }
                };
                if inside(current) != inside(prev) {
                    let factor = (edge - prev.position[axis])
                        / (current.position[axis] - prev.position[axis]);
                    let mut cut = prev;
                    for i in 0..2 {
                        cut.position[i] += factor * (current.position[i] - cut.position[i]);
                        cut.uv[i] += factor * (current.uv[i] - cut.uv[i]);
                    }
                    clipped.push(cut);
                }
                if inside(current) {
                    clipped.push(current);
                }
                prev = current;
            }
            verts = clipped;
        }
        for v in &mut verts {
            v.position = [
                (at[0] + v.position[0] / zoom) * 2. / sw - 1.,
                1. - (at[1] + v.position[1] / zoom) * 2. / sh,
            ];
        }
        for i in 2..verts.len() {
            p.frame
                .vertices
                .extend_from_slice(&[verts[0], verts[i - 1], verts[i]]);
        }
    }
}
fn wrapped(p: &Painter<'_>, mut text: &str, width: f32) -> Vec<String> {
    let mut out = vec![];
    while !text.is_empty() {
        let mut last = 0;
        let mut space = None;
        for (i, c) in text.char_indices() {
            if c == '\n' {
                break;
            }
            let end = i + c.len_utf8();
            if p.width(&text[..end]) > width && last > 0 {
                if let Some(s) = space {
                    last = s
                }
                break;
            }
            last = end;
            if c == ' ' {
                space = Some(i);
            }
        }
        if last == 0 && !text.starts_with('\n') {
            last = text.chars().next().unwrap().len_utf8();
        }
        out.push(text[..last].into());
        text = &text[last..];
        if text.starts_with([' ', '\n']) {
            text = &text[1..];
        }
    }
    out
}
fn tooltip(p: &mut Painter<'_>, node: &Node, dist: usize, owned: bool, allowed: bool) {
    if !allowed && dist > 3 {
        return;
    }
    let title = p.tr(if !allowed && dist == 3 {
        "achievement.unknown"
    } else {
        &node.0
    });
    let description = if allowed {
        let d = p.tr(&format!("{}.desc", node.0));
        let key = crate::keybindings::key_label(
            p.s.bindings.code(crate::keybindings::Binding::Inventory),
            &p.tr("key.mouseButton"),
        );
        d.replace("%1$s", &key).replace("%s", &key)
    } else {
        let parent = p.tr(&parent(node).unwrap().0);
        p.tr("achievement.requires")
            .replace("%1$s", &parent)
            .replace("%s", &parent)
    };
    let x = p.mouse[0] + 12.;
    let y = p.mouse[1] - 4.;
    let width = p.width(&title).max(120.);
    let lines = wrapped(p, &description, width);
    let height = lines.len() as f32 * 9. + if owned { 12. } else { 0. };
    p.rect(
        x - 3.,
        y - 3.,
        width + 6.,
        height + 18.,
        [0., 0., 0., 192. / 255.],
    );
    for (i, line) in lines.iter().enumerate() {
        p.text_pass(
            x,
            y + 12. + 9. * i as f32,
            line,
            color(if allowed { 0xa0a0a0 } else { 0x705050 }),
        );
    }
    if owned {
        p.text(
            x,
            y + height + 4.,
            &p.tr("achievement.taken"),
            color(0x9090ff),
        );
    }
    p.text(
        x,
        y,
        &title,
        color(match (allowed, node.6) {
            (true, true) => 0xffff80,
            (false, true) => 0x808040,
            (false, false) => 0x808080,
            _ => 0xffffff,
        }),
    );
}
pub(super) fn draw(p: &mut Painter<'_>, gui: &Gui) {
    let size = p.frame.size.map(f32::ceil);
    let [x, y] = panel(size);
    let data = &gui.statistics;
    let s = &gui.achievements;
    pause::background(p);
    if data.waiting {
        p.center(
            (size[0] / 2.).floor(),
            (size[1] / 2.).floor(),
            &p.tr("multiplayer.downloadingStats"),
            WHITE,
        );
        let t = (p.time / 0.15) as usize % 7;
        p.center(
            (size[0] / 2.).floor(),
            (size[1] / 2.).floor() + 18.,
            [
                "O o o", "o O o", "o o O", "o o o", "o o O", "o O o", "O o o",
            ][t],
            WHITE,
        );
        return;
    }
    let origin = [x + 16., y + 17.];
    let point = [
        (p.mouse[0] - origin[0]) * s.zoom,
        (p.mouse[1] - origin[1]) * s.zoom,
    ];
    let offset = s.offset.map(|v| v.floor() as i32);
    let xy = |n: &Node| [(n.1 * 24 - offset[0]) as f32, (n.2 * 24 - offset[1]) as f32];
    let start = p.frame.vertices.len();
    backdrop(p, s, offset);
    for n in NODES.iter() {
        let Some(parent) = parent(n) else { continue };
        if depth(n, &data.values) > 4 {
            continue;
        }
        let [x, y] = xy(n).map(|v| v + 11.);
        let [x1, y1] = xy(parent).map(|v| v + 11.);
        let ink = color(if unlocked(n, &data.values) {
            0xa0a0a0
        } else if unlocked(parent, &data.values) {
            0x00ff00
        } else {
            0
        });
        p.rect(x.min(x1), y, (x - x1).abs() + 1., 1., ink);
        p.rect(x1, y.min(y1) + 1., 1., (y1 - y).abs().max(1.) - 1., ink);
        let (dx, dy, u, v, w, h) = if x > x1 {
            (-18., -5., 114., 234., 7., 11.)
        } else if x < x1 {
            (11., -5., 107., 234., 7., 11.)
        } else if y > y1 {
            (-5., -18., 96., 234., 11., 7.)
        } else {
            (-5., 11., 96., 241., 11., 7.)
        };
        // drawVerticalLine leaves its RGB modulation set for the arrow sprite.
        p.image(IMAGE, rec(x + dx, y + dy, w, h), [u, v, w, h], ink);
    }
    let mut hover = None;
    for n in NODES.iter() {
        let [x, y] = xy(n);
        let dist = depth(n, &data.values);
        if x < -24. || y < -24. || x > 224. * s.zoom || y > 155. * s.zoom || dist > 4 {
            continue;
        }
        let owned = unlocked(n, &data.values);
        let allowed = parent(n).is_none_or(|parent| unlocked(parent, &data.values));
        let shade = if owned {
            0.75
        } else if allowed {
            1.
        } else {
            match dist {
                3 => 0.2,
                4 => 0.1,
                _ => 0.3,
            }
        };
        engrave(
            p,
            x - 2.,
            y - 2.,
            if n.6 { 26. } else { 0. },
            202.,
            26.,
            26.,
            shade,
        );
        let mut stack = Stack::new(n.3, 1);
        stack.meta = n.4;
        // RenderItem resets the GL color before painting the item. Locked
        // achievements dim the frame, while their item texture keeps its color.
        p.item(stack, x + 3., y + 3., false);
        if point[0] >= x && point[0] <= x + 22. && point[1] >= y && point[1] <= y + 22. {
            hover = Some((n, dist, owned, allowed));
        }
    }
    project(p, start, origin, s.zoom);
    engrave(p, x, y, 0., 0., 256., 202., 1.);
    p.button(
        rec(
            (size[0] / 2.).floor() + 24.,
            (size[1] / 2.).floor() + 74.,
            80.,
            20.,
        ),
        &p.tr("gui.done"),
        Action::Screen(Screen::Pause),
        true,
    );
    if let Some((n, d, o, a)) = hover {
        tooltip(p, n, d, o, a);
    }
    p.text_pass(x + 15., y + 5., &p.tr("gui.achievements"), color(0x404040));
}
