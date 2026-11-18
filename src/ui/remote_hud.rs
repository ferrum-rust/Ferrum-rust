use super::{chat, Gui, Painter, Rect, Screen, WHITE};
use crate::{
    gameplay::Simulation,
    net_presentation::{Objective, State},
};

fn component(json: &str, p: &Painter<'_>) -> String {
    chat::components::formatted(&chat::components::parse(json, |key| p.tr(key)))
}

fn advance_format(active: &mut String, text: &str) {
    let mut color: Option<char> = None;
    let mut styles = String::new();
    for part in [active.as_str(), text] {
        let mut chars = part.chars();
        while let Some(c) = chars.next() {
            if c == '§' {
                if let Some(raw) = chars.next() {
                    let code = raw.to_ascii_lowercase();
                    match code {
                        '0'..='9' | 'a'..='f' => {
                            color = Some(code);
                            styles.clear();
                        }
                        'k'..='o' => {
                            if !styles.contains(code) {
                                styles.push(code);
                            }
                        }
                        'r' => {
                            color = None;
                            styles.clear();
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    active.clear();
    if let Some(c) = color {
        active.push('§');
        active.push(c);
    }
    for s in styles.chars() {
        active.push('§');
        active.push(s);
    }
}

fn wrap_formatted(p: &Painter<'_>, text: &str, max_width: f32) -> Vec<String> {
    if chat::plain(text).is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut active = String::new();
    for raw_line in text.split('\n') {
        let mut line = active.clone();
        let mut chars = raw_line.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '§' {
                line.push('§');
                if let Some(code) = chars.next() {
                    line.push(code);
                }
                continue;
            }
            let prev_len = line.len();
            line.push(ch);
            if p.width(&line) > max_width && !chat::plain(&line[..prev_len]).is_empty() {
                let split = line[..prev_len]
                    .rfind(' ')
                    .filter(|&i| !chat::plain(&line[..i]).is_empty());
                if let Some(space_idx) = split {
                    let head = line[..space_idx].to_owned();
                    let tail = line[space_idx + 1..].trim_start_matches(' ').to_owned();
                    advance_format(&mut active, &head);
                    out.push(head);
                    line = format!("{active}{tail}");
                } else {
                    let head = line[..prev_len].to_owned();
                    advance_format(&mut active, &head);
                    out.push(head);
                    line = format!("{active}{ch}");
                }
            }
        }
        advance_format(&mut active, &line);
        out.push(line);
    }
    out
}

fn italicize_formatted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    out.push_str("§o");
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '§' {
            if let Some(code) = chars.next() {
                out.push(code);
                let lc = code.to_ascii_lowercase();
                if matches!(lc, '0'..='9' | 'a'..='f' | 'r') {
                    out.push_str("§o");
                }
            }
        }
    }
    out
}
pub(super) fn draw(p: &mut Painter<'_>, gui: &Gui, sim: &Simulation) {
    let state = &sim.presentation;
    let [w, h] = p.frame.size.map(f32::ceil);
    let side = state
        .team(&sim.local_name)
        .filter(|t| (0..16).contains(&t.color))
        .and_then(|t| state.objective(3 + t.color as usize))
        .or_else(|| state.objective(1));
    if let Some(board) = side {
        sidebar(p, state, board);
    }
    if state.timer > 0 {
        let [fade, stay, out] = state.times;
        let remaining = state.timer;
        let elapsed = fade
            .saturating_add(stay)
            .saturating_add(out)
            .saturating_sub(remaining);
        let alpha = if elapsed < fade {
            elapsed as f32 / fade as f32
        } else if remaining <= out {
            remaining as f32 / out.max(1) as f32
        } else {
            1.
        };
        if alpha > 3. / 255. {
            for (json, scale, y) in [(&state.title, 4., -10.), (&state.subtitle, 2., 5.)] {
                if json.is_empty() {
                    continue;
                }
                let text = component(json, p);
                let begin = p.frame.vertices.len();
                let cx = (w / 2.).floor();
                let cy = (h / 2.).floor();
                p.center(cx, cy + y, &text, [1., 1., 1., alpha]);
                let base = [
                    cx * 2. / p.frame.size[0] - 1.,
                    1. - cy * 2. / p.frame.size[1],
                ];
                for v in &mut p.frame.vertices[begin..] {
                    for (value, origin) in v.position.iter_mut().zip(base) {
                        *value = origin + (*value - origin) * scale;
                    }
                }
            }
        }
    }
    if gui.option_state.remote && gui.chat.tab_open && gui.screen == Screen::Playing {
        tab(p, sim);
    }
}
fn sidebar(p: &mut Painter<'_>, state: &State, obj: &Objective) {
    let [w, h] = p.frame.size.map(f32::ceil);
    let mut scores: Vec<_> = obj
        .scores
        .iter()
        .filter(|(s, _)| !s.starts_with('#'))
        .collect();
    scores.sort_by(|(a, sa), (b, sb)| {
        sa.cmp(sb)
            .then_with(|| b.to_lowercase().cmp(&a.to_lowercase()))
    });
    let rows: Vec<_> = scores
        .into_iter()
        .rev()
        .take(15)
        .map(|(s, score)| (state.formatted_name(s), format!("§c{score}")))
        .collect();
    let width = rows
        .iter()
        .map(|(n, v)| p.width(&format!("{n}: {v}")))
        .fold(p.width(&obj.title), f32::max);
    let x = w - width - 3.;
    let top = (h / 2.).floor() + (rows.len() as f32 * 3.) - rows.len() as f32 * 9.;
    for (i, (name, score)) in rows.iter().enumerate() {
        let y = top + i as f32 * 9.;
        p.rect(x - 2., y, width + 4., 9., [0., 0., 0., 80. / 255.]);
        p.text_pass(x, y, name, WHITE);
        p.text_pass(w - 1. - p.width(score), y, score, WHITE);
    }
    if !rows.is_empty() {
        p.rect(x - 2., top - 10., width + 4., 9., [0., 0., 0., 96. / 255.]);
        p.rect(x - 2., top - 1., width + 4., 1., [0., 0., 0., 80. / 255.]);
        p.text_pass(
            x + (width / 2.).floor() - (p.width(&obj.title) / 2.).floor(),
            top - 9.,
            &obj.title,
            WHITE,
        );
    }
}
fn tab(p: &mut Painter<'_>, sim: &Simulation) {
    let state = &sim.presentation;
    let w = p.frame.size[0].ceil();
    let cx = (w / 2.).floor();
    let mut players: Vec<_> = state.players.iter().collect();
    players.sort_by_cached_key(|(_, player)| {
        (
            player.mode == 3,
            state
                .memberships
                .get(&player.name)
                .cloned()
                .unwrap_or_default(),
            player.name.clone(),
        )
    });
    let show_faces = state.local_uuid.is_some()
        || state.mojang_uuid.is_some()
        || state.players.keys().any(|u| (u[6] >> 4) == 4)
        || state.skins.values().any(|s| s.url.is_some());
    let board = state.objective(0);
    let lines: Vec<_> = players
        .into_iter()
        .take(80)
        .map(|(&uuid, player)| {
            let display = player
                .display
                .as_ref()
                .map_or_else(|| state.formatted_name(&player.name), |v| component(v, p));
            (uuid, player, display)
        })
        .collect();
    let cols = lines.len().div_ceil(20).max(1);
    let rows = lines.len().div_ceil(cols);
    let name_width = lines.iter().map(|(_, _, s)| p.width(s)).fold(0., f32::max);
    let score_width = board.map_or(0., |o| {
        if o.render_type == "hearts" {
            90.
        } else {
            lines
                .iter()
                .map(|(_, pl, _)| p.width(&format!(" {}", o.scores.get(&pl.name).unwrap_or(&0))))
                .fold(0., f32::max)
        }
    });
    let face_pad = if show_faces { 9. } else { 0. };
    let col = (name_width + score_width + 13. + face_pad).min(((w - 50.) / cols as f32).floor());
    let grid = cols as f32 * col + 5. * (cols - 1) as f32;
    let text = |json: &str| {
        if json.is_empty() {
            String::new()
        } else {
            component(json, p)
        }
    };
    let header = text(&state.header);
    let footer = text(&state.footer);
    let header = wrap_formatted(p, &header, w - 50.);
    let footer = wrap_formatted(p, &footer, w - 50.);
    let width = header
        .iter()
        .chain(&footer)
        .map(|s| p.width(s))
        .fold(grid, f32::max);
    let background = [0., 0., 0., 128. / 255.];
    let mut y = 10.;
    if !header.is_empty() {
        p.rect(
            cx - (width / 2.).floor() - 1.,
            y - 1.,
            width + 2.,
            header.len() as f32 * 9. + 1.,
            background,
        );
        for line in header {
            p.center(cx, y, &line, WHITE);
            y += 9.;
        }
        y += 1.;
    }
    p.rect(
        cx - (width / 2.).floor() - 1.,
        y - 1.,
        width + 2.,
        rows as f32 * 9. + 1.,
        background,
    );
    for (idx, (uuid, player, name)) in lines.iter().enumerate() {
        let x_row = cx - (grid / 2.).floor() + (idx / rows) as f32 * (col + 5.);
        let y = y + (idx % rows) as f32 * 9.;
        p.rect(x_row, y, col, 8., [1., 1., 1., 32. / 255.]);
        let mut text_x = x_row;
        if show_faces {
            let skin = state.skin_region(p.r, *uuid);
            let plain_name = chat::plain(&player.name);
            let (vy, vh) = if matches!(plain_name.as_str(), "Dinnerbone" | "Grumm") {
                (16., -8.)
            } else {
                (8., 8.)
            };
            let show_hat = if state.local_uuid == Some(*uuid) {
                p.s.menu.skin_parts & (1 << 6) != 0
            } else {
                sim.server_scene
                    .actors
                    .values()
                    .find(|a| a.uuid == Some(*uuid))
                    .map_or(true, |a| a.skin & (1 << 6) != 0)
            };
            let face_rect = Rect {
                x: text_x,
                y,
                w: 8.,
                h: 8.,
            };
            p.quad(face_rect, skin.subrect_uv(8., vy, 8., vh), skin.layer, WHITE);
            if show_hat {
                p.quad(face_rect, skin.subrect_uv(40., vy, 8., vh), skin.layer, WHITE);
            }
            text_x += 9.;
        }
        let name = if player.mode == 3 {
            italicize_formatted(name)
        } else {
            name.clone()
        };
        p.text(
            text_x,
            y,
            &name,
            [1., 1., 1., if player.mode == 3 { 144. / 255. } else { 1. }],
        );
        if let Some(obj) = board.filter(|_| player.mode != 3) {
            let score = obj.scores.get(&player.name).copied().unwrap_or_default();
            let right = x_row + col - 13.;
            if obj.render_type == "hearts" && right - text_x - name_width >= 9. {
                let space = ((right - text_x - name_width - 4.) / 10.).clamp(1., 9.);
                for i in 0..10 {
                    let r = Rect {
                        x: text_x + name_width + 2. + i as f32 * space,
                        y,
                        w: 9.,
                        h: 9.,
                    };
                    p.image("textures/gui/icons.png", r, [16., 0., 9., 9.], WHITE);
                    if score > i * 2 {
                        p.image(
                            "textures/gui/icons.png",
                            r,
                            [if score == i * 2 + 1 { 61. } else { 52. }, 0., 9., 9.],
                            WHITE,
                        );
                    }
                }
            } else {
                let label = format!("§e{score}");
                p.text(right - p.width(&label), y, &label, WHITE);
            }
        }
        let ping = match player.latency {
            i32::MIN..=-1 => 5,
            0..=149 => 0,
            150..=299 => 1,
            300..=599 => 2,
            600..=999 => 3,
            _ => 4,
        };
        p.image(
            "textures/gui/icons.png",
            Rect {
                x: x_row + col - 11.,
                y,
                w: 10.,
                h: 8.,
            },
            [0., 176. + ping as f32 * 8., 10., 8.],
            WHITE,
        );
    }
    y += rows as f32 * 9. + 1.;
    if !footer.is_empty() {
        p.rect(
            cx - (width / 2.).floor() - 1.,
            y - 1.,
            width + 2.,
            footer.len() as f32 * 9. + 1.,
            background,
        );
        for line in footer {
            p.center(cx, y, &line, WHITE);
            y += 9.;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiline_format_and_spectator_italics_preserve_color_codes() {
        let mut active = String::new();
        advance_format(&mut active, "§b§lAnticheat Server");
        assert_eq!(active, "§b§l");
        advance_format(&mut active, "§7Lobby 1");
        assert_eq!(active, "§7");
        advance_format(&mut active, "§rPlain");
        assert_eq!(active, "");

        assert_eq!(
            italicize_formatted("§c[Admin] §r§ePlayer"),
            "§o§c§o[Admin] §r§o§e§oPlayer"
        );
    }
}
