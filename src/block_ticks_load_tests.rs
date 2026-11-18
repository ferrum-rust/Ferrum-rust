//! Load-vs-update regression against the local1.8.9 BlockFalling source:
//! onBlockAdded/onNeighborBlockChange schedule tickRate=2; Chunk.onChunkLoad
//! only attaches tile/entities and never enumerates gravity block callbacks.
use super::*;
use crate::world::Chunk;

fn published_world(full_halo:bool, blocks:&[(IVec3,u16)]) -> World {
    let mut center=Chunk::empty();
    for z in 0..16 { for x in 0..16 {center.set(x,1,z,16);} }
    for &(p,state) in blocks {center.set(p.x,p.y,p.z,state);}
    center.prepare_tick_indexes();
    let mut chunks=crate::world::ChunkMap::default();
    if full_halo {
        for x in -2..=2 {for z in -2..=2 {chunks.insert((x,z),Arc::new(Chunk::empty()));}}
    }
    chunks.insert((0,0),Arc::new(center));
    World{chunks,spawn:Vec3::new(8.5,2.,8.5),label:"native publication fixture".into(),
        save_path:Default::default(),edits:Default::default(),imported:true,light_revision:0}
}
fn quiet_ticks(ticks:&mut BlockTicks,world:&mut World,count:usize) {
    for _ in 0..count {
        let out=ticks.tick(world);
        assert!(out.changed.is_empty()&&out.drops.is_empty()&&out.washed.is_empty());
        assert!(ticks.falling.is_empty());
    }
}
#[test]
fn published_hanging_gravity_stays_put_through_eviction_republication_and_resume() {
    let blocks=[(IVec3::new(4,8,8),(12<<4)|1),(IVec3::new(8,8,8),13<<4),
        (IVec3::new(12,8,8),(145<<4)|2)];
    for full_halo in [false,true] {
        let mut world=published_world(full_halo,&blocks);let mut ticks=BlockTicks::default();
        let snapshot=world.chunks[&(0,0)].clone();let random=ticks.world_random().state();
        quiet_ticks(&mut ticks,&mut world,8);
        assert!(Arc::ptr_eq(&snapshot,&world.chunks[&(0,0)]));
        // Eviction forgets discovery membership, but returning the immutable
        // native snapshot must not behave as three newly placed blocks.
        world.chunks.remove(&(0,0));quiet_ticks(&mut ticks,&mut world,3);
        world.chunks.insert((0,0),snapshot.clone());quiet_ticks(&mut ticks,&mut world,8);
        // A fresh published Arc for the same column also is not a block edit.
        world.chunks.insert((0,0),Arc::new((*snapshot).clone()));quiet_ticks(&mut ticks,&mut world,3);
        let saved=serde_json::to_vec(&ticks).unwrap();
        ticks=serde_json::from_slice(&saved).unwrap();quiet_ticks(&mut ticks,&mut world,8);
        for &(p,state) in &blocks {assert_eq!(world.get(p),state);}
        assert!(ticks.state().queue.is_empty());assert!(world.edits.is_empty());
        assert_eq!(world.light_revision,0);assert_eq!(ticks.world_random().state(),random);
    }
}
#[test]
fn actual_gravity_placement_and_neighbor_event_keep_original_two_tick_schedule() {
    let p=IVec3::new(8,8,8);
    for state in [(12<<4)|1,13<<4,(145<<4)|2] {
        // BlockTicks::set_block represents the actual BlockFalling.onBlockAdded
        // path. The independent reference inserts the original tickRate=2 job.
        // Use the original missing-area fallback for this full-state paired
        // oracle, avoiding independently seeded falling-entity constructors.
        // Existing full-halo physics tests verify the actual entity motion.
        let mut actual_world=published_world(false,&[]);let mut expected_world=actual_world.clone();
        let mut actual=BlockTicks::default();let mut expected=BlockTicks::default();
        actual.set_block(&mut actual_world,p,state,false);
        expected_world.set(p,state);expected.schedule_block_update(p,state>>4,2,0);
        for step in 1..=3 {
            actual.tick(&mut actual_world);expected.tick(&mut expected_world);
            assert_eq!(actual_world.get(p),if step==1 {state} else {0});
            assert_eq!(actual_world.chunks[&(0,0)].blocks,expected_world.chunks[&(0,0)].blocks);
            assert_eq!(serde_json::to_value(&actual).unwrap(),serde_json::to_value(&expected).unwrap());
        }
        let mut actual_world=published_world(false,&[(p,state)]);let mut actual=BlockTicks::default();
        quiet_ticks(&mut actual,&mut actual_world,8);
        let mut expected_world=actual_world.clone();let mut expected=actual.clone();
        // A real adjacent edit wakes even naturally generated floating sand.
        actual_world.set(p-IVec3::X,16);expected_world.set(p-IVec3::X,16);
        actual.notify(&actual_world,p-IVec3::X);
        expected.schedule_block_update(p,state>>4,2,0);
        for step in 1..=3 {
            actual.tick(&mut actual_world);expected.tick(&mut expected_world);
            assert_eq!(actual_world.get(p),if step==1 {state} else {0});
            assert_eq!(actual_world.chunks[&(0,0)].blocks,expected_world.chunks[&(0,0)].blocks);
            assert_eq!(serde_json::to_value(&actual).unwrap(),serde_json::to_value(&expected).unwrap());
        }
    }
}
#[test]
fn saved_gravity_tick_keeps_its_due_time_without_a_synthetic_load_tick() {
    let p=IVec3::new(8,8,8);let mut world=published_world(true,&[(p,13<<4)]);
    let mut original=BlockTicks::default();original.schedule_block_update(p,13,7,-3);
    let mut resumed:BlockTicks=serde_json::from_slice(&serde_json::to_vec(&original).unwrap()).unwrap();
    quiet_ticks(&mut resumed,&mut world,6);assert_eq!(world.get(p),13<<4);
    resumed.tick(&mut world);assert_eq!(world.get(p),0);assert_eq!(resumed.falling.len(),1);
}
#[test]
fn missing_falling_halo_uses_instant_fallback_only_after_real_update() {
    let p=IVec3::new(8,8,8);let mut world=published_world(false,&[(p,(12<<4)|1)]);
    let mut ticks=BlockTicks::default();quiet_ticks(&mut ticks,&mut world,8);
    ticks.notify(&world,p);ticks.tick(&mut world);assert_eq!(world.get(p),(12<<4)|1);
    ticks.tick(&mut world);
    assert_eq!(world.get(p),0);assert_eq!(world.get(IVec3::new(8,2,8)),12<<4);
    assert!(ticks.falling.is_empty(),"original missing-area fallback is immediate and resets metadata");
}
