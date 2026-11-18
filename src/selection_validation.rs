use crate::vanilla::{
    collision_shapes,
    movement::Aabb,
    ray_trace::{self, Access, Options},
};
use glam::{DVec3, IVec3};
use std::collections::HashMap;

type Outline = (u16, [i32; 3], [u16; 6], [f64; 6]);
type OriginalHit = (u8, [i32; 3], u8, [f64; 3]);
type Probe = (f64, f64, Option<OriginalHit>);
type Narrow = (u16, [i32; 3], [u16; 6], Vec<[f64; 6]>, Vec<Probe>);
#[derive(Default)]
struct Grid(HashMap<IVec3, u16>);
impl Access for Grid {
    fn get(&self, p: IVec3) -> u16 {
        self.0.get(&p).copied().unwrap_or(0)
    }
}
impl Grid {
    fn load(&mut self, state: u16, pos: IVec3, neighbors: [u16; 6]) {
        self.0.clear();
        self.0.insert(pos, state);
        for (d, s) in [
            IVec3::NEG_Z,
            IVec3::X,
            IVec3::Z,
            IVec3::NEG_X,
            IVec3::NEG_Y,
            IVec3::Y,
        ]
        .into_iter()
        .zip(neighbors)
        {
            self.0.insert(pos + d, s);
        }
    }
}
fn bounds(b: Aabb) -> [f64; 6] {
    [b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z]
}

#[test]
fn selection_outlines_and_small_block_picks_match_the_original_game() {
    let (outlines, narrow): (Vec<Outline>, Vec<Narrow>) =
        serde_json::from_reader(flate2::read::GzDecoder::new(
            &include_bytes!("../tests/fixtures/vanilla189-selected-bounds.json.gz")[..],
        ))
        .unwrap();
    assert_eq!((outlines.len(), narrow.len()), (3458, 449));
    let mut grid = Grid::default();
    for (state, pos, neighbors, expected) in outlines {
        let p = IVec3::from_array(pos);
        grid.load(state, p, neighbors);
        assert_eq!(
            bounds(ray_trace::outline_bounds(&grid, p, state)),
            expected,
            "outline {state}, {p:?}, {neighbors:?}"
        );
    }
    let mut total = 0;
    for (state, pos, neighbors, expected, probes) in narrow {
        let p = IVec3::from_array(pos);
        grid.load(state, p, neighbors);
        let mut collision = Vec::new();
        collision_shapes::emit(
            p,
            state,
            |q| grid.get(q),
            None,
            |b| collision.push(bounds(b)),
        );
        assert_eq!(collision, expected, "walking collision {state}");
        for (x, y, hit) in probes {
            let a = p.as_dvec3() + DVec3::new(x, y, 2.5);
            let got = ray_trace::trace(&grid, a, a - DVec3::Z * 4., Options::PICK).map(|h| {
                (
                    u8::from(h.kind == ray_trace::Kind::Block),
                    h.position.to_array(),
                    h.face,
                    h.point.to_array(),
                )
            });
            assert_eq!(got, hit, "crosshair {state}, {x}, {y}");
            total += 1;
        }
    }
    assert_eq!(total, 32328);
}

