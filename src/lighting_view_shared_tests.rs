use super::*;
use serde::Serialize;
use std::{io::Write, time::Instant};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
struct StorageStats { columns: usize, shared_planes: usize, dense_planes: usize,
    native_cold_planes: usize, hydrated_shared_planes: usize, raw_capacity_bytes: usize }
fn storage(view: &LightingView) -> StorageStats {
    let mut out = StorageStats { columns: view.engine.chunks.len(), ..Default::default() };
    for column in view.engine.chunks.values() { for plane in [&column.sky, &column.block] {
        out.shared_planes += usize::from(plane.is_shared_packed());
        out.dense_planes += usize::from(plane.is_dense());
        out.native_cold_planes += usize::from(plane.is_cold());
        out.hydrated_shared_planes += usize::from(plane.is_shared_packed() && plane.has_read_cache());
        out.raw_capacity_bytes += plane.capacity();
    } }
    out
}
fn scalar(snapshot: &ChunkLight) -> LightChunk {
    let mut sky = Vec::new(); let mut block = Vec::new();
    for &value in &snapshot.values { sky.push(value >> 4); block.push(value & 15); }
    LightChunk { height: snapshot.height.clone(), minimum: *snapshot.height.iter().min().unwrap_or(&0),
        sections: snapshot.sections, sky: sky.into(), block: block.into(), gaps: vec![false;256], queued_checks:4096 }
}
fn assert_raw(a: &LightChunk, b: &LightChunk) {
    assert_eq!(a.height,b.height); assert_eq!(a.minimum,b.minimum); assert_eq!(a.sections,b.sections);
    assert_eq!(a.gaps,b.gaps); assert_eq!(a.queued_checks,b.queued_checks);
    a.sky.with_bytes(|a| b.sky.with_bytes(|b| assert_eq!(a,b)));
    a.block.with_bytes(|a| b.block.with_bytes(|b| assert_eq!(a,b)));
}
#[test]
fn shared_factory_preserves_scalar_fields_lengths_and_nonhydrating_identity() {
    for len in [0,1,65535,65536,65537] { for no_sky in [false,true] {
        let snapshot=Arc::new(ChunkLight { values:(0..len).map(|i|(i*13)as u8).collect(),
            height:(0..256).map(|i|i%129).collect(), sections:std::array::from_fn(|i|i%3!=0), no_sky });
        let reference=scalar(&snapshot); let mut actual=snapshot.to_shared_chunk();
        assert_raw(&actual,&reference);
        assert_eq!(serde_json::to_value(&actual).unwrap(),serde_json::to_value(&reference).unwrap());
        assert_eq!(actual.sky.is_shared_packed(),len==65536);
        assert_eq!(actual.block.is_shared_packed(),len==65536);
        assert!(snapshot_matches(&snapshot,&actual,no_sky));
        if len==65536 {
            assert_eq!(actual.sky.capacity()+actual.block.capacity(),0);
            actual.height[0]+=1; assert!(!snapshot_matches(&snapshot,&actual,no_sky)); actual.height[0]-=1;
            actual.sections[0]=!actual.sections[0]; assert!(!snapshot_matches(&snapshot,&actual,no_sky));
            actual.sections[0]=!actual.sections[0];
            assert!(!snapshot_matches(&snapshot,&actual,!no_sky));
            let equal=Arc::new((*snapshot).clone());
            assert!(snapshot_matches(&equal,&actual,no_sky));
            assert_eq!(actual.sky.capacity()+actual.block.capacity(),0,"fallback comparison hydrated a plane");
            actual.sky[17]^=1; assert!(!snapshot_matches(&snapshot,&actual,no_sky));
            assert!(actual.sky.is_dense() && actual.block.is_shared_packed());
            assert!(!actual.sky.take_recent_cold_access());
        }
    } }
}
#[test]
fn evicted_client_column_releases_shared_snapshot_after_seen_and_engine_retire() {
    let (mut world,old_view)=super::tests::lit_region(); drop(old_view);
    let weak=Arc::downgrade(world.chunks[&(0,0)].light.as_ref().unwrap());
    let mut view=LightingView::new(&mut world);
    assert!(view.engine.chunks[&(0,0)].sky.is_shared_packed());
    world.unload_chunk((0,0)); view.update(&mut world);
    assert!(!view.engine.chunks.contains_key(&(0,0)) && !view.seen.contains_key(&(0,0)));
    assert!(weak.upgrade().is_none());
}

