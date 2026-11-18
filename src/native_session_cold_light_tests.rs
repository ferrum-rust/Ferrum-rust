//! Ignored owner-only diagnostic: no production cold storage and no user saves.
use super::*;
use crate::{lighting_view::LightingView, vanilla::lighting::{ChunkLight, LightChunk}, world::World};
use sha1::{Digest, Sha1};
use std::{collections::VecDeque, io::Write, mem::size_of, sync::Weak};

const SEED: i64 = 8_924_706_420_471_469_774;
const RADIUS: i32 = 24;
const LOADED_RADIUS: i32 = RADIUS + 1;
const LIGHT_HALO: i32 = 2;
const TRAVEL_CENTERS: i32 = 90;
const SAMPLE_LIMIT: usize = 64;
const LOD_STRONG_LIMIT: usize = 128;
const LOD_WEAK_LIMIT: usize = 256;
const PACKED_LIMIT: usize = 64;

fn within(p: ChunkPos, center: ChunkPos, radius: i32) -> bool {
    (p.0 - center.0).abs() <= radius && (p.1 - center.1).abs() <= radius
}
fn region(center: ChunkPos, radius: i32) -> Vec<ChunkPos> {
    (-radius..=radius).flat_map(|x| (-radius..=radius)
        .map(move |z| (center.0 + x, center.1 + z))).collect()
}

struct ClientRefs {
    world: World,
    lighting: LightingView,
    center: ChunkPos,
    mesh: crate::world::ChunkMap<Arc<Chunk>>,
    lod: VecDeque<Arc<Chunk>>,
    observed: VecDeque<Weak<Chunk>>,
    packed: VecDeque<Arc<ChunkLight>>,
    evictions: usize,
}
impl ClientRefs {
    fn new() -> Self {
        let mut world = World { chunks: Default::default(), spawn: Default::default(),
            label: "cold-light probe client".into(), save_path: PathBuf::new(),
            edits: Default::default(), imported: true, light_revision: 0 };
        let lighting = LightingView::new(&mut world);
        Self { world, lighting, center: (0, 0), mesh: Default::default(),
            lod: VecDeque::new(), observed: VecDeque::new(), packed: VecDeque::new(), evictions: 0 }
    }
    fn set_center(&mut self, center: ChunkPos) {
        self.center = center;
        let mut expired = self.world.chunks.keys().copied()
            .filter(|p| !within(*p, center, LOADED_RADIUS)).collect::<Vec<_>>();
        expired.sort_unstable();
        for p in expired {
            let chunk = self.world.chunks.remove(&p).unwrap();
            self.evictions += 1;
            self.observed.push_back(Arc::downgrade(&chunk));
            if self.evictions % 4 == 0 {
                if let Some(light) = &chunk.light { self.packed.push_back(light.clone()); }
            }
            self.lod.push_back(chunk);
        }
        while self.lod.len() > LOD_STRONG_LIMIT { self.lod.pop_front(); }
        while self.observed.len() > LOD_WEAK_LIMIT { self.observed.pop_front(); }
        while self.packed.len() > PACKED_LIMIT { self.packed.pop_front(); }
        self.world.invalidate_lighting();
        self.lighting.update(&mut self.world);
    }
    fn accept(&mut self, update: Update) {
        // One prior client map models a bounded outstanding mesh snapshot. This
        // does not pretend to execute actual renderer/LOD jobs or their timing.
        self.mesh = self.world.chunks.clone();
        for (p, chunk) in update.chunks {
            if within(p, self.center, LOADED_RADIUS) { self.world.chunks.insert(p, chunk); }
        }
        self.world.invalidate_lighting();
        self.lighting.update(&mut self.world);
        assert!(self.world.chunks.len() <= (2 * LOADED_RADIUS + 1).pow(2) as usize);
    }
    fn retire_auxiliary(&mut self) {
        self.mesh.clear(); self.lod.clear(); self.observed.clear(); self.packed.clear();
    }
}

#[derive(Default, Serialize)]
struct WorkTimes { commands: usize, generation_ms: f64, publication_ms: f64, client_refs_ms: f64 }
fn request(worker: &mut Worker, client: &mut ClientRefs, command: Command, times: &mut WorkTimes) -> Reply {
    validate_command(&command).unwrap();
    assert!(!matches!(command, Command::Snapshot { .. } | Command::Shutdown { .. }));
    let deferred = matches!(&command, Command::LoadSpawnBatchDeferred { .. });
    let start = Instant::now();
    let (reply, stop) = worker.execute(command).unwrap();
    times.generation_ms += start.elapsed().as_secs_f64() * 1000.;
    assert!(!stop);
    let start = Instant::now();
    let update = worker.update_after_reply(&reply, deferred);
    times.publication_ms += start.elapsed().as_secs_f64() * 1000.;
    let start = Instant::now();
    match &reply {
        Reply::SpawnPrepared { point, .. } => {
            client.world.spawn = IVec3::from_array(*point).as_vec3();
            client.set_center((point[0].div_euclid(16), point[2].div_euclid(16)));
        }
        Reply::PlayerPrepared { initial } => client.set_center((
            (initial.position[0].floor() as i32).div_euclid(16),
            (initial.position[2].floor() as i32).div_euclid(16))),
        _ => {}
    }
    client.accept(update);
    times.client_refs_ms += start.elapsed().as_secs_f64() * 1000.;
    times.commands += 1;
    reply
}

