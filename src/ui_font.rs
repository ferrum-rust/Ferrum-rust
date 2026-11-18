use super::{Painter, Sprite};
#[cfg(windows)]
pub(super) fn visual_order(text: &str) -> String {
    use std::{ffi::c_void, ptr};
    #[link(name = "icu")]
    extern "C" {
        fn u_shapeArabic(
            src: *const u16,
            len: i32,
            dst: *mut u16,
            capacity: i32,
            flags: u32,
            error: *mut i32,
        ) -> i32;
        fn ubidi_open() -> *mut c_void;
        fn ubidi_setPara(
            bidi: *mut c_void,
            text: *const u16,
            len: i32,
            level: u8,
            embedding: *const u8,
            error: *mut i32,
        );
        fn ubidi_writeReordered(
            bidi: *mut c_void,
            dst: *mut u16,
            capacity: i32,
            options: u16,
            error: *mut i32,
        ) -> i32;
        fn ubidi_close(bidi: *mut c_void);
    }
    if text.is_ascii() {
        return text.to_owned();
    }
    let logical: Vec<_> = text.encode_utf16().collect();
    let mut shaped = vec![0; logical.len() + 1];
    let mut output = vec![0; shaped.len()];
    let mut error = 0;
    // 1.8.9 uses ICU ArabicShaping(8), default RTL paragraph direction
    // (127 in ICU4J, 0xff in ICU4C), and Bidi.writeReordered(DO_MIRRORING). All buffers stay alive
    // through ubidi_close, including the text borrowed by ubidi_setPara.
    unsafe {
        let len = u_shapeArabic(
            logical.as_ptr(),
            logical.len() as i32,
            shaped.as_mut_ptr(),
            shaped.len() as i32,
            8,
            &mut error,
        );
        if error > 0 {
            return text.to_owned();
        }
        // ICU4J 51 joins Persian letters to their neighbours but keeps the
        // letters themselves as base characters. Newer ICU adds FBxx forms.
        for unit in &mut shaped[..len as usize] {
            if (0xfb50..=0xfbff).contains(unit) && !logical.contains(unit) {
                let mut base = [0u16; 2];
                let mut form_error = 0;
                if u_shapeArabic(unit, 1, base.as_mut_ptr(), 2, 16, &mut form_error) == 1
                    && form_error <= 0
                {
                    *unit = base[0];
                }
            }
        }
        let bidi = ubidi_open();
        if bidi.is_null() {
            return text.to_owned();
        }
        ubidi_setPara(bidi, shaped.as_ptr(), len, 0xff, ptr::null(), &mut error);
        let written = ubidi_writeReordered(
            bidi,
            output.as_mut_ptr(),
            output.len() as i32,
            2,
            &mut error,
        );
        ubidi_close(bidi);
        if error <= 0 {
            return String::from_utf16_lossy(&output[..written as usize]);
        }
    }
    text.to_owned()
}

#[cfg(not(windows))]
pub(super) fn visual_order(text: &str) -> String {
    let bidi = unicode_bidi::BidiInfo::new(text, None);
    bidi.paragraphs.first().map_or_else(
        || text.to_owned(),
        |p| bidi.reorder_line(p, p.range.clone()).into_owned(),
    )
}
use std::collections::HashMap;
use std::sync::OnceLock;

