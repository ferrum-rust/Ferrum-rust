use super::*;

const POSITION: ChunkPos = (0, 0);
const CHEST: [i32; 3] = [3, 71, 5];
const SPAWNER: [i32; 3] = [8, 72, 9];
const HOPPERS: [[i32; 3]; 2] = [[7, 69, 3], [4, 70, 2]];

struct Quiet;
impl Hooks for Quiet {
    fn populate(&self, _: &mut Access<'_>, _: ChunkPos, _: &mut JavaRandom) -> Result<()> { Ok(()) }
}

fn fixture() -> (Runtime, Vec<u16>) {
    let mut runtime = Runtime::new(189, 1234, false, Arc::new(Quiet));
    runtime.use_overlay_journal(false);
    runtime.provide(POSITION).unwrap();
    assert_eq!(runtime.provider.loaded_order(), &[POSITION]);
    let mut expected = vec![0u16; 65536];
    for (p, state) in [(CHEST, (54 << 4) | 2), (SPAWNER, 52 << 4),
        (HOPPERS[0], (154 << 4) | 5), (HOPPERS[1], 154 << 4), ([15, 250, 15], 0xfedc)] {
        expected[Chunk::index(p[0], p[1], p[2])] = state;
    }
    let mut tile = LiveContainer::empty(CHEST, 54, 1, 27);
    tile.slots[0] = Some(crate::inventory::Stack::new(264, 3));
    let chunk = Arc::get_mut(runtime.backend.world.chunks.get_mut(&POSITION).unwrap()).unwrap();
    // A controlled storage fixture, independent of terrain/placement callbacks.
    chunk.blocks = expected.clone().into();
    chunk.top = 251;
    chunk.invalidate_tick_indexes();
    chunk.hoppers = vec![HOPPERS[1], HOPPERS[0]]; // Resume must reconstruct array order.
    chunk.containers = vec![(IVec3::from_array(CHEST), tile.legacy_chest().unwrap())];
    runtime.backend.live_containers.insert(CHEST, tile);
    runtime.backend.live_spawners.insert(SPAWNER, "Zombie".into());
    runtime.backend.tile_revision = 1;
    // Avoid the unrelated old-schema None -> Some(0) migration on resume.
    runtime.backend.entities.xp_owned_after = Some(0);
    (runtime, expected)
}

fn assert_cold_snapshot(snapshot: &mut RuntimeSnapshot) {
    for chunk in &mut snapshot.chunks {
        assert!(chunk.blocks.is_cold());
        assert!(!chunk.blocks.has_read_cache());
        assert_eq!(chunk.blocks.capacity(), 0);
        assert!(!chunk.blocks.take_recent_cold_access());
    }
}

fn assert_cold_runtime(runtime: &mut Runtime) {
    for chunk in runtime.backend.world.chunks.values_mut() {
        let blocks = &mut Arc::get_mut(chunk).expect("test owner remains unique").blocks;
        assert!(blocks.is_cold());
        assert!(!blocks.has_read_cache());
        assert_eq!(blocks.capacity(), 0);
        assert!(!blocks.take_recent_cold_access());
    }
}

#[test]
fn cold_checkpoint_validation_wire_and_resume_preserve_tiles_hoppers_and_rng_without_caches() {
    let (mut runtime, expected) = fixture();
    let dense_snapshot = runtime.checkpoint().unwrap();
    let wire = serde_json::to_value(&dense_snapshot).unwrap();
    assert_eq!(wire["chunks"][0]["blocks"], serde_json::to_value(&expected).unwrap());
    let rng = (runtime.backend.provider_random.state(), runtime.backend.world_random.state());
    for chunk in runtime.backend.world.chunks.values_mut() {
        assert!(Arc::get_mut(chunk).unwrap().blocks.compress_if_saving(1024));
    }
    assert_cold_runtime(&mut runtime);
    let mut snapshot = runtime.checkpoint().unwrap();
    assert_cold_runtime(&mut runtime);
    assert_cold_snapshot(&mut snapshot);
    snapshot.validate().unwrap();
    assert_cold_snapshot(&mut snapshot);
    assert_eq!(serde_json::to_value(&snapshot).unwrap(), wire);
    assert_cold_snapshot(&mut snapshot);
    let mut cloned = snapshot.clone();
    assert_cold_snapshot(&mut cloned);
    assert_cold_snapshot(&mut snapshot);
    let mut resumed = Runtime::resume(cloned, Arc::new(Quiet)).unwrap();
    assert_cold_runtime(&mut resumed);
    let restored = &resumed.backend.world.chunks[&POSITION];
    assert_eq!(restored.blocks, expected);
    assert_eq!(restored.hoppers, HOPPERS);
    assert_eq!(resumed.backend.live_spawners[&SPAWNER], "Zombie");
    assert_eq!(resumed.backend.live_containers[&CHEST].slots[0].as_ref().unwrap().id, 264);
    assert_eq!(resumed.backend.live_containers[&CHEST].slots[0].as_ref().unwrap().count, 3);
    assert_eq!(resumed.provider.loaded_order(), runtime.provider.loaded_order());
    assert_eq!((resumed.backend.provider_random.state(), resumed.backend.world_random.state()), rng);
    assert_eq!(serde_json::to_value(resumed.checkpoint().unwrap()).unwrap(), wire);
    assert_cold_runtime(&mut resumed);
    assert_cold_snapshot(&mut snapshot);
    assert_cold_runtime(&mut runtime);

    // The unchanged persisted numeric-array schema intentionally decodes Dense.
    let persisted: RuntimeSnapshot = serde_json::from_value(wire.clone()).unwrap();
    assert!(persisted.chunks.iter().all(|chunk| chunk.blocks.is_dense()));
    let from_disk = Runtime::resume(persisted, Arc::new(Quiet)).unwrap();
    assert_eq!(from_disk.backend.world.chunks[&POSITION].hoppers, HOPPERS);
    assert_eq!(serde_json::to_value(from_disk.checkpoint().unwrap()).unwrap(), wire);
}

#[test]
fn snapshot_block_dimensions_and_cold_tile_mismatch_errors_keep_original_validation_order() {
    let (runtime, _) = fixture();
    let original = runtime.checkpoint().unwrap();
    for length in [0, 65535, 65537] {
        let mut invalid = original.clone();
        invalid.chunks[0].blocks = vec![0u16; length].into();
        assert!(invalid.validate().unwrap_err().to_string().contains("Invalid checkpoint chunk dimensions"));
    }
    for (position, error) in [(CHEST, "Live container block mismatch"),
        (SPAWNER, "Live spawner block mismatch")] {
        let mut invalid = original.clone();
        invalid.chunks[0].blocks[Chunk::index(position[0], position[1], position[2])] = 0;
        assert!(invalid.chunks[0].blocks.compress_if_saving(1024));
        assert!(invalid.validate().unwrap_err().to_string().contains(error));
        assert_cold_snapshot(&mut invalid);
    }
}