// Same requested-target order, retained-chunk filter and ordered halo as the
// host, without wall-clock-dependent skipped camera centers or game ticks.
fn fill_view(worker: &mut Worker, client: &mut ClientRefs, initial: bool, times: &mut WorkTimes) {
    let targets = region(client.center, LOADED_RADIUS).into_iter()
        .filter(|p| !client.world.chunks.contains_key(p)).collect::<Vec<_>>();
    let target_set = targets.iter().copied().collect::<BTreeSet<_>>();
    let mut halo = BTreeSet::new();
    for &(x, z) in &targets { for dx in -1..=1 { for dz in -1..=1 {
        let p = (x + dx, z + dz);
        if !target_set.contains(&p) && !worker.runtime.provider.contains(p) { halo.insert(p); }
    } } }
    let plan = targets.into_iter().chain(halo)
        .filter(|p| !client.world.chunks.contains_key(p)).collect::<Vec<_>>();
    let batch_size = if initial { 32 } else { 8 };
    for (index, batch) in plan.chunks(batch_size).enumerate() {
        let command = if initial {
            let next = ((index + 1) * batch_size).min(plan.len());
            Command::GenerateAhead { positions: batch.to_vec(),
                ahead: plan[next..(next + 64).min(plan.len())].to_vec() }
        } else { Command::Generate(batch.to_vec()) };
        assert!(matches!(request(worker, client, command, times), Reply::Generated));
    }
}

#[derive(Default, Debug, PartialEq, Eq, Serialize)]
struct Census {
    columns: usize, light_columns: usize, missing_light: usize,
    near_columns: usize, dirty_block_columns: usize, dirty_light_columns: usize,
    cold_clean_columns: usize, sky_bytes: usize, block_bytes: usize,
    raw_capacity_bytes: usize, cold_raw_bytes: usize, cold_raw_capacity_bytes: usize,
    unique_chunk_columns: usize, strong_shared_columns: usize, weak_only_columns: usize,
    packed_columns: usize, packed_payload_bytes: usize, packed_capacity_bytes: usize,
    packed_shared_columns: usize, packed_weak_columns: usize,
    cold_unique_chunk_columns: usize, cold_removable_packed_capacity_bytes: usize,
    cold_immediately_reclaimable_packed_capacity_bytes: usize,
}
fn sample_column(worker: &Worker, center: ChunkPos, p: ChunkPos, out: &mut Census) -> bool {
    let b = &worker.runtime.backend;
    let chunk = &b.world.chunks[&p];
    out.columns += 1;
    let near = within(p, center, LOADED_RADIUS + LIGHT_HALO);
    let dirty_block = b.dirty_chunks.contains(&p);
    let dirty_light = b.light.diagnostic_render_dirty().contains(&p);
    out.near_columns += usize::from(near);
    out.dirty_block_columns += usize::from(dirty_block);
    out.dirty_light_columns += usize::from(dirty_light);
    let strong = Arc::strong_count(chunk);
    let weak = Arc::weak_count(chunk);
    let unique = strong == 1 && weak == 0;
    out.unique_chunk_columns += usize::from(unique);
    out.strong_shared_columns += usize::from(strong > 1);
    out.weak_only_columns += usize::from(strong == 1 && weak > 0);
    let Some(light) = b.light.chunks.get(&p) else { out.missing_light += 1; return false; };
    out.light_columns += 1;
    out.sky_bytes += light.sky.len(); out.block_bytes += light.block.len();
    out.raw_capacity_bytes += light.sky.capacity() + light.block.capacity();
    let cold = !near && !dirty_block && !dirty_light;
    if cold {
        out.cold_clean_columns += 1;
        out.cold_raw_bytes += light.sky.len() + light.block.len();
        out.cold_raw_capacity_bytes += light.sky.capacity() + light.block.capacity();
        out.cold_unique_chunk_columns += usize::from(unique);
    }
    if let Some(packed) = &chunk.light {
        let bytes = packed.values.capacity() + packed.height.capacity() * size_of::<i32>();
        out.packed_columns += 1;
        out.packed_payload_bytes += packed.values.len() + packed.height.len() * size_of::<i32>();
        out.packed_capacity_bytes += bytes;
        out.packed_shared_columns += usize::from(Arc::strong_count(packed) > 1);
        out.packed_weak_columns += usize::from(Arc::weak_count(packed) > 0);
        if cold && unique {
            out.cold_removable_packed_capacity_bytes += bytes;
            if Arc::strong_count(packed) == 1 {
                // Weak refs keep the Arc header alive, not its Vec payloads.
                out.cold_immediately_reclaimable_packed_capacity_bytes += bytes;
            }
        }
    }
    cold
}
fn census(worker: &Worker, center: ChunkPos) -> (Census, Vec<ChunkPos>) {
    let mut result = Census::default();
    let mut cold = Vec::new();
    for &p in worker.runtime.provider.loaded_order() {
        if sample_column(worker, center, p, &mut result) { cold.push(p); }
    }
    assert_eq!(result.columns, worker.runtime.backend.world.chunks.len());
    assert_eq!(result.columns, result.unique_chunk_columns + result.strong_shared_columns + result.weak_only_columns);
    (result, cold)
}
fn bounded_sweep(worker: &Worker, center: ChunkPos) -> (Census, usize, f64) {
    let order = worker.runtime.provider.loaded_order();
    let mut result = Census::default();
    let mut cursor = 0;
    let mut calls = 0;
    let mut max_batch_ms = 0.0f64;
    // O(history) only as a complete idle test sweep. Each future owner call
    // would retain cursor and perform just this inner <=64-position batch.
    while cursor < order.len() {
        let begin = Instant::now();
        let end = (cursor + 64).min(order.len());
        for &p in &order[cursor..end] { sample_column(worker, center, p, &mut result); }
        assert!(end - cursor <= 64);
        cursor = end; calls += 1;
        max_batch_ms = max_batch_ms.max(begin.elapsed().as_secs_f64() * 1000.);
    }
    (result, calls, max_batch_ms)
}

