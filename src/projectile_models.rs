//! RenderArrow, RenderSnowball and RenderFish geometry for native projectiles.
use super::*;
use crate::{gameplay::projectiles::Payload, vanilla::math};
#[cfg(test)]
#[path = "projectile_render_tests.rs"]
mod tests;

fn facing(c: &ModelContext<'_>) -> Mat4 {
    let yaw = c.player.java_rotation()[0] + if c.perspective == 2 { 180. } else { 0. };
    let pitch = c.player.java_rotation()[1];
    Mat4::from_rotation_y(-yaw.to_radians()) * Mat4::from_rotation_x(pitch.to_radians())
}

fn billboard_rotation([mut yaw, mut pitch]: [f32; 2], perspective: u8) -> Mat4 {
    if perspective == 2 {
        yaw += 180.;
        pitch = -pitch;
    }
    Mat4::from_rotation_y((180. - yaw).to_radians()) * Mat4::from_rotation_x((-pitch).to_radians())
}

fn billboard_facing(c: &ModelContext<'_>) -> Mat4 {
    billboard_rotation(c.player.java_rotation(), c.perspective)
}

#[allow(clippy::too_many_arguments)]
fn arrow_pose(
    mesh: &mut Mesh,
    c: &ModelContext<'_>,
    pos: Vec3,
    previous_yaw: f32,
    yaw: f32,
    previous_pitch: f32,
    pitch: f32,
    shake_ticks: u8,
) {
    let mut dy = yaw - previous_yaw;
    while dy < -180. {
        dy += 360.;
    }
    while dy >= 180. {
        dy -= 360.;
    }
    let yaw = previous_yaw + dy * c.alpha;
    let pitch = previous_pitch + (pitch - previous_pitch) * c.alpha;
    let shake = (f32::from(shake_ticks) - c.alpha).max(0.);
    let shake_angle = -math::sin(shake * 3.) * shake;
    let transform = Mat4::from_translation(pos)
        * Mat4::from_rotation_y((yaw - 90.).to_radians())
        * Mat4::from_rotation_z((pitch + shake_angle).to_radians())
        * Mat4::from_rotation_x(45_f32.to_radians())
        * Mat4::from_scale(Vec3::splat(0.05625))
        * Mat4::from_translation(Vec3::new(-4., 0., 0.));
    let sprite = c.resources.sprite("textures/entity/arrow.png");
    let start = mesh.vertices.len();
    let layer = sprite.layer | ATLAS;
    let width = sprite.width as f32 / 256.;
    let height = sprite.height as f32 / 256.;
    for sign in [-1., 1.] {
        let quad = [
            Vec3::new(-7., -2. * sign, -2.),
            Vec3::new(-7., -2. * sign, 2.),
            Vec3::new(-7., 2. * sign, 2.),
            Vec3::new(-7., 2. * sign, -2.),
        ];
        face(
            mesh,
            quad.map(|v| transform.transform_point3(v)),
            [0., height * 0.3125, width * 0.15625, -height * 0.15625],
            layer,
            [255; 4],
        );
    }
    for turn in 1..=4 {
        let rotation = transform * Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2 * turn as f32);
        let quad = [
            Vec3::new(-8., -2., 0.),
            Vec3::new(8., -2., 0.),
            Vec3::new(8., 2., 0.),
            Vec3::new(-8., 2., 0.),
        ];
        face(
            mesh,
            quad.map(|v| rotation.transform_point3(v)),
            [0., height * 0.15625, width * 0.5, -height * 0.15625],
            layer,
            [255; 4],
        );
    }
    light_since(mesh, start, c.world, pos);
}

fn arrow(
    mesh: &mut Mesh,
    c: &ModelContext<'_>,
    arrow: &crate::gameplay::projectiles::Projectile,
    pos: Vec3,
) {
    arrow_pose(
        mesh,
        c,
        pos,
        arrow.previous_yaw,
        arrow.yaw,
        arrow.previous_pitch,
        arrow.pitch,
        arrow.shake,
    );
}

fn thrown_stack(mesh: &mut Mesh, c: &ModelContext<'_>, pos: Vec3, stack: &Stack, bright: bool) {
    let start = mesh.vertices.len();
    item_models::projectile(
        mesh,
        Mat4::from_translation(pos) * facing(c),
        stack,
        c.assets,
        c.resources,
    );
    light_since(mesh, start, c.world, pos);
    if bright {
        for vertex in &mut mesh.vertices[start..] {
            vertex.light[..2].copy_from_slice(&[240, 240]);
        }
    }
}

fn thrown_item(mesh: &mut Mesh, c: &ModelContext<'_>, pos: Vec3, id: u16, bright: bool) {
    thrown_stack(mesh, c, pos, &Stack::new(id, 1), bright);
}

