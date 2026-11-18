use super::*;
use crate::vanilla::movement::Aabb;
use serde_json::{json, Value};
use std::io::Read;

struct World {
    layout: i32,
    ox: i32,
    oz: i32,
}
impl MovementWorld for World {
    fn get(&self, p: IVec3) -> u16 {
        let (x, y, z) = (p.x - self.ox, p.y, p.z - self.oz);
        let mut raw = if y == 0 { 16 } else { 0 };
        match self.layout {
            1 if x == 2 && z.abs() < 4 && (1..=3).contains(&y) => raw = 16,
            2 if x >= 2 && y == 1 => raw = 16,
            3 if x < -1 && (0..=3).contains(&y) => raw = 16,
            4 if x == 2 && z.abs() <= 6 && y == 1 => raw = 85 << 4,
            5 if x == 2 && z.abs() <= 6 && y == 1 => {
                raw = if z == 1 { 107 << 4 | 4 } else { 113 << 4 }
            }
            6 if (1..=3).contains(&x) && y == 1 => raw = 9 << 4,
            7 if x.abs() < 4 && z.abs() < 7 && y == 1 => raw = 10 << 4,
            8 if x == 2 && z.abs() < 4 && (1..=3).contains(&y) => {
                raw = if z == 0 && y < 3 {
                    64 << 4 | if y == 2 { 8 } else { 0 }
                } else {
                    16
                }
            }
            9 if x == 2 && z.abs() < 4 && (1..=3).contains(&y) => {
                raw = if z == 0 && y < 3 {
                    71 << 4 | if y == 2 { 8 } else { 4 }
                } else {
                    16
                }
            }
            10 if x == 2 && z.abs() < 4 && y == 1 => raw = 96 << 4 | z.rem_euclid(8) as u16,
            11 if (-4..=4).contains(&x) && z == 0 && y == 1 => raw = 66 << 4,
            12 if x == 2 && z.abs() < 5 && y == 1 => raw = 78 << 4 | z.rem_euclid(8) as u16,
            13 if (2..6).contains(&x) && y == 0 => raw = 0,
            14 if x <= 2 && (1..=5).contains(&y) => raw = 16,
            15 if (1..=2).contains(&y) && (x * 37 + z * 11).rem_euclid(13) < 3 => raw = 16,
            16 if x.abs() < 3 && z.abs() < 3 && y <= 3 => raw = 8 << 4,
            17 if z.abs() < 5 && y == 3 => raw = 16,
            _ => {}
        }
        raw
    }
    fn block_boxes(&self, p: IVec3, state: u16, out: &mut Vec<Aabb>) {
        crate::vanilla::collision_shapes::emit(p, state, |q| self.get(q), None, |b| out.push(b));
    }
    fn loaded_column(&self, _: i32, _: i32) -> bool {
        true
    }
}
#[test]
fn ground_navigator_direct_routes_stuck_detection_and_move_commands_match_original() {
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(
        include_bytes!("../tests/fixtures/vanilla189-ground-navigation.json.gz").as_slice(),
    )
    .read_to_end(&mut bytes)
    .unwrap();
    let data: Value = serde_json::from_slice(&bytes).unwrap();
    let mut updates = 0;
    let mut checks = 0;
    for (i, row) in data.as_array().unwrap().iter().enumerate() {
        let w = World {
            layout: row[0].as_i64().unwrap() as i32,
            ox: row[1].as_i64().unwrap() as i32,
            oz: row[2].as_i64().unwrap() as i32,
        };
        let (width, height) = [(0.6f32, 1.8f32), (0.9, 1.3), (1.4, 2.9), (0.3, 0.9)]
            [row[3].as_u64().unwrap() as usize];
        let swim = row[4].as_bool().unwrap();
        let requirement = row[5].as_f64().unwrap() as f32;
        let points: Vec<IVec3> = serde_json::from_value(row[6].clone()).unwrap();
        let mut nav = navigation::Navigator::default();
        let mut wet = false;
        for (tick, frame) in row[7].as_array().unwrap().iter().enumerate() {
            wet = frame[2].as_bool().unwrap();
            let walker = Walker {
                position: serde_json::from_value(frame[0].clone()).unwrap(),
                width,
                height,
                in_water: wet,
                in_lava: frame[3].as_bool().unwrap(),
                max_fall: 3,
            };
            if frame[4].as_bool().unwrap() {
                assert_eq!(
                    nav.set(&w, walker, swim, Some(points.clone())),
                    frame[5].as_bool().unwrap(),
                    "set sequence{i}, tick{tick}"
                );
            }
            let moved = nav.tick(&w, walker, frame[1].as_bool().unwrap(), swim, requirement);
            assert_eq!(
                json!(nav.cursor()),
                frame[6],
                "cursor sequence{i}, tick{tick}"
            );
            let command = moved.map(|v| [v.x, v.y, v.z, 0.6].map(|d| format!("{:x}", d.to_bits())));
            assert_eq!(json!(command), frame[7], "move sequence{i}, tick{tick}");
            updates += 1;
        }
        for (k, direct) in row[8].as_array().unwrap().iter().enumerate() {
            let from = serde_json::from_value(direct[0].clone()).unwrap();
            let to = serde_json::from_value(direct[1].clone()).unwrap();
            let side = width.ceil() as i32;
            assert_eq!(
                navigation::direct(&w, from, to, IVec3::new(side, height as i32 + 1, side), wet),
                direct[2].as_bool().unwrap(),
                "direct sequence{i}, query{k}"
            );
            checks += 1;
        }
    }
    assert_eq!(checks, 10368);
    assert_eq!(updates, 5544);
}
struct PathCase {
    world: World,
    size_index: usize,
    walker: Walker,
    options: Options,
    target: Target,
    expected: Value,
}
fn path_cases() -> Vec<PathCase> {
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(
        include_bytes!("../tests/fixtures/vanilla189-ground-paths.json.gz").as_slice(),
    )
    .read_to_end(&mut bytes)
    .unwrap();
    let data: Value = serde_json::from_slice(&bytes).unwrap();
    data["paths"].as_array().unwrap().iter().map(|row| {
        let w = World {
            layout: row[0].as_i64().unwrap() as i32,
            ox: row[1].as_i64().unwrap() as i32,
            oz: row[2].as_i64().unwrap() as i32,
        };
        let (width, height) =
            [(0.6, 1.8), (0.9, 1.3), (1.4, 2.9), (0.3, 0.9)][row[3].as_u64().unwrap() as usize];
        let walker = Walker {
            position: serde_json::from_value(row[4].clone()).unwrap(),
            width,
            height,
            in_water: row[6].as_bool().unwrap(),
            in_lava: row[7].as_bool().unwrap(),
            max_fall: row[8].as_i64().unwrap() as i32,
        };
        let point: DVec3 = serde_json::from_value(row[5].clone()).unwrap();
        let flags = row[9].as_i64().unwrap();
        let options = Options {
            enter_doors: flags & 1 != 0,
            break_doors: flags & 2 != 0,
            avoid_water: flags & 4 != 0,
            swim: flags & 8 != 0,
            range: row[10].as_f64().unwrap() as f32,
        };
        let target = if row[11].as_bool().unwrap() {
            Target::Block(point.floor().as_ivec3())
        } else {
            Target::Entity(point)
        };
        PathCase { world: w, size_index: row[3].as_u64().unwrap() as usize, walker, options, target, expected: row[12].clone() }
    }).collect()
}
#[test]
fn original_walk_node_processor_and_heap_match_all_1800_ground_paths() {
    let cases = path_cases();
    assert_eq!(cases.len(), 1800);
    for (i, case) in cases.iter().enumerate() {
        assert_eq!(
            json!(find(&case.world, case.walker, case.options, case.target)),
            case.expected,
            "path {i}"
        );
    }
}

