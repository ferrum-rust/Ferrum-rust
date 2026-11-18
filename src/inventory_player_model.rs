use super::*;
pub(crate) fn inventory_player(
    r: &Resources,
    assets: &Assets,
    skin: u32,
    layers: u8,
    yaw: f32,
    pitch: f32,
    sim: &Simulation,
    slim: bool,
) -> Mesh {
    let mut mesh = empty();
    let tick = sim.total_ticks as f32 + 1.;
    let idle_x = (tick * 0.067).sin() * 0.05;
    let idle_z = (tick * 0.09).cos() * 0.05 + 0.05;
    let held = if sim.inventory.held().is_some() {
        -0.31415927
    } else {
        0.
    };
    let arm_width = if slim { 3. } else { 4. };
    let arm_y = if slim { 2.5 } else { 2. };
    let pieces = [
        (
            6,
            [0., 0., 0.],
            [-4., -8., -4.],
            [8., 8., 8.],
            [0., 0.],
            [32., 0.],
            [pitch, yaw, 0.],
            39,
        ),
        (
            1,
            [0., 0., 0.],
            [-4., 0., -2.],
            [8., 12., 4.],
            [16., 16.],
            [16., 32.],
            [0.; 3],
            38,
        ),
        (
            2,
            [5., arm_y, 0.],
            [-1., -2., -2.],
            [arm_width, 12., 4.],
            [32., 48.],
            [48., 48.],
            [-idle_x, 0., -idle_z],
            38,
        ),
        (
            3,
            [-5., arm_y, 0.],
            [1. - arm_width, -2., -2.],
            [arm_width, 12., 4.],
            [40., 16.],
            [40., 32.],
            [held + idle_x, 0., idle_z],
            38,
        ),
        (
            4,
            [1.9, 12., 0.],
            [-2., 0., -2.],
            [4., 12., 4.],
            [16., 48.],
            [0., 48.],
            [0.; 3],
            37,
        ),
        (
            5,
            [-1.9, 12., 0.],
            [-2., 0., -2.],
            [4., 12., 4.],
            [0., 16.],
            [0., 32.],
            [0.; 3],
            37,
        ),
    ];
    for (part, pivot, min, size, uv, outer, angles, slot) in pieces {
        let skin_start = mesh.vertices.len();
        let mut painter = ModelPainter {
            mesh: &mut mesh,
            base: Mat4::IDENTITY,
            layer: skin,
            color: [255; 4],
        };
        painter.part(pivot, min, size, uv, angles, 0.);
        if layers & (1 << part) != 0 {
            painter.part(
                pivot,
                min,
                size,
                outer,
                angles,
                if part == 6 { 0.5 } else { 0.25 },
            );
        }
        for v in &mut painter.mesh.vertices[skin_start..] {
            v.color = [255; 4];
            if sim.hurt_camera.hurt_time > 0 || sim.health <= 0. {
                v.layer |= 1 << 31; // Inventory skin uses the HUD shader's damage combiner.
            }
        }
        let armor_uv = match part {
            6 => [0., 0.],
            1 => [16., 16.],
            2 | 3 => [40., 16.],
            _ => [0., 16.],
        };
        let mut armor = |slot: usize, layer: usize, inflate: f32| {
            let Some(a) = sim.inventory.slots[slot].as_ref() else {
                return;
            };
            if !(298..=317).contains(&a.id) {
                return;
            }
            let material =
                ["leather", "chainmail", "iron", "diamond", "gold"][usize::from(a.id - 298) / 4];
            painter.layer = r
                .sprite(&format!(
                    "textures/models/armor/{material}_layer_{layer}.png"
                ))
                .layer;
            let armor_start = painter.mesh.vertices.len();
            let start = armor_start;
            painter.part(pivot, min, size, armor_uv, angles, inflate);
            let dye = if material == "leather" {
                use crate::vanilla::item_nbt::{compound, number};
                a.nbt
                    .as_deref()
                    .and_then(compound)
                    .and_then(|v| v.get("display"))
                    .and_then(compound)
                    .and_then(|v| v.get("color"))
                    .map_or(10511680, |v| number(Some(v)) as u32)
            } else {
                0xffffff
            };
            let [_, red, green, blue] = dye.to_be_bytes();
            for v in &mut painter.mesh.vertices[start..] {
                v.color = [red, green, blue, 255];
            }
            if material == "leather" {
                let start = painter.mesh.vertices.len();
                painter.layer = r
                    .sprite(&format!(
                        "textures/models/armor/leather_layer_{layer}_overlay.png"
                    ))
                    .layer;
                painter.part(pivot, min, size, armor_uv, angles, inflate);
                for v in &mut painter.mesh.vertices[start..] {
                    v.color = [255; 4];
                }
            }
            if crate::ui::creative::world_glint(a) {
                for v in &mut painter.mesh.vertices[armor_start..] {
                    v.layer |= GLINT;
                }
            }
        };
        armor(
            slot,
            if slot == 37 { 2 } else { 1 },
            if slot == 37 { 0.5 } else { 1. },
        );
        if part == 1 {
            armor(37, 2, 0.5);
        }
        if part >= 4 && part <= 5 {
            armor(36, 1, 1.);
        }
    }
    // ModelBox.TexturedQuad assigns the opposite U orientation on the four
    // vertical faces to the world cuboid helper. The top uses the opposite V.
    for (face, quad) in mesh.vertices.chunks_exact_mut(4).enumerate() {
        let axis = match face % 6 {
            0..=3 => 0,
            4 => 1,
            _ => continue,
        };
        let min = quad
            .iter()
            .map(|v| v.uv[axis])
            .fold(f32::INFINITY, f32::min);
        let max = quad
            .iter()
            .map(|v| v.uv[axis])
            .fold(f32::NEG_INFINITY, f32::max);
        for v in quad {
            v.uv[axis] = min + max - v.uv[axis];
        }
    }
    for v in &mut mesh.vertices {
        if v.layer & GLINT != 0 {
            v.light[2] = ((v.uv[0] * 16.0).fract() * 255.0) as u8;
            v.light[3] = ((v.uv[1] * 16.0).fract() * 255.0) as u8;
        }
    }
    if let Some(item) = sim.inventory.held() {
        let arm = Mat4::from_translation(Vec3::new(-5. / 16., (24. - arm_y) / 16., 0.))
            * Mat4::from_rotation_z(-idle_z)
            * Mat4::from_rotation_x(-(held + idle_x))
            * Mat4::from_translation(Vec3::new(-0.0625, -0.4375, 0.0625));
        let start = mesh.vertices.len();
        // Arm poses above are Y-up. Convert the right-arm frame to ModelBiped's
        // Y-down axis before applying the shared LayerHeldItem grip.
        let arm = arm * Mat4::from_translation(Vec3::new(0.0625,0.4375,-0.0625)) * Mat4::from_scale(Vec3::new(1.,-1.,1.));
        third_person_item(&mut mesh,arm,&item,assets,r,false,sim.fishing.hook.is_some());
        // Terrain meshes encode GUI sprites with ATLAS. The UI pipeline encodes
        // terrain tiles with the same bit and samples ordinary sprites directly.
        for v in &mut mesh.vertices[start..] {
            let glint = v.layer & GLINT;
            v.layer = ((v.layer ^ ATLAS) & 0x1ffff) | glint;
        }
    }
    if sim.player_burning() {
        let start = mesh.vertices.len();
        crate::entity_visual::world_fire(&mut mesh, assets, Vec3::ZERO, 0., 0.6, 1.8);
        for v in &mut mesh.vertices[start..] {
            // GUI uses bit 16 to select block tiles; flame faces skip entity lighting.
            v.layer = (v.layer & 65535) | ATLAS | UNLIT;
        }
    }
    mesh
}