struct HashWriter(Sha1);
impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> { self.0.update(bytes); Ok(bytes.len()) }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}
fn hash_json(writer: &mut HashWriter, value: &impl Serialize) {
    serde_json::to_writer(&mut *writer, value).unwrap();
    writer.0.update([0xff]);
}
fn native_digest(worker: &Worker) -> String {
    let b = &worker.runtime.backend;
    let mut h = HashWriter(Sha1::new());
    hash_json(&mut h, &worker.runtime.provider);
    hash_json(&mut h, &(b.seed, b.map_features, b.provider_random.state(), b.world_random.state(),
        b.world_spawn, b.initial_player.as_ref(), b.light.no_sky, b.light.sky_subtracted));
    hash_json(&mut h, &(b.world.spawn, &b.world.label, &b.world.save_path, b.world.imported, b.world.light_revision));
    let mut edits = b.world.edits.iter().collect::<Vec<_>>(); edits.sort_unstable_by_key(|(p, _)| **p);
    hash_json(&mut h, &edits);
    for &p in worker.runtime.provider.loaded_order() {
        let chunk = &b.world.chunks[&p];
        let light = &b.light.chunks[&p];
        hash_json(&mut h, &(p, chunk.top, light.minimum, light.sections, light.queued_checks));
        // Chunk bytes are streamed in canonical little-endian form with bounded
        // scratch space, never copied into a full RuntimeSnapshot or JSON tree.
        chunk.blocks.with_slice(|blocks| {
            for part in blocks.chunks(4096) {
                let mut bytes = [0u8; 8192];
                for (dst, value) in bytes.chunks_exact_mut(2).zip(part) { dst.copy_from_slice(&value.to_le_bytes()); }
                h.0.update(&bytes[..part.len() * 2]);
            }
        });
        h.0.update(chunk.biomes);
        light.sky.with_bytes(|bytes| h.0.update(bytes)); light.block.with_bytes(|bytes| h.0.update(bytes));
        hash_json(&mut h, &light.height); hash_json(&mut h, &light.gaps);
        hash_json(&mut h, &chunk.containers); hash_json(&mut h, &chunk.entities); hash_json(&mut h, &chunk.hoppers);
        let mut pistons = chunk.pistons.iter().collect::<Vec<_>>(); pistons.sort_unstable_by_key(|(p, _)| **p);
        hash_json(&mut h, &pistons);
        let mut tiles = chunk.tiles.iter().map(|(p, tag)| (p, tag.as_ref())).collect::<Vec<_>>();
        tiles.sort_unstable_by_key(|(p, _)| **p); hash_json(&mut h, &tiles);
        if let Some(packed) = &chunk.light {
            h.0.update([1]); h.0.update(&packed.values);
            hash_json(&mut h, &(&packed.height, packed.sections, packed.no_sky));
        } else { h.0.update([0]); }
    }
    hash_json(&mut h, &b.structures); hash_json(&mut h, &b.ticks);
    hash_json(&mut h, &b.output); hash_json(&mut h, &b.events);
    hash_json(&mut h, &(&b.block_effects.changed, &b.block_effects.drops, &b.block_effects.washed,
        &b.block_effects.primed_tnt, &b.block_effects.note_updates,
        &b.block_effects.sounds, &b.block_effects.mix_smoke));
    hash_json(&mut h, &b.block_effects.impacts.len());
    for impact in &b.block_effects.impacts {
        hash_json(&mut h, &(&impact.bounds, impact.anvil, impact.amount));
    }
    hash_json(&mut h, &b.entities); hash_json(&mut h, &b.falling);
    hash_json(&mut h, &b.animals); hash_json(&mut h, &b.drops); hash_json(&mut h, &b.pending_tnt);
    hash_json(&mut h, &b.structure_entities);
    hash_json(&mut h, &(b.tile_revision, &b.player_inventory, b.do_tile_drops, &b.fire_environment, b.double_plant_type));
    hash_json(&mut h, &b.drop_tiles.iter().collect::<Vec<_>>());
    hash_json(&mut h, &b.plains_counts);
    hash_json(&mut h, &b.live_containers.values().collect::<Vec<_>>());
    hash_json(&mut h, &b.live_spawners.iter().collect::<Vec<_>>());
    hash_json(&mut h, &b.precipitation.iter().collect::<Vec<_>>());
    let dirty = b.dirty_chunks.iter().copied().collect::<BTreeSet<_>>(); hash_json(&mut h, &dirty);
    let dirty = b.light.diagnostic_render_dirty().iter().copied().collect::<BTreeSet<_>>(); hash_json(&mut h, &dirty);
    format!("{:x}", h.0.finalize())
}

