//! Chest tile-entity geometry, lid movement and skins in 1.8.9.
use crate::{
    mesh::{Frustum, Mesh},
    model_box::{self, Texture},
    resources::Resources,
    world::World,
};
use glam::{IVec3, Mat4, Vec3};
use std::collections::HashMap;

pub(crate) fn is_chest(state: u16) -> bool {
    matches!(state >> 4, 54 | 130 | 146)
}

static FESTIVE: std::sync::LazyLock<bool> =
    std::sync::LazyLock::new(|| matches!(crate::ui::local_date(), (12, 24..=26)));

#[derive(Debug, Clone, Copy)]
struct Chest {
    anchor: IVec3,
    second: Option<IVec3>,
    id: u16,
    facing: u16,
}
impl Chest {
    fn at(world: &World, pos: IVec3) -> Option<Self> {
        let state = world.get(pos);
        if !is_chest(state) {
            return None;
        }
        let id = state >> 4;
        let other = if id == 130 {
            None
        } else {
            [IVec3::NEG_Z, IVec3::NEG_X, IVec3::Z, IVec3::X]
                .into_iter()
                .map(|step| pos + step)
                .find(|&p| world.get(p) >> 4 == id)
        };
        let anchor = other.map_or(pos, |p| pos.min(p));
        let second = other.map(|p| pos.max(p));
        let mut facing = world.get(anchor) & 7;
        if let Some(second) = second {
            // Also handle old local saves with a facing parallel to the join.
            // BlockChest.checkForSurroundingChests chooses the perpendicular
            // axis using the other half's facing and walls alongside both.
            let across_x = anchor.x != second.x;
            if !(if across_x { 2..=3 } else { 4..=5 }).contains(&facing) {
                facing =
                    crate::vanilla::facing_events::chest_facing(anchor, world.get(anchor), |q| {
                        world.get(q)
                    });
            }
        }
        Some(Self {
            anchor,
            second,
            id,
            facing,
        })
    }

    fn texture(self, festive: bool) -> &'static str {
        match (self.id, festive, self.second.is_some()) {
            (130, _, _) => "textures/entity/chest/ender.png",
            (_, true, true) => "textures/entity/chest/christmas_double.png",
            (_, true, false) => "textures/entity/chest/christmas.png",
            (146, _, true) => "textures/entity/chest/trapped_double.png",
            (146, _, false) => "textures/entity/chest/trapped.png",
            (_, _, true) => "textures/entity/chest/normal_double.png",
            (_, _, false) => "textures/entity/chest/normal.png",
        }
    }

    fn draw_transformed(
        self,
        mesh: &mut Mesh,
        base: Mat4,
        angle: f32,
        texture: Texture,
        light: [u8; 4],
    ) {
        let width = if self.second.is_some() { 30. } else { 14. };
        let degrees: f32 = match self.facing {
            2 => 180.,
            4 => 90.,
            5 => -90.,
            _ => 0.,
        };
        let offset = match (self.second, self.facing) {
            (Some(_), 2) => Vec3::X,
            (Some(_), 5) => Vec3::NEG_Z,
            _ => Vec3::ZERO,
        };
        let transform = base
            * Mat4::from_translation(Vec3::new(0., 1., 1.))
            * Mat4::from_scale(Vec3::new(1., -1., -1.))
            * Mat4::from_translation(Vec3::splat(0.5) + offset)
            * Mat4::from_rotation_y(degrees.to_radians())
            * Mat4::from_translation(Vec3::splat(-0.5))
            * Mat4::from_scale(Vec3::splat(1. / 16.));
        let hinge = Mat4::from_rotation_x(-angle * std::f32::consts::FRAC_PI_2);
        for (part, min, size, uv) in [
            (
                Mat4::from_translation(Vec3::new(1., 7., 15.)) * hinge,
                [0., -5., -14.],
                [width, 5., 14.],
                [0., 0.],
            ),
            (
                Mat4::from_translation(Vec3::new(1. + width / 2., 7., 15.)) * hinge,
                [-1., -2., -15.],
                [2., 4., 1.],
                [0., 0.],
            ),
            (
                Mat4::from_translation(Vec3::new(1., 6., 1.)),
                [0.; 3],
                [width, 10., 14.],
                [0., 19.],
            ),
        ] {
            model_box::draw(mesh, transform * part, min, size, uv, texture, light);
        }
    }

    fn draw(self, mesh: &mut Mesh, angle: f32, texture: Texture, light: [u8; 4]) {
        self.draw_transformed(
            mesh,
            Mat4::from_translation(self.anchor.as_vec3()),
            angle,
            texture,
            light,
        );
    }
}

