use super::*;
use crate::player::third_person::Bone;
use std::sync::LazyLock;

type Quad = [[f32; 5]; 4];
// First four sets are classic/slim skins and their outer layers, final two are the original armor models.
static GEOMETRY: LazyLock<Vec<Vec<Vec<Quad>>>> = LazyLock::new(|| {
    serde_json::from_reader(flate2::read::GzDecoder::new(
        &include_bytes!("vanilla/player_models189.json.gz")[..],
    ))
    .expect("1.8.9 player model parts")
});
pub(super) fn warm_geometry() -> usize {
    LazyLock::force(&GEOMETRY).len()
}
fn draw_part(
    mesh: &mut Mesh,
    matrix: Mat4,
    faces: &[Quad],
    skin: crate::player_skins::SkinRegion,
    color: [u8; 3],
) {
    for q in faces {
        let positions = q.map(|v| matrix.transform_point3(Vec3::new(v[0], v[1], v[2]) / 16.));
        let normal = (positions[1] - positions[0])
            .cross(positions[2] - positions[0])
            .normalize_or_zero();
        let shade = crate::models::standard_item_light(normal);
        let [r, g, b] = color.map(|c| (f32::from(c) * shade).round().clamp(0., 255.) as u8);
        let start = mesh.vertices.len() as u32;
        for (p, v) in positions.into_iter().zip(q) {
            mesh.vertices.push(Vertex {
                position: p.to_array(),
                uv: skin.uv(v[3], v[4]),
                layer: skin.layer | ATLAS,
                color: [r, g, b, 255],
                light: [0, 240, 0, 0],
            });
        }
        mesh.indices.extend([0, 1, 2, 0, 2, 3].map(|i| start + i));
        mesh.visible_faces += 1;
    }
}
pub(crate) struct PlayerModelInput<'a> {
    pub feet: Vec3,
    pub anim: &'a crate::player::third_person::Animation,
    pub skin: crate::player_skins::SkinRegion,
    pub bed: Option<glam::IVec3>,
    pub pitch: f32,
    pub swing: f32,
    pub sneak: bool,
    pub using_item: bool,
    pub held: Option<&'a Stack>,
    pub armor: [Option<&'a Stack>; 4],
    pub skin_parts: u8,
    pub spectator: bool,
    pub invisible: bool,
    pub hurt: bool,
    pub fishing_cast: bool,
}

