use super::*;
use std::{cell::Cell, io::Write, rc::Rc, time::Instant};

#[test]
fn startup_lookahead_command_preserves_order_duplicates_publication_and_snapshot_boundary() {
    let config = Config::new(0, "lookahead-command", std::env::temp_dir());
    let (mut ordinary, _) = Worker::open(config.clone(), PathBuf::new(), Some(1234)).unwrap();
    let (mut ahead, _) = Worker::open(config, PathBuf::new(), Some(1234)).unwrap();
    let batches = [vec![(0, 0), (0, 1)], vec![(1, 0), (2, 1), (1, 1), (0, 0)]];
    for (index, positions) in batches.iter().enumerate() {
        let upcoming = batches.get(index + 1).cloned().unwrap_or_default();
        let command = Command::GenerateAhead { positions: positions.clone(), ahead: upcoming };
        validate_command(&command).unwrap();
        assert!(matches!(ordinary.execute(Command::Generate(positions.clone())).unwrap().0, Reply::Generated));
        assert!(matches!(ahead.execute(command).unwrap().0, Reply::Generated));
        let before = ordinary.update(false);
        let after = ahead.update(false);
        assert_eq!(before.chunks.iter().map(|(p,_)| p).collect::<Vec<_>>(),
            after.chunks.iter().map(|(p,_)| p).collect::<Vec<_>>());
        assert_eq!(before.remesh, after.remesh);
        assert_eq!(ordinary.runtime.provider.loaded_order(), ahead.runtime.provider.loaded_order());
        assert_eq!(ordinary.runtime.backend.provider_random.state(), ahead.runtime.backend.provider_random.state());
        assert_eq!(ordinary.runtime.backend.world_random.state(), ahead.runtime.backend.world_random.state());
        let mut baseline = serde_json::to_value(ordinary.runtime.checkpoint().unwrap()).unwrap();
        let mut actual = serde_json::to_value(ahead.runtime.checkpoint().unwrap()).unwrap();
        // Entity instances independently seed their own Random; these two
        // separately constructed owners intentionally have different IDs/RNG.
        baseline.as_object_mut().unwrap().remove("entities");
        actual.as_object_mut().unwrap().remove("entities");
        assert_eq!(actual, baseline, "pure upcoming work must not enter the authoritative snapshot");
        assert!(after.generation_timings.generate_ms > 0.);
        assert!(after.generation_timings.terrain.lookahead_peak <= 64);
        if index == 0 {
            assert!(!ahead.runtime.provider.contains((1,1)), "future requests must not become visible early");
        }
    }
    assert!(validate_command(&Command::GenerateAhead { positions: vec![], ahead: vec![(0,0);65] }).is_err());
    assert!(validate_command(&Command::GenerateAhead { positions: vec![], ahead: vec![(1_875_000,0)] }).is_err());
}

struct CountReads<R> {
    reader: R,
    calls: Rc<Cell<usize>>,
}
impl<R: Read> Read for CountReads<R> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        self.calls.set(self.calls.get() + 1);
        self.reader.read(bytes)
    }
}