pub(super) fn ascii_index(c: char) -> Option<usize> {
    if (' '..='~').contains(&c) {
        return Some(c as usize);
    }
    if c == '\0' {
        return None;
    }
    static EXTENDED: OnceLock<HashMap<char, usize>> = OnceLock::new();
    EXTENDED.get_or_init(|| {
        let prefix="ÀÁÂÈÊËÍÓÔÕÚßãõğİıŒœŞşŴŵžȇ";
        let suffix="ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜø£Ø×ƒáíóúñÑªº¿®¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αβΓπΣσμτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■";
        prefix.chars().enumerate().chain(suffix.chars().enumerate().map(|(i,c)|(i+128,c))).map(|(i,c)|(c,i)).collect()
    }).get(&c).copied()
}
#[derive(Default)]
struct Style {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    random: bool,
    code: Option<u8>,
}
impl Style {
    fn apply(&mut self, c: char) {
        match c.to_ascii_lowercase() {
            'k' => self.random = true,
            'l' => self.bold = true,
            'm' => self.strike = true,
            'n' => self.underline = true,
            'o' => self.italic = true,
            'r' => *self = Self::default(),
            color => {
                *self = Self {
                    code: Some(color.to_digit(16).unwrap_or(15) as u8),
                    ..Default::default()
                }
            }
        }
    }
}
fn tint(base: [f32; 4], code: Option<u8>, shadow: bool, stereo: bool) -> [f32; 4] {
    let [mut r, mut g, mut b] = if let Some(n) = code {
        let plus = i32::from(n >> 3) * 85;
        [
            i32::from((n >> 2) & 1) * 170 + plus + if n == 6 { 85 } else { 0 },
            i32::from((n >> 1) & 1) * 170 + plus,
            i32::from(n & 1) * 170 + plus,
        ]
    } else {
        [base[0], base[1], base[2]].map(|v| (v * 255.).round() as i32)
    };
    if stereo && code.is_some() {
        (r, g, b) = (
            (r * 30 + g * 59 + b * 11) / 100,
            (r * 30 + g * 70) / 100,
            (r * 30 + b * 70) / 100,
        );
    }
    if shadow {
        r /= 4;
        g /= 4;
        b /= 4;
    }
    [r as f32 / 255., g as f32 / 255., b as f32 / 255., base[3]]
}
impl Painter<'_> {
    fn unicode_font(&self) -> bool {
        self.s.menu.force_unicode || self.s.locale_unicode
    }
    pub(super) fn glyph_width(&self,c:char)->f32 { character_width(self.r,self.s,c) }
    pub(super) fn width(&self, text: &str) -> f32 {
        let mut total = 0.;
        let mut chars = text.chars();
        let mut bold = false;
        while let Some(c) = chars.next() {
            if c == '§' {
                if let Some(code) = chars.next() {
                    match code.to_ascii_lowercase() {
                        'l' => bold = true,
                        '0'..='9' | 'a'..='f' | 'r' => bold = false,
                        _ => {}
                    }
                    continue;
                }
            }
            total += self.glyph_width(c) + f32::from(bold);
        }
        total
    }
    pub(super) fn text_pass(&mut self, x: f32, y: f32, text: &str, color: [f32; 4]) {
        let visual = self.bidi.then(|| visual_order(text));
        self.draw_text_run(x, y, visual.as_deref().unwrap_or(text), color, false);
    }
    pub(super) fn draw_text_run(
        &mut self,
        mut x: f32,
        y: f32,
        text: &str,
        base: [f32; 4],
        shadow: bool,
    ) -> f32 {
        let mut style = Style::default();
        let mut chars = text.chars();
        let mut color = tint(base, None, shadow, false);
        while let Some(mut c) = chars.next() {
            if c == '§' {
                if let Some(code) = chars.next() {
                    style.apply(code);
                    color = tint(base, style.code, shadow, self.s.anaglyph);
                    continue;
                }
            }
            let unicode = self.unicode_font();
            let n = ascii_index(c);
            if style.random && n.is_some() {
                let width = self.glyph_width(c);
                let phase = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
                    / 50;
                let index = (phase as usize).wrapping_add(x.to_bits() as usize);
                if let Some(replacement) = (0..94)
                    .map(|i| (33 + (index.wrapping_add(i)) % 94) as u8 as char)
                    .find(|k| self.glyph_width(*k) == width)
                {
                    c = replacement;
                }
            }
            let step = if unicode { 0.5 } else { 1. };
            let offset = if shadow && (unicode || n.is_none()) {
                step
            } else {
                0.
            };
            let mut advance = 4.;
            if c != ' ' {
                let glyph = if let Some(i) = ascii_index(c).filter(|_| !unicode) {
                    let width = self.r.ascii_width[i];
                    let sprite = self.r.sprite("textures/font/ascii.png");
                    let uv_scale = sprite.width as f32 / 128.;
                    Some((
                        sprite,
                        [(i % 16 * 8) as f32, (i / 16 * 8) as f32, width - 1.01, 7.99]
                            .map(|n| n * uv_scale),
                        [width - 1.01, 7.99],
                        width,
                    ))
                } else {
                    let index = c as usize;
                    let bits = self.r.glyph_sizes.get(index).copied().unwrap_or(0);
                    let start = (bits >> 4) as f32;
                    let size = ((bits & 15) as f32 + 1. - start).max(0.);
                    (bits != 0).then(|| {
                        (
                            self.r.sprite(&format!(
                                "textures/font/unicode_page_{:02x}.png",
                                index >> 8
                            )),
                            [
                                (index % 16 * 16) as f32 + start,
                                ((index & 255) / 16 * 16) as f32,
                                size - 0.02,
                                15.98,
                            ],
                            [(size - 0.02) * 0.5, 7.99],
                            size * 0.5 + 1.,
                        )
                    })
                };
                let Some((sprite, uv, size, width)) = glyph else {
                    continue;
                };
                advance = width;
                self.draw_glyph(
                    sprite,
                    [x - offset, y - offset],
                    uv,
                    size,
                    style.italic,
                    color,
                );
                if style.bold {
                    self.draw_glyph(
                        sprite,
                        [x - offset + step, y - offset],
                        uv,
                        size,
                        style.italic,
                        color,
                    );
                }
            }
            if style.bold {
                advance += 1.;
            }
            if style.strike {
                self.rect(x, y + 3., advance, 1., color);
            }
            if style.underline {
                self.rect(x - 1., y + 8., advance + 1., 1., color);
            }
            x += advance.trunc();
        }
        x.trunc()
    }
    fn draw_glyph(
        &mut self,
        s: Sprite,
        [x, y]: [f32; 2],
        uv: [f32; 4],
        [w, h]: [f32; 2],
        italic: bool,
        color: [f32; 4],
    ) {
        // Atlas pages store the logical vanilla sprite in their top-left corner.
        let skew = f32::from(italic);
        let [u, v, du, dv] = uv.map(|n| n / 256.);
        self.polygon(
            [
                [x + skew, y],
                [x - skew, y + h],
                [x + w - skew, y + h],
                [x + w + skew, y],
            ],
            [[u, v], [u, v + dv], [u + du, v + dv], [u + du, v]],
            s.layer,
            color,
        );
    }
}
#[cfg(all(test, windows))]
#[test]
fn local_189_icu4j_arabic_persian_and_mirrored_hebrew_order() {
    // Sequences produced by the installed 1.8.9 icu4j-core-mojang 51.2 JAR.
    for (text, expected) in [
        ("العربية", "ﺔﻴﺑﺮﻌﻟﺍ"),
        ("עברית (123)", "(123) תירבע"),
        ("فارسی", "یﺳﺭﺎﻓ"),
    ] {
        assert_eq!(visual_order(text), expected);
    }
}

