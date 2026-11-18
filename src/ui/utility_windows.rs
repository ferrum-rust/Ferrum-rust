use super::*;
use crate::container::{Container, Special};
use glam::{Mat4, Vec3};

pub(crate) fn capture(name: &str, c: &mut Container, sim: &mut Simulation) -> bool {
    let (kind, count) = match name {
        "dispenser" | "dropper" => (Special::Dispenser, 9),
        "brewing" => (Special::Brewing, 4),
        "hopper" => (Special::Hopper, 5),
        "horse-inventory" => (Special::Horse { armor: true }, 2),
        "donkey-inventory" => (Special::Horse { armor: false }, 17),
        _ => return false,
    };
    c.special = kind;
    c.slots = vec![None; count];
    sim.inventory.slots.fill(None);
    if kind == Special::Brewing {
        c.brew_time = 200;
    }
    if let Special::Horse { armor } = kind {
        c.horse_id = Some(123);
        sim.server_scene.mobs.insert(
            123,
            crate::remote_mobs::Actor {
                species: 100,
                previous: Vec3::ZERO,
                feet: Vec3::ZERO,
                yaw: 0.,
                gait: Default::default(),
                appearance: crate::remote_mobs::Appearance {
                    subtype: u8::from(!armor),
                    ..Default::default()
                },
                hurt: false,
                ..Default::default()
            },
        );
    }
    true
}

pub(crate) fn height(kind: Special) -> Option<f32> {
    match kind {
        Special::Hopper => Some(133.),
        Special::Dispenser | Special::Brewing | Special::Horse { .. } => Some(166.),
        _ => None,
    }
}

fn slot_position(kind: Special, i: usize) -> Option<[usize; 2]> {
    match kind {
        Special::Brewing => [[56, 46], [79, 53], [102, 46], [79, 17]].get(i).copied(),
        Special::Dispenser if i < 9 => Some([62 + i % 3 * 18, 17 + i / 3 * 18]),
        Special::Hopper if i < 5 => Some([44 + i * 18, 20]),
        Special::Horse { armor } => match i {
            0 => Some([8, 18]),
            1 if armor => Some([8, 36]),
            2..=16 => Some([80 + (i - 2) % 5 * 18, 18 + (i - 2) / 5 * 18]),
            _ => None,
        },
        _ => None,
    }
}

fn brew_overlays(ticks: i16) -> (usize, usize) {
    if ticks <= 0 {
        return (0, 0);
    }
    let arrow = (28. * (1. - f32::from(ticks) / 400.)).max(0.) as usize;
    let bubbles = [29, 24, 20, 16, 11, 6, 0][usize::from(ticks as u16 / 2 % 7)];
    (arrow, bubbles)
}
fn player_slot(i: usize, height: f32) -> [f32; 2] {
    let dy = if i < 9 {
        height - 24.
    } else {
        height - 82. + 18. * ((i - 9) / 9) as f32
    };
    [8. + 18. * (i % 9) as f32, dy]
}