struct Counted<'a, W> {
    world: &'a W,
    reads: std::cell::Cell<usize>,
}
impl<W: MovementWorld> MovementWorld for Counted<'_, W> {
    fn get(&self, p: IVec3) -> u16 {
        self.reads.set(self.reads.get() + 1);
        self.world.get(p)
    }
    fn block_boxes(&self, p: IVec3, state: u16, out: &mut Vec<Aabb>) {
        self.world.block_boxes(p, state, out)
    }
    fn loaded_column(&self, x: i32, z: i32) -> bool {
        self.world.loaded_column(x, z)
    }
}
#[test]
fn per_search_clearance_cache_preserves_all_original_routes_and_reduces_block_queries() {
    let mut uncached = 0;
    let mut cached = 0;
    for (i, case) in path_cases().iter().enumerate() {
        let world = Counted { world: &case.world, reads: Default::default() };
        let expected = find_inner::<false>(&world, case.walker, case.options, case.target);
        let before = world.reads.replace(0);
        let actual = find(&world, case.walker, case.options, case.target);
        let after = world.reads.get();
        assert_eq!(actual, expected, "cached route {i}");
        assert_eq!(json!(actual), case.expected, "original route {i}");
        assert!(after <= before, "extra block probes in path {i}: {after} > {before}");
        uncached += before;
        cached += after;
    }
    assert!(cached < uncached, "the repeated clearance probes must be eliminated");
    eprintln!("1800 original paths: world reads {uncached} -> {cached} ({:.1}% removed)",
        100. * (uncached - cached) as f64 / uncached as f64);
}