pub(crate) fn draw_player(mesh: &mut Mesh, c: &ModelContext<'_>, p: PlayerModelInput<'_>) {
    let anim = p.anim;
    let frame = anim.frame(c.alpha);
    let root = if let Some(head) = p.bed {
        crate::vanilla::sleep::body_transform(p.feet, c.world.get(head))
            * crate::player::third_person::root(Vec3::ZERO, 180.)
    } else {
        crate::player::third_person::root(p.feet, frame.body)
    };
    let holding = p.held.map_or(0, |s| {
        if p.using_item && matches!(s.id, 267 | 268 | 272 | 276 | 283) {
            3
        } else {
            1
        }
    });
    let bow = p.using_item && p.held.is_some_and(|s| s.id == 261);
    let sneak = p.sneak && p.bed.is_none();
    let bones = frame.bones(
        p.pitch,
        p.swing,
        anim.slim,
        sneak,
        anim.riding,
        holding,
        bow,
    );
    let posed = if sneak {
        root * Mat4::from_translation(Vec3::Y * 0.2)
    } else {
        root
    };
    let skin = p.skin;
    let index = usize::from(anim.slim) * 2;
    if p.spectator {
        let head = posed * bones[0].transform();
        draw_part(mesh, head, &GEOMETRY[index][0], skin, [255; 3]);
        draw_part(mesh, head, &GEOMETRY[index + 1][0], skin, [255; 3]);
        return;
    }
    if !p.invisible {
        let skin_start = mesh.vertices.len();
        for (i, &b) in bones.iter().enumerate() {
            let matrix = posed * b.transform();
            draw_part(mesh, matrix, &GEOMETRY[index][i], skin, [255; 3]);
            if p.skin_parts & (1 << [6, 1, 3, 2, 5, 4][i]) != 0 {
                draw_part(mesh, matrix, &GEOMETRY[index + 1][i], skin, [255; 3]);
            }
        }
        // Armor models have four-pixel arms on both skins. They follow the same Biped joint angles.
        if p.hurt {
            crate::entity_visual::mark_hurt(mesh, skin_start);
        }
    }
    let mut armored = bones;
    armored[2].pivot.y = 2.;
    armored[3].pivot.y = 2.;
    for (armor_opt, is_leggings, parts) in [
        (p.armor[0], false, &[0usize][..]),
        (p.armor[1], false, &[1, 2, 3][..]),
        (p.armor[2], true, &[1, 4, 5][..]),
        (p.armor[3], false, &[4, 5][..]),
    ] {
        let Some(armor) = armor_opt.filter(|s| (298..=317).contains(&s.id)) else {
            continue;
        };
        let mat =
            ["leather", "chainmail", "iron", "diamond", "gold"][usize::from(armor.id - 298) / 4];
        let layer = if is_leggings { 2 } else { 1 };
        let shape = if layer == 2 { 5 } else { 4 };
        let sprite = crate::player_skins::SkinRegion::from_sprite(
            c.resources
                .sprite(&format!("textures/models/armor/{mat}_layer_{layer}.png")),
        );
        let tint = if mat == "leather" {
            use crate::vanilla::item_nbt::{compound, number};
            let rgb = armor
                .nbt
                .as_deref()
                .and_then(compound)
                .and_then(|v| v.get("display"))
                .and_then(compound)
                .and_then(|v| v.get("color"))
                .map_or(10511680, |v| number(Some(v)) as u32);
            let [_, r, g, b] = rgb.to_be_bytes();
            [r, g, b]
        } else {
            [255; 3]
        };
        let armor_start = mesh.vertices.len();
        for &i in parts {
            let matrix = posed * armored[i].transform();
            draw_part(mesh, matrix, &GEOMETRY[shape][i], sprite, tint);
            if mat == "leather" {
                let overlay = crate::player_skins::SkinRegion::from_sprite(c.resources.sprite(
                    &format!("textures/models/armor/leather_layer_{layer}_overlay.png"),
                ));
                draw_part(mesh, matrix, &GEOMETRY[shape][i], overlay, [255; 3]);
            }
        }
        if crate::ui::creative::world_glint(armor) {
            for v in &mut mesh.vertices[armor_start..] {
                v.layer |= GLINT;
                v.light[2] = ((v.uv[0] * 16.0).fract() * 255.0) as u8;
                v.light[3] = ((v.uv[1] * 16.0).fract() * 255.0) as u8;
            }
        }
    }
    if let Some(head_item) = p.armor[0] {
        let head_mat = posed * bones[0].transform();
        if head_item.id == 397 {
            crate::skull::draw_equipped(
                mesh,
                head_mat,
                head_item.meta as u8,
                crate::player_skins::item_skull_owner(head_item.nbt.as_deref()),
                c.resources,
                Some(&c.sim.presentation),
                [0, 240, 0, 0],
                p.hurt,
            );
        } else if head_item.id < 256 && !(298..=317).contains(&head_item.id) {
            let state = (head_item.id << 4) | (head_item.meta & 15);
            let hurt_flag = if p.hurt {
                crate::entity_visual::HURT
            } else {
                0
            };
            if let Some(faces) = c.assets.block_models.carried(state) {
                let block_mat = head_mat
                    * Mat4::from_translation(Vec3::new(0.0, -0.25, 0.0))
                    * Mat4::from_rotation_y(std::f32::consts::PI)
                    * Mat4::from_scale(Vec3::new(0.625, -0.625, -0.625))
                    * Mat4::from_translation(Vec3::new(-0.5, -0.5, -0.5));
                let reverse = block_mat.determinant() < 0.0;
                for f in faces {
                    let first = mesh.vertices.len() as u32;
                    for v in f.vertices {
                        mesh.vertices.push(Vertex {
                            position: block_mat.transform_point3(v.position).to_array(),
                            uv: v.uv,
                            layer: f.layer | hurt_flag,
                            color: [f.shade, f.shade, f.shade, 255],
                            light: [0, 240, 0, 0],
                        });
                    }
                    let order = if reverse {
                        [0, 2, 1, 0, 3, 2]
                    } else {
                        [0, 1, 2, 0, 2, 3]
                    };
                    mesh.indices.extend(order.map(|k| first + k));
                    mesh.visible_faces += 1;
                }
            } else {
                let block_start = mesh.vertices.len();
                let block_mat = head_mat
                    * Mat4::from_translation(Vec3::new(0.0, -0.25, 0.0))
                    * Mat4::from_rotation_y(std::f32::consts::PI)
                    * Mat4::from_scale(Vec3::new(0.625, -0.625, -0.625));
                super::block_cube(mesh, block_mat, state, 1.0, c.assets);
                if p.hurt {
                    crate::entity_visual::mark_hurt(mesh, block_start);
                }
            }
        }
    }
    if let Some(item) = p.held {
        let mut arm: Bone = bones[2];
        if anim.slim {
            arm.pivot.x += 1.;
        }
        super::third_person_item(
            mesh,
            root * arm.transform(),
            item,
            c.assets,
            c.resources,
            sneak,
            p.fishing_cast,
        );
    }
}