#[test]
fn checkpoint_decode_buffers_inflater_reads_and_preserves_every_saved_field() {
    let mut runtime = Runtime::for_overworld_verification(189, 1234, false);
    for p in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        runtime.provide(p).unwrap();
    }
    let original = Checkpoint {
        format: FORMAT.into(),
        generator: GENERATOR,
        world_id: "buffered-read".into(),
        seed: 189,
        gameplay: serde_json::json!({"position":[0.5,76.0,-0.5],"map":42}),
        runtime: runtime.checkpoint().unwrap(),
    };
    let bytes = serde_json::to_vec(&original).unwrap();
    let calls = Rc::new(Cell::new(0));
    let start = Instant::now();
    let decoded = read_checkpoint_json_buffered(CountReads {
        reader: bytes.as_slice(),
        calls: calls.clone(),
    })
    .unwrap();
    let elapsed = start.elapsed();
    assert!(
        calls.get() <= bytes.len().div_ceil(256 * 1024) + 2,
        "JSON parsing must not turn each byte into a separate inflater call"
    );
    assert_eq!(
        serde_json::to_value(&decoded).unwrap(),
        serde_json::to_value(&original).unwrap()
    );
    let mut gzip = GzEncoder::new(Vec::new(), Compression::fast());
    gzip.write_all(&bytes).unwrap();
    let gzip = gzip.finish().unwrap();
    let decoded = read_checkpoint(gzip.as_slice()).unwrap();
    assert_eq!(
        serde_json::to_value(&decoded).unwrap(),
        serde_json::to_value(&original).unwrap()
    );
    assert!(
        read_checkpoint(&gzip[..gzip.len() - 5]).is_err(),
        "a truncated gzip trailer must still fail"
    );
    for limit in [0, 31, bytes.len() - 1, bytes.len(), bytes.len() + 1] {
        let decoded = read_checkpoint_json_limited(GzDecoder::new(gzip.as_slice()), limit).unwrap();
        assert_eq!(
            serde_json::to_value(&decoded).unwrap(),
            serde_json::to_value(&original).unwrap(),
            "slice/stream boundary at {limit} must preserve the complete checkpoint"
        );
        assert!(
            read_checkpoint_json_limited(GzDecoder::new(&gzip[..gzip.len() - 5]), limit).is_err(),
            "truncated trailer must fail in slice and streaming paths at {limit}"
        );
    }
    eprintln!(
        "Native checkpoint: {} JSON bytes, {} buffered reads, decode {:?}",
        bytes.len(),
        calls.get(),
        elapsed
    );
}

/// Copy a user's checkpoint into a temporary diagnostic directory before
/// supplying this path. The test only reads it; no session/lock/save is opened.
#[test]
#[ignore = "set FERRUM_NATIVE_PROFILE_CHECKPOINT to a temporary checkpoint copy"]
fn retained_checkpoint_load_reports_decode_restore_publication_cost() {
    let path = PathBuf::from(
        std::env::var_os("FERRUM_NATIVE_PROFILE_CHECKPOINT")
            .expect("temporary native checkpoint copy"),
    );
    let mut expected = None;
    for mode in ["previous", "buffered", "slice"] {
        let start = Instant::now();
        let saved: Checkpoint = match mode {
            "slice" => read_checkpoint(File::open(&path).unwrap()).unwrap(),
            "buffered" => read_checkpoint_json_buffered(GzDecoder::new(BufReader::new(
                File::open(&path).unwrap(),
            )))
            .unwrap(),
            _ => {
                serde_json::from_reader(GzDecoder::new(BufReader::new(File::open(&path).unwrap())))
                    .unwrap()
            }
        };
        let decode = start.elapsed();
        let point = saved.runtime.world_spawn;
        let rng = (saved.runtime.world_random, saved.runtime.provider_random);
        let gameplay = saved.gameplay;
        let start = Instant::now();
        let mut runtime = Runtime::resume(saved.runtime, Arc::new(VanillaPopulation)).unwrap();
        let restore = start.elapsed();
        let start = Instant::now();
        let chunks = runtime.take_chunk_updates();
        let publish = start.elapsed();
        let digest = chunks
            .iter()
            .fold(0xcbf29ce484222325u64, |mut hash, (p, c)| {
                for b in
                    p.0.to_le_bytes()
                        .into_iter()
                        .chain(p.1.to_le_bytes())
                        .chain(c.blocks.iter().flat_map(|s| s.to_le_bytes()))
                        .chain(c.biomes)
                        .chain(c.light.as_ref().unwrap().values.iter().copied())
                {
                    hash = (hash ^ u64::from(b)).wrapping_mul(0x100000001b3);
                }
                hash
            });
        let actual = (point, rng, gameplay, chunks.len(), digest);
        if let Some(expected) = &expected {
            assert_eq!(&actual, expected);
        }
        eprintln!("Native read {}: decode {decode:?}, restore {restore:?}, publish {publish:?}, {} chunks, digest {digest:016x}",
            mode, chunks.len());
        expected = Some(actual);
    }
}