pub(super) fn draw(p: &mut Painter<'_>, gui: &Gui, sim: &Simulation, c: &Container) -> bool {
    let Some(height) = height(c.special) else {
        return false;
    };
    let (texture, title, center) = match c.special {
        Special::Dispenser => (
            "textures/gui/container/dispenser.png",
            "container.dispenser",
            true,
        ),
        Special::Brewing => (
            "textures/gui/container/brewing_stand.png",
            "container.brewing",
            true,
        ),
        Special::Hopper => (
            "textures/gui/container/hopper.png",
            "container.hopper",
            false,
        ),
        Special::Horse { .. } => (
            "textures/gui/container/horse.png",
            "entity.horse.name",
            false,
        ),
        _ => unreachable!(),
    };
    let [width, screen_h] = p.frame.size.map(f32::ceil);
    let x = ((width - 176.) / 2.).floor();
    let y = ((screen_h - height) / 2.).floor();
    let rect = |dx, dy, w, h| Rect {
        x: x + dx,
        y: y + dy,
        w,
        h,
    };
    main_menu::gradient(
        p,
        Rect {
            x: 0.,
            y: 0.,
            w: width,
            h: screen_h,
        },
        [16. / 255., 16. / 255., 16. / 255., 192. / 255.],
        [16. / 255., 16. / 255., 16. / 255., 208. / 255.],
    );
    p.image(
        texture,
        rect(0., 0., 176., height),
        [0., 0., 176., height],
        WHITE,
    );
    if c.special == Special::Brewing {
        let (arrow, bubbles) = brew_overlays(c.brew_time);
        if arrow > 0 {
            let h = arrow as f32;
            p.image(texture, rect(97., 16., 9., h), [176., 0., 9., h], WHITE);
        }
        if bubbles > 0 {
            let h = bubbles as f32;
            p.image(
                texture,
                rect(65., 43. - h, 12., h),
                [185., 29. - h, 12., h],
                WHITE,
            );
        }
    }
    let mut custom_horse_title = None;
    let mut horse_subtype = 0u8;
    if let Special::Horse { armor } = c.special {
        if c.slots.len() > 2 {
            p.image(
                texture,
                rect(79., 17., 90., 54.),
                [0., 166., 90., 54.],
                WHITE,
            );
        }
        if armor {
            p.image(
                texture,
                rect(7., 35., 18., 18.),
                [0., 220., 18., 18.],
                WHITE,
            );
        }
        let horse = c
            .horse_id
            .and_then(|id| sim.server_scene.mobs.get(&id))
            .filter(|m| m.species == 100)
            .map(|m| (m.appearance, m.horse_armor))
            .or_else(|| {
                let id = sim.horse_window?;
                let mob = sim.mobs.iter().find(|m| m.id == id)?;
                if let Some(crate::vanilla::drops::DropNbt::String(name)) = sim
                    .command_entities
                    .mobs
                    .get(&id)
                    .and_then(|d| d.extra.get("CustomName"))
                {
                    if !name.is_empty() {
                        custom_horse_title = Some(name.clone());
                    }
                }
                let armor_id = sim
                    .command_entities
                    .mobs
                    .get(&id)
                    .and_then(|d| d.extra.get("ArmorItem"))
                    .and_then(crate::gameplay::local_commands::read_stack)
                    .map_or(0, |s| match s.id {
                        417 => 1,
                        418 => 2,
                        419 => 3,
                        _ => 0,
                    });
                Some((crate::remote_mobs::Appearance::local(sim, mob), armor_id))
            });
        if let Some((appearance, fallback_armor)) = horse {
            horse_subtype = appearance.subtype;
            horse_preview(p, x + 51., y + 60., appearance, fallback_armor, c);
        }
    }
    let default_title = if matches!(c.special, Special::Horse { .. }) {
        match horse_subtype {
            1 => "entity.donkey.name",
            2 => "entity.mule.name",
            3 => "entity.zombiehorse.name",
            4 => "entity.skeletonhorse.name",
            _ => title,
        }
    } else {
        title
    };
    let title = gui
        .container_title
        .clone()
        .or(custom_horse_title)
        .unwrap_or_else(|| p.tr(default_title));
    let label = [64. / 255., 64. / 255., 64. / 255., 1.];
    p.text_pass(
        x + if center {
            88. - (p.width(&title) / 2.).floor()
        } else {
            8.
        },
        y + 6.,
        &title,
        label,
    );
    p.text_pass(
        x + 8.,
        y + height - 94.,
        &p.tr("container.inventory"),
        label,
    );
    let mut hovered = None;
    for (i, s) in c.slots.iter().enumerate() {
        if let Some([dx, dy]) = slot_position(c.special, i) {
            creative::cell(
                p,
                x + dx as f32,
                y + dy as f32,
                s.as_ref(),
                Action::ContainerSlot(i),
                &mut hovered,
            );
        }
    }
    for i in (9..36).chain(0..9) {
        let [dx, dy] = player_slot(i, height);
        creative::cell(
            p,
            x + dx,
            y + dy,
            sim.inventory.slots[i].as_ref(),
            Action::Slot(i),
            &mut hovered,
        );
    }
    if let Some(cursor) = &sim.inventory.cursor {
        p.item(
            cursor.clone(),
            p.mouse[0].floor() - 8.,
            p.mouse[1].floor() - 8.,
            true,
        );
    } else if let Some(s) = hovered {
        let tip = creative::item_tip(p, &s, false, gui.advanced);
        creative::tooltip(p, &tip);
    }
    true
}

type HorseModels = std::collections::HashMap<u8, Vec<(bool, [i32; 15])>>;
static HORSES: std::sync::LazyLock<HorseModels> = std::sync::LazyLock::new(|| {
    serde_json::from_reader(flate2::read::GzDecoder::new(
        &include_bytes!("horse_inventory_models.json.gz")[..],
    ))
    .expect("original horse inventory models")
});