fn rod_tip(c: &ModelContext<'_>) -> Vec3 {
    let position = c.player.previous.lerp(c.player.position, c.alpha);
    let [yaw, pitch] = c.player.java_rotation().map(f32::to_radians);
    if c.perspective != 0 {
        let body = c.player.third_person.frame(c.alpha).body.to_radians();
        return position
            + Vec3::new(
                -math::cos(body) * 0.35 - math::sin(body) * 0.8,
                c.player.eye_height() - 0.45 - if c.player.sneaking() { 0.1875 } else { 0. },
                -math::sin(body) * 0.35 + math::cos(body) * 0.8,
            );
    }
    let swing = math::sin(c.sim.hand_swing.sample(c.alpha).sqrt() * std::f32::consts::PI);
    // Vec3.rotatePitch/rotateYaw use the original row-vector sign convention.
    let turn = Mat4::from_rotation_x(swing * 0.7)
        * Mat4::from_rotation_y(swing * 0.5)
        * Mat4::from_rotation_y(-yaw)
        * Mat4::from_rotation_x(pitch);
    position
        + Vec3::Y * c.player.eye_height()
        + turn.transform_vector3(Vec3::new(-0.36, 0.03, 0.35))
}

fn remote_angler_tip(a: &crate::models::server::Actor, alpha: f32) -> Vec3 {
    let position = a.previous.lerp(a.feet, alpha);
    let body = a.anim.frame(alpha).body.to_radians();
    let eye = if a.sneaking { 1.62 - 0.08 } else { 1.62 };
    position
        + Vec3::new(
            -math::cos(body) * 0.35 - math::sin(body) * 0.8,
            eye - 0.45 - if a.sneaking { 0.1875 } else { 0. },
            -math::sin(body) * 0.35 + math::cos(body) * 0.8,
        )
}

fn fishing_with_tip(mesh: &mut Mesh, c: &ModelContext<'_>, pos: Vec3, tip: Option<Vec3>) {
    let sprite = c.resources.sprite("textures/particle/particles.png");
    let rot = facing(c) * Mat4::from_rotation_y(std::f32::consts::PI);
    let start = mesh.vertices.len();
    let quad = [
        Vec3::new(-0.25, -0.25, 0.),
        Vec3::new(0.25, -0.25, 0.),
        Vec3::new(0.25, 0.25, 0.),
        Vec3::new(-0.25, 0.25, 0.),
    ]
    .map(|p| pos + rot.transform_vector3(p));
    face(
        mesh,
        quad,
        [8. / 256., 16. / 256., 8. / 256., 8. / 256.],
        sprite.layer | ATLAS,
        [255; 4],
    );
    light_since(mesh, start, c.world, pos);
    let Some(tip) = tip else {
        return;
    };
    let from = pos + Vec3::Y * 0.25;
    let delta = tip - from;
    let mut previous = from;
    for step in 1..=16 {
        let t = step as f32 / 16.;
        let next = from + Vec3::new(delta.x * t, delta.y * (t * t + t) * 0.5, delta.z * t);
        if let Some(quad) = c.frustum.pixel_line(previous, next, c.viewport_height) {
            face(mesh, quad, [0.; 4], SOLID | UNLIT, [0, 0, 0, 255]);
        }
        previous = next;
    }
}

fn fishing(mesh: &mut Mesh, c: &ModelContext<'_>, pos: Vec3) {
    fishing_with_tip(mesh, c, pos, Some(rod_tip(c)));
}

