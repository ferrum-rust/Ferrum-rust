//! Frame-bounded publication of immutable lighting and mesh snapshots.
use super::*;
use crate::{lighting_view::LightingView, renderer::PreparedChunk};
use std::sync::OnceLock;

const MESH_BATCH: usize = 8;
const UPLOAD_BYTES: usize = 4 * 1024 * 1024;
const UPLOAD_TIME: Duration = Duration::from_millis(2);

fn workers() -> &'static rayon::ThreadPool {
    static POOL: OnceLock<rayon::ThreadPool> = OnceLock::new();
    POOL.get_or_init(|| rayon::ThreadPoolBuilder::new()
        .num_threads(std::thread::available_parallelism().map_or(1, |n| n.get().saturating_sub(1).clamp(1, 2)))
        .thread_name(|i| format!("visual-mesh-{i}"))
        .start_handler(|index| crate::worker_priority::on_start(crate::worker_priority::Pool::VisualMesh, index))
        .build().expect("Create visual workers"))
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct MeshSettings(bool, u8, bool, bool);
impl From<&Settings> for MeshSettings {
    fn from(s: &Settings) -> Self { Self(s.fancy, s.smooth_lighting, s.greedy_meshing, s.alternate_blocks) }
}
type Inputs = crate::world::ChunkMap<Arc<Chunk>>;
type Stamp = [(ChunkPos, Option<Arc<Chunk>>); 9];
fn stamp(chunks: &Inputs, p: ChunkPos) -> Stamp {
    mesh_neighbors(p).map(|p| (p, chunks.get(&p).cloned()))
}
fn unchanged(chunks: &Inputs, expected: &Stamp) -> bool {
    expected.iter().all(|(p, previous)| match (previous, chunks.get(p)) {
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    })
}
fn light_inputs_unchanged(chunks: &Inputs, previous: &Inputs, p: ChunkPos) -> bool {
    // World.checkLightFor has a 17-block propagation radius, which can cross
    // two chunk boundaries when a light starts at the edge of a column.
    (-2..=2).all(|dx| (-2..=2).all(|dz| {
        let at = (p.0 + dx, p.1 + dz);
        match (previous.get(&at), chunks.get(&at)) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
    }))
}

// Only these original snapshots are consulted after a job finishes. Keep
// all 5x5 light-validation cells (including retry centers) and all 3x3 mesh
// stamps. Release unrelated snapshots and shrink their table on this worker.
fn retain_validation_inputs(inputs: &mut Inputs,
    lights: impl IntoIterator<Item = ChunkPos>, meshes: impl IntoIterator<Item = ChunkPos>) {
    let mut required = HashSet::new();
    for (x, z) in lights {
        for dx in -2..=2 { for dz in -2..=2 { required.insert((x + dx, z + dz)); } }
    }
    for p in meshes { required.extend(mesh_neighbors(p)); }
    inputs.retain(|p, _| required.contains(p));
    inputs.shrink_to_fit();
}

pub(super) struct PendingMesh {
    position: ChunkPos,
    mesh: PreparedChunk,
    state: MeshStamp,
}
pub(super) struct MeshStamp {
    expected: Stamp,
    settings: MeshSettings,
    assets: Arc<()>,
}
impl MeshStamp {
    fn new(world: &World, p: ChunkPos, assets: &Assets, settings: &Settings) -> Self {
        Self { expected: stamp(&world.chunks, p), settings:settings.into(), assets:assets.identity.clone() }
    }
    fn valid(&self, world: &World, assets: &Assets, settings: &Settings) -> bool {
        self.settings == settings.into() && Arc::ptr_eq(&self.assets, &assets.identity)
            && unchanged(&world.chunks, &self.expected)
    }
}
pub(super) fn prepare_initial(world: &World, meshes: MeshBatch, assets: &Assets, settings: &Settings) -> VecDeque<PendingMesh> {
    meshes.into_par_iter().map(|(position, mesh)| PendingMesh { position,
        mesh: PreparedChunk::new(mesh), state:MeshStamp::new(world,position,assets,settings) })
        .collect::<Vec<_>>().into()
}
pub(super) struct MeshWork {
    lighting: Option<LightingView>,
    inputs: Inputs,
    lights: Vec<(ChunkPos, Arc<Chunk>)>,
    meshes: Vec<(ChunkPos, PreparedChunk)>,
    deferred: HashSet<ChunkPos>,
    settings: MeshSettings,
    assets: Arc<Assets>,
    revision: u64,
}

