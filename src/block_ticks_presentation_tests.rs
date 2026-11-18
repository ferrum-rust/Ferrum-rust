use super::*;

#[test]
fn presentation_scheduler_retains_scalars_and_falling_without_work_or_aliases() {
    let mut owner = BlockTicks::default();
    owner.falling.push(FallingBlock::new(12 << 4, glam::DVec3::new(2.5, 8., 3.5), 1234));
    owner.set_clock(123);
    owner.schedule_block_update(IVec3::new(2, 7, 3), 8, 50, -2);
    {
        let mut state = owner.state();
        state.notifications.push([4, 5, 6]);
        state.neighbor_events.push([6, 5, 4]);
        state.known_chunks.insert((-1, 3));
        state.random_seed = 123456789;
        state.nether = true;
        state.do_entity_drops = false;
        state.indexed = true;
        state.immediate = true;
        state.fall_instantly = true;
        state.end_dimension = true;
        state.fire_environment = FireEnvironment { do_fire_tick: false, difficulty: 3, raining: true };
    }
    let before = serde_json::to_value(&owner).unwrap();
    let ordinary = owner.clone();
    let count = crate::vanilla::random::independent_seed_calls_for_thread();
    let mut mirror = owner.presentation_clone();
    assert_eq!(crate::vanilla::random::independent_seed_calls_for_thread(), count);
    assert!(!Arc::ptr_eq(&owner.scheduler, &mirror.scheduler));
    assert!(!Arc::ptr_eq(&owner.scheduler, &ordinary.scheduler));
    assert_eq!(serde_json::to_value(&ordinary).unwrap(), before);
    assert_eq!(serde_json::to_value(&mirror.falling).unwrap(), before["falling"]);
    {
        let a = owner.state();
        let b = mirror.state();
        assert_eq!((b.clock, b.nether, b.do_entity_drops, b.random_seed),
            (a.clock, a.nether, a.do_entity_drops, a.random_seed));
        assert_eq!((b.indexed, b.immediate, b.fall_instantly, b.end_dimension),
            (a.indexed, a.immediate, a.fall_instantly, a.end_dimension));
        assert_eq!(format!("{:?}", b.fire_environment), format!("{:?}", a.fire_environment));
        assert!(!a.queue.is_empty() && !a.pending.is_empty() && !a.known_chunks.is_empty());
        assert!(b.queue.is_empty() && b.pending.is_empty() && b.known_chunks.is_empty());
        assert!(b.notifications.is_empty() && b.neighbor_events.is_empty());
    }
    mirror.set_clock(900);
    mirror.schedule_block_update(IVec3::new(3, 7, 4), 10, 17, 0);
    mirror.falling[0].position.x = -100.;
    assert_eq!(serde_json::to_value(&owner).unwrap(), before, "mirror writes cannot reach the owner");
    owner.set_clock(456);
    assert_eq!(mirror.clock(), 900, "owner writes cannot reach the mirror");
}