pub(super) fn draw(d: &mut Dynamic, c: &ModelContext<'_>) {
    for projectile in &c.sim.projectiles {
        d.total += 1;
        let pos = projectile
            .previous
            .lerp(projectile.position, c.alpha as f64)
            .as_vec3();
        if !c.frustum.visible(pos - Vec3::ONE, pos + Vec3::ONE) {
            continue;
        }
        d.visible += 1;
        match projectile.payload {
            Payload::Arrow(_) => arrow(&mut d.opaque, c, projectile, pos),
            Payload::Snowball => thrown_item(&mut d.opaque, c, pos, 332, false),
            Payload::Egg => thrown_item(&mut d.opaque, c, pos, 344, false),
            Payload::Pearl => thrown_item(&mut d.opaque, c, pos, 368, false),
            Payload::ExperienceBottle => thrown_item(&mut d.opaque, c, pos, 384, false),
        }
    }
    for eye in &c.sim.fishing.eyes {
        d.total += 1;
        let pos = eye.previous.lerp(eye.position, c.alpha as f64).as_vec3();
        if !c.frustum.visible(pos - Vec3::ONE, pos + Vec3::ONE) {
            continue;
        }
        d.visible += 1;
        thrown_item(&mut d.opaque, c, pos, 381, true);
    }
    if let Some(hook) = &c.sim.fishing.hook {
        d.total += 1;
        d.visible += 1;
        fishing(
            &mut d.opaque,
            c,
            hook.previous.lerp(hook.position, c.alpha as f64).as_vec3(),
        );
    }
    for proj in &c.sim.server_scene.projectiles {
        d.total += 1;
        let pos = proj.previous.lerp(proj.position, c.alpha);
        if proj.kind != 90 && !c.frustum.visible(pos - Vec3::splat(2.), pos + Vec3::splat(2.)) {
            d.culled += 1;
            continue;
        }
        d.visible += 1;
        match proj.kind {
            51 => {
                if pos.distance_squared(c.eye) <= (c.settings.entity_distance as f32).powi(2) {
                    draw_end_crystal(d, c, proj.id as u64, pos, !proj.invulnerable, None);
                    if c.hitboxes {
                        wirebox(
                            &mut d.opaque,
                            pos - Vec3::new(1.0, 0.0, 1.0),
                            pos + Vec3::new(1.0, 2.0, 1.0),
                        );
                    }
                }
            }
            60 => {
                let start = d.opaque.vertices.len();
                arrow_pose(
                    &mut d.opaque,
                    c,
                    pos,
                    proj.previous_yaw,
                    proj.yaw,
                    proj.previous_pitch,
                    proj.pitch,
                    0,
                );
                if c.sim.server_scene.burning.contains(&proj.id) {
                    c.fire(&mut d.opaque, start, pos, 0.5, 0.5);
                }
            }
            61 => thrown_item(&mut d.opaque, c, pos, 332, false),
            62 => thrown_item(&mut d.opaque, c, pos, 344, false),
            63 | 64 => {
                let side = if proj.kind == 63 { 2.0 } else { 0.5 };
                let sprite = c.resources.sprite("textures/items/fireball.png");
                let rot = billboard_facing(c);
                let start = d.opaque.vertices.len();
                let q = [
                    Vec3::new(-0.5, -0.25, 0.),
                    Vec3::new(0.5, -0.25, 0.),
                    Vec3::new(0.5, 0.75, 0.),
                    Vec3::new(-0.5, 0.75, 0.),
                ]
                .map(|v| pos + rot.transform_vector3(v * side));
                face(
                    &mut d.opaque,
                    q,
                    [0., 0., sprite.width as f32 / 256., sprite.height as f32 / 256.],
                    sprite.layer | ATLAS,
                    [255; 4],
                );
                let hit = if proj.kind == 63 { 1.0 } else { 0.3125 };
                c.fire(&mut d.opaque, start, pos, hit, hit);
            }
            65 => thrown_item(&mut d.opaque, c, pos, 368, false),
            66 => {
                let tex = if proj.invulnerable {
                    "textures/entity/wither/wither_invulnerable.png"
                } else {
                    "textures/entity/wither/wither.png"
                };
                let sprite = c.resources.sprite(tex);
                let mut dy = proj.yaw - proj.previous_yaw;
                while dy < -180. {
                    dy += 360.;
                }
                while dy >= 180. {
                    dy -= 360.;
                }
                let yaw = proj.previous_yaw + dy * c.alpha;
                let pitch = proj.previous_pitch + (proj.pitch - proj.previous_pitch) * c.alpha;
                let transform = Mat4::from_translation(pos + Vec3::Y * 0.25)
                    * Mat4::from_rotation_y((180. - yaw).to_radians())
                    * Mat4::from_rotation_x((-pitch).to_radians());
                let start = d.opaque.vertices.len();
                cuboid(
                    &mut d.opaque,
                    transform,
                    Vec3::splat(-0.25),
                    Vec3::splat(0.5),
                    [0., 0.],
                    sprite.layer | ATLAS,
                    [255; 4],
                );
                for v in &mut d.opaque.vertices[start..] {
                    v.light = [240, 240, 0, 0];
                }
            }
            70 => {
                let state = (((proj.data & 0xFFF) << 4) | ((proj.data >> 12) & 0xF)) as u16;
                if state >> 4 != 0 {
                    draw_falling_block(&mut d.opaque, c.world, c.assets, pos, state);
                    shadow_radius(&mut d.translucent, c, pos, 0.5, 1.0, false);
                    if c.hitboxes {
                        wirebox(
                            &mut d.opaque,
                            pos - Vec3::new(0.49, 0.0, 0.49),
                            pos + Vec3::new(0.49, 0.98, 0.49),
                        );
                    }
                }
            }
            72 => thrown_item(&mut d.opaque, c, pos, 381, true),
            73 => {
                let potion = Stack {
                    id: 373,
                    meta: (proj.data as u16) | 0x4000,
                    count: 1,
                    damage: 0,
                    nbt: None,
                    identity: 0,
                };
                thrown_stack(&mut d.opaque, c, pos, &potion, false);
            }
            75 => thrown_item(&mut d.opaque, c, pos, 384, false),
            76 => thrown_item(&mut d.opaque, c, pos, 401, false),
            90 => {
                let tip = if proj.is_local_owner {
                    Some(rod_tip(c))
                } else {
                    c.sim
                        .server_scene
                        .actors
                        .get(&proj.data)
                        .map(|a| remote_angler_tip(a, c.alpha))
                };
                fishing_with_tip(&mut d.opaque, c, pos, tip);
            }
            _ => {}
        }
    }
}
