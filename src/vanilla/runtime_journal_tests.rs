use super::*;
use sha1::{Digest,Sha1};

fn signature(runtime:&Runtime)->String {
    let mut snapshot=runtime.checkpoint().unwrap();
    snapshot.edits.clear(); // only the redundant diagnostic journal differs
    snapshot.dirty_chunks.sort_unstable();
    snapshot.entities=None; // separately constructed entity RNGs are independent
    format!("{:x}",Sha1::digest(serde_json::to_vec(&snapshot).unwrap()))
}

struct NestedWrites;
impl Hooks for NestedWrites {
    fn populate(&self,_:&mut Access<'_>,_:ChunkPos,_:&mut JavaRandom)->Result<()> {Ok(())}
    fn block_added(&self,access:&mut Access<'_>,p:IVec3,old:u16,new:u16)->Result<()> {
        access.with(|_,b|b.event(&format!("add:{old}:{new}"),(p.x,p.y)));
        if new==41<<4 {
            access.set_block_flags(p+IVec3::X,57<<4,2)?;
            access.set_block_flags(p+IVec3::Y,89<<4,2)?;
        }
        Ok(())
    }
}

#[test]
fn untracked_native_writes_keep_nested_callbacks_light_dirty_state_and_resume() {
    let mut expected=None;
    for recording in [true,false] {
        let mut runtime=Runtime::new(189,1234,false,Arc::new(NestedWrites));
        assert!(runtime.backend.record_overlay_edits,"diagnostic runtimes default to tracking");
        runtime.use_overlay_journal(recording);
        runtime.provide((0,0)).unwrap();
        let at=IVec3::new(7,220,7);
        assert!(runtime.access().set_block_flags(at,41<<4,2).unwrap());
        assert_eq!(runtime.backend.block_effects.changed,vec![at+IVec3::X,at+IVec3::Y,at]);
        assert_eq!(runtime.backend.world.get(at+IVec3::X),57<<4);
        assert_eq!(runtime.backend.world.get(at+IVec3::Y),89<<4);
        assert_eq!(runtime.backend.world.edits.is_empty(),!recording);
        let before=signature(&runtime);
        assert!(!runtime.access().set_block_flags(at,41<<4,2).unwrap());
        assert!(!runtime.access().set_block_flags(IVec3::new(7,-1,7),41<<4,2).unwrap());
        assert_eq!(signature(&runtime),before);
        let updates=runtime.take_chunk_updates();
        assert!(updates.iter().any(|(p,chunk)|*p==(0,0)&&chunk.get(7,220,7)==41<<4&&chunk.light.is_some()));
        let checkpoint=runtime.checkpoint().unwrap();
        let mut resumed=Runtime::resume(checkpoint,Arc::new(NestedWrites)).unwrap();
        resumed.use_overlay_journal(recording);
        assert!(resumed.access().set_block_flags(at+IVec3::Z,1<<4,2).unwrap());
        let actual=signature(&resumed);
        if let Some(expected)=&expected {assert_eq!(&actual,expected);} else {expected=Some(actual);}
    }
}

#[test]
fn untracked_structure_and_population_writes_preserve_original_metadata() {
    let fixture:serde_json::Value=serde_json::from_str(include_str!(
        "../../tests/fixtures/vanilla189-runtime-extended.json")).unwrap();
    for name in ["189_-47_47_0","189_-73_82_0"] {
        let case=fixture["cases"].as_array().unwrap().iter().find(|case|case["raw_prefix"]==name).unwrap();
        let operations:Vec<ChunkPos>=serde_json::from_value(case["operations"].clone()).unwrap();
        let mut expected=None;
        for recording in [true,false] {
            let mut runtime=Runtime::for_overworld_verification(189,1234,true);
            runtime.use_overlay_journal(recording);
            for &p in &operations { runtime.provide(p).unwrap(); }
            assert!(!runtime.backend.live_containers.is_empty(),"exercise generated structure inventories");
            assert_eq!(runtime.backend.world.edits.is_empty(),!recording);
            let actual=signature(&runtime);
            if let Some(expected)=&expected {assert_eq!(&actual,expected,"structure {name}");}
            else {expected=Some(actual);}
        }
    }
}