#[derive(Serialize)]
struct CodecSample {
    position: ChunkPos, raw_bytes: usize, compressed_bytes: usize, compressed_capacity_bytes: usize,
    encode_ms: f64, decode_ms: f64, max_array_encode_ms: f64, max_array_decode_ms: f64,
    pack_ms: f64, temporary_payload_capacity_bytes: usize,
    simple_payload_bytes: usize, simple_retained_encoded_capacity_bytes: usize,
    simple_encode_ms: f64, simple_decode_ms: f64,
    simple_max_array_encode_ms: f64, simple_max_array_decode_ms: f64,
    simple_uniform_arrays: usize, simple_rle_arrays: usize, simple_raw_arrays: usize,
    simple_max_array_temporary_payload_capacity_bytes: usize,
}
fn codec_samples(worker: &Worker, cold: &[ChunkPos]) -> Vec<CodecSample> {
    let count = cold.len().min(SAMPLE_LIMIT);
    let mut rows = Vec::with_capacity(count);
    for sample in 0..count {
        let index = if count <= 1 { 0 } else { sample * (cold.len() - 1) / (count - 1) };
        let p = cold[index];
        let light = &worker.runtime.backend.light.chunks[&p];
        let mut row = CodecSample { position: p, raw_bytes: 0, compressed_bytes: 0,
            compressed_capacity_bytes: 0, encode_ms: 0., decode_ms: 0.,
            max_array_encode_ms: 0., max_array_decode_ms: 0., pack_ms: 0., temporary_payload_capacity_bytes: 0,
            simple_payload_bytes: 0, simple_retained_encoded_capacity_bytes: 0,
            simple_encode_ms: 0., simple_decode_ms: 0., simple_max_array_encode_ms: 0., simple_max_array_decode_ms: 0.,
            simple_uniform_arrays: 0, simple_rle_arrays: 0, simple_raw_arrays: 0,
            simple_max_array_temporary_payload_capacity_bytes: 0 };
        let mut decoded = Vec::with_capacity(2);
        let mut retained_decode_bytes = 0;
        for raw in [&light.sky, &light.block] {
            raw.with_bytes(|raw| {
            assert_eq!(raw.len(), 65536); assert!(raw.iter().all(|v| *v <= 15));
            measure_simple(raw, &mut row);
            let begin = Instant::now();
            let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), Compression::fast());
            encoder.write_all(raw).unwrap();
            let compressed = encoder.finish().unwrap();
            let encode_ms = begin.elapsed().as_secs_f64() * 1000.;
            let begin = Instant::now();
            let mut unpacked = Vec::with_capacity(65536);
            flate2::read::ZlibDecoder::new(compressed.as_slice()).take(65537)
                .read_to_end(&mut unpacked).unwrap();
            let decode_ms = begin.elapsed().as_secs_f64() * 1000.;
            assert_eq!(unpacked.as_slice(), raw);
            assert!(unpacked.iter().all(|v| *v <= 15));
            row.raw_bytes += raw.len(); row.compressed_bytes += compressed.len();
            row.compressed_capacity_bytes += compressed.capacity();
            row.encode_ms += encode_ms; row.decode_ms += decode_ms;
            row.max_array_encode_ms = row.max_array_encode_ms.max(encode_ms);
            row.max_array_decode_ms = row.max_array_decode_ms.max(decode_ms);
            row.temporary_payload_capacity_bytes = row.temporary_payload_capacity_bytes.max(
                retained_decode_bytes + compressed.capacity() + unpacked.capacity());
            retained_decode_bytes += unpacked.capacity();
            decoded.push(unpacked);
            });
        }
        let shadow = LightChunk { sky: decoded.remove(0).into(), block: decoded.remove(0).into(),
            height: light.height.clone(), minimum: light.minimum, sections: light.sections,
            gaps: light.gaps.clone(), queued_checks: light.queued_checks };
        let begin = Instant::now();
        let packed = ChunkLight::from_chunk(&shadow, worker.runtime.backend.light.no_sky);
        row.pack_ms = begin.elapsed().as_secs_f64() * 1000.;
        assert!(packed.values.iter().zip(shadow.sky.iter().zip(&shadow.block))
            .all(|(&p, (&s, &b))| p == ((s << 4) | b)));
        row.temporary_payload_capacity_bytes = row.temporary_payload_capacity_bytes.max(
            retained_decode_bytes + packed.values.capacity() + (shadow.height.capacity() + packed.height.capacity()) * size_of::<i32>()
                + shadow.gaps.capacity() * size_of::<bool>());
        rows.push(row);
    }
    rows
}

fn checkpoint_report(worker: &Worker, client: &ClientRefs, step: i32) -> Value {
    let begin = Instant::now(); let before = native_digest(worker);
    let hash_before_ms = begin.elapsed().as_secs_f64() * 1000.;
    let rng_before = (worker.runtime.backend.provider_random.state(), worker.runtime.backend.world_random.state());
    let begin = Instant::now(); let (exact, cold) = census(worker, client.center);
    let census_ms = begin.elapsed().as_secs_f64() * 1000.;
    let begin = Instant::now(); let (sampled, sweep_calls, max_batch_ms) = bounded_sweep(worker, client.center);
    let sweep_ms = begin.elapsed().as_secs_f64() * 1000.;
    assert_eq!(exact, sampled, "bounded sweep must cover exactly the idle census");
    let begin = Instant::now(); let codecs = codec_samples(worker, &cold);
    let codec_ms = begin.elapsed().as_secs_f64() * 1000.;
    let begin = Instant::now(); let after = native_digest(worker);
    let hash_after_ms = begin.elapsed().as_secs_f64() * 1000.;
    let rng_after = (worker.runtime.backend.provider_random.state(), worker.runtime.backend.world_random.state());
    assert_eq!(before, after, "read-only census and codec must not change native state");
    assert_eq!(rng_before, rng_after);
    serde_json::json!({"step":step,"center":client.center,"client_columns":client.world.chunks.len(),
        "controlled_references":{"mesh_columns":client.mesh.len(),"lod_strong":client.lod.len(),
            "lod_weak":client.observed.len(),"packed":client.packed.len()},
        "census":exact,"bounded_sweep_calls":sweep_calls,"bounded_sweep_max_positions":64,
        "census_ms":census_ms,"bounded_sweep_ms":sweep_ms,"bounded_sweep_max_batch_ms":max_batch_ms,"codec_wall_ms":codec_ms,
        "hash_before_ms":hash_before_ms,"hash_after_ms":hash_after_ms,
        "canonical_hash_before":before,"canonical_hash_after":after,"rng_before":rng_before,"rng_after":rng_after,
        "codec_samples":codecs})
}