/// Only render data belongs in worker snapshots. In particular, cloning a
/// potentially huge sparse save/edit journal on the frame thread is unnecessary.
fn snapshot(world: &World) -> World {
    World { chunks: world.chunks.clone(), spawn: world.spawn, label: String::new(),
        save_path: PathBuf::new(), edits: Default::default(), imported: world.imported,
        light_revision: world.light_revision }
}

fn build(mut world: World, mut lighting: Option<LightingView>, mut dirty: HashSet<ChunkPos>,
    assets: Arc<Assets>, settings: Settings, center: ChunkPos) -> MeshWork {
    let mut inputs = world.chunks.clone();
    let mut lights = Vec::new();
    if let Some(view) = &mut lighting {
        for p in view.update(&mut world) {
            dirty.extend(mesh_neighbors(p));
            if let Some(chunk) = world.chunks.get(&p) { lights.push((p, chunk.clone())); }
        }
    }
    let mut positions = dirty.into_iter().filter(|p| world.chunks.contains_key(p)).collect::<Vec<_>>();
    positions.sort_unstable_by_key(|p| ((i64::from(p.0)-i64::from(center.0)).pow(2)
        +(i64::from(p.1)-i64::from(center.1)).pow(2), *p));
    let deferred = positions.iter().skip(MESH_BATCH).copied().collect();
    positions.truncate(MESH_BATCH);
    let meshes: Vec<_> = positions.par_iter().map(|&p|
        (p, PreparedChunk::new(mesh::build_with_settings(&world, p, &assets, &settings)))).collect();
    retain_validation_inputs(&mut inputs, lights.iter().map(|(p, _)| *p), meshes.iter().map(|(p, _)| *p));
    MeshWork { lighting, inputs, lights, meshes, deferred, settings: (&settings).into(),
        assets, revision: world.light_revision }
}

impl Game {
    fn accept_mesh_work(&mut self, mut work: MeshWork, assets: &Arc<Assets>, settings: &Settings) {
        let mut phase = self.options.benchmark_streaming.then(Instant::now);
        let settings_match = work.settings == settings.into() && Arc::ptr_eq(&work.assets, assets);
        let rejected_lights = work.lights.iter().map(|(p, _)| *p)
            .filter(|&p| !light_inputs_unchanged(&self.world.chunks, &work.inputs, p)).collect::<HashSet<_>>();
        // Validate BEFORE publishing worker light Arcs. Every mesh reads its
        // complete 3x3 neighborhood; an edit/unload/new neighbor invalidates it.
        let valid = work.meshes.iter().map(|(p, _)| settings_match
            && unchanged(&self.world.chunks, &stamp(&work.inputs, *p))
            && mesh_neighbors(*p).iter().all(|p| !rejected_lights.contains(p))).collect::<Vec<_>>();
        if let Some(phase) = &mut phase { self.record_chunk_phase("visual_accept_validate", phase); }
        // Diagnostic children remain inside visual_accept_lights. This does
        // not split the validated publication transaction or its drop order.
        let mut light_phase = phase.as_ref().map(|_| Instant::now());
        for (p, chunk) in work.lights {
            if !rejected_lights.contains(&p) {
                if let Some(current) = self.world.chunks.get_mut(&p) { *current = chunk; }
            }
        }
        if let Some(phase) = &mut light_phase { self.record_chunk_phase("visual_light_arc_publish", phase); }
        if let Some(view) = &mut work.lighting { view.retry_publications(&work.inputs, &rejected_lights); }
        self.lighting = work.lighting;
        self.visual_revision = work.revision;
        if let Some(phase) = &mut light_phase { self.record_chunk_phase("visual_light_owner_return", phase); }
        self.dirty.extend(work.deferred);
        for p in rejected_lights { self.dirty.extend(mesh_neighbors(p)); }
        if let Some(phase) = &mut light_phase { self.record_chunk_phase("visual_light_dirty_merge", phase); }
        if let Some(phase) = &mut phase { self.record_chunk_phase("visual_accept_lights", phase); }
        for ((p, mesh), valid) in work.meshes.into_iter().zip(valid) {
            if valid {
                // A newer publication replaces any older queued result for p.
                self.prepared.retain(|m| m.position != p);
                self.uploads.retain(|(old, _)| *old != p);
                self.prepared.push_back(PendingMesh { position:p, mesh,
                    state:MeshStamp::new(&self.world,p,assets,settings) });
            } else if self.world.chunks.contains_key(&p) {
                self.dirty.insert(p);
            }
        }
        if let Some(phase) = &mut phase { self.record_chunk_phase("visual_accept_meshes", phase); }
        // This map otherwise drops at this same function boundary. Time its
        // Arc releases separately from validation/adoption, without deferral.
        drop(work.inputs);
        if let Some(phase) = &mut phase { self.record_chunk_phase("visual_snapshot_release", phase); }
    }

