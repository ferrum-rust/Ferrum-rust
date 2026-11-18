use super::*;
use std::sync::Arc;
fn rows(start:i32,n:usize)->Vec<Row>{(0..n).map(|i|Row{visible_index:i,
    key:key((start+i as i32,0),1,7,2,&[0..6,6..6,12..18],18).unwrap()}).collect()}
fn exact_plan(cache:&mut Cache<Arc<Vec<Key>>>,rows:&[Row],commands:&mut Vec<Command<Arc<Vec<Key>>>>)->PlanCounts{
    let counts=plan(cache,rows,commands,|input|Arc::new(input.iter().map(|r|r.key).collect()));
    // Independently expand actual cached payloads, not their current cache keys.
    // An evicted entry must remain usable in the already prepared command list.
    let mut actual=Vec::new();
    for command in commands{match command{
        Command::Cached{value,..}=>actual.extend(value.iter().copied()),
        Command::Direct(index)=>actual.push(rows.iter().find(|row|row.visible_index==*index).unwrap().key),
    }}
    let expected:Vec<_>=rows.iter().map(|row|row.key).collect();assert_eq!(actual,expected);
    assert!(counts.built<=BUILDS as u64);assert!(cache.entries.len()<=CACHE);counts
}
#[test]
fn default_is_unallocated_and_cli_requires_a_clean_streaming_arm(){
    use clap::Parser;
    let mut disabled=Runs::default();disabled.payload_changed((0,0));disabled.removed((0,0));
    disabled.bindings_changed();disabled.cleared();assert!(disabled.state.is_none());assert_eq!(disabled.report()["enabled"],false);
    assert!(!crate::Options::try_parse_from(["ferrum"]).unwrap().benchmark_near_opaque_runs);
    assert!(crate::Options::try_parse_from(["ferrum","--benchmark-near-opaque-runs"]).is_err());
    assert!(crate::Options::try_parse_from(["ferrum","--benchmark","--benchmark-streaming","--benchmark-near-opaque-runs"]).unwrap().benchmark_near_opaque_runs);
    assert!(crate::Options::try_parse_from(["ferrum","--benchmark","--benchmark-streaming","--benchmark-near-opaque-runs","--benchmark-capture-near-opaque"]).is_err());
}
#[test]
fn ordered_run_plan_respects_build_cap_lookahead_and_all_payload_bits(){
    let mut cache=Cache::new();let mut commands=Vec::with_capacity(MAX_VISIBLE);let original=rows(0,24);
    let first=exact_plan(&mut cache,&original,&mut commands);assert_eq!((first.built,first.direct_chunks),(4,8));
    let second=exact_plan(&mut cache,&original,&mut commands);assert_eq!((second.reused,second.built),(4,2));
    assert_eq!(exact_plan(&mut cache,&original,&mut commands).reused,6);
    let mut prefix=rows(-1,1);prefix.extend_from_slice(&original);for (i,row) in prefix.iter_mut().enumerate(){row.visible_index=i;}
    let shifted=exact_plan(&mut cache,&prefix,&mut commands);assert_eq!((shifted.reused,shifted.direct_chunks),(6,1));
    for field in 0..6{
        let mut isolated=Cache::new();exact_plan(&mut isolated,&original[..8],&mut commands);
        let mut changed=original[..8].to_vec();match field{
            0=>changed[1].key.revision+=1,1=>changed[1].key.mask^=4,2=>changed[1].key.width=4,
            3=>changed[1].key.ranges[0].end+=6,4=>changed[1].key.len=1,_=>changed.swap(0,1),
        }
        for (i,row) in changed.iter_mut().enumerate(){row.visible_index=i;}
        assert_eq!(exact_plan(&mut isolated,&changed,&mut commands).reused,1);
    }
    assert_eq!(commands.capacity(),MAX_VISIBLE);
}
#[test]
fn fifo_eviction_does_not_destroy_current_frame_payload_and_bounds_remain_fixed(){
    let mut cache=Cache::new();let mut commands=Vec::with_capacity(MAX_VISIBLE);let mut weak=None;
    for group in 0..CACHE{
        let input=rows((group*4)as i32,4);let key=std::array::from_fn(|i|input[i].key);
        let payload=Arc::new(key.to_vec());if group==0{weak=Some(Arc::downgrade(&payload));}
        cache.insert(key,payload);
    }
    let fifo_capacity=cache.fifo.capacity();let mut input=rows(0,4);input.extend(rows((CACHE*4)as i32,16));
    for (i,row) in input.iter_mut().enumerate(){row.visible_index=i;}
    let counts=exact_plan(&mut cache,&input,&mut commands);
    assert_eq!((counts.reused,counts.built,counts.evicted),(1,4,4));assert!(!cache.entries.contains_key(&(0,0)));
    assert!(weak.as_ref().unwrap().upgrade().is_some());commands.clear();assert!(weak.unwrap().upgrade().is_none());
    for group in CACHE+4..CACHE+700{let input=rows((group*4)as i32,4);exact_plan(&mut cache,&input,&mut commands);}
    assert_eq!(cache.entries.len(),CACHE);assert_eq!(cache.fifo.capacity(),fifo_capacity);
}
#[test]
fn uploads_remove_nonanchor_aliases_and_clear_previous_plan_before_aba_or_binding_changes(){
    let mut state=State::new();for i in 0..12{state.payload((i,0),false);}
    let original=state.identities[&(1,0)];
    for anchor in [0,4,8]{
        let mut input=rows(anchor,4);if anchor==4{input[1].key.position=(1,0);}
        state.cache.insert(std::array::from_fn(|i|input[i].key),Bundles([None,None]));
    }
    state.commands.push(Command::Cached{value:Bundles([None,None]),draws:8,built:false});state.active=true;
    state.payload((1,0),false);assert!(state.commands.is_empty());assert!(!state.active);
    assert_eq!(state.cache.entries.len(),1);assert!(state.cache.entries.contains_key(&(8,0)));
    assert_eq!(state.events.invalidated_entries,2);assert_ne!(state.identities[&(1,0)],original);
    state.payload((1,0),true);state.payload((1,0),false);assert_ne!(state.identities[&(1,0)],original);
    let mut runs=Runs{state:Some(Box::new(state))};runs.bindings_changed();
    let state=runs.state.as_mut().unwrap();assert!(state.cache.entries.is_empty());assert!(state.meta.is_none());
    state.next_revision=u64::MAX;state.payload((99,0),false);
    assert_eq!(state.identities.len(),1);assert_eq!(state.events.revision_wraps,1);
    runs.cleared();assert!(runs.state.as_ref().unwrap().identities.is_empty());
}
#[test]
fn key_and_admission_bounds_fail_closed_without_changing_nonempty_range_order(){
    assert!(admits(0,1));assert!(admits(MAX_VISIBLE,2));assert!(!admits(MAX_VISIBLE+1,1));assert!(!admits(1,0));assert!(!admits(1,3));
    let ranges=[12..18,0..0,0..6,6..12];let result=key((0,0),4,7,4,&ranges,18).unwrap();
    assert_eq!(&result.ranges[..3],&[DrawRange{start:12,end:18},DrawRange{start:0,end:6},DrawRange{start:6,end:12}]);
    assert!(key((0,0),0,1,2,&[],0).is_some());
    assert!(key((0,0),0,1,2,&vec![0..0;RANGE_LIMIT+1],0).is_none());
    assert!(key((0,0),0,1,2,&[0..19],18).is_none());
    let mut state=State::new();for i in 0..MAX_VISIBLE+1{state.payload((i as i32,0),false);}
    assert_eq!(state.identities.len(),MAX_VISIBLE);assert_eq!(state.events.identity_capacity_rejections,1);
    state.payload((0,0),false);assert_eq!(state.events.identity_capacity_rejections,1);
    state.payload((0,0),true);state.payload((MAX_VISIBLE as i32,0),false);assert_eq!(state.identities.len(),MAX_VISIBLE);
}