#[test]
fn clearance_cache_keeps_exact_coordinates_and_resets_between_world_changes() {
    let world = World { layout: 1, ox: 0, oz: 0 };
    let walker = Walker {
        position: DVec3::new(0.5, 1., 0.5), width: 0.6, height: 1.8,
        in_water: false, in_lava: false, max_fall: 3,
    };
    let options = Options { enter_doors: true, break_doors: false, avoid_water: false, swim: false, range: 16. };
    let mut processor = Processor::<_, true> {
        world: &world, walker, options, size: IVec3::new(1, 2, 1),
        on_rail: false, offsets: Default::default(),
    };
    let blocked = IVec3::new(2, 1, 0);
    let alias = IVec3::new(32770, 1, 0);
    assert_eq!(path_hash(blocked), path_hash(alias));
    assert_eq!(processor.offset(blocked), 0);
    assert_eq!(processor.offset(alias), 1);
    assert_eq!(processor.offset(blocked), 0);
    let mut world = world;
    let target = Target::Entity(DVec3::new(5.5, 1., 0.5));
    let blocked = find(&world, walker, options, target);
    world.layout = 0;
    let opened = find(&world, walker, options, target);
    assert_ne!(opened, blocked, "opening the wall must invalidate the previous search's clearance");
    assert_eq!(opened, find_inner::<false>(&world, walker, options, target));
    world.layout = 1;
    assert_eq!(find(&world, walker, options, target), blocked);
}

fn case_groups(cases: &[PathCase]) -> std::collections::BTreeMap<(i32, i32, i32), Vec<usize>> {
    let mut groups = std::collections::BTreeMap::<_, Vec<_>>::new();
    for (i, case) in cases.iter().enumerate() {
        groups.entry((case.world.layout, case.world.ox, case.world.oz)).or_default().push(i);
    }
    groups
}

fn chunk_case_world(cases: &[PathCase], group: &[usize]) -> crate::world::World {
    let mut lo = IVec3::splat(i32::MAX);
    let mut hi = IVec3::splat(i32::MIN);
    for &i in group {
        let case = &cases[i];
        // Original search distance is bounded by twice the range. Include
        // clearance/fall probes and rounding beyond the end point as well.
        let radius = (case.options.range * 2.) as i32 + 16;
        let center = case.walker.position.floor().as_ivec3();
        lo = lo.min(center - IVec3::splat(radius));
        hi = hi.max(center + IVec3::splat(radius));
    }
    let source = &cases[group[0]].world;
    let mut chunks = crate::world::ChunkMap::new();
    for cx in lo.x.div_euclid(16)..=hi.x.div_euclid(16) {
        for cz in lo.z.div_euclid(16)..=hi.z.div_euclid(16) {
            let mut chunk = crate::world::Chunk::empty();
            // Every original oracle layout has its last non-air row at y<=5.
            for y in 0..6 { for z in 0..16 { for x in 0..16 {
                let state = source.get(IVec3::new(cx * 16 + x, y, cz * 16 + z));
                if state != 0 { chunk.set(x, y, z, state); }
            } } }
            chunks.insert((cx, cz), std::sync::Arc::new(chunk));
        }
    }
    crate::world::World {
        chunks, spawn: cases[group[0]].walker.position.as_vec3(), label: "path fixture".into(),
        save_path: Default::default(), edits: Default::default(), imported: false,
        light_revision: 0,
    }
}

