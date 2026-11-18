use super::*;
use crate::vanilla::{drops::DropNbt as Tag, item_nbt};

#[derive(Default)]
pub(crate) enum Special {
    #[default]
    None,
    Anvil {
        slot: Option<Stack>,
        field: text_field::TextField,
        focused: bool,
        default_name: Option<String>,
        cost: i16,
    },
    Beacon {
        level: i16,
        primary: i16,
        secondary: i16,
        server: [i16; 2],
    },
}
impl Special {
    pub fn sync(
        &mut self,
        kind: &str,
        input: Option<&Stack>,
        props: &std::collections::HashMap<i16, i16>,
        res: &Resources,
        lang: &str,
    ) -> Option<String> {
        let prop = |id| props.get(&id).copied().unwrap_or(0);
        match kind {
            "minecraft:anvil" => {
                if !matches!(self, Self::Anvil { .. }) {
                    *self = Self::Anvil {
                        slot: None,
                        field: Default::default(),
                        focused: false,
                        default_name: None,
                        cost: 0,
                    };
                }
                let Self::Anvil {
                    slot,
                    field,
                    default_name,
                    cost,
                    ..
                } = self
                else {
                    unreachable!()
                };
                *cost = prop(0);
                if slot.as_ref() != input {
                    *slot = input.cloned();
                    let custom = input
                        .and_then(|s| s.nbt.as_deref())
                        .and_then(item_nbt::compound)
                        .and_then(|m| m.get("display"))
                        .and_then(item_nbt::compound)
                        .and_then(|m| m.get("Name"))
                        .and_then(|v| {
                            if let Tag::String(n) = v {
                                Some(n.as_str())
                            } else {
                                None
                            }
                        });
                    let label = input.map_or_else(String::new, |s| {
                        custom.map_or_else(|| res.item_name(lang, s.id, s.meta), String::from)
                    });
                    *default_name = (custom.is_none()).then_some(label.clone());
                    *field = text_field::TextField::with_limit(&label, 30);
                    return input.map(|_| {
                        if default_name.as_deref() == Some(&field.value) {
                            String::new()
                        } else {
                            field.value.clone()
                        }
                    });
                }
            }
            "minecraft:beacon" => {
                let pair = [prop(1), prop(2)];
                match self {
                    Self::Beacon {
                        level,
                        primary,
                        secondary,
                        server,
                    } => {
                        *level = prop(0);
                        if server[0] != pair[0] {
                            *primary = pair[0]
                        }
                        if server[1] != pair[1] {
                            *secondary = pair[1]
                        }
                        *server = pair;
                    }
                    _ => {
                        *self = Self::Beacon {
                            level: prop(0),
                            primary: pair[0],
                            secondary: pair[1],
                            server: pair,
                        }
                    }
                }
            }
            _ => *self = Self::None,
        }
        None
    }
    pub fn rename(&self) -> Option<String> {
        let Self::Anvil {
            slot: Some(_),
            field,
            default_name,
            ..
        } = self
        else {
            return None;
        };
        Some(if default_name.as_deref() == Some(&field.value) {
            String::new()
        } else {
            field.value.clone()
        })
    }
    pub fn beacon(&mut self, secondary_choice: bool, id: i16) {
        let Self::Beacon {
            level,
            primary,
            secondary,
            ..
        } = self
        else {
            return;
        };
        if secondary_choice {
            if *level >= 4 && (id == 10 || id > 0 && id == *primary) {
                *secondary = id;
            }
        } else if (matches!(id, 1 | 3) && *level >= 1)
            || (matches!(id, 11 | 8) && *level >= 2)
            || (id == 5 && *level >= 3)
        {
            *primary = id;
        }
    }
}
fn button(
    p: &mut Painter<'_>,
    r: Rect,
    icon: (&str, f32, f32),
    act: Action,
    enabled: bool,
    selected: bool,
) {
    let skin = if !enabled {
        44.
    } else if selected {
        22.
    } else if r.contains(p.mouse) {
        66.
    } else {
        0.
    };
    p.image(
        "textures/gui/container/beacon.png",
        r,
        [skin, 219., 22., 22.],
        WHITE,
    );
    p.image(
        icon.0,
        Rect {
            x: r.x + 2.,
            y: r.y + 2.,
            w: 18.,
            h: 18.,
        },
        [icon.1, icon.2, 18., 18.],
        WHITE,
    );
    p.frame.buttons.push(Button {
        rect: r,
        action: act,
        enabled,
    });
}
pub(super) fn draw(
    p: &mut Painter<'_>,
    gui: &Gui,
    sim: &Simulation,
    c: &crate::container::Container,
) -> bool {
    let [w, h] = p.frame.size.map(f32::ceil);
    let beacon = matches!(gui.special, Special::Beacon { .. });
    let [width, height] = if beacon { [230., 219.] } else { [176., 166.] };
    let x = ((w - width) / 2.).trunc();
    let y = ((h - height) / 2.).trunc();
    let tex = match gui.special {
        Special::Anvil { .. } => "textures/gui/container/anvil.png",
        Special::Beacon { .. } => "textures/gui/container/beacon.png",
        Special::None => return false,
    };
    main_menu::gradient(
        p,
        Rect { x: 0., y: 0., w, h },
        [16. / 255., 16. / 255., 16. / 255., 192. / 255.],
        [16. / 255., 16. / 255., 16. / 255., 208. / 255.],
    );
    p.image(
        tex,
        Rect {
            x,
            y,
            w: width,
            h: height,
        },
        [0., 0., width, height],
        WHITE,
    );
    let at = |x1: f32, y1: f32, w: f32, h: f32| Rect {
        x: x + x1,
        y: y + y1,
        w,
        h,
    };
    match &gui.special {
        Special::Anvil {
            slot,
            field,
            cost,
            focused,
            ..
        } => {
            p.text_pass(
                x + 60.,
                y + 6.,
                &p.tr("container.repair"),
                [0.25, 0.25, 0.25, 1.],
            );
            p.image(
                tex,
                at(59., 20., 110., 16.),
                [0., if slot.is_some() { 166. } else { 182. }, 110., 16.],
                WHITE,
            );
            if slot.is_some() {
                field.paint(
                    p,
                    at(62., 24., 103., 12.),
                    *focused,
                    0.,
                    30,
                    false,
                    Action::AnvilCursor,
                );
            }
            if (slot.is_some() || c.slots[1].is_some()) && c.slots[2].is_none() {
                p.image(tex, at(99., 45., 28., 21.), [176., 0., 28., 21.], WHITE);
            }
            if *cost > 0 && (c.slots[2].is_some() || *cost >= 40) {
                let expensive = *cost >= 40 && sim.mode != Mode::Creative;
                let text = if expensive {
                    p.tr("container.repair.expensive")
                } else {
                    p.tr("container.repair.cost")
                        .replace("%1$s", &cost.to_string())
                        .replace("%s", &cost.to_string())
                };
                let color =
                    if expensive || sim.xp.level < i32::from(*cost) && sim.mode != Mode::Creative {
                        [1., 96. / 255., 96. / 255., 1.]
                    } else {
                        [128. / 255., 1., 32. / 255., 1.]
                    };
                let tx = x + 168. - p.width(&text);
                let ty = y + 67.;
                if p.s.menu.force_unicode || p.s.locale_unicode {
                    p.rect(
                        tx - 3.,
                        ty - 2.,
                        x + 169. - (tx - 3.),
                        12.,
                        [0., 0., 0., 1.],
                    );
                    p.rect(
                        tx - 2.,
                        ty - 1.,
                        x + 168. - (tx - 2.),
                        10.,
                        [59. / 255., 59. / 255., 59. / 255., 1.],
                    );
                } else {
                    let shade = [color[0] * 0.25, color[1] * 0.25, color[2] * 0.25, 1.];
                    for (dx, dy) in [(0., 1.), (1., 0.), (1., 1.)] {
                        p.text_pass(tx + dx, ty + dy, &text, shade);
                    }
                }
                p.text_pass(tx, ty, &text, color);
            }
        }
        Special::Beacon {
            level,
            primary,
            secondary,
            ..
        } => {
            p.center(
                x + 62.,
                y + 10.,
                &p.tr("tile.beacon.primary"),
                [0.88, 0.88, 0.88, 1.],
            );
            p.center(
                x + 169.,
                y + 10.,
                &p.tr("tile.beacon.secondary"),
                [0.88, 0.88, 0.88, 1.],
            );
            for (i, id) in [388, 264, 266, 265].into_iter().enumerate() {
                p.item(Stack::new(id, 1), x + 42. + 22. * i as f32, y + 109., false);
            }
            let power = |p: &mut Painter<'_>, xx: f32, yy: f32, id: i16, tier: i16| {
                let icon = match id {
                    1 => 0,
                    3 => 2,
                    8 => 10,
                    11 => 6,
                    5 => 4,
                    10 => 7,
                    _ => 0,
                };
                let idx = if tier == 3 { *secondary } else { *primary };
                button(
                    p,
                    at(xx, yy, 22., 22.),
                    (
                        "textures/gui/container/inventory.png",
                        (icon % 8) as f32 * 18.,
                        198. + (icon / 8) as f32 * 18.,
                    ),
                    Action::BeaconSelect(tier == 3, id),
                    tier < *level,
                    id == idx,
                );
            };
            for (tier, ids) in [&[1i16, 3][..], &[11, 8], &[5]].into_iter().enumerate() {
                let len = (ids.len() * 24 - 2) as f32;
                for (i, &id) in ids.iter().enumerate() {
                    power(
                        p,
                        76. + 24. * i as f32 - (len / 2.).floor(),
                        22. + tier as f32 * 25.,
                        id,
                        tier as i16,
                    );
                }
            }
            power(p, 144., 47., 10, 3);
            if *primary > 0 {
                power(p, 168., 47., *primary, 3);
            }
            button(
                p,
                at(164., 107., 22., 22.),
                (tex, 90., 220.),
                Action::BeaconDone,
                *primary > 0 && c.slots[0].is_some(),
                false,
            );
            button(
                p,
                at(190., 107., 22., 22.),
                (tex, 112., 220.),
                Action::Screen(Screen::Playing),
                true,
                false,
            );
        }
        Special::None => unreachable!(),
    }
    for (i, s) in c.slots.iter().enumerate() {
        let [xx, yy] = if beacon {
            [136., 110.]
        } else {
            [[27., 47.], [76., 47.], [134., 47.]][i]
        };
        let r = at(xx, yy, 16., 16.);
        if let Some(s) = s {
            p.item(s.clone(), r.x, r.y, true)
        }
        if r.contains(p.mouse) {
            p.rect(r.x, r.y, 16., 16., [1., 1., 1., 0.3]);
        }
        p.frame.buttons.push(Button {
            rect: r,
            enabled: true,
            action: Action::ContainerSlot(i),
        });
    }
    let [xx, yy] = if beacon { [36., 137.] } else { [8., 84.] };
    for (i, s) in sim.inventory.slots.iter().take(36).enumerate() {
        p.slot(
            i,
            s.clone(),
            x + xx + (i % 9) as f32 * 18.,
            y + yy + if i < 9 { 58. } else { (i / 9 - 1) as f32 * 18. },
        );
    }
    if let Some(s) = sim.inventory.cursor.clone() {
        p.item(s, p.mouse[0].floor() - 8., p.mouse[1].floor() - 8., true);
    } else if let Some(item) = p.frame.buttons.iter().find_map(|b| {
        if !b.rect.contains(p.mouse) {
            return None;
        }
        match b.action {
            Action::Slot(i) => sim.inventory.slots.get(i)?.as_ref(),
            Action::ContainerSlot(i) => c.slots.get(i)?.as_ref(),
            _ => None,
        }
    }) {
        let lines = creative::item_tip(p, item, false, gui.advanced);
        creative::tooltip(p, &lines);
    }
    true
}