pub(super) fn character_width(r:&crate::resources::Resources,s:&crate::settings::Settings,c:char)->f32{
        if c == ' ' {
            return 4.;
        }
        if !(s.menu.force_unicode || s.locale_unicode) {
            if let Some(i) = ascii_index(c) {
                return r.ascii_width[i];
            }
        }
        let bits = r.glyph_sizes.get(c as usize).copied().unwrap_or(0);
        if bits == 0 {
            return 0.;
        }
        if bits & 15 > 7 {
            return 9.;
        }
        ((i32::from(bits & 15) + 1 - i32::from(bits >> 4)) / 2 + 1) as f32
}

fn string_width(
    r: &crate::resources::Resources,
    s: &crate::settings::Settings,
    text: &str,
) -> f32 {
    let mut total = 0.;
    let mut chars = text.chars();
    let mut bold = false;
    while let Some(c) = chars.next() {
        if c == '§' {
            if let Some(code) = chars.next() {
                match code.to_ascii_lowercase() {
                    'l' => bold = true,
                    'r' | '0'..='9' | 'a'..='f' => bold = false,
                    _ => {}
                }
                continue;
            }
        }
        total += character_width(r, s, c) + f32::from(bold);
    }
    total
}

#[allow(clippy::too_many_arguments)]
fn emit_world_quad(
    mesh: &mut crate::mesh::Mesh,
    origin: glam::Vec3,
    right: glam::Vec3,
    up: glam::Vec3,
    scale: f32,
    pts: [[f32; 2]; 4],
    uvs: [[f32; 2]; 4],
    layer: u32,
    color: [f32; 4],
) {
    let rgba = color.map(|v| (v.clamp(0., 1.) * 255.).round() as u8);
    let to_world = |[px, py]: [f32; 2]| origin + right * (px * scale) - up * (py * scale);
    // pts from draw_glyph order: [top-left, bottom-left, bottom-right, top-right]
    // Reorder to [bottom-left, bottom-right, top-right, top-left] for CCW front face (+right x +up).
    let order = [1usize, 2, 3, 0];
    let start = mesh.vertices.len() as u32;
    for k in order {
        mesh.vertices.push(crate::mesh::Vertex {
            position: to_world(pts[k]).to_array(),
            uv: uvs[k],
            layer,
            color: rgba,
            light: [0, 240, 0, 0],
        });
    }
    mesh.indices.extend([0, 1, 2, 0, 2, 3].map(|i| start + i));
    mesh.visible_faces += 1;
}