#[test]
#[ignore = "requires a Vulkan adapter and installed 1.8.9 resources"]
fn vulkan_aims_at_the_torch_or_through_the_gap_to_the_wall() {
    use crate::{
        gameplay::{Mode, Simulation},
        models,
        player::Player,
        renderer::{RenderInput, Renderer},
        settings::Settings,
        ui::{self, Gui, Screen, Stats},
        vanilla::entity_pick::Hit,
        world::{Chunk, World},
    };
    use glam::{Mat4, Vec2, Vec3};
    use std::sync::Arc;
    let resources = crate::validation::resources();
    let assets = crate::validation::assets();
    let settings = Settings {
        clouds: 0,
        bobbing: false,
        render_distance: 4,
        ..Default::default()
    };
    let mut renderer =
        pollster::block_on(Renderer::new(None, assets, resources, &settings, None)).unwrap();
    let (width, height) = (512, 384);
    renderer.resize(width, height);
    let mut chunk = Chunk::empty();
    for z in 0..16 {
        for x in 0..16 {
            chunk.set(x, 63, z, 2 << 4);
        }
    }
    for x in 4..13 {
        for y in 64..68 {
            chunk.set(x, y, 6, 98 << 4);
        }
    }
    let p = IVec3::new(8, 65, 7);
    chunk.set(8, 64, 7, 1 << 4);
    let mut world = World {
        chunks: crate::world::ChunkMap::from([((0, 0), Arc::new(chunk))]),
        spawn: Vec3::new(9.1, 64., 10.4),
        label: "Selection fixture".into(),
        save_path: Default::default(),
        edits: Default::default(),
        imported: false,
        light_revision: 0,
    };
    let mut player = Player::new(world.spawn);
    let mut sim = Simulation::new(Mode::Survival, 189);
    sim.clock = 6000;
    let gui = Gui {
        screen: Screen::Playing,
        ..Default::default()
    };
    let frame = ui::build(
        &gui,
        &settings,
        resources,
        assets,
        renderer.resolution(),
        Some(&sim),
        &Stats::default(),
    );
    let temp =
        std::env::temp_dir().join(format!("ferrum-torch-outline-{}.png", std::process::id()));
    let mut strip = image::RgbImage::new(width * 3, height);
    for (index, (meta, look, expected)) in [
        (5, DVec3::new(8.5, 65.3, 7.5), p),
        (5, DVec3::new(8.78, 65.3, 7.5), IVec3::new(8, 65, 6)),
        (3, DVec3::new(8.5, 65.5, 7.15), p),
    ]
    .into_iter()
    .enumerate()
    {
        world.set(p, 50 << 4 | meta);
        renderer.upload(
            (0, 0),
            crate::mesh::build_with_settings(&world, (0, 0), assets, &settings),
        );
        let feet = world.spawn.as_dvec3();
        let look = (look - feet - DVec3::Y * f64::from(player.eye_height())).normalize();
        player.set_pose_f64(
            feet.to_array(),
            (-look.x).atan2(look.z).to_degrees() as f32,
            -look.y.asin().to_degrees() as f32,
            true,
        );
        let Some(Hit::Block(hit)) = sim.pick_at(&world, &player, 1.) else {
            panic!("Expected crosshair block")
        };
        assert_eq!(hit.position, expected);
        let mut capture = |selected_block| {
            renderer
                .screenshot(
                    &temp,
                    &RenderInput {
                        assets,
                        resources,
                        settings: &settings,
                        gui: &gui,
                        ui: &frame,
                        selected_block,
                        world: Some(&world),
                        player: Some(&player),
                        simulation: Some(&sim),
                        alpha: 1.,
                        time: 4.,
                    },
                )
                .unwrap();
            image::open(&temp).unwrap().to_rgb8()
        };
        let plain = capture(None);
        let marked = capture(Some(hit.position));
        let view = models::camera(&player, &world, &settings, 0, 1.);
        let matrix = Mat4::perspective_rh(
            settings.fov.to_radians(),
            width as f32 / height as f32,
            0.05,
            64. * std::f32::consts::SQRT_2,
        ) * view.view_at(view.eye);
        let b = ray_trace::outline_bounds(&world, expected, world.get(expected))
            .expand(DVec3::splat(f64::from(0.002f32)));
        let mut min = Vec2::splat(f32::INFINITY);
        let mut max = Vec2::splat(f32::NEG_INFINITY);
        for x in [b.min.x, b.max.x] {
            for y in [b.min.y, b.max.y] {
                for z in [b.min.z, b.max.z] {
                    let clip = matrix.project_point3(DVec3::new(x, y, z).as_vec3());
                    let pixel = Vec2::new(
                        (clip.x + 1.) * width as f32 / 2.,
                        (1. - clip.y) * height as f32 / 2.,
                    );
                    min = min.min(pixel);
                    max = max.max(pixel);
                }
            }
        }
        let mut touched = 0;
        for (x, y, px) in marked.enumerate_pixels() {
            if px == plain.get_pixel(x, y) {
                continue;
            }
            let screen = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            assert!(
                screen.cmpge(min - 3.).all() && screen.cmple(max + 3.).all(),
                "outline pixel {x},{y} outside {min:?}..{max:?}"
            );
            touched += 1;
        }
        assert!(touched > 20, "missing projected outline for {expected:?}");
        image::imageops::replace(&mut strip, &marked, i64::from(width) * index as i64, 0);
    }
    if let Some(path) = std::env::var_os("FERRUM_TORCH_OUTLINE_CAPTURE") {
        strip.save(path).unwrap();
    }
    std::fs::remove_file(temp).unwrap();
}
