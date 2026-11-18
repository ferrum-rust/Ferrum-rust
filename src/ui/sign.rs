use super::*;
#[derive(Default)]
pub(crate) struct Editor {
    pub pos: [i32; 3],
    pub lines: [String; 4],
    pub row: usize,
}
impl Editor {
    pub fn insert(&mut self, text: &str, r: &Resources, s: &Settings) {
        let width = |c: char| -> u32 {
            if c == ' ' {
                return 4;
            }
            if !(s.menu.force_unicode || s.locale_unicode) {
                if let Some(i) = font::ascii_index(c) {
                    return r.ascii_width[i] as u32;
                }
            }
            let b = r.glyph_sizes.get(c as usize).copied().unwrap_or(0);
            if b == 0 {
                0
            } else if b & 15 > 7 {
                9
            } else {
                ((i32::from(b & 15) + 1 - i32::from(b >> 4)) / 2 + 1).max(0) as u32
            }
        };
        let line = &mut self.lines[self.row];
        let mut total = line.chars().map(width).sum::<u32>();
        for c in text
            .chars()
            .filter(|&c| c >= ' ' && c != '\u{7f}' && c != '§')
        {
            let w = width(c);
            if total + w <= 90 && line.encode_utf16().count() + c.len_utf16() <= 90 {
                line.push(c);
                total += w;
            }
        }
    }
}
pub(super) fn draw(p: &mut Painter<'_>, edit: &Editor, time: f32) {
    let [w, h] = p.frame.size;
    let cx = (w / 2.).floor();
    p.rect(0., 0., w, h, [0., 0., 0., 0.6]);
    p.center(cx, 40., &p.tr("sign.edit"), WHITE);
    let sprite = p.r.sprite("textures/entity/sign.png");
    let sx = sprite.width as f32 / 64.;
    let sy = sprite.height as f32 / 32.;
    p.sprite(
        sprite,
        Rect {
            x: cx - 75.,
            y: 69.,
            w: 150.,
            h: 75.,
        },
        [2. * sx, 2. * sy, 24. * sx, 12. * sy],
        WHITE,
    );
    p.sprite(
        sprite,
        Rect {
            x: cx - 6.25,
            y: 144.,
            w: 12.5,
            h: 87.5,
        },
        [2. * sx, 16. * sy, 2. * sx, 14. * sy],
        WHITE,
    );
    for (i, line) in edit.lines.iter().enumerate() {
        let text = if edit.row == i && (time * 20.) as i32 / 6 % 2 == 0 {
            format!("> {line} <")
        } else {
            line.clone()
        };
        p.draw_text_run(
            cx - (p.width(&text) / 2.).floor(),
            85. + i as f32 * 10.,
            &text,
            [0., 0., 0., 1.],
            false,
        );
    }
    p.button(
        Rect {
            x: cx - 100.,
            y: (h / 4.).floor() + 120.,
            w: 200.,
            h: 20.,
        },
        &p.tr("gui.done"),
        Action::FinishSign,
        true,
    );
}
