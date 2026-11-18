use super::*;
use serde::Deserialize;
use std::sync::OnceLock;
#[derive(Deserialize)]
pub(super) struct Preset {
    pub name: String,
    #[serde(default)]
    item: u16,
    #[serde(default)]
    damage: u16,
    #[serde(default)]
    texture: String,
    pub settings: String,
}
#[derive(Deserialize)]
pub(super) struct Registry {
    pub flat: Vec<Preset>,
    pub customized: Vec<Preset>,
    pub defaults: String,
}
pub(super) fn data() -> &'static Registry {
    static DATA: OnceLock<Registry> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!("world_presets.json"))
            .expect("bundled 1.8.9 world presets")
    })
}
fn layers(s: &str) -> impl DoubleEndedIterator<Item = &str> {
    s.split(';')
        .nth(1)
        .unwrap_or("")
        .split(',')
        .filter(|s| !s.is_empty())
}
pub(super) fn layer_count(s: &str) -> usize {
    layers(s).count()
}
pub(super) fn remove_layer(s: &mut String, from_top: usize) {
    let parts: Vec<_> = s.splitn(4, ';').collect();
    let mut layers = layers(s).collect::<Vec<_>>();
    if let Some(i) = layers.len().checked_sub(from_top + 1) {
        layers.remove(i);
        *s = format!(
            "3;{};{};{}",
            layers.join(","),
            parts.get(2).unwrap_or(&"1"),
            parts.get(3).unwrap_or(&"village")
        );
    }
}
pub(super) fn rows(
    p: &mut Painter<'_>,
    form: &Form,
    draw: impl Fn(&mut Painter<'_>, usize, f32, f32),
    interactive: bool,
) {
    let size = p.frame.size.map(f32::ceil);
    let (top, bottom, row, n, bar) = form.list(size).unwrap();
    let w = size[0];
    let center = (w / 2.).floor();
    let scroll = form
        .scroll
        .min((row * n as f32 + 4. - (bottom - top)).max(0.))
        .floor();
    super::super::options::dirt(
        p,
        Rect {
            x: 0.,
            y: top,
            w,
            h: bottom - top,
        },
        [0., scroll],
        32. / 255.,
    );
    let start = p.frame.buttons.len();
    for i in 0..n {
        let y = top + 4. - scroll + row * i as f32;
        if y + row < top || y >= bottom {
            continue;
        }
        if interactive {
            if form.selected == Some(i) {
                p.rect(
                    center - 110.,
                    y - 2.,
                    220.,
                    row,
                    [128. / 255., 128. / 255., 128. / 255., 1.],
                );
                p.rect(center - 109., y - 1., 218., row - 2., [0., 0., 0., 1.]);
            }
            p.frame.buttons.push(super::super::Button {
                rect: Rect {
                    x: center - 110.,
                    y,
                    w: 220.,
                    h: row,
                },
                action: Action::WorldForm(Command::Select(i)),
                enabled: true,
            });
        }
        draw(p, i, center - 108., y);
    }
    super::super::options::mask_outside(p, 0., w, top, bottom);
    super::super::options::scrollbar(p, bar, top, bottom, row * n as f32, scroll);
    // GuiSlot clips clicks to its viewport including partially visible rows.
    for b in &mut p.frame.buttons[start..] {
        let end = (b.rect.y + b.rect.h).min(bottom);
        b.rect.y = b.rect.y.max(top);
        b.rect.h = (end - b.rect.y).max(0.);
    }
}
fn flat_icon(p: &mut Painter<'_>, x: f32, y: f32, id: u16, meta: u16) {
    p.image(
        "textures/gui/container/stats_icons.png",
        Rect {
            x: x + 1.,
            y: y + 1.,
            w: 18.,
            h: 18.,
        },
        [0., 0., 18., 18.],
        WHITE,
    );
    let (x, y) = (x + 2., y + 2.);
    let r = Rect {
        x,
        y,
        w: 16.,
        h: 16.,
    };
    if id == 0 {
        return;
    }
    if id >= 256 {
        let sprite = p.r.item_sprite(id, meta);
        p.sprite(
            sprite,
            r,
            [0., 0., sprite.width as f32, sprite.height as f32],
            WHITE,
        );
        return;
    }
    let palette = p.a.biome_colors.grass[0x7f7f];
    let tint = |shade: f32, colored: bool| {
        let color = if colored { palette } else { 0xffffff };
        let [_, r, g, b] = color.to_be_bytes();
        [
            r as f32 * shade / 255.,
            g as f32 * shade / 255.,
            b as f32 * shade / 255.,
            1.,
        ]
    };
    if id == 31 {
        if let Some(&layer) =
            p.a.materials
                .get(if meta == 2 { "fern" } else { "tallgrass" })
        {
            p.quad(r, [0., 0., 1., 1.], layer | 65536, tint(1., true));
        }
        return;
    }
    // RenderItem.setupGuiTransform plus the half-unit model scale. FaceBakery
    // GUI item colors are overwritten by RenderItem before OpenGL lighting.
    use crate::vanilla::ambient_occlusion::{vertices, DIRECTIONS};
    use glam::{Mat3, Vec3};
    let model = Mat3::from_rotation_x(210_f32.to_radians())
        * Mat3::from_rotation_y((-135_f32).to_radians());
    let lights =
        Mat3::from_rotation_y((-30_f32).to_radians()) * Mat3::from_rotation_x(165_f32.to_radians());
    let lights =
        [Vec3::new(0.2, 1., -0.7), Vec3::new(-0.2, 1., 0.7)].map(|v| lights * v.normalize());
    let state = id << 4 | (meta & 15);
    let height = if id == 78 {
        (meta.min(7) + 1) as f32 / 8.
    } else {
        1.
    };
    for (face, axis, positive) in [(1, 1, true), (2, 2, false), (5, 0, true)] {
        let normal = (model * DIRECTIONS[face].as_vec3()) * Vec3::new(1., 1., -1.);
        let intensity = (0.4
            + 0.6
                * lights
                    .iter()
                    .map(|light| normal.dot(*light).max(0.))
                    .sum::<f32>())
        .min(1.);
        let points = vertices(face).map(|[ix, iy, iz]| {
            let v = model
                * (Vec3::new((ix & 1) as f32, (iy & 1) as f32 * height, (iz & 1) as f32)
                    - Vec3::splat(0.5));
            [x + 8. + 10. * v.x, y + 8. + 10. * v.y]
        });
        let top = if face == 1 { 0. } else { 1. - height };
        let uv = [[0., top], [0., 1.], [1., 1.], [1., top]];
        p.polygon(
            points,
            uv,
            p.a.texture(state, axis, positive) | 65536,
            tint(intensity, id == 2 && face == 1),
        );
        if id == 2 && face != 1 {
            if let Some(&overlay) = p.a.materials.get("grass_side_overlay") {
                p.polygon(points, uv, overlay | 65536, tint(intensity, true));
            }
        }
    }
}
fn label(p: &mut Painter<'_>, x: f32, y: f32, text: &str) {
    p.text_pass(x, y, text, WHITE);
}
fn layer_parts(p: &Painter<'_>, s: &str) -> (u16, u16, i32) {
    let (count, key) = s
        .split_once('*')
        .map_or((1, s), |(count, key)| (count.parse().unwrap_or(1), key));
    let key = key.strip_prefix("minecraft:").unwrap_or(key);
    let (name, meta) = key
        .split_once(':')
        .map_or((key, 0), |(k, m)| (k, m.parse().unwrap_or(0)));
    let id = name
        .parse()
        .ok()
        .or_else(|| {
            p.r.registry
                .blocks
                .iter()
                .find(|(_, d)| d.name.trim_start_matches("minecraft:") == name)
                .map(|(&id, _)| id)
        })
        .unwrap_or(0);
    (id, meta, count)
}
pub(super) fn draw(p: &mut Painter<'_>, form: &Form, time: f32) {
    let [w, h] = p.frame.size.map(f32::ceil);
    let cx = (w / 2.).floor();
    let flat = form.page == Page::Flat;
    rows(
        p,
        form,
        |p, i, x, y| {
            if flat {
                let Some(layer) = layers(&form.draft).rev().nth(i) else {
                    return;
                };
                let (id, meta, n) = layer_parts(p, layer);
                let icon = match id {
                    8 | 9 => 326,
                    10 | 11 => 327,
                    _ => id,
                };
                flat_icon(p, x, y, icon, meta);
                let name = if id == 0 {
                    "Air".into()
                } else {
                    p.r.item_name(&p.s.language, id, meta)
                };
                label(p, x + 23., y + 3., &name);
                let key = if i == 0 {
                    "top"
                } else if i + 1 == layer_count(&form.draft) {
                    "bottom"
                } else {
                    ""
                };
                let qty = p
                    .tr(&if key.is_empty() {
                        "createWorld.customize.flat.layer".into()
                    } else {
                        format!("createWorld.customize.flat.layer.{key}")
                    })
                    .replace("%s", &n.to_string())
                    .replace("%d", &n.to_string());
                label(p, x + 215. - p.width(&qty), y + 3., &qty);
            } else if form.page == Page::FlatPresets {
                let item = &data().flat[i];
                flat_icon(p, x, y, item.item, item.damage);
                label(p, x + 23., y + 6., &item.name);
            } else {
                let entry = &data().customized[i];
                let path = entry.texture.trim_start_matches("minecraft:");
                let sprite = p.r.sprite(path);
                p.rect(
                    x + 4.,
                    y - 1.,
                    34.,
                    34.,
                    [224. / 255., 224. / 255., 224. / 255., 1.],
                );
                p.rect(
                    x + 5.,
                    y,
                    33.,
                    33.,
                    [160. / 255., 160. / 255., 160. / 255., 1.],
                );
                p.rect(
                    x + 4.,
                    y + 32.,
                    1.,
                    1.,
                    [160. / 255., 160. / 255., 160. / 255., 1.],
                );
                p.image(
                    path,
                    Rect {
                        x: x + 5.,
                        y,
                        w: 32.,
                        h: 32.,
                    },
                    [0., 0., sprite.width as f32, sprite.height as f32],
                    WHITE,
                );
                let key = [
                    "waterWorld",
                    "isleLand",
                    "caveDelight",
                    "mountains",
                    "drought",
                    "caveChaos",
                    "goodLuck",
                ][i];
                label(
                    p,
                    x + 42.,
                    y + 14.,
                    &p.tr(&format!("createWorld.customize.custom.preset.{key}")),
                );
            }
        },
        true,
    );
    if flat {
        p.center(cx, 8., &p.tr("createWorld.customize.flat.title"), WHITE);
        p.text(
            cx - 108.,
            32.,
            &p.tr("createWorld.customize.flat.tile"),
            WHITE,
        );
        let height = p.tr("createWorld.customize.flat.height");
        p.text(cx + 107. - p.width(&height), 32., &height, WHITE);
        button(
            p,
            rect(cx - 155., h - 52., 150.),
            &p.tr("createWorld.customize.flat.removeLayer"),
            Command::RemoveLayer,
            form.selected.is_some(),
        );
        button(
            p,
            rect(cx + 5., h - 52., 150.),
            &p.tr("createWorld.customize.presets"),
            Command::Presets,
            true,
        );
    } else {
        let title = if form.page == Page::CustomPresets {
            "createWorld.customize.custom.presets.title"
        } else {
            "createWorld.customize.presets.title"
        };
        p.center(cx, 8., &p.tr(title), WHITE);
        let gray = [160. / 255., 160. / 255., 160. / 255., 1.];
        p.text(50., 30., &p.tr("createWorld.customize.presets.share"), gray);
        p.text(50., 70., &p.tr("createWorld.customize.presets.list"), gray);
        form.share.draw(
            p,
            rect(50., 40., w - 100.),
            form.focused,
            time,
            if form.page == Page::FlatPresets {
                1230
            } else {
                2000
            },
            |i| Action::WorldForm(Command::Cursor(i)),
        );
    }
    let key = if flat {
        "gui.done"
    } else {
        "createWorld.customize.presets.select"
    };
    let (left, right) = if form.page == Page::CustomPresets {
        (rect(cx - 102., h - 27., 100.), rect(cx + 3., h - 27., 100.))
    } else {
        (rect(cx - 155., h - 28., 150.), rect(cx + 5., h - 28., 150.))
    };
    button(
        p,
        left,
        &p.tr(key),
        Command::Done,
        flat || form.selected.is_some() || form.share.value.len() > 1,
    );
    button(p, right, &p.tr("gui.cancel"), Command::Cancel, true);
}
