//! Per-tick limb state shared by native locomotion and network entity renderers.
use crate::{
    assets::Assets,
    mesh::Mesh,
    models::{self, BLEND, UNLIT},
};
use glam::{Mat4, Vec3};

pub(crate) const HURT: u32 = 1 << 28;
const FIRE_OVERLAY: u32 = 1 << 29;

#[derive(Clone, Copy, Debug, Default)]
pub struct Gait {
    pub previous_yaw: Option<f32>,
    pub previous_amount: f32,
    pub amount: f32,
    pub swing: f32,
}

impl Gait {
    /// Yaw is stored in radians; distance is the horizontal displacement this tick.
    pub fn advance(&mut self, yaw: f32, distance: f32) {
        self.previous_yaw = Some(yaw);
        self.previous_amount = self.amount;
        self.amount += ((distance * 4.).min(1.) - self.amount) * 0.4;
        self.swing += self.amount;
    }

    pub fn sample(self, yaw: f32, partial: f32, child: bool) -> (f32, f32, f32) {
        let previous = self.previous_yaw.unwrap_or(yaw);
        let angle = (yaw - previous + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        let swing = self.swing - self.amount * (1. - partial);
        let amplitude = self.previous_amount + (self.amount - self.previous_amount) * partial;
        (
            previous + angle * partial,
            swing * if child { 3. } else { 1. },
            amplitude.min(1.),
        )
    }
}

pub(crate) fn mark_hurt(mesh: &mut Mesh, start: usize) {
    for v in &mut mesh.vertices[start..] {
        v.layer |= HURT;
    }
}

pub(crate) fn world_fire(
    mesh: &mut Mesh,
    assets: &Assets,
    feet: Vec3,
    yaw: f32,
    width: f32,
    height: f32,
) {
    let layers = ["fire_layer_0", "fire_layer_1"].map(|name| assets.materials.get(name).copied());
    let [Some(bottom), Some(top)] = layers else {
        return;
    };
    let scale = width * 1.4;
    let transform = Mat4::from_translation(feet)
        * Mat4::from_rotation_y(-yaw)
        * Mat4::from_scale(Vec3::splat(scale));
    let mut remaining = height / scale;
    let mut z = -0.3 + remaining.floor() * 0.02;
    let (mut y, mut half, mut n) = (0., 0.5, 0);
    while remaining > 0. {
        let corners = [
            [half, y, z],
            [-half, y, z],
            [-half, y + 1.4, z],
            [half, y + 1.4, z],
        ];
        models::face(
            mesh,
            corners.map(|p| transform.transform_point3(Vec3::from(p))),
            if n / 2 % 2 == 0 {
                [0., 0., 1., 1.]
            } else {
                [1., 0., -1., 1.]
            },
            [bottom, top][n % 2] | UNLIT,
            [255; 4],
        );
        remaining -= 0.45;
        half *= 0.9;
        y += 0.45;
        z += 0.03;
        n += 1;
    }
}

pub(crate) fn first_person_fire(assets: &Assets) -> Mesh {
    let mut mesh = models::empty();
    let Some(&layer) = assets.materials.get("fire_layer_1") else {
        return mesh;
    };
    for sign in [-1.0_f32, 1.] {
        let transform = Mat4::from_translation(Vec3::new(-sign * 0.24, -0.3, 0.))
            * Mat4::from_rotation_y((sign * 10.).to_radians());
        let corners = [
            [-0.5, -0.5, -0.5],
            [0.5, -0.5, -0.5],
            [0.5, 0.5, -0.5],
            [-0.5, 0.5, -0.5],
        ];
        models::face(
            &mut mesh,
            corners.map(|p| transform.transform_point3(Vec3::from(p))),
            [1., 0., -1., 1.],
            layer | crate::first_person::CAMERA_HAND | FIRE_OVERLAY | UNLIT | BLEND,
            [255, 255, 255, 230],
        );
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_samples_continuously_across_ticks_stops_and_wrapped_yaw() {
        let mut gait = Gait::default();
        gait.advance(179_f32.to_radians(), 0.25);
        let (mid_yaw, mid_swing, mid_weight) = gait.sample((-179_f32).to_radians(), 0.5, false);
        assert!((mid_yaw.to_degrees() - 180.).abs() < 0.001);
        assert_eq!((mid_swing, mid_weight), (0.2, 0.2));
        let end = gait.sample(0., 1., false);
        gait.advance(0., 0.);
        let start = gait.sample(0., 0., false);
        assert!((start.1 - end.1).abs() < 1e-6);
        assert_eq!(start.2, end.2);
        let (_, phase, amount) = gait.sample(0., 0.5, true);
        assert!((phase - (0.4 + 0.12) * 3.).abs() < 1e-6);
        assert!((amount - 0.32).abs() < 1e-6);
    }

    #[test]
    #[ignore = "uses the installed 1.8.9 animated block resources"]
    fn both_burning_billboards_use_original_animated_layers_and_uv_order() {
        let assets = crate::validation::assets();
        let overlay = first_person_fire(assets);
        let layer = assets.materials["fire_layer_1"];
        assert_eq!(overlay.vertices.len(), 8);
        for quad in overlay.vertices.chunks_exact(4) {
            assert_eq!(
                [quad[0].uv, quad[1].uv, quad[2].uv, quad[3].uv],
                [[1., 1.], [0., 1.], [0., 0.], [1., 0.]]
            );
            assert!(quad
                .iter()
                .all(|v| v.layer & 65535 == layer && v.color[3] == 230));
        }
        let mut mesh = models::empty();
        world_fire(&mut mesh, assets, Vec3::ZERO, 0., 0.6, 1.8);
        assert_eq!(mesh.vertices.len(), 5 * 4);
        for (n, q) in mesh.vertices.chunks_exact(4).enumerate() {
            assert_eq!(
                q[0].layer & 65535,
                assets.materials[["fire_layer_0", "fire_layer_1"][n % 2]]
            );
            assert_eq!(q[0].uv[0], if n / 2 % 2 == 0 { 0. } else { 1. });
            assert!((q[0].position[1] - n as f32 * 0.45 * 0.84).abs() < 1e-5);
        }
    }

    #[test]
    #[ignore = "uses the installed 1.8.9 entity and armor textures"]
    fn inventory_preview_tints_skin_only_and_uses_fire_tiles() {
        use crate::{gameplay::Simulation, inventory::Stack};
        let resources = crate::validation::resources();
        let assets = crate::validation::assets();
        let skin = resources.sprite("textures/entity/steve.png").layer;
        let mut sim = Simulation::default();
        sim.inventory.slots[39] = Some(Stack::new(310, 1));
        sim.hurt_camera.hurt();
        sim.fire_flag = true;
        let mesh = models::inventory_player(resources, assets, skin, 127, 0., 0., &sim, false);
        let skin_vertices = mesh.vertices.iter().filter(|v| v.layer & 65535 == skin);
        assert!(
            skin_vertices.clone().count() > 100
                && skin_vertices.clone().all(|v| v.layer & (1 << 31) != 0)
        );
        let armor = resources
            .sprite("textures/models/armor/diamond_layer_1.png")
            .layer;
        assert!(mesh.vertices.iter().any(|v| v.layer == armor));
        assert!(mesh
            .vertices
            .iter()
            .any(|v| v.layer == assets.materials["fire_layer_1"] | models::ATLAS | UNLIT));
    }
}

#[cfg(test)]
#[path = "entity_visual_gpu_tests.rs"]
mod gpu_tests;