    fn validate_entry_uploads(&mut self, assets: &Assets, settings: &Settings) {
        self.entry_uploaded.retain(|p, state| {
            if state.valid(&self.world, assets, settings) { true } else {
                self.entry_pending.insert(*p);
                self.dirty.insert(*p);
                false
            }
        });
    }

    pub(super) fn update_visuals(&mut self, renderer: &mut Renderer, assets: &Arc<Assets>, s: &Settings,
        center: ChunkPos) -> Result<()> {
        if let Some(rx) = &self.rebuild {
            match rx.try_recv() {
                Ok(work) => { self.rebuild = None; self.accept_mesh_work(work, assets, s); }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => anyhow::bail!("Visual worker stopped"),
            }
        }
        // Accepting worker lighting can replace a neighbor of an already
        // uploaded entry mesh. Revalidate after those publications, before the
        // same frame can upload the last pending mesh and open the entry gate.
        self.validate_entry_uploads(assets, s);
        let started = Instant::now();
        let mut bytes = 0;
        for _ in 0..s.chunk_uploads.max(1) {
            if bytes != 0 && (bytes >= UPLOAD_BYTES || started.elapsed() >= UPLOAD_TIME) { break; }
            // Edits take priority over initial distant uploads. Never republish
            // an obsolete worker result after a newer local edit or pack switch.
            if let Some(next) = self.prepared.front() {
                if !next.state.valid(&self.world, assets, s) {
                    let next = self.prepared.pop_front().unwrap();
                    if self.world.chunks.contains_key(&next.position) { self.dirty.insert(next.position); }
                    continue;
                }
                if bytes != 0 && bytes + next.mesh.byte_len() > UPLOAD_BYTES { break; }
                let next = self.prepared.pop_front().unwrap();
                bytes += next.mesh.byte_len();
                let mut upload_phase = self.options.benchmark_streaming.then(Instant::now);
                renderer.upload_prepared(next.position, next.mesh);
                if let Some(phase) = &mut upload_phase { self.record_chunk_phase("visual_gpu_upload", phase); }
                self.record_upload_observation(renderer);
                if self.entry_pending.remove(&next.position) {
                    self.entry_uploaded.insert(next.position, next.state);
                }
            } else if let Some((p, m)) = self.uploads.front() {
                if bytes != 0 && bytes + m.byte_len() > UPLOAD_BYTES { break; }
                let p = *p;
                let (_, mesh) = self.uploads.pop_front().unwrap();
                if self.world.chunks.contains_key(&p) {
                    bytes += mesh.byte_len();
                    let mut upload_phase = self.options.benchmark_streaming.then(Instant::now);
                    renderer.upload_prepared(p, mesh);
                    if let Some(phase) = &mut upload_phase { self.record_chunk_phase("visual_gpu_upload", phase); }
                self.record_upload_observation(renderer);
                    if self.entry_pending.remove(&p) {
                        self.entry_uploaded.insert(p, MeshStamp::new(&self.world,p,assets,s));
                    }
                }
            } else { break; }
        }
        if self.rebuild.is_none() && self.prepared.len() < MESH_BATCH
            && (!self.dirty.is_empty() || self.lighting.is_some() && self.visual_revision != self.world.light_revision) {
            let mut phase = self.options.benchmark_streaming.then(Instant::now);
            let world = snapshot(&self.world);
            if let Some(phase) = &mut phase { self.record_chunk_phase("visual_snapshot_clone", phase); }
            let lighting = self.lighting.take();
            let dirty = std::mem::take(&mut self.dirty);
            let assets = assets.clone();
            let settings = s.clone();
            let (tx, rx) = mpsc::channel();
            workers().spawn(move || { let _ = tx.send(build(world, lighting, dirty, assets, settings, center)); });
            self.rebuild = Some(rx);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn assets() -> Arc<Assets> {
        Arc::new(Assets {
            identity: Default::default(),
            animations: Arc::default(), flowing_tiles: Arc::default(),
            pixels: vec![255; 16 * 16 * 4], tile_size: 16, layers: 1,
            materials: HashMap::new(), fallback: HashMap::new(),
            biome_colors: Default::default(), block_models: Default::default(),
        })
    }
    fn game(positions: impl IntoIterator<Item = ChunkPos>) -> Game {
        let world = World {
            chunks: positions.into_iter().map(|p| (p, Arc::new(Chunk::empty()))).collect(),
            spawn: Vec3::new(0.5, 2., 0.5), label: String::new(),
            save_path: PathBuf::new(), edits: Default::default(), imported: true, light_revision: 0,
        };
        Game {
            options: Options::parse_from(["ferrum189"]), player: Player::new(world.spawn), world,
            sim: Simulation::new(Mode::Creative, 189), lighting: None,
            dirty: HashSet::new(), rebuild: None, uploads: VecDeque::new(), prepared: VecDeque::new(),
            entry_pending: HashSet::from([(0, 0)]), entry_uploaded: Default::default(), visual_revision: 0, chunk_timings: Default::default(), presentation_copy_counts: Default::default(),
            spare_presentation_brains: Default::default(),
            stream: None, stream_center: None, imports_pending: false, attempted: HashSet::new(),
            remote: None, population: None, distant: None,
        }
    }
    fn result(game: &Game, assets: &Arc<Assets>, settings: &Settings,
        positions: &[ChunkPos], lights: &[ChunkPos]) -> MeshWork {
        MeshWork {
            lighting: None, inputs: game.world.chunks.clone(),
            lights: lights.iter().map(|&p| {
                let mut changed = (*game.world.chunks[&p]).clone();
                changed.light = Some(Arc::new(crate::vanilla::lighting::ChunkLight {
                    values: vec![0xf7; 65536], height: vec![0; 256], sections: [true; 16], no_sky: false,
                }));
                (p, Arc::new(changed))
            }).collect(),
            meshes: positions.iter().map(|&p| (p, PreparedChunk::new(crate::models::empty()))).collect(),
            deferred: HashSet::new(), settings: settings.into(), assets: assets.clone(),
            revision: game.world.light_revision,
        }
    }

    #[test]
    fn accepted_neighbor_lighting_rearms_previously_uploaded_entry_before_gate() {
        let assets = assets();
        let settings = Settings::default();
        let mut game = game([(0,0),(1,0)]);
        game.entry_pending.clear();
        game.entry_uploaded.insert((0,0), MeshStamp::new(&game.world,(0,0),&assets,&settings));
        let work = result(&game,&assets,&settings,&[(1,0)],&[(1,0)]);
        game.accept_mesh_work(work,&assets,&settings);
        game.validate_entry_uploads(&assets,&settings);
        assert!(game.entry_pending.contains(&(0,0)));
        assert!(game.dirty.contains(&(0,0)));
    }

    #[test]
    fn distant_mesh_completion_cannot_starve_initial_neighborhood_uploads() {
        let assets = assets();
        let settings = Settings::default();
        let mut game = game((-2..=2).flat_map(|x| (-2..=2).map(move |z| (x,z))));
        let near = mesh_neighbors((0,0));
        game.prepared = prepare_initial(&game.world, near.into_iter()
            .map(|p| (p,crate::models::empty())).collect(), &assets, &settings);
        game.prepared.pop_front(); // first successful upload
        let far = [(-2,-2),(-2,-1),(-2,0),(-2,1),(-2,2),(2,0)];
        let work = result(&game,&assets,&settings,&far,&[]);
        game.accept_mesh_work(work,&assets,&settings);
        assert_eq!(game.prepared.iter().take(8).map(|p|p.position).collect::<Vec<_>>(), near[1..]);
    }

    #[test]
    fn initial_meshes_and_completed_entry_uploads_reject_changed_inputs() {
        let original_assets = assets();
        let original_settings = Settings::default();
        for case in 0..6 {
            let mut game = game([(0, 0), (1, 0)]);
            let meshes = vec![((0, 0), crate::models::empty())];
            let pending = prepare_initial(&game.world, meshes, &original_assets, &original_settings)
                .pop_front().unwrap();
            assert!(pending.state.valid(&game.world, &original_assets, &original_settings));
            game.entry_pending.remove(&(0,0));
            game.entry_uploaded.insert((0,0), MeshStamp::new(&game.world,(0,0),&original_assets,&original_settings));
            let mut settings = original_settings.clone();
            let mut current_assets = original_assets.clone();
            match case {
                0 => { game.world.set(IVec3::new(1,1,1),1<<4); }
                1 => { game.world.set(IVec3::new(16,1,1),1<<4); }
                2 => { game.world.chunks.insert((-1,0),Arc::new(Chunk::empty())); }
                3 => { game.world.unload_chunk((1,0)); }
                4 => { settings.fancy = !settings.fancy; }
                _ => { current_assets = assets(); }
            }
            assert!(!pending.state.valid(&game.world, &current_assets, &settings), "stale initial mesh, case {case}");
            game.validate_entry_uploads(&current_assets,&settings);
            assert!(game.entry_pending.contains(&(0,0)), "already uploaded entry must wait again, case {case}");
            assert!(game.dirty.contains(&(0,0)));
            assert!(game.entry_uploaded.is_empty());
        }
    }

    #[test]
    fn asynchronous_meshes_reject_local_neighbor_new_load_and_unload_changes() {
        let assets = assets();
        let settings = Settings::default();
        for case in 0..5 {
            let mut game = game([(0, 0), (1, 0)]);
            let work = build(snapshot(&game.world), None, HashSet::from([(0, 0)]),
                assets.clone(), settings.clone(), (0, 0));
            assert_eq!(work.meshes.len(), 1);
            match case {
                0 => { game.world.set(IVec3::new(1, 1, 1), 1 << 4); }
                1 => { game.world.set(IVec3::new(16, 1, 1), 1 << 4); }
                2 => { game.world.chunks.insert((-1, 0), Arc::new(Chunk::empty())); game.world.invalidate_lighting(); }
                3 => { game.world.unload_chunk((1, 0)); }
                4 => { game.world.unload_chunk((0, 0)); }
                _ => unreachable!(),
            }
            game.accept_mesh_work(work, &assets, &settings);
            assert!(game.prepared.is_empty(), "stale mesh published for case {case}");
            assert_eq!(game.dirty.contains(&(0, 0)), case != 4);
            // Receiving or rejecting a mesh is not a successful GPU upload.
            assert!(game.entry_pending.contains(&(0, 0)));
        }
    }

    #[test]
    fn asynchronous_meshes_reject_old_settings_and_resource_packs() {
        let original_assets = assets();
        let settings = Settings::default();
        for change in 0..5 {
            let mut game = game([(0, 0)]);
            let work = result(&game, &original_assets, &settings, &[(0, 0)], &[]);
            let mut latest = settings.clone();
            match change {
                0 => latest.fancy = !latest.fancy,
                1 => latest.smooth_lighting = (latest.smooth_lighting + 1) % 3,
                2 => latest.greedy_meshing = !latest.greedy_meshing,
                3 => latest.alternate_blocks = !latest.alternate_blocks,
                _ => (),
            }
            let latest_assets = if change == 4 { assets() } else { original_assets.clone() };
            game.accept_mesh_work(work, &latest_assets, &latest);
            assert!(game.prepared.is_empty(), "obsolete settings/pack accepted: {change}");
            assert!(game.dirty.contains(&(0, 0)));
            assert!(game.entry_pending.contains(&(0, 0)));
        }
    }

    #[test]
    fn bounded_mesh_batches_prioritize_nearest_eight_and_keep_all_deferred_work() {
        let assets = assets();
        let settings = Settings::default();
        let mut game = game((-2..=2).flat_map(|x| (-2..=2).map(move |z| (x, z))));
        game.dirty.extend(game.world.chunks.keys().copied());
        let center = (1, -1);
        let mut expected = game.world.chunks.keys().copied().collect::<Vec<_>>();
        expected.sort_unstable_by_key(|&(x, z)| ((x-center.0).pow(2)+(z-center.1).pow(2), (x,z)));
        let mut published = Vec::new();
        while !game.dirty.is_empty() {
            let work = build(snapshot(&game.world), None, std::mem::take(&mut game.dirty),
                assets.clone(), settings.clone(), center);
            assert!(work.meshes.len() <= 8);
            let batch = work.meshes.iter().map(|(p, _)| *p).collect::<Vec<_>>();
            let start = published.len();
            assert_eq!(batch, expected[start..(start + 8).min(expected.len())]);
            published.extend(batch);
            game.accept_mesh_work(work, &assets, &settings);
            // Stand in for completed GPU uploads before accepting the next batch.
            game.prepared.clear();
        }
        assert_eq!(published, expected);
        assert_eq!(published.len(), 25);
    }

    #[test]
    fn asynchronous_lighting_rejects_changed_column_neighbors_and_two_chunk_dependencies() {
        let assets = assets();
        let settings = Settings::default();
        for changed in [(0, 0), (1, 0), (2, 0)] {
            let mut game = game([(0, 0), (1, 0), (2, 0)]);
            let work = result(&game, &assets, &settings, &[(0, 0)], &[(0, 0)]);
            game.world.set(IVec3::new(changed.0 * 16, 1, 0), 89 << 4);
            let latest = game.world.chunks[&(0, 0)].clone();
            game.accept_mesh_work(work, &assets, &settings);
            assert!(Arc::ptr_eq(&latest, &game.world.chunks[&(0, 0)]),
                "lighting overwritten after input change at {changed:?}");
            assert!(game.prepared.is_empty(), "mesh used rejected lighting after {changed:?}");
            assert!(game.dirty.contains(&(0, 0)), "discarded lighting must be retried");
        }
        // A new second-neighbor is also an input change: absence is stamped.
        let mut game = game([(0, 0)]);
        let work = result(&game, &assets, &settings, &[(0, 0)], &[(0, 0)]);
        let latest = game.world.chunks[&(0, 0)].clone();
        game.world.chunks.insert((2, 0), Arc::new(Chunk::empty()));
        game.world.invalidate_lighting();
        game.accept_mesh_work(work, &assets, &settings);
        assert!(Arc::ptr_eq(&latest, &game.world.chunks[&(0, 0)]));
        assert!(game.prepared.is_empty());
        assert!(game.dirty.contains(&(0, 0)));
    }

    #[test]
    fn asynchronous_lighting_publications_are_validated_before_any_arc_is_replaced() {
        let assets = assets();
        let settings = Settings::default();
        let mut game = game([(0, 0), (1, 0)]);
        let work = result(&game, &assets, &settings, &[(0, 0), (1, 0)], &[(0, 0), (1, 0)]);
        let expected = work.lights.clone();
        game.accept_mesh_work(work, &assets, &settings);
        for (p, expected) in expected {
            assert!(Arc::ptr_eq(&expected, &game.world.chunks[&p]), "sibling publication invalidated {p:?}");
        }
        assert_eq!(game.prepared.len(), 2);
        for pending in &game.prepared {
            assert!(unchanged(&game.world.chunks, &pending.state.expected));
        }
        // An edit after acceptance must invalidate the queued upload stamp too.
        game.world.set(IVec3::new(16, 1, 0), 1 << 4);
        assert!(game.prepared.iter().all(|pending| !unchanged(&game.world.chunks, &pending.state.expected)));
    }

    #[test]
    fn rejected_async_lighting_retries_and_converges_after_new_block_or_tile_edits() {
        let assets = assets();
        let settings = Settings::default();
        let source = IVec3::new(15, 4, 8);
        let obstruction = IVec3::new(16, 4, 8);
        for tile_only in [false, true] {
            let mut game = game((-2..=2).flat_map(|x| (-2..=2).map(move |z| (x, z))));
            // Flat ground ensures all neighborhoods have an initialized light
            // section; this exercises the real light engine and actual meshes.
            for chunk in game.world.chunks.values_mut() {
                for x in 0..16 { for z in 0..16 { Arc::make_mut(chunk).set(x, 0, z, 7 << 4); } }
            }
            game.lighting = Some(LightingView::new(&mut game.world));
            let mut reference = snapshot(&game.world);
            let mut synchronous = LightingView::new(&mut reference);
            game.world.set(source, 89 << 4);
            reference.set(source, 89 << 4);
            synchronous.update(&mut reference);
            let work = build(snapshot(&game.world), game.lighting.take(), HashSet::from([(0, 0)]),
                assets.clone(), settings.clone(), (0, 0));
            assert!(work.lights.iter().any(|(p, _)| *p == (0, 0)));
            let revision = game.world.light_revision;
            if tile_only {
                let data = Arc::new(crate::vanilla::drops::DropNbt::Compound(Default::default()));
                Arc::make_mut(game.world.chunks.get_mut(&(1, 0)).unwrap()).tiles.insert([0, 4, 8], data);
                assert_eq!(game.world.light_revision, revision);
            } else {
                game.world.set(obstruction, 1 << 4);
                reference.set(obstruction, 1 << 4);
                synchronous.update(&mut reference);
            }
            let old_light = game.world.chunks[&(0, 0)].light.clone().unwrap();
            game.accept_mesh_work(work, &assets, &settings);
            assert!(Arc::ptr_eq(&old_light, game.world.chunks[&(0, 0)].light.as_ref().unwrap()));
            assert!(!game.dirty.is_empty());
            // Even a tile-only Arc replacement must cause retry, although it
            // did not advance the authoritative block/light revision.
            for _ in 0..8 {
                game.prepared.clear();
                if game.dirty.is_empty() { break; }
                let work = build(snapshot(&game.world), game.lighting.take(), std::mem::take(&mut game.dirty),
                    assets.clone(), settings.clone(), (0, 0));
                game.accept_mesh_work(work, &assets, &settings);
            }
            assert!(game.dirty.is_empty(), "retry failed to finish, tile_only={tile_only}");
            assert_eq!(game.world.get(source), 89 << 4);
            if !tile_only { assert_eq!(game.world.get(obstruction), 1 << 4); }
            assert_eq!(game.world.light_uv(source - IVec3::X, 0)[0], 14 * 16);
            for (&p, expected) in &reference.chunks {
                let actual = game.world.chunks[&p].light.as_ref().unwrap();
                let expected = expected.light.as_ref().unwrap();
                assert_eq!(actual.values, expected.values, "retry light differs in {p:?}, tile_only={tile_only}");
                assert_eq!(actual.height, expected.height, "retry height differs in {p:?}");
            }
        }
    }
}

#[cfg(test)]
#[path = "client_meshing_input_tests.rs"]
mod input_tests;