pub(crate) fn draw_cargo_chest(
    mesh: &mut Mesh,
    transform: Mat4,
    state: u16,
    resources: &Resources,
    light: [u8; 4],
) {
    if !is_chest(state) {
        return;
    }
    let chest = Chest {
        anchor: IVec3::ZERO,
        second: None,
        id: state >> 4,
        facing: 0,
    };
    let sprite = resources.sprite(chest.texture(*FESTIVE));
    let texture = Texture {
        layer: sprite.layer | crate::models::ATLAS,
        origin: [0.; 2],
        scale: [
            sprite.width as f32 / (64. * 256.),
            sprite.height as f32 / (64. * 256.),
        ],
    };
    chest.draw_transformed(mesh, transform, 0., texture, light);
}

#[derive(Default, Clone, Debug)]
pub(crate) struct Lids {
    /// Only chests in use or still moving need tick state.
    state: HashMap<IVec3, Lid>,
}
#[derive(Default, Clone, Debug)]
struct Lid {
    users: u8,
    previous: f32,
    current: f32,
}
impl Lids {
    pub(crate) fn server_users(&mut self, p: IVec3, users: u8) {
        if users > 0 || self.state.contains_key(&p) {
            self.state.entry(p).or_default().users = users;
        }
    }
    pub(crate) fn tick(&mut self, world: &World, local: Option<IVec3>) {
        let local = local.and_then(|p| Chest::at(world, p)).map(|c| c.anchor);
        if let Some(p) = local {
            self.state.entry(p).or_default();
        }
        self.state.retain(|p, lid| {
            if !is_chest(world.get(*p)) {
                return false;
            }
            lid.previous = lid.current;
            let active = Some(*p) == local || lid.users > 0;
            lid.current = (lid.current + if active { 0.1 } else { -0.1 }).clamp(0., 1.);
            active || lid.previous > 0.
        });
    }
    fn interpolated(&self, chest: Chest, alpha: f32) -> f32 {
        let value = std::iter::once(chest.anchor)
            .chain(chest.second)
            .filter_map(|p| self.state.get(&p))
            .map(|v| v.previous + (v.current - v.previous) * alpha)
            .fold(0., f32::max);
        let closed = 1. - value;
        1. - closed * closed * closed
    }
    pub(crate) fn angle(&self, world: &World, pos: IVec3, alpha: f32) -> f32 {
        if self.state.is_empty() {
            return 0.;
        }
        Chest::at(world, pos).map_or(0., |c| self.interpolated(c, alpha))
    }
}

pub(crate) fn visible(
    mesh: &mut Mesh,
    world: &World,
    lids: &Lids,
    positions: impl Iterator<Item = IVec3>,
    resources: &Resources,
    eye: Vec3,
    frustum: &Frustum,
    alpha: f32,
) {
    for p in positions {
        let at = p.as_vec3();
        if eye.distance_squared(at + Vec3::splat(0.5)) >= 4096. {
            continue;
        }
        let Some(chest) = Chest::at(world, p) else {
            continue;
        };
        let max = chest.second.unwrap_or(p).as_vec3() + Vec3::new(1., 1.875, 1.);
        if p != chest.anchor || !frustum.visible(at, max) {
            continue;
        }
        let sprite = resources.sprite(chest.texture(*FESTIVE));
        let width = if chest.second.is_some() { 128. } else { 64. };
        let texture = Texture {
            layer: sprite.layer | crate::models::ATLAS,
            origin: [0.; 2],
            scale: [
                sprite.width as f32 / (width * 256.),
                sprite.height as f32 / (64. * 256.),
            ],
        };
        chest.draw(
            mesh,
            lids.interpolated(chest, alpha),
            texture,
            world.visual_light_uv(p, 0),
        );
    }
}

pub(crate) fn damage(mesh: &mut Mesh, world: &World, pos: IVec3, layer: u32, angle: f32) {
    if let Some(chest) = Chest::at(world, pos) {
        let texture = Texture {
            layer,
            origin: [if chest.second.is_some() { 0.5 } else { 0.25 }, 0.25],
            scale: [1. / 16.; 2],
        };
        chest.draw(mesh, angle, texture, world.visual_light_uv(chest.anchor, 0));
    }
}

#[cfg(test)]
#[path = "chest_render_tests.rs"]
mod tests;