#[test]
#[ignore = "large in-memory native travel/light codec probe; run alone with optimized tests"]
fn retained_travel_light_eligibility_and_codec_probe() {
    let config = Config::new(SEED, "cold-light-probe", PathBuf::new());
    let (mut worker, resumed) = Worker::open(config, PathBuf::new(), Some(1234)).unwrap();
    assert!(!resumed && worker.path.as_os_str().is_empty() && worker._lock.is_none());
    let mut client = ClientRefs::new();
    client.accept(worker.update(true));
    let mut times = WorkTimes::default();
    assert!(matches!(request(&mut worker, &mut client, Command::PrepareSpawn, &mut times), Reply::SpawnPrepared { .. }));
    loop {
        let Reply::SpawnAreaProgress { loaded } = request(&mut worker, &mut client,
            Command::LoadSpawnBatchDeferred { count: 32 }, &mut times) else { panic!("spawn progress"); };
        if loaded == 625 { break; }
    }
    assert!(matches!(request(&mut worker, &mut client, Command::PreparePlayer { adventure: false }, &mut times), Reply::PlayerPrepared { .. }));
    fill_view(&mut worker, &mut client, true, &mut times);
    let origin = client.center;
    let mut reports = vec![checkpoint_report(&worker, &client, 0)];
    for step in 1..=TRAVEL_CENTERS {
        let begin = Instant::now();
        client.set_center((origin.0 + step, origin.1));
        times.client_refs_ms += begin.elapsed().as_secs_f64() * 1000.;
        fill_view(&mut worker, &mut client, false, &mut times);
        if step % 30 == 0 { reports.push(checkpoint_report(&worker, &client, step)); }
    }
    let before = native_digest(&worker);
    client.retire_auxiliary();
    let retired = census(&worker, client.center).0;
    assert_eq!(before, native_digest(&worker), "retiring external references must not change owner data");
    let exe_hash = std::env::current_exe().ok().and_then(|p| File::open(p).ok()).map(|mut file| {
        let mut h = Sha1::new(); let mut buffer = [0u8; 65536];
        loop { let n = file.read(&mut buffer).unwrap(); if n == 0 { break; } h.update(&buffer[..n]); }
        format!("{:x}", h.finalize())
    });
    let report = serde_json::json!({"kind":"isolated_native_cold_light_shadow_probe","seed":SEED,"world_random_seed":1234,
        "simple_codec_note":"Uniform stores one inline byte; RLE stores little-endian u16 nonzero count + u8 value. Raw fallback retains original dense bytes with no decode allocation. Candidate is diagnostic only. Codec timings use the same native arrays in fixed Simple-then-Zlib order; no standalone speedup claim. Temporary payload capacities exclude original raw storage, codec workspaces/allocator headers, and discarded fallback candidates.",
        "test_executable_sha1":exe_hash,"radius":RADIUS,"loaded_radius":LOADED_RADIUS,"light_halo":LIGHT_HALO,
        "travel_centers":TRAVEL_CENTERS,"steps_between_checkpoints":30,"sample_limit_per_checkpoint":SAMPLE_LIMIT,
        "retention_note":"The fixed route bounds requested camera centers, not cascading native generation or process RSS. External reference queues and codec samples have explicit caps; authoritative provider retention is intentionally preserved.",
        "note":"Controlled fixed-center owner replay, not an exact asynchronous flight or GPU benchmark. No compression is installed, no logical chunks are unloaded, no user save is opened. Canonical borrowed hash covers terrain/light/provider/order/RNG and public native state, not private generator caches. Temporary payload capacity excludes codec-internal workspaces and allocator headers. Every codec sample passed exact byte/nibble roundtrip assertions.",
        "timing":times,"checkpoints":reports,"final_census_after_auxiliary_retirement":retired});
    fs::create_dir_all("reports").unwrap();
    fs::write("reports/native-cold-light-eligibility-probe.json", serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    eprintln!("Native cold-light probe completed: {} retained, {} client columns; report written without saves",
        worker.runtime.provider.loaded_order().len(), client.world.chunks.len());
}

// Diagnostic candidate only. No production array representation uses this
// format. Native generation and the world RNG never participate in encoding.
#[derive(Debug)]
enum SimpleEncoding {
    Uniform(u8),
    Runs(Vec<u8>), // repeated [length low, length high, value], length != 0
    Raw,          // Keep original Dense bytes; no second allocation or thaw.
}
fn encode_simple(raw: &[u8]) -> SimpleEncoding {
    if raw.is_empty() || raw.len() > 65536 { return SimpleEncoding::Raw; }
    let mut encoded = Vec::new();
    let mut start = 0;
    while start < raw.len() {
        let value = raw[start];
        let mut end = start + 1;
        while end < raw.len() && raw[end] == value { end += 1; }
        if start == 0 && end == raw.len() { return SimpleEncoding::Uniform(value); }
        if start == 0 { encoded.reserve(raw.len().min(512)); }
        let mut remaining = end - start;
        while remaining != 0 {
            // Bail before the output becomes as large as its input. Worst-case
            // alternating data is kept Dense rather than expanding threefold.
            if encoded.len() + 3 >= raw.len() { return SimpleEncoding::Raw; }
            let count = remaining.min(u16::MAX as usize) as u16;
            encoded.extend_from_slice(&count.to_le_bytes());
            encoded.push(value);
            remaining -= usize::from(count);
        }
        start = end;
    }
    SimpleEncoding::Runs(encoded)
}
fn decode_simple(encoded: &SimpleEncoding, length: usize) -> Result<Option<Vec<u8>>> {
    ensure!(length <= 65536, "Simple light output exceeds one native array");
    match encoded {
        SimpleEncoding::Uniform(value) => Ok(Some(vec![*value; length])),
        SimpleEncoding::Raw => Ok(None),
        SimpleEncoding::Runs(runs) => {
            ensure!(runs.len() % 3 == 0, "Truncated simple light run");
            let mut result = Vec::with_capacity(length);
            for run in runs.chunks_exact(3) {
                let count = usize::from(u16::from_le_bytes([run[0], run[1]]));
                ensure!(count != 0 && count <= length.saturating_sub(result.len()),
                    "Simple light run exceeds exact output size");
                result.resize(result.len() + count, run[2]);
            }
            ensure!(result.len() == length, "Simple light output length differs");
            Ok(Some(result))
        }
    }
}
fn measure_simple(raw: &[u8], row: &mut CodecSample) {
    let begin = Instant::now();
    let encoded = encode_simple(raw);
    let encode_ms = begin.elapsed().as_secs_f64() * 1000.;
    let begin = Instant::now();
    let decoded = decode_simple(&encoded, raw.len()).unwrap();
    let decode_ms = begin.elapsed().as_secs_f64() * 1000.;
    let (bytes, capacity) = match &encoded {
        SimpleEncoding::Uniform(_) => { row.simple_uniform_arrays += 1; (1, 0) }
        SimpleEncoding::Runs(runs) => { row.simple_rle_arrays += 1; (runs.len(), runs.capacity()) }
        SimpleEncoding::Raw => { row.simple_raw_arrays += 1; (raw.len(), 0) }
    };
    if let Some(decoded) = &decoded {
        assert_eq!(decoded.as_slice(), raw);
        assert!(decoded.iter().all(|v| *v <= 15));
    } else {
        assert!(matches!(encoded, SimpleEncoding::Raw), "only dense fallback omits decoding");
    }
    row.simple_payload_bytes += bytes;
    row.simple_retained_encoded_capacity_bytes += capacity;
    row.simple_encode_ms += encode_ms;
    row.simple_decode_ms += decode_ms;
    row.simple_max_array_encode_ms = row.simple_max_array_encode_ms.max(encode_ms);
    row.simple_max_array_decode_ms = row.simple_max_array_decode_ms.max(decode_ms);
    row.simple_max_array_temporary_payload_capacity_bytes = row.simple_max_array_temporary_payload_capacity_bytes
        .max(capacity + decoded.as_ref().map_or(0, Vec::capacity));
}

#[test]
fn simple_light_probe_codec_preserves_runs_uniform_and_dense_fallback() {
    let mut random = 0x8131e4d3278abcf9u64;
    let noise = (0..65536).map(|_| {
        random ^= random << 13; random ^= random >> 7; random ^= random << 17;
        (random >> 32) as u8 & 15
    }).collect::<Vec<_>>();
    let mut almost_uniform = vec![7; 65536]; almost_uniform[65535] = 3;
    for input in [vec![0; 65536], vec![15; 65536], almost_uniform,
        (0..65536).map(|i| ((i / 97) % 16) as u8).collect(), noise] {
        let encoded = encode_simple(&input);
        if let Some(decoded) = decode_simple(&encoded, input.len()).unwrap() {
            assert_eq!(decoded, input);
        } else { assert!(matches!(encoded, SimpleEncoding::Raw)); }
    }
    assert!(matches!(encode_simple(&vec![11; 65536]), SimpleEncoding::Uniform(11)));
    assert!(matches!(encode_simple(&(0..65536).map(|i| (i & 1) as u8).collect::<Vec<_>>()), SimpleEncoding::Raw));
    let split_run = SimpleEncoding::Runs(vec![255, 255, 7, 1, 0, 7]);
    assert_eq!(decode_simple(&split_run, 65536).unwrap().unwrap(), vec![7; 65536]);
    for bytes in [vec![0, 0, 1], vec![1, 0], vec![2, 0, 1], vec![255, 255, 1, 2, 0, 2]] {
        assert!(decode_simple(&SimpleEncoding::Runs(bytes), 1).is_err());
    }
    assert!(decode_simple(&SimpleEncoding::Runs(vec![1, 0, 4]), 2).is_err());
    assert!(decode_simple(&SimpleEncoding::Uniform(0), 65537).is_err());
}

fn maintained_request(worker: &mut Worker, client: &mut ClientRefs, command: Command, times: &mut WorkTimes) -> Reply {
    let mut view = maintenance::ViewState::default();
    view.desired(client.center, LOADED_RADIUS); view.commit(client.center, LOADED_RADIUS);
    view.first_play_presented(); // this isolated replay has finished PreparePlayer
    let context = maintenance::Context::capture(&command, view.hint(),
        worker.maintenance_policy.enabled(), worker.fresh_spawn_publication);
    validate_command(&command).unwrap();
    assert!(!matches!(command, Command::Snapshot { .. } | Command::Shutdown { .. }));
    let deferred = matches!(&command, Command::LoadSpawnBatchDeferred { .. });
    let start = Instant::now();
    let (reply, stop) = worker.execute(command).unwrap();
    times.generation_ms += start.elapsed().as_secs_f64() * 1000.;
    assert!(!stop);
    let context = maintenance::successful(context, &reply);
    let start = Instant::now();
    let update = worker.update_after_reply(&reply, deferred);
    times.publication_ms += start.elapsed().as_secs_f64() * 1000.;
    let start = Instant::now();
    match &reply {
        Reply::SpawnPrepared { point, .. } => {
            client.world.spawn = IVec3::from_array(*point).as_vec3();
            client.set_center((point[0].div_euclid(16), point[2].div_euclid(16)));
        }
        Reply::PlayerPrepared { initial } => client.set_center((
            (initial.position[0].floor() as i32).div_euclid(16),
            (initial.position[2].floor() as i32).div_euclid(16))),
        _ => {}
    }
    client.accept(update);
    times.client_refs_ms += start.elapsed().as_secs_f64() * 1000.;
    let before = (worker.runtime.provider.loaded_order().len(),
        worker.runtime.backend.provider_random.state(), worker.runtime.backend.world_random.state());
    let block_dirty = worker.runtime.backend.dirty_chunks.clone();
    let light_dirty = worker.runtime.backend.light.diagnostic_render_dirty().clone();
    worker.maintenance_policy.after_publication(&mut worker.runtime, &mut worker.maintenance_cursor, context);
    worker.generation_timings.maintenance = worker.maintenance_policy.metrics();
    assert_eq!(before, (worker.runtime.provider.loaded_order().len(),
        worker.runtime.backend.provider_random.state(), worker.runtime.backend.world_random.state()));
    assert_eq!(block_dirty, worker.runtime.backend.dirty_chunks);
    assert_eq!(&light_dirty, worker.runtime.backend.light.diagnostic_render_dirty());
    times.commands += 1;
    reply
}

fn fill_maintained_view(worker: &mut Worker, client: &mut ClientRefs, initial: bool, times: &mut WorkTimes) {
    let targets = region(client.center, LOADED_RADIUS).into_iter()
        .filter(|p| !client.world.chunks.contains_key(p)).collect::<Vec<_>>();
    let target_set = targets.iter().copied().collect::<BTreeSet<_>>();
    let mut halo = BTreeSet::new();
    for &(x, z) in &targets { for dx in -1..=1 { for dz in -1..=1 {
        let p = (x + dx, z + dz);
        if !target_set.contains(&p) && !worker.runtime.provider.contains(p) { halo.insert(p); }
    } } }
    let plan = targets.into_iter().chain(halo)
        .filter(|p| !client.world.chunks.contains_key(p)).collect::<Vec<_>>();
    let batch_size = if initial { 32 } else { 8 };
    for (index, batch) in plan.chunks(batch_size).enumerate() {
        let command = if initial {
            let next = ((index + 1) * batch_size).min(plan.len());
            Command::GenerateAhead { positions: batch.to_vec(),
                ahead: plan[next..(next + 64).min(plan.len())].to_vec() }
        } else { Command::Generate(batch.to_vec()) };
        assert!(matches!(maintained_request(worker, client, command, times), Reply::Generated));
    }
}


fn maintained_block_storage(worker: &Worker) -> Value {
    let (mut dense, mut cold, mut cached) = (0usize, 0usize, 0usize);
    let (mut logical, mut raw, mut encoded, mut payload) = (0usize, 0usize, 0usize, 0usize);
    for chunk in worker.runtime.backend.world.chunks.values() {
        let blocks = &chunk.blocks;
        dense += usize::from(blocks.is_dense()); cold += usize::from(blocks.is_cold());
        cached += usize::from(blocks.has_read_cache()); logical += blocks.len() * 2;
        raw += blocks.capacity() * 2; encoded += blocks.encoded_allocation_bytes();
        payload += blocks.encoded_payload_bytes();
    }
    serde_json::json!({"dense_columns":dense,"cold_columns":cold,"cached_cold_columns":cached,
        "logical_raw_bytes":logical,"remaining_raw_capacity_bytes":raw,
        "encoded_heap_payload_bytes":encoded,"encoded_payload_bytes":payload,
        "resident_payload_capacity_bytes":raw+encoded,
        "scope":"Current authoritative owner census at an idle diagnostic checkpoint. No decode, cache creation, recency mutation or provider request. Excludes allocator/Arc/Box metadata and separate client/retained snapshots; not process RSS or a cumulative release subtraction."})
}

#[test]
fn block_census_and_canonical_digest_do_not_hydrate_or_mark_cold_owner_storage() {
    let config = Config::new(189, "cold-block-digest", PathBuf::new());
    let (mut worker, _) = Worker::open(config, PathBuf::new(), Some(1234)).unwrap();
    worker.runtime.provide((0, 0)).unwrap(); drop(worker.update(false));
    let dense_hash = native_digest(&worker);
    let mut compressed = 0;
    for chunk in worker.runtime.backend.world.chunks.values_mut() {
        let blocks = &mut Arc::get_mut(chunk).unwrap().blocks;
        compressed += usize::from(blocks.compress_if_saving(4096));
    }
    assert!(compressed > 0);
    let storage = maintained_block_storage(&worker);
    assert_eq!(native_digest(&worker), dense_hash);
    assert_eq!(maintained_block_storage(&worker), storage);
    for chunk in worker.runtime.backend.world.chunks.values_mut() {
        let blocks = &mut Arc::get_mut(chunk).unwrap().blocks;
        assert!(!blocks.has_read_cache()); assert!(!blocks.take_recent_cold_access());
    }
}

fn maintained_storage_report(worker: &Worker, client: &ClientRefs, step: i32, times: &WorkTimes) -> Value {
    let mut dense = 0usize; let mut cold = 0usize; let mut cached = 0usize;
    let mut raw_capacity = 0usize; let mut encoded_payload = 0usize; let mut encoded_allocation = 0usize;
    for c in worker.runtime.backend.light.chunks.values() {
        for bytes in [&c.sky, &c.block] {
            dense += usize::from(!bytes.is_cold()); cold += usize::from(bytes.is_cold());
            cached += usize::from(bytes.has_read_cache()); raw_capacity += bytes.capacity();
            encoded_payload += bytes.encoded_bytes(); encoded_allocation += bytes.encoded_capacity_bytes();
        }
    }
    let hash = native_digest(worker);
    let metrics = worker.maintenance_policy.metrics();
    serde_json::json!({"step":step,"center":client.center,"client_columns":client.world.chunks.len(),
        "canonical_hash":hash,
        "rng":[worker.runtime.backend.provider_random.state(),worker.runtime.backend.world_random.state()],
        "census":census(worker,client.center).0,"maintenance":metrics,"path_work":times,
        "block_storage":maintained_block_storage(worker),
        "path_work_plus_maintenance_ms":times.generation_ms+times.publication_ms+times.client_refs_ms+metrics.pass_ms+metrics.block_storage.pass_ms,
        "light_storage":{"dense_arrays":dense,"cold_arrays":cold,"read_cache_arrays":cached,
            "raw_capacity_bytes":raw_capacity,"encoded_payload_bytes":encoded_payload,
            "encoded_heap_payload_bytes":encoded_allocation},
        "controlled_references":{"mesh_columns":client.mesh.len(),"lod_strong":client.lod.len(),
            "lod_weak":client.observed.len(),"packed":client.packed.len()}})
}

#[test]
#[ignore = "large in-memory maintained native travel probe; run alone with optimized tests"]
fn retained_travel_bounded_cold_light_maintenance_probe() {
    let config = Config::new(SEED, "cold-light-probe", PathBuf::new());
    let (mut worker, resumed) = Worker::open(config, PathBuf::new(), Some(1234)).unwrap();
    assert!(!resumed && worker.path.as_os_str().is_empty() && worker._lock.is_none());
    let mut client = ClientRefs::new(); client.accept(worker.update(true));
    let mut times = WorkTimes::default();
    assert!(matches!(request(&mut worker, &mut client, Command::PrepareSpawn, &mut times), Reply::SpawnPrepared { .. }));
    loop {
        let Reply::SpawnAreaProgress { loaded } = request(&mut worker, &mut client,
            Command::LoadSpawnBatchDeferred { count: 32 }, &mut times) else { panic!("spawn progress"); };
        if loaded == 625 { break; }
    }
    assert!(matches!(request(&mut worker, &mut client, Command::PreparePlayer { adventure: false }, &mut times), Reply::PlayerPrepared { .. }));
    assert_eq!(worker.maintenance_policy.metrics().passes, 0, "original625 must not run maintenance");
    fill_maintained_view(&mut worker, &mut client, true, &mut times);
    let origin = client.center;
    let mut checkpoints = vec![maintained_storage_report(&worker, &client, 0, &times)];
    for step in 1..=TRAVEL_CENTERS {
        let start = Instant::now(); client.set_center((origin.0 + step, origin.1));
        times.client_refs_ms += start.elapsed().as_secs_f64() * 1000.;
        fill_maintained_view(&mut worker, &mut client, false, &mut times);
        if step % 30 == 0 { checkpoints.push(maintained_storage_report(&worker, &client, step, &times)); }
    }
    let metrics = worker.maintenance_policy.metrics();
    assert!(metrics.compressed_arrays > 0); assert_eq!(metrics.codec_errors, 0);
    assert!(metrics.max_visited <= 64 && metrics.max_encode_attempts <= 96);
    assert!(metrics.max_encode_input_bytes <= 6 * 1024 * 1024);
    assert!(metrics.max_cache_releases <= 32 && metrics.max_packed_attempts <= 64);
    assert!(metrics.max_release_bytes_budgeted <= 8 * 1024 * 1024);
    let blocks = metrics.block_storage;
    assert!(blocks.compressed_columns > 0 && blocks.max_visited <= 64);
    assert!(blocks.max_encode_attempts <= 8 && blocks.max_encode_input_bytes <= 1024 * 1024);
    assert!(blocks.max_cache_releases <= 4 && blocks.max_release_bytes_budgeted <= 1024 * 1024);
    let exe_hash = std::env::current_exe().ok().and_then(|p| File::open(p).ok()).map(|mut file| {
        let mut h = Sha1::new(); let mut buffer = [0u8; 65536];
        loop { let n = file.read(&mut buffer).unwrap(); if n == 0 { break; } h.update(&buffer[..n]); }
        format!("{:x}", h.finalize())
    });
    let report = serde_json::json!({"kind":"isolated_native_bounded_cold_light_maintenance_probe", "seed":SEED,
        "world_random_seed":1234,"radius":RADIUS,"loaded_radius":LOADED_RADIUS,"travel_centers":TRAVEL_CENTERS,
        "test_executable_sha1":exe_hash,"checkpoints":checkpoints,"maintenance":metrics,
        "note":"Fixed serial camera route with bounded external references, not a Vulkan flight. Applies the production candidate only after ordinary generated publications; no user save or provider unloading. The canonical hash records this run; independent entity constructor seeds prevent claiming equality with an older arbitrary Vanilla run. Exact maintained/disabled state equality is asserted separately by the deterministic command/save/resume unit oracle. Metrics are cumulative work, not current live bytes. elapsed_since_first_pass_ms includes diagnostic hash pauses; path_work_plus_maintenance_ms excludes those pauses. Payload capacities exclude allocator/Arc headers and separate client copies. The light2ms plus block1ms caps are cooperative, not hard deadlines. Block current-storage census is diagnostic-only. The90-column +X path spans1440blocks (120seconds at12blocks/s), but serial8-column requests and controlled aliases do not reproduce asynchronous Vulkan request batching."});
    fs::create_dir_all("reports").unwrap();
    fs::write("reports/native-cold-light-maintained-travel.json", serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    eprintln!("Maintained cold-light travel complete: {} retained, {} compressed arrays, max pass {:.3}ms",
        worker.runtime.provider.loaded_order().len(), metrics.compressed_arrays, metrics.max_pass_ms);
}

#[path = "native_session_cold_block_probe.rs"]
mod block_shadow;
