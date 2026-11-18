use super::{Action, Gui, Painter, Rect, Screen, WHITE};
use crate::keybindings::{key_label, Binding, ControlAction, ControlsList};

fn dirt(p: &mut Painter<'_>, top: i32, bottom: i32, scroll: i32, level: u8) {
    let sprite = p.r.sprite("textures/gui/options_background.png");
    let width = p.frame.size[0].ceil() as i32;
    let brightness = f32::from(level) / 255.;
    let mut y = top;
    while y < bottom {
        let offset = (y + scroll).rem_euclid(32);
        let height = (32 - offset).min(bottom - y);
        for x in (0..width).step_by(32) {
            p.sprite(
                sprite,
                Rect {
                    x: x as f32,
                    y: y as f32,
                    w: 32.,
                    h: height as f32,
                },
                [
                    0.,
                    offset as f32 * sprite.height as f32 / 32.,
                    sprite.width as f32,
                    height as f32 * sprite.height as f32 / 32.,
                ],
                [brightness, brightness, brightness, 1.],
            );
        }
        y += height;
    }
}

fn fade(p: &mut Painter<'_>, y: f32, down: bool) {
    let first = p.frame.vertices.len();
    p.rect(0., y, p.frame.size[0], 4., [0., 0., 0., 1.]);
    for (v, bottom) in p.frame.vertices[first..]
        .iter_mut()
        .zip([false, true, true, false, true, false])
    {
        v.color[3] = f32::from(bottom != down);
    }
}

fn key_button(p: &mut Painter<'_>, gui: &Gui, binding: Binding, rect: Rect, mouse_format: &str) {
    let key = key_label(p.s.bindings.code(binding), mouse_format);
    let label = if gui.editing_binding == Some(binding) {
        format!("§f> §e{key}§f <")
    } else if p.s.bindings.conflicts(binding) {
        format!("§c{key}")
    } else {
        key
    };
    p.button(
        rect,
        &label,
        Action::Controls(ControlAction::Edit(binding)),
        true,
    );
}