#[allow(clippy::too_many_arguments)]
fn emit_world_text_run(
    mesh: &mut crate::mesh::Mesh,
    r: &crate::resources::Resources,
    s: &crate::settings::Settings,
    origin: glam::Vec3,
    right: glam::Vec3,
    up: glam::Vec3,
    scale: f32,
    mut x: f32,
    y: f32,
    text: &str,
    base: [f32; 4],
    extra_flags: u32,
) {
    use crate::models::{ATLAS, BLEND, NO_MIPMAP, SOLID, UNLIT};
    let unicode = s.menu.force_unicode || s.locale_unicode;
    let mut style = Style::default();
    let mut chars = text.chars();
    let mut color = tint(base, None, false, false);
    while let Some(mut c) = chars.next() {
        if c == '§' {
            if let Some(code) = chars.next() {
                style.apply(code);
                color = tint(base, style.code, false, s.anaglyph);
                continue;
            }
        }
        let n = ascii_index(c);
        if style.random && n.is_some() {
            let width = character_width(r, s, c);
            let phase = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                / 50;
            let index = (phase as usize).wrapping_add(x.to_bits() as usize);
            if let Some(replacement) = (0..94)
                .map(|i| (33 + (index.wrapping_add(i)) % 94) as u8 as char)
                .find(|k| character_width(r, s, *k) == width)
            {
                c = replacement;
            }
        }
        let step = if unicode { 0.5 } else { 1. };
        let mut advance = 4.;
        if c != ' ' {
            let glyph = if let Some(i) = ascii_index(c).filter(|_| !unicode) {
                let width = r.ascii_width[i];
                let sprite = r.sprite("textures/font/ascii.png");
                let uv_scale = sprite.width as f32 / 128.;
                Some((
                    sprite,
                    [(i % 16 * 8) as f32, (i / 16 * 8) as f32, width - 1.01, 7.99]
                        .map(|n| n * uv_scale),
                    [width - 1.01, 7.99],
                    width,
                ))
            } else {
                let index = c as usize;
                let bits = r.glyph_sizes.get(index).copied().unwrap_or(0);
                let start = (bits >> 4) as f32;
                let size = ((bits & 15) as f32 + 1. - start).max(0.);
                (bits != 0).then(|| {
                    (
                        r.sprite(&format!(
                            "textures/font/unicode_page_{:02x}.png",
                            index >> 8
                        )),
                        [
                            (index % 16 * 16) as f32 + start,
                            ((index & 255) / 16 * 16) as f32,
                            size - 0.02,
                            15.98,
                        ],
                        [(size - 0.02) * 0.5, 7.99],
                        size * 0.5 + 1.,
                    )
                })
            };
            let Some((sprite, uv, [w, h], width)) = glyph else {
                continue;
            };
            advance = width;
            let skew = f32::from(style.italic);
            let [u, v, du, dv] = uv.map(|n| n / 256.);
            let uvs = [[u, v], [u, v + dv], [u + du, v + dv], [u + du, v]];
            let glyph_layer = sprite.layer | ATLAS | NO_MIPMAP | UNLIT | BLEND | extra_flags;
            for pass in 0..=usize::from(style.bold) {
                let gx = x + pass as f32 * step;
                emit_world_quad(
                    mesh,
                    origin,
                    right,
                    up,
                    scale,
                    [
                        [gx + skew, y],
                        [gx - skew, y + h],
                        [gx + w - skew, y + h],
                        [gx + w + skew, y],
                    ],
                    uvs,
                    glyph_layer,
                    color,
                );
            }
        }
        if style.bold {
            advance += 1.;
        }
        let line_layer = SOLID | UNLIT | BLEND | extra_flags;
        if style.strike {
            emit_world_quad(
                mesh,
                origin,
                right,
                up,
                scale,
                [
                    [x, y + 3.],
                    [x, y + 4.],
                    [x + advance, y + 4.],
                    [x + advance, y + 3.],
                ],
                [[0.; 2]; 4],
                line_layer,
                color,
            );
        }
        if style.underline {
            emit_world_quad(
                mesh,
                origin,
                right,
                up,
                scale,
                [
                    [x - 1., y + 8.],
                    [x - 1., y + 9.],
                    [x + advance, y + 9.],
                    [x + advance, y + 8.],
                ],
                [[0.; 2]; 4],
                line_layer,
                color,
            );
        }
        x += advance.trunc();
    }
}