fn native_variant(world: &crate::world::World, case: &PathCase, variant: usize) -> Option<Vec<IVec3>> {
    match variant {
        0 => find_inner::<false>(world, case.walker, case.options, case.target),
        1 => find(world, case.walker, case.options, case.target),
        2 => find_inner::<false>(
            &world_view::ChunkWindow::new(world, case.walker, case.options, case.target),
            case.walker, case.options, case.target,
        ),
        3 => find_inner::<true>(
            &world_view::ChunkWindow::new(world, case.walker, case.options, case.target),
            case.walker, case.options, case.target,
        ),
        _ => unreachable!(),
    }
}

#[test]
fn dense_chunk_window_matches_original_1800_paths_for_all_cache_combinations() {
    let cases = path_cases();
    assert_eq!(cases.len(), 1800);
    for group in case_groups(&cases).values() {
        let world = chunk_case_world(&cases, group);
        for &i in group {
            let case = &cases[i];
            assert_eq!(json!(find_world(&world, case.walker, case.options, case.target)),
                case.expected, "production path {i}");
            for variant in 0..4 {
                assert_eq!(json!(native_variant(&world, &cases[i], variant)), cases[i].expected,
                    "original path {i}, cache variant {variant}");
            }
        }
    }
}

#[test]
fn dense_chunk_window_preserves_missing_negative_boundary_and_fallback_queries() {
    use std::sync::Arc;
    let mut world = crate::world::World {
        chunks: crate::world::ChunkMap::new(), spawn: Default::default(), label: "window bounds".into(),
        save_path: Default::default(), edits: Default::default(), imported: false,
        light_revision: 0,
    };
    for (cx, cz) in [(-4, -4), (-2, -1), (-1, -1), (0, 0), (1, 1), (4, 4),
        (i32::MIN >> 4, i32::MAX >> 4)] {
        let mut chunk = crate::world::Chunk::empty();
        for (x, y, z, state) in [(0, 0, 0, 16), (15, 255, 15, 81 << 4),
            (15, 1, 15, 64 << 4), (15, 2, 15, 64 << 4 | 8)] {
            chunk.set(x, y, z, state);
        }
        world.chunks.insert((cx, cz), Arc::new(chunk));
    }
    let options = Options { enter_doors: true, break_doors: false, avoid_water: false, swim: true, range: 4. };
    let walker = Walker { position: DVec3::new(-0.5, 1., -0.5), width: 0.6, height: 1.8,
        in_water: false, in_lava: false, max_fall: 3 };
    let target = Target::Block(IVec3::new(100, 1, 100));
    {
        let view = world_view::ChunkWindow::new(&world, walker, options, target);
        for x in [-65, -64, -49, -33, -32, -17, -16, -1, 0, 15, 16, 31, 63, 64, 79, i32::MIN] {
            for z in [-65, -64, -17, -16, -1, 0, 15, 16, 31, 64, 79, i32::MAX] {
                assert_eq!(view.loaded_column(x, z), world.loaded_column(x, z));
                for y in [-1, 0, 1, 2, 127, 255, 256] {
                    let p = IVec3::new(x, y, z);
                    let state = world.get(p);
                    assert_eq!(view.get(p), state, "block {p}");
                    let mut expected = Vec::new();
                    let mut actual = Vec::new();
                    world.block_boxes(p, state, &mut expected);
                    view.block_boxes(p, state, &mut actual);
                    assert_eq!(actual.iter().map(|b| (b.min, b.max)).collect::<Vec<_>>(),
                        expected.iter().map(|b| (b.min, b.max)).collect::<Vec<_>>());
                }
            }
        }
        // A real loaded column beyond the preload window must not become air.
        assert_eq!(view.get(IVec3::new(64, 0, 64)), 16);
    }
    world.set(IVec3::new(-1, 1, -1), 64 << 4 | 4);
    let view = world_view::ChunkWindow::new(&world, walker, options, target);
    assert_eq!(view.get(IVec3::new(-1, 1, -1)), 64 << 4 | 4);
    drop(view);
    let removed = world.chunks.remove(&(-1, -1)).unwrap();
    assert_eq!(world_view::ChunkWindow::new(&world, walker, options, target)
        .get(IVec3::new(-1, 1, -1)), 0);
    world.chunks.insert((-1, -1), removed);
    assert_eq!(world_view::ChunkWindow::new(&world, walker, options, target)
        .get(IVec3::new(-1, 1, -1)), 64 << 4 | 4);
    // Large custom ranges remain bounded; saturated centers remain queryable.
    let extreme = world_view::ChunkWindow::new(&world,
        Walker { position: DVec3::new(f64::from(i32::MIN), 1., f64::from(i32::MAX)), ..walker },
        Options { range: f32::INFINITY, ..options }, target);
    assert_eq!(extreme.get(IVec3::new(i32::MIN, 0, i32::MAX & !15)), 16);
}

