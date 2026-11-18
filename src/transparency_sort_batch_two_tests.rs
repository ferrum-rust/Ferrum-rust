//! ScopedTwo-only contracts, using the existing independent original tuple oracle.
use super::*;

fn weighted_jobs(weights:&[usize])->Vec<Job> {
    weights.iter().enumerate().map(|(slot,&count)|Job {
        slot,mask:u16::MAX,result:Sort::Skipped,elapsed_ms:0.,
        order:Order {quads:vec![Quad {center:Vec3::ZERO,indices:[0;6],section:0};count],..Default::default()},
    }).collect()
}

fn check_split(weights:&[usize]) {
    let jobs=weighted_jobs(weights);
    let(first,second,owner)=split_three(&jobs);
    assert!(0<first&&first<second&&second<jobs.len());
    let spans=[0..first,first..second,second..jobs.len()];
    assert_eq!(spans.iter().flat_map(|r|r.clone()).collect::<Vec<_>>(),(0..jobs.len()).collect::<Vec<_>>());
    let sums=spans.clone().map(|r|weights[r].iter().sum::<usize>());
    let largest=*sums.iter().max().unwrap();
    assert_eq!(sums[owner],largest);
    assert_eq!(owner,sums.iter().position(|&v|v==largest).unwrap(),"first largest wins owner ties");
    // Independent mathematical partition contract: nearest one-third prefix,
    // then nearest half of the remaining weight; earliest legal cut wins ties.
    let total=weights.iter().sum::<usize>();
    let expected_first=(1..=weights.len()-2).min_by_key(|&i|
        ((weights[..i].iter().sum::<usize>()*3).abs_diff(total),i)).unwrap();
    let rest=weights[expected_first..].iter().sum::<usize>();
    let expected_second=(expected_first+1..weights.len()).min_by_key(|&i|
        ((weights[expected_first..i].iter().sum::<usize>()*2).abs_diff(rest),i)).unwrap();
    assert_eq!((first,second),(expected_first,expected_second));
    assert_eq!(split_three(&jobs),(first,second,owner));
    for(slot,job)in jobs.iter().enumerate() {assert_eq!(job.slot,slot);assert_eq!(job.order.quads.len(),weights[slot]);}
}

#[test]
fn three_contiguous_spans_cover_every_job_and_main_owns_largest_weight() {
    for count in 3..=7usize {
        for mut code in 0..3usize.pow(count as u32) {
            let weights=(0..count).map(|_|{let value=[0,1,5][code%3];code/=3;value}).collect::<Vec<_>>();
            check_split(&weights);
        }
    }
    for heavy in 0..32 {
        let mut weights=vec![1;32];weights[heavy]=4096;
        check_split(&weights);
    }
    check_split(&[3,3,3]);check_split(&[0,0,0]);check_split(&[0,100,0]);
    check_split(&vec![1;1024]);
}