struct Pair { lazy_world: World, eager_world: World, lazy: LightingView, eager: LightingView }
impl Pair {
    fn new(base:&World,lazy_first:bool)->(Self,f64,f64) {
        let mut a=base.clone(); let mut b=base.clone();
        let timed=|world:&mut World,lazy:bool| {
            let start=Instant::now(); let view=if lazy {LightingView::new(world)} else {LightingView::new_eager_reference(world)};
            (view,start.elapsed().as_secs_f64()*1000.)
        };
        let ((lazy,lazy_ms),(eager,eager_ms))=if lazy_first {(timed(&mut a,true),timed(&mut b,false))}
            else {let eager=timed(&mut b,false);let lazy=timed(&mut a,true);(lazy,eager)};
        (Self{lazy_world:a,eager_world:b,lazy,eager},lazy_ms,eager_ms)
    }
    fn assert_equal(&self) {
        let before=storage(&self.lazy);
        assert_eq!(self.lazy_world.chunks.len(),self.eager_world.chunks.len());
        assert_eq!(self.lazy.engine.chunks.len(),self.eager.engine.chunks.len());
        assert_eq!(self.lazy.pending,self.eager.pending);
        assert_eq!(self.lazy.engine.diagnostic_render_dirty(),self.eager.engine.diagnostic_render_dirty());
        for (p,a) in &self.lazy.engine.chunks {assert_raw(a,&self.eager.engine.chunks[p]);}
        for (p,a) in &self.lazy_world.chunks {
            let b=&self.eager_world.chunks[p];assert_eq!(a.blocks,b.blocks);
            match (&a.light,&b.light) {
                (Some(a),Some(b))=>{assert_eq!(a.values,b.values);assert_eq!(a.height,b.height);
                    assert_eq!(a.sections,b.sections);assert_eq!(a.no_sky,b.no_sky);},
                (None,None)=>{},_=>panic!("packed availability differs at {p:?}"),
            }
        }
        assert_eq!(before,storage(&self.lazy),"oracle comparison must not retain raw hydration");
    }
}
enum Action { Set(IVec3,u16), Install(ChunkPos,Arc<Chunk>), Evict(ChunkPos) }
fn action(world:&World,source:IVec3,step:usize,saved:&mut Option<Arc<Chunk>>)->Action {
    let target=(1,0);
    match step {
        0|6=>Action::Set(source,50<<4|5),
        1|7=>Action::Set(source,0),
        2=>{
            // Construct the server's immutable payload outside client timing.
            let mut chunk=(*world.chunks[&target]).clone();
            let mut packed=(**chunk.light.as_ref().unwrap()).clone();
            let index=Chunk::index(0,source.y,source.z.rem_euclid(16));
            packed.values[index]=(packed.values[index]&0xf0)|7;
            chunk.light=Some(Arc::new(packed));Action::Install(target,Arc::new(chunk))
        }
        3=>Action::Set(source+IVec3::X+IVec3::Y,1<<4),
        4=>{*saved=Some(world.chunks[&target].clone());Action::Evict(target)},
        5=>Action::Install(target,saved.take().unwrap()),
        _=>unreachable!(),
    }
}
fn run_action(world:&mut World,view:&mut LightingView,action:Action)->(f64,Vec<ChunkPos>) {
    let start=Instant::now();
    match action {Action::Set(p,s)=>{world.set(p,s);},Action::Install(p,c)=>{world.chunks.insert(p,c);world.invalidate_lighting();},
        Action::Evict(p)=>world.unload_chunk(p)}
    let changed=view.update(world);(start.elapsed().as_secs_f64()*1000.,changed)
}
#[derive(Serialize)]
struct Observation { name:&'static str,lazy_ms:f64,eager_ms:f64,lazy:StorageStats,eager:StorageStats,changed:Vec<ChunkPos> }
const STEPS:[&str;8]=["first_torch_place","torch_remove","authoritative_light_only","neighbor_obstruction",
    "evict_neighbor","revisit_neighbor","revisit_torch_place","revisit_torch_remove"];
fn sequence(pair:&mut Pair,source:IVec3,lazy_first:bool)->Vec<Observation> {
    let(mut a_saved,mut b_saved)=(None,None);let mut rows=Vec::new();
    for (step,&name) in STEPS.iter().enumerate() {
        let a=action(&pair.lazy_world,source,step,&mut a_saved);let b=action(&pair.eager_world,source,step,&mut b_saved);
        let((lazy_ms,a_changed),(eager_ms,b_changed))=if lazy_first {
            (run_action(&mut pair.lazy_world,&mut pair.lazy,a),run_action(&mut pair.eager_world,&mut pair.eager,b))
        }else{let b=run_action(&mut pair.eager_world,&mut pair.eager,b);let a=run_action(&mut pair.lazy_world,&mut pair.lazy,a);(a,b)};
        assert_eq!(a_changed,b_changed,"dirty order differs at {name}");pair.assert_equal();
        rows.push(Observation{name,lazy_ms,eager_ms,lazy:storage(&pair.lazy),eager:storage(&pair.eager),changed:a_changed});
    }
    rows
}
#[test]
fn eager_and_lazy_local_light_corrections_eviction_revisit_and_retry_match() {
    let(base,old)=super::tests::lit_region();drop(old);
    let(mut pair,_,_)=Pair::new(&base,true);pair.assert_equal();
    assert_eq!(storage(&pair.lazy).raw_capacity_bytes,0);
    let rows=sequence(&mut pair,IVec3::new(15,4,8),true);
    assert!(rows[0].lazy.raw_capacity_bytes>0,"torch must exercise deferred raw access");
    let(a_inputs,b_inputs)=(pair.lazy_world.chunks.clone(),pair.eager_world.chunks.clone());
    let(_,a)=run_action(&mut pair.lazy_world,&mut pair.lazy,Action::Set(IVec3::new(15,4,8),89<<4));
    let(_,b)=run_action(&mut pair.eager_world,&mut pair.eager,Action::Set(IVec3::new(15,4,8),89<<4));assert_eq!(a,b);
    let affected=a.into_iter().collect::<HashSet<_>>();
    for (world,view,inputs) in [(&mut pair.lazy_world,&mut pair.lazy,&a_inputs),(&mut pair.eager_world,&mut pair.eager,&b_inputs)] {
        for p in &affected {if let(Some(chunk),Some(old))=(world.chunks.get_mut(p),inputs.get(p)){Arc::make_mut(chunk).light=old.light.clone();}}
        view.retry_publications(inputs,&affected);
    }
    assert_eq!(pair.lazy.update(&mut pair.lazy_world),pair.eager.update(&mut pair.eager_world));pair.assert_equal();
}

fn native_fixture()->(World,IVec3) {
    let mut runtime=crate::vanilla::runtime::Runtime::for_overworld_verification(189,1234,false);
    runtime.use_overlay_journal(false);
    for x in -2..=2 {for z in -2..=2 {runtime.provide((x,z)).unwrap();}}
    drop(runtime.take_chunk_updates());
    assert!(runtime.backend.light.chunks.values().all(|c|!c.sky.is_shared_packed()&&!c.block.is_shared_packed()));
    let mut world=runtime.backend.world.clone();world.chunks.retain(|&(x,z),_|x.abs()<=2&&z.abs()<=2);
    let y=world.chunks[&(0,0)].light.as_ref().unwrap().height[8*16+15]
        .max(world.chunks[&(1,0)].light.as_ref().unwrap().height[8*16])+1;
    (world,IVec3::new(15,y.clamp(1,253),8))
}
struct HashWriter(sha1::Sha1);
impl Write for HashWriter {
    fn write(&mut self,bytes:&[u8])->std::io::Result<usize>{use sha1::Digest;self.0.update(bytes);Ok(bytes.len())}
    fn flush(&mut self)->std::io::Result<()>{Ok(())}
}
fn final_digest(view:&LightingView)->String {
    use sha1::Digest;
    let mut hash=HashWriter(sha1::Sha1::new());let mut positions=view.engine.chunks.keys().copied().collect::<Vec<_>>();positions.sort_unstable();
    for p in positions {serde_json::to_writer(&mut hash,&(p,&view.engine.chunks[&p])).unwrap();}
    format!("{:x}",hash.0.finalize())
}
#[test]
#[ignore="timed eager/lazy native-light edit and revisit probe; run alone after optimized build"]
fn timed_eager_lazy_native_light_edit_and_revisit_probe() {
    let(base,source)=native_fixture();let mut runs=Vec::new();
    for lazy_first in [true,false] {
        let(mut pair,lazy_ms,eager_ms)=Pair::new(&base,lazy_first);pair.assert_equal();
        let initial=Observation{name:"adopt",lazy_ms,eager_ms,lazy:storage(&pair.lazy),eager:storage(&pair.eager),changed:Vec::new()};
        assert_eq!(initial.lazy.raw_capacity_bytes,0);
        let rows=sequence(&mut pair,source,lazy_first);let before=storage(&pair.lazy);
        let a=final_digest(&pair.lazy);let b=final_digest(&pair.eager);assert_eq!(a,b);assert_eq!(before,storage(&pair.lazy));
        runs.push(serde_json::json!({"lazy_first":lazy_first,"initial":initial,"operations":rows,"final_raw_fields_sha1":a}));
    }
    let report=serde_json::json!({"kind":"client_lazy_light_edit_revisit_probe","seed":189,"world_random_seed":1234,
        "columns":base.chunks.len(),"source":source.to_array(),"runs":runs,
        "note":"Actual nearby native terrain; eager scalar and lazy client adoption use the same immutable inputs, in both execution orders. Timings cover client world mutation plus LightingView update; fixture creation, incoming server payload construction, equality assertions and final digest are excluded. Every step compares complete raw fields, packed output and dirty order. This is a CPU-only microprobe, not GPU or end-to-end input latency."});
    std::fs::create_dir_all("reports").unwrap();
    std::fs::write("reports/client-lazy-light-edit-revisit.json",serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}