#[test]
#[ignore = "optimized four-variant native path profile, all1800 original cases by body size"]
fn profile_all_original_paths_by_body_size_on_chunk_worlds() {
    use std::{hint::black_box, time::{Duration, Instant}};
    let cases = path_cases();
    let mut elapsed = [[Duration::ZERO; 4]; 4];
    let mut counts = [0usize; 4];
    for case in &cases { counts[case.size_index] += 1; }
    const PASSES: usize = 4;
    for (group_index, group) in case_groups(&cases).values().enumerate() {
        let world = chunk_case_world(&cases, group);
        let sizes: [Vec<usize>; 4] = std::array::from_fn(|size|
            group.iter().copied().filter(|&i| cases[i].size_index == size).collect());
        for pass in 0..PASSES {
            for turn in 0..4 {
                let variant = (turn + pass + group_index) % 4;
                for size in 0..4 {
                    let start = Instant::now();
                    for &i in &sizes[size] { black_box(native_variant(&world, &cases[i], variant)); }
                    elapsed[size][variant] += start.elapsed();
                }
            }
        }
    }
    eprintln!("All1800 original paths x{PASSES}, includes per-find window setup; columns: World/raw, World/memo, window/raw, window/memo");
    for (size, label) in ["humanoid 0.6x1.8", "animal 0.9x1.3", "wide/tall 1.4x2.9", "small 0.3x0.9"].iter().enumerate() {
        eprintln!("{label}, {} paths/variant: {:?}", counts[size] * PASSES, elapsed[size]);
    }
    let totals: [Duration; 4] = std::array::from_fn(|variant| elapsed.iter().map(|row| row[variant]).sum());
    eprintln!("Totals: {totals:?}");
}