#[test]
fn scoped_two_keeps_wide_admission_and_selected_mode_controls_helper_count() {
    for(mode,expected)in [(Mode::Original,0),(Mode::OwnerBatched,0),(Mode::ScopedOne,1),
        (Mode::ScopedLarge,1),(Mode::ScopedWide,1),(Mode::ScopedTwo,2)]
    {
        let candidate=batch(mode);
        assert_eq!(candidate.helper_threads(),expected);
        assert_eq!(candidate.report()["configured_helpers"],expected);
        assert_eq!(candidate.report()["helper_threads"],expected);
        assert_eq!(candidate.report()["helper_available"],expected!=0);
    }
    let mut candidate=batch(Mode::ScopedTwo);
    let report=candidate.report();
    assert_eq!(report["mode"],"scoped-two");
    assert_eq!(report["min_jobs"],32);assert_eq!(report["min_quads"],98304);
    assert_eq!(report["max_jobs"],1024);assert_eq!(report["max_quads"],1048576);
    assert_eq!(candidate.jobs.capacity(),1024);
    assert!(!candidate.wants_batch(true));assert!(candidate.wants_batch(false));
    let storage=(candidate.jobs.as_ptr(),candidate.jobs.capacity());
    candidate.configure(Mode::ScopedTwo);
    assert_eq!((candidate.jobs.as_ptr(),candidate.jobs.capacity()),storage);
    for(count,repeated,last,expected)in [
        (0,0,0,Admission::Small),(31,4096,4096,Admission::Small),
        (32,3072,3071,Admission::Small),
        (32,3072,3072,Admission::Ready{jobs:32,quads:98304}),
        (544,576,576,Admission::Ready{jobs:544,quads:313344}),
        (1024,1024,1024,Admission::Ready{jobs:1024,quads:1048576}),
        (1025,96,96,Admission::Limit),(32,32768,32769,Admission::Limit),
    ] {
        let mut slots=LengthSlots::new(count,repeated,last);
        assert_eq!(admission(&slots,Vec3::ZERO,Mode::ScopedTwo),expected);
        assert_eq!(admission(&slots,Vec3::ZERO,Mode::ScopedWide),expected);
        if !matches!(expected,Admission::Ready{..}) {
            let mut profile=frame(true);
            assert!(!candidate.run_slots(&mut slots,Vec3::ZERO,&mut profile));
            assert_eq!(profile.counters.transparency_batch_frames,0);
            assert!(candidate.jobs.is_empty());
            assert_eq!((candidate.jobs.as_ptr(),candidate.jobs.capacity()),storage);
        }
    }
    candidate.pool=None;
    let mut slots=LengthSlots::new(32,3072,3072);let mut profile=frame(true);
    assert!(!candidate.wants_batch(false));
    assert!(!candidate.run_slots(&mut slots,Vec3::ZERO,&mut profile));
    assert_eq!(profile.counters.transparency_batch_unavailable,1);
    assert_eq!(candidate.report()["configured_helpers"],2);
    assert_eq!(candidate.report()["helper_threads"],0);
}

#[test]
fn each_of_three_lanes_panics_before_or_after_sort_only_after_other_lanes_join() {
    let mut candidate=batch(Mode::ScopedTwo);
    assert_eq!(candidate.helper_threads(),2);
    let job_storage=(candidate.jobs.as_ptr(),candidate.jobs.capacity());
    for lane in [Lane::Owner,Lane::Worker,Lane::WorkerSecond] {
        for last_in_lane in [false,true] {
            for after in [false,true] {
                let mut slots=MemorySlots::new(36,3072);
                let pointers=slots.rows.iter().map(|(o,_)|allocations(o)).collect::<Vec<_>>();
                let probe=weighted_jobs(&vec![3072;36]);
                let(first,second,owner)=split_three(&probe);drop(probe);
                let spans=[0..first,first..second,second..36];
                let helper_spans=(0..3).filter(|&i|i!=owner).collect::<Vec<_>>();
                let lane_index=match lane {Lane::Owner=>owner,Lane::Worker=>helper_spans[0],Lane::WorkerSecond=>helper_spans[1]};
                let target=if last_in_lane{spans[lane_index].end-1}else{spans[lane_index].start};
                let mut expected=slots.rows.iter().map(|(o,_)|original(o)).collect::<Vec<_>>();
                for oracle in &mut expected {original_sort(oracle,Vec3::ZERO,u16::MAX);}
                candidate.fault=Fault {lane:Some(lane),slot:target,after};
                let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||
                    candidate.run_slots(&mut slots,Vec3::ZERO,&mut frame(false))));
                assert!(result.is_err(),"{lane:?}/last={last_in_lane}/after={after}");
                assert_eq!(slots.takes,(0..36).collect::<Vec<_>>());assert_eq!(slots.puts,slots.takes);
                assert_eq!(slots.rows.iter().map(|(o,_)|allocations(o)).collect::<Vec<_>>(),pointers);
                assert!(candidate.jobs.is_empty());
                assert_eq!((candidate.jobs.as_ptr(),candidate.jobs.capacity()),job_storage);
                // Every other lane must have completed, even if its task had
                // not started when the selected lane panicked. A retained task
                // cannot safely mutate any Order after Restore returned it.
                for(slot,((order,_),oracle))in slots.rows.iter().zip(&expected).enumerate() {
                    if !spans[lane_index].contains(&slot)||slot<target||(slot==target&&after) {
                        same(order,oracle);
                    }else{
                        assert!(order.eye.is_none());assert!(order.indices.is_empty());
                    }
                }
                candidate.fault=Fault::default();slots.takes.clear();slots.puts.clear();
                assert!(candidate.run_slots(&mut slots,Vec3::X*16.,&mut frame(false)));
                for((order,_),oracle)in slots.rows.iter().zip(&mut expected) {
                    original_sort(oracle,Vec3::X*16.,u16::MAX);same(order,oracle);
                }
                assert_eq!(slots.takes,slots.puts);
                assert_eq!(slots.rows.iter().map(|(o,_)|allocations(o)).collect::<Vec<_>>(),pointers);
                candidate.jobs.clear();
                assert_eq!((candidate.jobs.as_ptr(),candidate.jobs.capacity()),job_storage);
            }
        }
    }
}