/// Render.renderLivingLabel / RendererLivingEntity.renderName 3D billboard nameplate.
pub(crate) fn world_nametag(
    translucent: &mut crate::mesh::Mesh,
    r: &crate::resources::Resources,
    s: &crate::settings::Settings,
    center: glam::Vec3,
    right: glam::Vec3,
    up: glam::Vec3,
    text: &str,
    sneaking: bool,
) {
    use crate::models::{BLEND, SEE_THROUGH, SOLID, UNLIT};
    if text.is_empty() {
        return;
    }
    let ordered = (!text.is_ascii()).then(|| visual_order(text));
    let shown = ordered.as_deref().unwrap_or(text);
    let width = string_width(r, s, shown) as i32;
    let half = (width / 2) as f32;
    let y0 = if text == "deadmau5" { -10. } else { 0. };
    let scale = 0.016666668_f32 * 1.6_f32;
    let normal = right.cross(up).normalize_or_zero();
    let plate_pts = [
        [-half - 1., y0 - 1.],
        [-half - 1., y0 + 8.],
        [half + 1., y0 + 8.],
        [half + 1., y0 - 1.],
    ];
    if sneaking {
        emit_world_quad(
            translucent,
            center,
            right,
            up,
            scale,
            plate_pts,
            [[0.; 2]; 4],
            SOLID | UNLIT | BLEND,
            [0., 0., 0., 0.25],
        );
        emit_world_text_run(
            translucent,
            r,
            s,
            center + normal * 0.001,
            right,
            up,
            scale,
            -half,
            y0,
            shown,
            [1., 1., 1., 32. / 255.],
            0,
        );
    } else {
        emit_world_quad(
            translucent,
            center,
            right,
            up,
            scale,
            plate_pts,
            [[0.; 2]; 4],
            SOLID | UNLIT | BLEND | SEE_THROUGH,
            [0., 0., 0., 0.25],
        );
        emit_world_text_run(
            translucent,
            r,
            s,
            center,
            right,
            up,
            scale,
            -half,
            y0,
            shown,
            [1., 1., 1., 32. / 255.],
            SEE_THROUGH,
        );
        emit_world_text_run(
            translucent,
            r,
            s,
            center + normal * 0.001,
            right,
            up,
            scale,
            -half,
            y0,
            shown,
            [1., 1., 1., 1.],
            0,
        );
    }
}