struct NavigationFrame {
    walker: Walker,
    grounded: bool,
    reset: Option<bool>,
    cursor: Option<[i32; 2]>,
    command: Option<[u64; 4]>,
}
struct NavigationCase {
    world: World,
    size_index: usize,
    swim: bool,
    requirement: f32,
    points: Vec<IVec3>,
    frames: Vec<NavigationFrame>,
    direct: Vec<(DVec3, DVec3, bool)>,
}
fn navigation_cases() -> Vec<NavigationCase> {
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(
        include_bytes!("../tests/fixtures/vanilla189-ground-navigation.json.gz").as_slice(),
    ).read_to_end(&mut bytes).unwrap();
    let data: Value = serde_json::from_slice(&bytes).unwrap();
    data.as_array().unwrap().iter().map(|row| {
        let size_index = row[3].as_u64().unwrap() as usize;
        let (width, height) = [(0.6, 1.8), (0.9, 1.3), (1.4, 2.9), (0.3, 0.9)][size_index];
        NavigationCase {
            world: World { layout: row[0].as_i64().unwrap() as i32,
                ox: row[1].as_i64().unwrap() as i32, oz: row[2].as_i64().unwrap() as i32 },
            size_index, swim: row[4].as_bool().unwrap(), requirement: row[5].as_f64().unwrap() as f32,
            points: serde_json::from_value(row[6].clone()).unwrap(),
            frames: row[7].as_array().unwrap().iter().map(|f| NavigationFrame {
                walker: Walker { position: serde_json::from_value(f[0].clone()).unwrap(), width, height,
                    in_water: f[2].as_bool().unwrap(), in_lava: f[3].as_bool().unwrap(), max_fall: 3 },
                grounded: f[1].as_bool().unwrap(),
                reset: f[4].as_bool().unwrap().then(|| f[5].as_bool().unwrap()),
                cursor: serde_json::from_value(f[6].clone()).unwrap(),
                command: f[7].as_array().map(|values| std::array::from_fn(|i|
                    u64::from_str_radix(values[i].as_str().unwrap(), 16).unwrap())),
            }).collect(),
            direct: row[8].as_array().unwrap().iter().map(|q|
                (serde_json::from_value(q[0].clone()).unwrap(), serde_json::from_value(q[1].clone()).unwrap(), q[2].as_bool().unwrap())).collect(),
        }
    }).collect()
}
fn navigation_options(swim: bool, range: f32) -> Options {
    Options { enter_doors: true, break_doors: false, avoid_water: false, swim, range }
}
fn navigation_world(case: &NavigationCase) -> crate::world::World {
    let source = PathCase {
        world: World { layout: case.world.layout, ox: case.world.ox, oz: case.world.oz },
        size_index: case.size_index, walker: case.frames[0].walker,
        options: navigation_options(case.swim, 35.),
        target: Target::Block(*case.points.last().unwrap()), expected: Value::Null,
    };
    chunk_case_world(&[source], &[0])
}
fn navigation_frame(
    nav: &mut navigation::Navigator, world: &crate::world::World, case: &NavigationCase,
    frame: &NavigationFrame, dense: bool, range: f32, entity: bool,
) -> (Option<bool>, Option<DVec3>) {
    if dense && (frame.reset.is_some() || !nav.no_path()) {
        let target = *case.points.last().unwrap();
        let target = if entity { Target::Entity(target.as_dvec3()) } else { Target::Block(target) };
        let view = world_view::ChunkWindow::new(world, frame.walker, navigation_options(case.swim, range), target);
        let set = frame.reset.map(|_| nav.set(&view, frame.walker, case.swim, Some(case.points.clone())));
        (set, nav.tick(&view, frame.walker, frame.grounded, case.swim, case.requirement))
    } else {
        let set = frame.reset.map(|_| nav.set(world, frame.walker, case.swim, Some(case.points.clone())));
        (set, nav.tick(world, frame.walker, frame.grounded, case.swim, case.requirement))
    }
}

#[test]
fn dense_navigation_matches_all_original_ticks_direct_paths_and_counter_state() {
    let cases = navigation_cases();
    let mut ticks = 0;
    let mut direct = 0;
    for (sequence, case) in cases.iter().enumerate() {
        let world = navigation_world(case);
        let mut plain = navigation::Navigator::default();
        let mut dense = navigation::Navigator::default();
        for (i, frame) in case.frames.iter().enumerate() {
            let expected = navigation_frame(&mut plain, &world, case, frame, false, 16., false);
            let actual = navigation_frame(&mut dense, &world, case, frame, true, 16., false);
            assert_eq!(actual, expected, "sequence {sequence}, frame {i}");
            assert_eq!(actual.0, frame.reset);
            assert_eq!(dense.cursor(), frame.cursor, "cursor sequence {sequence}, frame {i}");
            let bits = actual.1.map(|v| [v.x, v.y, v.z, 0.6].map(f64::to_bits));
            assert_eq!(bits, frame.command, "command sequence {sequence}, frame {i}");
            // Includes tick/stuck counters and checked_position, even after a
            // path ended and the dense prefetch was intentionally skipped.
            assert_eq!(format!("{dense:?}"), format!("{plain:?}"));
            ticks += 1;
        }
        let walker = case.frames.last().unwrap().walker;
        let side = walker.width.ceil() as i32;
        let size = IVec3::new(side, walker.height as i32 + 1, side);
        for &(from, to, expected) in &case.direct {
            let view = world_view::ChunkWindow::new(&world, Walker { position: from, ..walker },
                navigation_options(case.swim, 16.), Target::Entity(to));
            assert_eq!(navigation::direct(&world, from, to, size, walker.in_water), expected);
            assert_eq!(navigation::direct(&view, from, to, size, walker.in_water), expected);
            direct += 1;
        }
    }
    assert_eq!((ticks, direct), (5544, 10368));
}

