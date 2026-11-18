//! Ignored measurement of the actual staged BlockStates Arc codec on copied
//! native columns. The owner itself remains Dense and unmodified.
use super::*;
use crate::block_states::BlockStates;
#[derive(Serialize)]
struct ArcSample {
    position:ChunkPos,arc_first:bool,compressed:bool,raw_bytes:usize,encoded_payload_bytes:usize,
    encoded_allocation_bytes:usize,raw_capacity_after_encode:usize,
    box_encode_ms:f64,box_decode_ms:f64,arc_encode_and_release_ms:f64,arc_promote_ms:f64,
}
fn old_times(raw:&[u16])->(f64,f64,usize) {
    let begin=Instant::now();let encoded=encode16(raw);let encode=begin.elapsed().as_secs_f64()*1000.;
    let begin=Instant::now();let decoded=decode16(&encoded.encoding,raw.len()).unwrap();
    let decode=begin.elapsed().as_secs_f64()*1000.;
    if let Some(values)=decoded{assert_eq!(values,raw);}
    let bytes=match encoded.encoding{Encoding16::Uniform(_)=>2,Encoding16::Runs(v)=>v.len(),Encoding16::Raw=>raw.len()*2};
    (encode,decode,bytes)
}
fn actual_times(raw:&[u16])->(bool,usize,usize,usize,f64,f64) {
    // Explicitly outside the encoder timer: production already owns this Vec.
    let mut candidate=BlockStates::from(raw.to_vec());
    let begin=Instant::now();let compressed=candidate.compress_if_saving(0);
    let encode=begin.elapsed().as_secs_f64()*1000.;
    let payload=if compressed{candidate.encoded_payload_bytes()}else{raw.len()*2};
    let allocation=candidate.encoded_allocation_bytes();let resident=candidate.capacity()*2;
    assert!(!candidate.has_read_cache());
    let begin=Instant::now();let promoted=candidate.promote_to_dense();
    let decode=begin.elapsed().as_secs_f64()*1000.;
    assert_eq!(promoted,compressed);assert!(candidate.is_dense());assert_eq!(candidate.as_slice(),raw);
    (compressed,payload,allocation,resident,encode,decode)
}
fn checkpoint(worker:&Worker,client:&ClientRefs,step:i32)->Value {
    assert!(worker.runtime.backend.world.chunks.values().all(|c|c.blocks.is_dense()),"production admission is disabled");
    let before=native_digest(worker);let rng=(worker.runtime.backend.provider_random.state(),worker.runtime.backend.world_random.state());
    let (_,cold)=census(worker,client.center);let chunks=&worker.runtime.backend.world.chunks;
    let eligible=cold.into_iter().filter(|p|Arc::strong_count(&chunks[p])==1&&Arc::weak_count(&chunks[p])==0
        &&chunks[p].blocks.len()==STATES).collect::<Vec<_>>();
    let count=eligible.len().min(SAMPLE_LIMIT);let mut rows=Vec::with_capacity(count);
    for sample in 0..count {
        let at=if count<=1{0}else{sample*(eligible.len()-1)/(count-1)};let position=eligible[at];
        let raw=chunks[&position].blocks.as_slice();let arc_first=(sample+step as usize)%2==0;
        let (a,b)=if arc_first{let a=actual_times(raw);let b=old_times(raw);(a,b)}
            else{let b=old_times(raw);let a=actual_times(raw);(a,b)};
        assert_eq!(a.1,b.2,"actual/reference codec decision or payload differs");
        rows.push(ArcSample{position,arc_first,compressed:a.0,raw_bytes:raw.len()*2,
            encoded_payload_bytes:a.1,encoded_allocation_bytes:a.2,raw_capacity_after_encode:a.3,
            box_encode_ms:b.0,box_decode_ms:b.1,arc_encode_and_release_ms:a.4,arc_promote_ms:a.5});
    }
    let after=native_digest(worker);assert_eq!(before,after);
    assert_eq!(rng,(worker.runtime.backend.provider_random.state(),worker.runtime.backend.world_random.state()));
    assert!(worker.runtime.backend.world.chunks.values().all(|c|c.blocks.is_dense()));
    serde_json::json!({"step":step,"eligible_columns":eligible.len(),"sample_count":count,
        "arc_encode_median_ms":median(rows.iter().map(|r|r.arc_encode_and_release_ms)),
        "arc_encode_max_ms":rows.iter().map(|r|r.arc_encode_and_release_ms).fold(0.,f64::max),
        "arc_promote_median_ms":median(rows.iter().map(|r|r.arc_promote_ms)),
        "arc_promote_max_ms":rows.iter().map(|r|r.arc_promote_ms).fold(0.,f64::max),
        "box_encode_median_ms":median(rows.iter().map(|r|r.box_encode_ms)),
        "box_decode_median_ms":median(rows.iter().map(|r|r.box_decode_ms)),
        "canonical_hash_before":before,"canonical_hash_after":after,"rng":rng,"samples":rows})
}
#[test]
#[ignore="actual BlockStates Arc codec shadow on retained owner route; run alone in optimized tests"]
fn retained_travel_actual_arc_block_codec_shadow_probe() {
    let (mut worker,resumed)=Worker::open(Config::new(SEED,"arc-block-shadow",PathBuf::new()),PathBuf::new(),Some(1234)).unwrap();
    assert!(!resumed&&worker.path.as_os_str().is_empty()&&worker._lock.is_none());
    let mut client=ClientRefs::new();client.accept(worker.update(true));let mut times=WorkTimes::default();
    assert!(matches!(request(&mut worker,&mut client,Command::PrepareSpawn,&mut times),Reply::SpawnPrepared{..}));
    loop{let Reply::SpawnAreaProgress{loaded}=request(&mut worker,&mut client,Command::LoadSpawnBatchDeferred{count:32},&mut times)
        else{panic!("spawn progress")};if loaded==625{break;}}
    assert!(matches!(request(&mut worker,&mut client,Command::PreparePlayer{adventure:false},&mut times),Reply::PlayerPrepared{..}));
    fill_view(&mut worker,&mut client,true,&mut times);let origin=client.center;
    let mut checkpoints=vec![checkpoint(&worker,&client,0)];
    for step in 1..=TRAVEL_CENTERS {client.set_center((origin.0+step,origin.1));
        fill_view(&mut worker,&mut client,false,&mut times);if step%30==0{checkpoints.push(checkpoint(&worker,&client,step));}}
    let report=serde_json::json!({"kind":"actual_block_states_arc_codec_shadow","seed":SEED,
        "columns":worker.runtime.provider.loaded_order().len(),"sample_limit_per_checkpoint":SAMPLE_LIMIT,
        "layout_bytes":{"block_states":size_of::<BlockStates>(),"vec_u16":size_of::<Vec<u16>>(),"chunk":size_of::<crate::world::Chunk>()},
        "note":"All authoritative columns stay Dense. Only one copied candidate column is compressed at a time. Candidate input Vec copy is outside timers and leaves that candidate cache-warm; absolute encode timings may be optimistic for untouched historical columns. Actual encode includes Arc allocation, boxed metadata and release of its owned Dense Vec; promotion includes decode and release of unique encoded backing. The old shadow borrows its input and retains its encoded Box until after timing, so timings describe different ownership boundaries and are not a direct speedup ratio. Arms alternate per sampled column. Exact values and full native digests must match. No saved world or production maintenance is used.",
        "allocation_note":"Payload/allocation counters exclude Box/Arc/control/allocator headers; no raw world dump. Input/shadow work stays bounded to one65536-state column per arm.",
        "timing":times,"checkpoints":checkpoints});
    fs::create_dir_all("reports").unwrap();fs::write("reports/native-actual-arc-block-shadow-probe.json",serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}