fn horse_preview(
    p: &mut Painter<'_>,
    x: f32,
    y: f32,
    appearance: crate::remote_mobs::Appearance,
    fallback_armor: u8,
    c: &Container,
) {
    let yaw = ((x - p.mouse[0]) / 40.).atan() * 20f32.to_radians();
    let pitch = -((y - 35. - p.mouse[1]) / 40.).atan() * 20f32.to_radians();
    let breed = match appearance.subtype {
        1 | 2 => appearance.subtype,
        _ => 0,
    };
    let sprite = p.r.sprite(appearance.texture(100));
    let armor_tex = (appearance.subtype == 0).then(|| {
        if c.slots.len() >= 2 {
            c.slots
                .get(1)
                .and_then(Option::as_ref)
                .and_then(|s| match s.id {
                    417 => Some("textures/entity/horse/armor/horse_armor_iron.png"),
                    418 => Some("textures/entity/horse/armor/horse_armor_gold.png"),
                    419 => Some("textures/entity/horse/armor/horse_armor_diamond.png"),
                    _ => None,
                })
        } else {
            match fallback_armor {
                1 => Some("textures/entity/horse/armor/horse_armor_iron.png"),
                2 => Some("textures/entity/horse/armor/horse_armor_gold.png"),
                3 => Some("textures/entity/horse/armor/horse_armor_diamond.png"),
                _ => None,
            }
        }
    }).flatten();
    let layers = [
        Some(sprite),
        appearance.horse_markings().map(|texture| p.r.sprite(texture)),
        armor_tex.map(|texture| p.r.sprite(texture)),
    ];
    let foal = appearance.age < 0;
    let saddled = c
        .slots
        .first()
        .and_then(Option::as_ref)
        .is_some_and(|s| s.id == 329);
    let flags = if foal {
        8
    } else {
        u8::from(saddled)
            | u8::from(breed != 0 && c.slots.len() > 2) * 2
            | u8::from(saddled && c.horse_ridden) * 4
    };
    let body_scale = match breed {
        1 => 0.87,
        2 => 0.92,
        _ => 1.,
    };
    let pivot = if foal {
        Vec3::new(0., 1.5078125 - 0.625 * (0.675 + 0.25), 0.625 * 0.625)
    } else {
        Vec3::new(0., 1.2578125, 0.625)
    } * body_scale;
    let transform = Mat4::from_scale(Vec3::new(17., -17., 17.))
        * Mat4::from_rotation_x(pitch)
        * Mat4::from_rotation_y(-yaw);
    let head_transform = transform
        * Mat4::from_translation(pivot)
        * Mat4::from_rotation_y(-yaw.clamp(-20f32.to_radians(), 20f32.to_radians()))
        * Mat4::from_rotation_x(pitch)
        * Mat4::from_translation(-pivot);
    let mut triangles = Vec::new();
    for (head, tri) in &HORSES[&(breed * 16 + flags)] {
        let matrix = if *head { head_transform } else { transform };
        let pos: [Vec3; 3] = std::array::from_fn(|i| {
            let k = i * 5;
            matrix.transform_point3(
                Vec3::new(tri[k] as f32, tri[k + 1] as f32, tri[k + 2] as f32) / 2048.,
            )
        });
        let normal = (pos[1] - pos[0]).cross(pos[2] - pos[0]).normalize_or_zero();
        if normal.z >= 0. {
            continue;
        }
        let uv: [[f32; 2]; 3] = std::array::from_fn(|i| {
            [
                tri[i * 5 + 3] as f32 * sprite.width as f32 / (4096. * 256.),
                tri[i * 5 + 4] as f32 * sprite.height as f32 / (4096. * 256.),
            ]
        });
        triangles.push((pos.iter().map(|v| v.z).sum::<f32>(), pos, uv, -normal));
    }
    triangles.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, pos, uv, normal) in triangles {
        let light = creative_preview::entity_lighting(normal);
        let vertices = pos.map(|v| [x + v.x, y + v.y]);
        for layer in layers.iter().flatten() {
            p.polygon(
                [vertices[0], vertices[1], vertices[2], vertices[2]],
                [uv[0], uv[1], uv[2], uv[2]],
                layer.layer,
                [light, light, light, 1.],
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn utility_windows_use_original_slot_order_and_brewing_indicators() {
        let original: std::collections::HashMap<String, Vec<[usize; 3]>> = serde_json::from_str(
            include_str!("../../tests/fixtures/vanilla189-utility-slots.json"),
        )
        .unwrap();
        for (name, kind, n) in [
            ("dispenser", Special::Dispenser, 9),
            ("dropper", Special::Dispenser, 9),
            ("brewing", Special::Brewing, 4),
            ("hopper", Special::Hopper, 5),
            ("horse-inventory", Special::Horse { armor: true }, 2),
            ("donkey-inventory", Special::Horse { armor: false }, 17),
        ] {
            let mut actual: Vec<_> = (0..n)
                .filter_map(|i| slot_position(kind, i).map(|[x, y]| [i, x, y]))
                .collect();
            actual.extend((9..36).chain(0..9).enumerate().map(|(offset, i)| {
                let [x, y] = player_slot(i, height(kind).unwrap());
                [n + offset, x as usize, y as usize]
            }));
            assert_eq!(actual, original[name], "{name}");
        }
        assert_eq!(brew_overlays(0), (0, 0));
        assert_eq!(brew_overlays(400), (0, 11));
        assert_eq!(brew_overlays(200), (14, 20));
        assert_eq!(brew_overlays(1), (27, 29));
    }
}