#[test]
#[ignore = "optimized World versus per-active-tick dense navigation profile, setup included"]
fn profile_original_navigation_world_and_dense_by_body_size() {
    use std::{hint::black_box, time::{Duration, Instant}};
    let cases = navigation_cases();
    const PASSES: usize = 16;
    for (range, entity, label) in [(16., false, "range16 block"), (35., true, "range35 entity")] {
        let mut elapsed = [[Duration::ZERO; 2]; 4];
        let mut frames = [0usize; 4];
        for (i, case) in cases.iter().enumerate() {
            let world = navigation_world(case);
            frames[case.size_index] += case.frames.len();
            for pass in 0..PASSES {
                for turn in 0..2 {
                    let dense = (turn + pass + i) % 2 == 1;
                    let mut nav = navigation::Navigator::default();
                    let start = Instant::now();
                    for frame in &case.frames {
                        black_box(navigation_frame(&mut nav, &world, case, frame, dense, range, entity));
                    }
                    elapsed[case.size_index][usize::from(dense)] += start.elapsed();
                }
            }
        }
        eprintln!("Navigation {label}, all5544 frames x{PASSES}, includes per-active-tick window setup; columns World,dense:");
        for (i, label) in ["humanoid", "animal", "wide/tall", "small"].iter().enumerate() {
            eprintln!("{label}, {} frames/variant: {:?}", frames[i] * PASSES, elapsed[i]);
        }
        let totals: [Duration; 2] = std::array::from_fn(|v| elapsed.iter().map(|row| row[v]).sum());
        eprintln!("Totals: {totals:?}");
    }
}

/// Uses real chunk lookups, and prints timings without asserting a noisy speed
/// ratio. Each of the 18 worlds is the most probe-heavy original oracle case
/// for that layout; fixture construction is outside the timed sections.
#[test]
#[ignore = "standalone path CPU profile; no assets, GPU or saved worlds"]
fn profile_cached_original_paths_on_chunk_backed_worlds() {
    use std::{hint::black_box, sync::Arc, time::{Duration, Instant}};
    let cases = path_cases();
    let mut selected = [(0usize, 0usize); 18];
    for (i, case) in cases.iter().enumerate() {
        let world = Counted { world: &case.world, reads: Default::default() };
        black_box(find_inner::<false>(&world, case.walker, case.options, case.target));
        let slot = &mut selected[case.world.layout as usize];
        if world.reads.get() > slot.1 { *slot = (i, world.reads.get()); }
    }
    let mut times = [Duration::ZERO; 2];
    let mut queries = [0usize; 2];
    for (i, _) in selected {
        let case = &cases[i];
        let radius = (case.options.range * 2.) as i32 + 16;
        let center = case.walker.position.floor().as_ivec3();
        let mut chunks = crate::world::ChunkMap::new();
        for cx in (center.x - radius).div_euclid(16)..=(center.x + radius).div_euclid(16) {
            for cz in (center.z - radius).div_euclid(16)..=(center.z + radius).div_euclid(16) {
                let mut chunk = crate::world::Chunk::empty();
                for y in 0..16 { for z in 0..16 { for x in 0..16 {
                    let state = case.world.get(IVec3::new(cx * 16 + x, y, cz * 16 + z));
                    if state != 0 { chunk.set(x, y, z, state); }
                } } }
                chunks.insert((cx, cz), Arc::new(chunk));
            }
        }
        let world = crate::world::World {
            chunks, spawn: case.walker.position.as_vec3(), label: "path profile".into(),
            save_path: Default::default(), edits: Default::default(), imported: false,
            light_revision: 0,
        };
        assert_eq!(json!(find(&world, case.walker, case.options, case.target)), case.expected);
        let counted = Counted { world: &world, reads: Default::default() };
        for pass in 0..8 {
            // Alternate first run to reduce ordering/CPU warmup bias.
            for cached in [pass % 2 == 0, pass % 2 != 0] {
                let start = Instant::now();
                for _ in 0..16 {
                    black_box(if cached {
                        find(&counted, case.walker, case.options, case.target)
                    } else {
                        find_inner::<false>(&counted, case.walker, case.options, case.target)
                    });
                }
                let slot = usize::from(cached);
                times[slot] += start.elapsed();
                queries[slot] += counted.reads.replace(0);
            }
        }
    }
    eprintln!("2304 chunk-backed paths per variant: uncached {:?} / {} block reads; cached {:?} / {} reads; {:.2}x speedup",
        times[0], queries[0], times[1], queries[1], times[0].as_secs_f64() / times[1].as_secs_f64());
}