pub(super) fn draw(mesh: &mut Mesh, c: &ModelContext<'_>, feet: Vec3) {
    let player = c.player;
    let held = c.sim.inventory.held();
    let skin = c
        .sim
        .presentation
        .local_skin_region(c.resources, player.third_person.slim);
    draw_player(
        mesh,
        c,
        PlayerModelInput {
            feet,
            anim: &player.third_person,
            skin,
            bed: player.sleeping.then(|| player.position.floor().as_ivec3()),
            pitch: player.java_rotation()[1],
            swing: c.sim.hand_swing.sample(c.alpha),
            sneak: player.sneaking(),
            using_item: player.using_item,
            held: held.as_ref(),
            armor: [
                c.sim.inventory.slots[39].as_ref(),
                c.sim.inventory.slots[38].as_ref(),
                c.sim.inventory.slots[37].as_ref(),
                c.sim.inventory.slots[36].as_ref(),
            ],
            skin_parts: c.settings.menu.skin_parts,
            spectator: player.is_spectator(),
            invisible: c.sim.effects.get(14).is_some(),
            hurt: c.sim.hurt_camera.hurt_time > 0 || c.sim.health <= 0.,
            fishing_cast: c.sim.fishing.hook.is_some() || c.sim.server_scene.local_fishing_cast,
        },
    );
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "uses the installed player skin and item atlas"]
    fn original_sleeping_camera_and_body_for_all_bed_directions() {
        #[derive(serde::Deserialize)]
        struct Sample { facing: u16, perspective: u8, view: [f32; 16], body: [f32; 16] }
        let samples: Vec<Sample> = serde_json::from_str(include_str!("../tests/fixtures/vanilla189-sleep-view.json")).unwrap();
        let assets = crate::validation::assets();
        let resources = crate::validation::resources();
        let skin = resources.sprite("textures/entity/steve.png").layer | ATLAS;
        let mut world = World {
            chunks: [((0, 0), std::sync::Arc::new(crate::world::Chunk::empty()))].into(),
            spawn: Vec3::ZERO, label: String::new(), edits: Default::default(),
            save_path: Default::default(), imported: false, light_revision: 0,
        };
        let sim = Simulation::default();
        let settings = Settings::default();
        let head = glam::IVec3::new(8, 75, 8);
        assert_eq!(samples.len(), 12);
        for sample in samples {
            let state = (26 << 4) | 8 | sample.facing;
            world.set(head, state);
            let mut player = Player::new(Vec3::ZERO);
            player.set_pose_f64(crate::vanilla::sleep::resting_position(head, state), 63., 39., true);
            player.sleeping = true;
            let view = camera(&player, &world, &settings, sample.perspective, 0.625);
            let relative_view = view.view_at(view.eye - player.position);
            assert!(relative_view.abs_diff_eq(Mat4::from_cols_array(&sample.view), 1e-5));
            let body = crate::vanilla::sleep::body_transform(Vec3::ZERO, state);
            assert!(body.abs_diff_eq(Mat4::from_cols_array(&sample.body), 1e-5));
            let frustum = Frustum::new(Mat4::perspective_rh(1.1, 16./9., 0.05, 256.) * view.view_at(view.eye));
            let meshes = build(ModelContext {
                sim: &sim, player: &player, world: &world, settings: &settings, assets, resources,
                frustum: &frustum, viewport_height: 1080, eye: view.eye, alpha: 0.625, perspective: sample.perspective,
                render_hand: false, hitboxes: false,
            });
            let mut min = Vec3::splat(f32::INFINITY);
            let mut max = Vec3::splat(f32::NEG_INFINITY);
            for v in meshes.opaque.vertices.iter().filter(|v| v.layer == skin) {
                let point = Vec3::from_array(v.position);
                min = min.min(point);
                max = max.max(point);
            }
            let size = max - min;
            assert!(size.y > 0.4 && size.y < 1. && size.x.max(size.z) > 1.7,
                    "expected the full lying player in view {}, facing {}: {size:?}", sample.perspective, sample.facing);
        }
    }
    #[test]
    fn original_front_and_rear_camera_clips_at_eight_ray_corners() {
        type Case = (Vec<[i32; 4]>, u8, [f32; 2], [f32; 16]);
        let cases: Vec<Case> = serde_json::from_reader(flate2::read::GzDecoder::new(
            &include_bytes!("../tests/fixtures/vanilla189-f5-camera.json.gz")[..],
        ))
        .unwrap();
        for (blocks, perspective, [yaw, pitch], matrix) in cases {
            let mut chunk = crate::world::Chunk::empty();
            for x in 0..16 {
                for z in 0..16 {
                    chunk.set(x, 79, z, 16);
                }
            }
            for [x, y, z, state] in blocks {
                chunk.set(x, y, z, state as u16);
            }
            let world = World {
                chunks: [((0, 0), std::sync::Arc::new(chunk))].into(),
                spawn: Vec3::ZERO,
                label: String::new(),
                edits: Default::default(),
                save_path: Default::default(),
                imported: false,
                light_revision: 0,
            };
            let mut p = Player::new(Vec3::ZERO);
            let at = [8.46, 80., 8.62];
            p.set_pose_f64(at, yaw, pitch, true);
            let gl = Mat4::from_cols_array(&matrix).inverse();
            let expected = gl.transform_point3(Vec3::ZERO) + glam::DVec3::from_array(at).as_vec3();
            let view = camera(&p, &world, &Settings::default(), perspective, 1.);
            let eye = view.eye;
            assert!(
                eye.distance(expected) < 0.0001,
                "F5 {perspective} {yaw},{pitch}: {eye:?} != {expected:?}"
            );
            assert!(view.direction.distance(gl.transform_vector3(Vec3::NEG_Z)) < 0.00001);
            let original = Mat4::from_cols_array(&matrix);
            let native = view.view_at(eye);
            for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
                assert!(native.transform_vector3(axis).distance(original.transform_vector3(axis)) < 0.00001,
                    "Camera basis at perspective {perspective}, yaw {yaw}, pitch {pitch}, axis {axis:?}");
            }
        }
    }
    #[test]
    #[ignore = "compares original OpenGL feedback with the world renderer using the imported 1.8.9 skins"]
    fn f5_world_mesh_matches_original_renderplayer_gl_vertices_and_skin_uvs() {
        type Sample = ([f32; 6], [f32; 16], [[f32; 6]; 6]);
        type Tick = (
            i32,
            bool,
            [f64; 2],
            [f32; 3],
            [f32; 4],
            bool,
            i32,
            bool,
            [Sample; 3],
        );
        let rows: Vec<Tick> = serde_json::from_reader(flate2::read::GzDecoder::new(
            &include_bytes!("../tests/fixtures/vanilla189-third-person.json.gz")[..],
        ))
        .unwrap();
        let meshes: Vec<(i32, Vec<[i32; 5]>)> =
            serde_json::from_reader(flate2::read::GzDecoder::new(
                &include_bytes!("../tests/fixtures/vanilla189-f5-vertices.json.gz")[..],
            ))
            .unwrap();
        let mut p = Player::new(Vec3::ZERO);
        let mut sim = Simulation::default();
        let world = World {
            chunks: Default::default(),
            spawn: Vec3::ZERO,
            label: "F5".into(),
            edits: Default::default(),
            save_path: Default::default(),
            imported: false,
            light_revision: 0,
        };
        let mut settings = Settings::default();
        settings.menu.skin_parts = 127;
        let resources = crate::validation::resources();
        let assets = crate::validation::assets();
        // Opt-in visual comparison shares the exact tick replay with the numeric mesh check.
        let captures = std::env::var_os("FERRUM_F5_CAPTURE").map(std::path::PathBuf::from);
        settings.render_distance = 2;
        settings.fov = 70.;
        settings.clouds = 0;
        settings.entity_shadows = false;
        sim.clock = 6000;
        let mut gpu = captures.as_ref().map(|_| {
            pollster::block_on(crate::renderer::Renderer::new(None, assets, resources, &settings, None)).unwrap()
        });
        for (tick, slim, [dx, dz], [yaw, pitch, swing], _, sneak, held, bow, _) in rows {
            p.third_person.slim = slim;
            p.third_person.swinging = swing > 0.;
            p.third_person.tick(glam::DVec3::new(dx, 0., dz), yaw);
            p.set_rotation_degrees(yaw, pitch);
            if sneak {
                p.keys.insert(winit::keyboard::KeyCode::ShiftLeft);
            } else {
                p.keys.remove(&winit::keyboard::KeyCode::ShiftLeft);
            }
            if tick == 11 || tick == 35 {
                sim.hand_swing.start(6);
            }
            sim.hand_swing.advance(6);
            let Some((_, original)) = meshes.iter().find(|(n, _)| *n == tick) else {
                continue;
            };
            sim.inventory.slots[0] = if held == 0 {
                None
            } else {
                Some(Stack::new(
                    if bow {
                        261
                    } else if held == 3 {
                        267
                    } else {
                        3
                    },
                    1,
                ))
            };
            p.using_item = bow || held == 3;
            let alpha = 0.375;
            let perspective = 1 + u8::from(slim);
            if let (Some(out), Some(gpu)) = (&captures, &mut gpu) {
                if tick == 4 || tick == 13 {
                    std::fs::create_dir_all(out).unwrap();
                    let gui = crate::ui::Gui { screen: crate::ui::Screen::Playing, perspective, hide_hud: true, ..Default::default() };
                    let frame = crate::ui::Frame::default();
                    let input = crate::renderer::RenderInput {
                        selected_block: None,
                        assets, resources, settings: &settings, gui: &gui, ui: &frame,
                        world: Some(&world), player: Some(&p), simulation: Some(&sim), alpha, time: 0.
                    };
                    gpu.screenshot(&out.join(format!("rust-{tick}.png")), &input).unwrap();
                }
            }
            let view = camera(&p, &world, &settings, perspective, alpha);
            let eye = view.eye;
            let frustum = Frustum::new(
                Mat4::perspective_rh(1.1, 16. / 9., 0.05, 256.)
                    * view.view_at(eye),
            );
            let got = build(ModelContext {
                sim: &sim,
                player: &p,
                world: &world,
                assets,
                resources,
                settings: &settings,
                frustum: &frustum,
                viewport_height: 1080,
                eye,
                alpha,
                perspective,
                render_hand: true,
                hitboxes: false,
            });
            let layer = resources
                .sprite(if slim {
                    "textures/entity/alex.png"
                } else {
                    "textures/entity/steve.png"
                })
                .layer
                | ATLAS;
            let vertices = got
                .opaque
                .vertices
                .iter()
                .filter(|p| p.layer == layer)
                .collect::<Vec<_>>();
            let equal = |v: &Vertex, q: &[i32; 5]| {
                let xyz = Vec3::from_array(v.position) * 8192.
                    - Vec3::new(q[0] as f32, q[1] as f32, q[2] as f32);
                xyz.abs().max_element() < 0.67
                    && (v.uv[0] * 256.).round() as i32 == q[3]
                    && (v.uv[1] * 256.).round() as i32 == q[4]
            };
            for q in original {
                assert!(
                    vertices.iter().any(|v| equal(v, q)),
                    "F5 tick {tick}: original skin vertex {q:?} missing"
                );
            }
            for v in vertices {
                assert!(
                    original.iter().any(|q| equal(v, q)),
                    "F5 tick {tick}: unexpected native skin vertex {v:?}"
                );
            }
        }
    }
}