#[test]
fn scoped_two_sorts_above_old_caps_with_original_tuple_payload_and_retained_storage() {
    let mut candidate=batch(Mode::ScopedTwo);
    let mut slots=MemorySlots::new(544,576);
    let mut expected=slots.rows.iter().map(|(o,_)|original(o)).collect::<Vec<_>>();
    let storage=slots.rows.iter().map(|(o,_)|allocations(o)).collect::<Vec<_>>();
    let job_storage=(candidate.jobs.as_ptr(),candidate.jobs.capacity());
    for(step,(eye,mask))in [(Vec3::ZERO,u16::MAX),(Vec3::ZERO,u16::MAX),
        (Vec3::X*4.,0xaaaa)].into_iter().enumerate()
    {
        for(_,at)in &mut slots.rows {*at=mask;}
        let results=expected.iter_mut().map(|o|original_sort(o,eye,mask)).collect::<Vec<_>>();
        slots.takes.clear();slots.puts.clear();let mut profile=frame(true);
        let admitted=candidate.run_slots(&mut slots,eye,&mut profile);
        assert_eq!(admitted,step!=1);
        let actual=if admitted {
            assert_eq!(profile.counters.transparency_batch_jobs,544);
            assert_eq!(profile.counters.transparency_batch_quads,313344);
            assert!(profile.counters.transparency_batch_worker_jobs>0);
            assert_eq!(slots.takes,(0..544).collect::<Vec<_>>());assert_eq!(slots.puts,slots.takes);
            let mut values=vec![Sort::Skipped;544];
            for job in &candidate.jobs {values[job.slot]=job.result;}values
        }else{
            assert!(slots.takes.is_empty()&&slots.puts.is_empty());
            slots.rows.iter_mut().map(|(o,m)|o.sort(eye,*m)).collect()
        };
        assert_eq!(actual,results);
        for((order,_),oracle)in slots.rows.iter().zip(&expected) {same(order,oracle);}
        assert_eq!(actual_writes(&mut slots,&actual),original_writes(&expected,&results));
        assert_eq!(slots.rows.iter().map(|(o,_)|allocations(o)).collect::<Vec<_>>(),storage);
        candidate.jobs.clear();
        assert_eq!((candidate.jobs.as_ptr(),candidate.jobs.capacity()),job_storage);
    }
}

#[test]
fn ordinary_cpu_policy_has_exact_sixteen_boundary_and_unknown_is_wide() {
    assert_eq!(Mode::ordinary_for_parallelism(None),Mode::ScopedWide);
    for count in 0..16 {
        assert_eq!(Mode::ordinary_for_parallelism(Some(count)),Mode::ScopedWide);
    }
    for count in [16,17,24,32,64,usize::MAX] {
        assert_eq!(Mode::ordinary_for_parallelism(Some(count)),Mode::ScopedTwo);
    }
}