fn trim_to_width(
    r: &crate::resources::Resources,
    s: &crate::settings::Settings,
    text: &str,
    max_width: f32,
) -> String {
    let mut total = 0.;
    let mut bold = false;
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '§' {
            if let Some(code) = chars.next() {
                match code.to_ascii_lowercase() {
                    'l' => bold = true,
                    'r' | '0'..='9' | 'a'..='f' => bold = false,
                    _ => {}
                }
                out.push('§');
                out.push(code);
            }
            continue;
        }
        let w = character_width(r, s, c) + f32::from(bold);
        if total + w > max_width {
            break;
        }
        total += w;
        out.push(c);
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn emit_lit_world_quad(
    mesh: &mut crate::mesh::Mesh,
    origin: glam::Vec3,
    right: glam::Vec3,
    up: glam::Vec3,
    scale: f32,
    pts: [[f32; 2]; 4],
    uvs: [[f32; 2]; 4],
    layer: u32,
    color: [f32; 4],
    light_uv: [u8; 2],
) {
    let rgba = color.map(|v| (v.clamp(0., 1.) * 255.).round() as u8);
    let to_world = |[px, py]: [f32; 2]| origin + right * (px * scale) - up * (py * scale);
    let order = [1usize, 2, 3, 0];
    let start = mesh.vertices.len() as u32;
    for k in order {
        mesh.vertices.push(crate::mesh::Vertex {
            position: to_world(pts[k]).to_array(),
            uv: uvs[k],
            layer,
            color: rgba,
            light: [light_uv[0], light_uv[1], 0, 0],
        });
    }
    mesh.indices.extend([0, 1, 2, 0, 2, 3].map(|i| start + i));
    mesh.visible_faces += 1;
}

/// TileEntitySignRenderer 3D world-lit sign text row (clamped to <= 90 font pixels).
#[allow(clippy::too_many_arguments)]
pub(crate) fn world_sign_line(
    translucent: &mut crate::mesh::Mesh,
    r: &crate::resources::Resources,
    s: &crate::settings::Settings,
    origin: glam::Vec3,
    right: glam::Vec3,
    up: glam::Vec3,
    normal: glam::Vec3,
    scale: f32,
    y_px: f32,
    text: &str,
    light_uv: [u8; 2],
) {
    use crate::models::{ATLAS, BLEND, NO_MIPMAP, SOLID};
    if text.is_empty() {
        return;
    }
    let trimmed = trim_to_width(r, s, text, 90.);
    if trimmed.is_empty() {
        return;
    }
    let ordered = (!trimmed.is_ascii()).then(|| visual_order(&trimmed));
    let shown = ordered.as_deref().unwrap_or(&trimmed);
    let line_width = string_width(r, s, shown);
    let mut x = -line_width * 0.5;
    let y = y_px;
    let unicode = s.menu.force_unicode || s.locale_unicode;
    let shade = crate::models::standard_item_light(normal);
    let base = [0., 0., 0., 1.];
    let shaded = |c: [f32; 4]| [c[0] * shade, c[1] * shade, c[2] * shade, c[3]];
    let mut style = Style::default();
    let mut chars = shown.chars();
    let mut color = shaded(tint(base, None, false, false));
    while let Some(mut c) = chars.next() {
        if c == '§' {
            if let Some(code) = chars.next() {
                style.apply(code);
                color = shaded(tint(base, style.code, false, s.anaglyph));
                continue;
            }
        }
        let n = ascii_index(c);
        if style.random && n.is_some() {
            let width = character_width(r, s, c);
            let phase = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                / 50;
            let index = (phase as usize).wrapping_add(x.to_bits() as usize);
            if let Some(replacement) = (0..94)
                .map(|i| (33 + (index.wrapping_add(i)) % 94) as u8 as char)
                .find(|k| character_width(r, s, *k) == width)
            {
                c = replacement;
            }
        }
        let step = if unicode { 0.5 } else { 1. };
        let mut advance = 4.;
        if c != ' ' {
            let glyph = if let Some(i) = ascii_index(c).filter(|_| !unicode) {
                let width = r.ascii_width[i];
                let sprite = r.sprite("textures/font/ascii.png");
                let uv_scale = sprite.width as f32 / 128.;
                Some((
                    sprite,
                    [(i % 16 * 8) as f32, (i / 16 * 8) as f32, width - 1.01, 7.99]
                        .map(|n| n * uv_scale),
                    [width - 1.01, 7.99],
                    width,
                ))
            } else {
                let index = c as usize;
                let bits = r.glyph_sizes.get(index).copied().unwrap_or(0);
                let start = (bits >> 4) as f32;
                let size = ((bits & 15) as f32 + 1. - start).max(0.);
                (bits != 0).then(|| {
                    (
                        r.sprite(&format!(
                            "textures/font/unicode_page_{:02x}.png",
                            index >> 8
                        )),
                        [
                            (index % 16 * 16) as f32 + start,
                            ((index & 255) / 16 * 16) as f32,
                            size - 0.02,
                            15.98,
                        ],
                        [(size - 0.02) * 0.5, 7.99],
                        size * 0.5 + 1.,
                    )
                })
            };
            let Some((sprite, uv, [w, h], width)) = glyph else {
                continue;
            };
            advance = width;
            let skew = f32::from(style.italic);
            let [u, v, du, dv] = uv.map(|n| n / 256.);
            let uvs = [[u, v], [u, v + dv], [u + du, v + dv], [u + du, v]];
            let glyph_layer = sprite.layer | ATLAS | NO_MIPMAP | BLEND;
            for pass in 0..=usize::from(style.bold) {
                let gx = x + pass as f32 * step;
                emit_lit_world_quad(
                    translucent,
                    origin,
                    right,
                    up,
                    scale,
                    [
                        [gx + skew, y],
                        [gx - skew, y + h],
                        [gx + w - skew, y + h],
                        [gx + w + skew, y],
                    ],
                    uvs,
                    glyph_layer,
                    color,
                    light_uv,
                );
            }
        }
        if style.bold {
            advance += 1.;
        }
        let line_layer = SOLID | BLEND;
        if style.strike {
            emit_lit_world_quad(
                translucent,
                origin,
                right,
                up,
                scale,
                [
                    [x, y + 3.],
                    [x, y + 4.],
                    [x + advance, y + 4.],
                    [x + advance, y + 3.],
                ],
                [[0.; 2]; 4],
                line_layer,
                color,
                light_uv,
            );
        }
        if style.underline {
            emit_lit_world_quad(
                translucent,
                origin,
                right,
                up,
                scale,
                [
                    [x - 1., y + 8.],
                    [x - 1., y + 9.],
                    [x + advance, y + 9.],
                    [x + advance, y + 8.],
                ],
                [[0.; 2]; 4],
                line_layer,
                color,
                light_uv,
            );
        }
        x += advance.trunc();
    }
}