pub(super) fn draw(p: &mut Painter<'_>, gui: &Gui) {
    let size = p.frame.size;
    let cx = (size[0] * 0.5).floor();
    let height = size[1] as i32;
    let top = ControlsList::TOP;
    let bottom = height - 32;
    let list = ControlsList::new(size);
    let scroll = list.clamp(gui.scroll as f32);
    dirt(p, top, bottom, scroll, 32);
    let mut entries: Vec<_> = Binding::ALL
        .iter()
        .map(|&b| (p.tr(b.category()), p.tr(b.key()), b))
        .collect();
    entries.sort_by(|a, b| {
        a.0.encode_utf16()
            .cmp(b.0.encode_utf16())
            .then_with(|| a.1.encode_utf16().cmp(b.1.encode_utf16()))
    });
    let label_width = entries
        .iter()
        .map(|row| p.width(&row.1) as i32)
        .max()
        .unwrap_or(0) as f32;
    let mouse_format = p.tr("key.mouseButton");
    let reset = p.tr("controls.reset");
    let x = cx - 124.;
    let mut y = top + 4 - scroll;
    let mut category = None;
    for (category_name, label, binding) in entries {
        if category != Some(binding.category()) {
            category = Some(binding.category());
            if y + 20 > top && y < bottom {
                p.text_pass(
                    cx - (p.width(&category_name) * 0.5).floor(),
                    (y + 6) as f32,
                    &category_name,
                    WHITE,
                );
            }
            y += 20;
        }
        if y + 20 > top && y < bottom {
            let rect = Rect {
                x: x + 105.,
                y: y as f32,
                w: 75.,
                h: 20.,
            };
            let first = p.frame.buttons.len();
            p.text_pass(x + 90. - label_width, (y + 4) as f32, &label, WHITE);
            p.button(
                Rect {
                    x: x + 190.,
                    w: 50.,
                    ..rect
                },
                &reset,
                Action::Controls(ControlAction::Reset(binding)),
                p.s.bindings.code(binding) != binding.default_code(),
            );
            key_button(p, gui, binding, rect, &mouse_format);
            for button in &mut p.frame.buttons[first..] {
                let y0 = button.rect.y.max(top as f32);
                let y1 = (button.rect.y + button.rect.h).min(bottom as f32);
                button.rect.y = y0;
                button.rect.h = (y1 - y0).max(0.);
            }
        }
        y += 20;
    }
    // GuiSlot draws the opaque headers over partial rows before its edge fades and scroll thumb.
    dirt(p, 0, top, 0, 64);
    dirt(p, bottom, height, 0, 64);
    fade(p, top as f32, true);
    fade(p, (bottom - 4) as f32, false);
    if let Some((x, y, h)) = list.thumb(scroll) {
        p.rect(x, top as f32, 6., (bottom - top) as f32, [0., 0., 0., 1.]);
        p.rect(x, y, 6., h, [128. / 255., 128. / 255., 128. / 255., 1.]);
        p.rect(
            x,
            y,
            5.,
            h - 1.,
            [192. / 255., 192. / 255., 192. / 255., 1.],
        );
    }
    p.center(cx, 8., &p.tr("controls.title"), WHITE);
    let mut rect = Rect {
        x: cx - 155.,
        y: 18.,
        w: 150.,
        h: 20.,
    };
    for (key, enabled, action, y) in [
        (
            "options.invertMouse",
            p.s.invert_mouse,
            ControlAction::InvertMouse,
            18.,
        ),
        (
            "options.touchscreen",
            p.s.touchscreen,
            ControlAction::Touchscreen,
            42.,
        ),
    ] {
        rect.y = y;
        let label = format!(
            "{}: {}",
            p.tr(key),
            p.tr(if enabled { "options.on" } else { "options.off" })
        );
        p.button(rect, &label, Action::Controls(action), true);
    }
    let slider = Rect {
        x: cx + 5.,
        y: 18.,
        ..rect
    };
    let widgets = "textures/gui/widgets.png";
    // GuiOptionSlider always uses hover state zero (disabled track, enabled knob).
    for (x, u) in [(0., 0.), (75., 125.)] {
        p.image(
            widgets,
            Rect {
                x: slider.x + x,
                w: 75.,
                ..slider
            },
            [u, 46., 75., 20.],
            WHITE,
        );
    }
    let knob = slider.x + (p.s.sensitivity * (slider.w - 8.)).floor();
    for (x, u) in [(0., 0.), (4., 196.)] {
        p.image(
            widgets,
            Rect {
                x: knob + x,
                w: 4.,
                ..slider
            },
            [u, 66., 4., 20.],
            WHITE,
        );
    }
    let value = if p.s.sensitivity == 0. {
        p.tr("options.sensitivity.min")
    } else if p.s.sensitivity == 1. {
        p.tr("options.sensitivity.max")
    } else {
        format!("{}%", (p.s.sensitivity * 200.) as i32)
    };
    let label = format!("{}: {value}", p.tr("options.sensitivity"));
    p.center(
        slider.x + 75.,
        slider.y + 6.,
        &label,
        if slider.contains(p.mouse) {
            [1., 1., 160. / 255., 1.]
        } else {
            [224. / 255., 224. / 255., 224. / 255., 1.]
        },
    );
    #[cfg(test)]
    if let Some(labels) = &mut p.frame.captions {
        labels.push((slider, label));
    }
    p.frame.buttons.push(super::Button {
        rect: slider,
        enabled: true,
        action: Action::Controls(ControlAction::Sensitivity),
    });
    rect.y = (height - 29) as f32;
    p.button(
        rect,
        &p.tr("gui.done"),
        Action::Screen(Screen::Options),
        true,
    );
    p.button(
        Rect { x: cx + 5., ..rect },
        &p.tr("controls.resetAll"),
        Action::Controls(ControlAction::ResetAll),
        Binding::ALL
            .iter()
            .any(|&b| p.s.bindings.code(b) != b.default_code()),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires locally imported Minecraft resources"]
    fn controls_widgets_match_original_and_scrolled_bindings_stay_inside_list() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/vanilla189-menu-layouts.json"
        ))
        .unwrap();
        let mut settings = crate::settings::Settings {
            gui_scale: 2,
            language: "ko_kr".into(),
            ..Default::default()
        };
        let mut gui = Gui {
            screen: Screen::Controls,
            ..Default::default()
        };
        settings.locale_unicode = crate::validation::resources().unicode(&settings.language);
        let render = |gui: &Gui, settings: &crate::settings::Settings| {
            let mut frame = super::super::Frame {
                captions: Some(Vec::new()),
                ..Default::default()
            };
            frame.set_viewport([1600, 900], settings);
            super::super::paint(
                &mut frame,
                gui,
                settings,
                crate::validation::resources(),
                crate::validation::assets(),
                None,
                &Default::default(),
            );
            frame
        };
        let frame = render(&gui, &settings);
        let mut ids = Vec::new();
        for button in &frame.buttons {
            let id = match button.action {
                Action::Screen(Screen::Options) => 200,
                Action::Controls(ControlAction::ResetAll) => 201,
                Action::Controls(ControlAction::InvertMouse) => 0,
                Action::Controls(ControlAction::Sensitivity) => 1,
                Action::Controls(ControlAction::Touchscreen) => 24,
                _ => continue,
            };
            let original = oracle["controls"]["buttons"]
                .as_array()
                .unwrap()
                .iter()
                .find(|b| b[0] == id)
                .unwrap();
            let rect = button.rect;
            assert_eq!(
                [rect.x, rect.y, rect.w, rect.h].map(|v| v as i64),
                [1, 2, 3, 4].map(|i| original[i].as_i64().unwrap())
            );
            assert_eq!(button.enabled, original[5].as_bool().unwrap());
            let caption = frame
                .captions
                .as_ref()
                .unwrap()
                .iter()
                .find(|(r, _)| r.x == rect.x && r.y == rect.y)
                .unwrap();
            assert_eq!(caption.1, original[7].as_str().unwrap());
            ids.push(id);
        }
        ids.sort_unstable();
        assert_eq!(ids, [0, 1, 24, 200, 201]);
        settings.bindings.set(Binding::Forward, 0);
        let mut seen = std::collections::HashSet::new();
        for scroll in (0..=420).step_by(15).chain([i32::MAX]) {
            gui.scroll = scroll;
            let frame = render(&gui, &settings);
            for button in &frame.buttons {
                match button.action {
                    Action::Controls(ControlAction::Edit(binding)) => {
                        assert!(button.rect.y >= 63. && button.rect.y + button.rect.h <= 418.);
                        seen.insert(binding);
                    }
                    Action::Controls(ControlAction::Reset(binding)) => {
                        assert_eq!(button.enabled, binding == Binding::Forward);
                    }
                    Action::Controls(ControlAction::ResetAll) => assert!(button.enabled),
                    _ => (),
                }
            }
        }
        assert_eq!(seen.len(), Binding::ALL.len());
    }
}
