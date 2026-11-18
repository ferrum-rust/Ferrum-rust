//! Test-only shadow codec: immutable native blocks, no installed representation.
use super::*;

const STATES: usize = 65536;
const RAW_BYTES: usize = STATES * 2;
#[derive(Debug)]
enum Encoding16 { Uniform(u16), Runs(Box<[u8]>), Raw }
struct Encoded16 { encoding: Encoding16, peak_encode_payload_bytes: usize }
fn encode16(raw: &[u16]) -> Encoded16 {
    let finish = |encoding, peak_encode_payload_bytes| Encoded16 { encoding, peak_encode_payload_bytes };
    if raw.len() != STATES { return finish(Encoding16::Raw, 0); }
    let mut runs = Vec::new();
    let mut start = 0;
    while start < raw.len() {
        let value = raw[start];
        let mut end = start + 1;
        while end < raw.len() && raw[end] == value { end += 1; }
        if start == 0 && end == raw.len() { return finish(Encoding16::Uniform(value), 0); }
        let mut remaining = end - start;
        while remaining != 0 {
            if runs.len() + 4 >= RAW_BYTES {
                return finish(Encoding16::Raw, runs.capacity());
            }
            if runs.len() + 4 > runs.capacity() {
                let target = runs.capacity().max(512).saturating_mul(2).min(RAW_BYTES);
                runs.reserve_exact(target - runs.len());
                assert!(runs.capacity() <= RAW_BYTES, "shadow codec scratch bound");
            }
            let count = remaining.min(usize::from(u16::MAX)) as u16;
            runs.extend_from_slice(&count.to_le_bytes());
            runs.extend_from_slice(&value.to_le_bytes());
            remaining -= usize::from(count);
        }
        start = end;
    }
    // Include a conservative overlapping Vec + exact Box allocation during
    // shrinking; allocator headers/workspace and original raw input excluded.
    let peak = runs.capacity() + runs.len();
    finish(Encoding16::Runs(runs.into_boxed_slice()), peak)
}
fn decode16(encoded: &Encoding16, length: usize) -> Result<Option<Vec<u16>>> {
    anyhow::ensure!(length == STATES, "invalid shadow block length");
    match encoded {
        Encoding16::Uniform(value) => Ok(Some(vec![*value; length])),
        Encoding16::Raw => Ok(None),
        Encoding16::Runs(runs) => {
            anyhow::ensure!(!runs.is_empty() && runs.len() < RAW_BYTES && runs.len() % 4 == 0,
                "invalid shadow block runs");
            let mut decoded = Vec::with_capacity(length);
            for run in runs.chunks_exact(4) {
                let count = usize::from(u16::from_le_bytes([run[0],run[1]]));
                let value = u16::from_le_bytes([run[2],run[3]]);
                anyhow::ensure!(count != 0 && count <= length - decoded.len(), "shadow block run overflow");
                decoded.resize(decoded.len()+count,value);
            }
            anyhow::ensure!(decoded.len() == length, "short shadow block output");
            Ok(Some(decoded))
        }
    }
}
#[derive(Serialize)]
struct BlockSample {
    position: ChunkPos, top: i32, distinct_states: usize, nonzero_states: usize,
    random_index_ready: bool, scheduled_index_ready: bool,
    raw_logical_bytes: usize, raw_capacity_bytes: usize,
    encoding: &'static str, retained_payload_bytes: usize,
    encode_ms: f64, decode_ms: f64, exact_roundtrip: bool,
    peak_codec_payload_bytes: usize,
}
fn block_sample(position: ChunkPos, chunk: &crate::world::Chunk) -> BlockSample {
    let raw = chunk.blocks.as_slice();
    let begin = Instant::now(); let encoded = encode16(raw);
    let encode_ms = begin.elapsed().as_secs_f64()*1000.;
    let begin = Instant::now(); let decoded = decode16(&encoded.encoding,raw.len()).unwrap();
    let decode_ms = begin.elapsed().as_secs_f64()*1000.;
    if let Some(values) = &decoded { assert_eq!(values.as_slice(),raw); }
    else { assert!(matches!(&encoded.encoding,Encoding16::Raw)); }
    let (encoding,retained_payload_bytes,encoded_allocation) = match &encoded.encoding {
        Encoding16::Uniform(_) => ("uniform",2,0),
        Encoding16::Runs(runs) => ("rle16",runs.len(),runs.len()),
        Encoding16::Raw => ("dense",raw.len()*2,0),
    };
    let peak_codec_payload_bytes = encoded.peak_encode_payload_bytes.max(
        encoded_allocation+decoded.as_ref().map_or(0,|v|v.capacity()*2));
    assert!(peak_codec_payload_bytes <= RAW_BYTES*2);
    BlockSample { position,top:chunk.top,
        distinct_states:raw.iter().copied().collect::<std::collections::BTreeSet<_>>().len(),
        nonzero_states:raw.iter().filter(|&&v|v!=0).count(),
        random_index_ready:chunk.random_tick_sections.get().is_some(),
        scheduled_index_ready:chunk.scheduled_tick_blocks.get().is_some(),
        raw_logical_bytes:raw.len()*2,raw_capacity_bytes:chunk.blocks.capacity()*2,
        encoding,retained_payload_bytes,encode_ms,decode_ms,exact_roundtrip:true,
        peak_codec_payload_bytes }
}
fn median(values: impl Iterator<Item=f64>) -> f64 {
    let mut values=values.collect::<Vec<_>>(); values.sort_unstable_by(f64::total_cmp);
    match values.len() {0=>0.,n if n%2==0=>(values[n/2-1]+values[n/2])*0.5,n=>values[n/2]}
}
fn block_checkpoint(worker: &Worker, client: &ClientRefs, step: i32) -> Value {
    let before=native_digest(worker);
    let rng_before=(worker.runtime.backend.provider_random.state(),worker.runtime.backend.world_random.state());
    let (census,cold)=census(worker,client.center);
    let chunks=&worker.runtime.backend.world.chunks;
    let eligible=cold.iter().copied().filter(|p| {
        let chunk=&chunks[p];
        // Diagnostic snapshot only. Real maintenance MUST call Arc::get_mut,
        // which atomically excludes concurrent Weak upgrades.
        Arc::strong_count(chunk)==1 && Arc::weak_count(chunk)==0 && chunk.blocks.len()==STATES
    }).collect::<Vec<_>>();
    let count=eligible.len().min(SAMPLE_LIMIT);
    let samples=(0..count).map(|sample| {
        let index=if count<=1 {0}else{sample*(eligible.len()-1)/(count-1)};
        let p=eligible[index]; block_sample(p,&chunks[&p])
    }).collect::<Vec<_>>();
    let raw_logical=chunks.values().map(|c|c.blocks.len()*2).sum::<usize>();
    let raw_resident=chunks.values().map(|c|c.blocks.capacity()*2).sum::<usize>();
    let eligible_logical=eligible.iter().map(|p|chunks[p].blocks.len()*2).sum::<usize>();
    let eligible_resident=eligible.iter().map(|p|chunks[p].blocks.capacity()*2).sum::<usize>();
    let sampled_raw=samples.iter().map(|s|s.raw_logical_bytes).sum::<usize>();
    let sampled_payload=samples.iter().map(|s|s.retained_payload_bytes).sum::<usize>();
    let ratio=(sampled_raw!=0).then(||sampled_payload as f64/sampled_raw as f64);
    let after=native_digest(worker);
    let rng_after=(worker.runtime.backend.provider_random.state(),worker.runtime.backend.world_random.state());
    assert_eq!(before,after,"shadow block codec changed native logical state");
    assert_eq!(rng_before,rng_after);
    serde_json::json!({"step":step,"center":client.center,"census":census,
        "native_block_logical_bytes":raw_logical,"native_block_resident_capacity_bytes":raw_resident,
        "offview_clean_unique_columns":eligible.len(),"eligible_block_logical_bytes":eligible_logical,
        "eligible_block_resident_capacity_bytes":eligible_resident,
        "sample_count":samples.len(),"sample_raw_bytes":sampled_raw,"sample_retained_payload_bytes":sampled_payload,
        "sample_payload_ratio":ratio,"sample_profitable_columns":samples.iter().filter(|s|s.encoding!="dense").count(),
        "encode_median_ms":median(samples.iter().map(|s|s.encode_ms)),
        "encode_max_ms":samples.iter().map(|s|s.encode_ms).fold(0.,f64::max),
        "decode_median_ms":median(samples.iter().map(|s|s.decode_ms)),
        "decode_max_ms":samples.iter().map(|s|s.decode_ms).fold(0.,f64::max),
        "sample_scaled_remaining_native_block_payload_estimate_bytes":ratio.map(|r|raw_resident as f64-eligible_logical as f64*(1.-r)),
        "estimate_note":"The scaled estimate applies this bounded evenly spaced sample ratio only to eligible raw logical block bytes. It excludes metadata/allocator overhead, hydration caches and future growth; it is not a measured process RSS or memory cap.",
        "canonical_hash_before":before,"canonical_hash_after":after,"rng_before":rng_before,"rng_after":rng_after,
        "samples":samples})
}
#[test]
fn block_shadow_codec_roundtrips_all_u16_bits_and_rejects_malformed_runs() {
    let mut long=vec![0xffff;STATES];long[STATES-1]=0x8001;
    let mut layers=vec![0;STATES];for (i,v) in layers.iter_mut().enumerate(){*v=((i/256)*257) as u16;}
    for raw in [vec![0;STATES],vec![0xffff;STATES],long,layers,(0..=u16::MAX).collect()] {
        let encoded=encode16(&raw);
        if let Some(decoded)=decode16(&encoded.encoding,raw.len()).unwrap(){assert_eq!(decoded,raw);}
        else{assert!(matches!(encoded.encoding,Encoding16::Raw));}
        assert!(encoded.peak_encode_payload_bytes<=RAW_BYTES*2);
    }
    assert!(matches!(encode16(&vec![0xffff;STATES]).encoding,Encoding16::Uniform(0xffff)));
    assert!(matches!(encode16(&(0..=u16::MAX).collect::<Vec<_>>()).encoding,Encoding16::Raw));
    for bytes in [vec![],vec![0,0,1,0],vec![1,0,255,255],vec![255,255,0,0,2,0,0,0],vec![1,0,0]] {
        assert!(decode16(&Encoding16::Runs(bytes.into_boxed_slice()),STATES).is_err());
    }
    assert!(decode16(&Encoding16::Uniform(0),STATES+1).is_err());
    assert!(decode16(&Encoding16::Raw,STATES-1).is_err());
}
#[test]
#[ignore = "large immutable retained-owner block codec probe; run alone with optimized tests"]
fn retained_travel_block_codec_shadow_probe() {
    let config=Config::new(SEED,"cold-block-shadow-probe",PathBuf::new());
    let (mut worker,resumed)=Worker::open(config,PathBuf::new(),Some(1234)).unwrap();
    assert!(!resumed&&worker.path.as_os_str().is_empty()&&worker._lock.is_none());
    let mut client=ClientRefs::new();client.accept(worker.update(true));
    let mut times=WorkTimes::default();
    assert!(matches!(request(&mut worker,&mut client,Command::PrepareSpawn,&mut times),Reply::SpawnPrepared{..}));
    loop {
        let Reply::SpawnAreaProgress{loaded}=request(&mut worker,&mut client,
            Command::LoadSpawnBatchDeferred{count:32},&mut times) else{panic!("spawn progress");};
        if loaded==625 {break;}
    }
    assert!(matches!(request(&mut worker,&mut client,Command::PreparePlayer{adventure:false},&mut times),Reply::PlayerPrepared{..}));
    fill_view(&mut worker,&mut client,true,&mut times);
    let origin=client.center;let mut checkpoints=vec![block_checkpoint(&worker,&client,0)];
    for step in 1..=TRAVEL_CENTERS {
        client.set_center((origin.0+step,origin.1));
        fill_view(&mut worker,&mut client,false,&mut times);
        if step%30==0 {checkpoints.push(block_checkpoint(&worker,&client,step));}
    }
    let report=serde_json::json!({"kind":"immutable_native_block_u16_codec_shadow_probe","seed":SEED,
        "world_random_seed":1234,"radius":RADIUS,"loaded_radius":LOADED_RADIUS,"halo":LIGHT_HALO,
        "travel_centers":TRAVEL_CENTERS,"sample_limit_per_checkpoint":SAMPLE_LIMIT,
        "codec":"Uniform u16 or little-endian nonzero u16 run count + exact u16 state; incompressible data stays Dense. Runs shrink to exact Box payload; all 16 bits are preserved.",
        "bounds":"At most64 sampled columns per checkpoint; one131072-byte raw column is borrowed at a time and codec-owned peak payload <=262144 bytes. JSON contains counts/times/hashes, never raw blocks. Retained route may cascade additional native chunks; this is not a total-memory cap.",
        "note":"Existing controlled owner route reused unchanged; no installed compression, saved world, logical eviction or production behavior change. Shadows preserve provider/block/light/tick/RNG digests. Encode/decode timings include bounded allocation; assertions and state census are outside codec timers. This overworld sample does not establish Nether/End or arbitrary edited-world ratios.",
        "timing":times,"checkpoints":checkpoints});
    fs::create_dir_all("reports").unwrap();
    fs::write("reports/native-cold-block-shadow-probe.json",serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    eprintln!("Native block shadow probe completed: {} retained columns; no compression installed",worker.runtime.provider.loaded_order().len());
}

#[path="native_session_cold_block_arc_probe.rs"]
mod arc_shadow;
