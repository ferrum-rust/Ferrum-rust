use super::*;
const FORMAT:wgpu::TextureFormat=wgpu::TextureFormat::Rgba8Unorm;
#[derive(Clone)]
struct Row {position:ChunkPos,mask:u16,width:u8,ranges:Vec<Range<u32>>}
struct Rows(Vec<Row>);
impl Source for Rows {
    fn len(&self)->usize{self.0.len()}
    fn key(&self,i:usize)->(ChunkPos,u16,u8){let r=&self.0[i];(r.position,r.mask,r.width)}
    fn ranges(&self,i:usize)->&[Range<u32>]{&self.0[i].ranges}
}
fn rows(n:usize)->Rows {Rows((0..n).map(|i|Row {position:(i as i32,0),mask:7,width:2,
    ranges:vec![0..6,6..6,12..18]}).collect())}
fn key(rows:&Rows)->Vec<(ChunkPos,u16)>{rows.0.iter().map(|r|(r.position,r.mask)).collect()}
fn populated(rows:&Rows)->State {let mut state=State::new();for r in &rows.0{state.payload(r.position);}state}
fn tokens(n:usize)->Vec<Token>{(0..n).map(|i|{let mut t=Token {position:(i as i32,0),revision:1,
    mask:3,width:2,len:2,..Default::default()};t.ranges[0]=DrawRange{start:0,end:6};
    t.ranges[1]=DrawRange{start:12,end:18};t}).collect()}
fn exact_walk(model:&mut Model,tokens:&[Token],eyes:usize,format:wgpu::TextureFormat)->Coverage{
    // Independent scalar expansion: cached/build/direct partitions must never
    // change original chunk/range encounter order, even after membership edits.
    let expected:Vec<_>=tokens.iter().flat_map(|t|t.ranges[..usize::from(t.len)].iter()
        .map(move|r|(t.position,t.revision,t.mask,t.width,r.start,r.end))).collect();
    let mut actual=Vec::new();let mut next=0;
    let result=model.walk(tokens,eyes,format,|_,range|{
        assert_eq!(range.start,next);next=range.end;
        for t in &tokens[range]{for r in &t.ranges[..usize::from(t.len)]{
            actual.push((t.position,t.revision,t.mask,t.width,r.start,r.end));
        }}
    });
    assert_eq!(next,tokens.len());assert_eq!(actual,expected);
    assert_eq!(result.reused_draws+result.built_draws+result.direct_draws,expected.len() as u64);
    assert!(result.built_groups<=BUILDS as u64);result
}
#[test]
fn consecutive_measured_frames_exclude_warmup_and_ordinary_state(){
    let mut disabled=Shadow::default();assert!(!disabled.begin_frame(true));
    disabled.payload_changed((0,0),&[],false);disabled.removed((0,0),&[],false);
    disabled.bindings_changed();disabled.cleared();assert!(disabled.state.is_none());
    assert_eq!(disabled.report()["enabled"],false);
    let mut shadow=Shadow::new(true);assert!(!shadow.begin_frame(false));
    for _ in 0..65{assert!(shadow.begin_frame(true));}
    let state=shadow.state.as_mut().unwrap();exact_walk(&mut state.model,&tokens(8),1,FORMAT);
    assert!(!state.model.entries.is_empty());
    assert!(!shadow.begin_frame(false));assert!(shadow.state.as_ref().unwrap().model.entries.is_empty());
    assert!(shadow.begin_frame(true));let report=shadow.report();
    assert_eq!(report["diagnostic_not_baseline"],true);
    assert_eq!(report["sample_every_enabled_render_frames"],1);
    assert_eq!(report["stats"]["eligible_frames"],66);
    assert_eq!(report["stats"]["warmup_or_disabled_frames"],2);
}
#[test]
fn exact_contiguous_runs_preserve_order_and_all_payload_constraints(){
    let mut model=Model::new();let base=tokens(32);
    assert_eq!(exact_walk(&mut model,&base,1,FORMAT).built_groups,4);
    let next=exact_walk(&mut model,&base,1,FORMAT);assert_eq!((next.reused_groups,next.built_groups),(4,4));
    assert_eq!(exact_walk(&mut model,&base,1,FORMAT).reused_groups,8);
    // Retain the old exact runs after a new one-chunk prefix; lookahead must
    // emit that prefix directly instead of building over a reusable run.
    let mut shifted=vec![Token{position:(-1,0),..base[0]}];shifted.extend_from_slice(&base);
    let shifted_result=exact_walk(&mut model,&shifted,1,FORMAT);
    assert_eq!((shifted_result.reused_groups,shifted_result.direct_chunks),(8,1));
    for field in 0..5{
        let mut isolated=Model::new();exact_walk(&mut isolated,&base[..8],1,FORMAT);
        let mut changed=base[..8].to_vec();match field{
            0=>changed[1].revision+=1,1=>changed[1].mask^=4,2=>changed[1].width=4,
            3=>changed[1].ranges[0].end+=6,_=>changed.swap(0,1),
        }
        let result=exact_walk(&mut isolated,&changed,1,FORMAT);assert_eq!(result.reused_groups,1);
    }
    assert_eq!(exact_walk(&mut model,&base,2,FORMAT).reused_groups,0);
    assert_eq!(exact_walk(&mut model,&base,2,wgpu::TextureFormat::Bgra8Unorm).reused_groups,0);
}
#[test]
fn actual_whole_key_reasons_and_bind_pairs_have_explicit_denominators(){
    let mut data=rows(3);data.0[1].ranges=vec![9..9];
    let previous=key(&data);let mut shadow=Shadow{state:Some(Box::new(populated(&data)))};
    let state=shadow.state.as_mut().unwrap();
    state.sample(10,2,FORMAT,&previous,2,1,false,&data);
    assert_eq!(state.stats.whole_gate_wait_samples,1);
    assert_eq!(state.stats.nonempty_solid_chunks,2);assert_eq!(state.stats.nonempty_solid_ranges,4);
    assert_eq!(state.stats.prospective_repeated_buffer_bind_pairs,4); // two ranges above chunks, two eyes
    shadow.completed(false,false);
    assert_eq!(shadow.state.as_ref().unwrap().stats.actual_direct_repeated_buffer_bind_pairs,4);
    let state=shadow.state.as_mut().unwrap();state.sample(11,2,FORMAT,&previous,2,3,true,&data);
    shadow.completed(false,true);let state=shadow.state.as_mut().unwrap();
    assert_eq!(state.stats.actual_direct_repeated_buffer_bind_pairs,4);
    assert_eq!(state.stats.whole_existing_reusable,1);assert_eq!(state.stats.whole_cached_samples,1);
    data.0[0].mask^=1;data.0.swap(1,2);
    state.sample(12,1,FORMAT,&previous,2,3,true,&data);
    assert_eq!(state.stats.whole_key_changed,1);assert_eq!(state.stats.whole_eye_count_changed,1);
    assert_eq!(state.stats.whole_position_order_changed,1);assert_eq!(state.stats.whole_aligned_mask_changed,1);
    assert_eq!(state.stats.whole_length_changed,0);
    assert_eq!(state.stats.complete_samples,3);
}
#[test]
fn bounds_fail_closed_and_fifo_keeps_memory_bounded(){
    let mut model=Model::new();let capacity=model.ring.capacity();
    for i in 0..700{let mut set=tokens(4);for t in &mut set{t.position.0+=i*4;}
        exact_walk(&mut model,&set,1,FORMAT);assert!(model.entries.len()<=CACHE);}
    assert_eq!(model.entries.len(),CACHE);assert_eq!(model.ring.capacity(),capacity);
    assert!(!model.entries.contains_key(&(0,0)));
    let mut data=rows(4);let mut state=populated(&data);let previous=key(&data);
    state.sample(1,1,FORMAT,&previous,1,1,false,&data);assert_eq!(state.model.entries.len(),1);
    data.0[0].ranges=vec![0..0;RANGE_LIMIT+1];
    state.sample(2,1,FORMAT,&previous,1,1,false,&data);
    assert_eq!(state.stats.too_many_ranges,1);assert!(state.model.entries.is_empty());assert!(state.pending.is_none());
    data.0[0].ranges=vec![0..6];state.identities.remove(&(0,0));
    state.sample(3,1,FORMAT,&previous,1,1,false,&data);assert_eq!(state.stats.unknown_payload_samples,1);
    let oversized=rows(MAX_VISIBLE+1);
    state.sample(4,1,FORMAT,&[],1,1,false,&oversized);assert_eq!(state.stats.too_many_visible,1);
    assert_eq!(state.tokens.capacity(),MAX_VISIBLE);
    let mut bounded=State::new();for i in 0..MAX_VISIBLE+1{bounded.payload((i as i32,0));}
    assert_eq!(bounded.identities.len(),MAX_VISIBLE);assert_eq!(bounded.stats.payload_capacity_rejections,1);
}
#[test]
fn payload_remove_reinsert_and_wrap_cannot_reuse_stale_identity(){
    let data=rows(8);let previous=key(&data);let mut shadow=Shadow{state:Some(Box::new(populated(&data)))};
    let old=shadow.state.as_ref().unwrap().identities[&(0,0)];
    shadow.removed((0,0),&previous,true);shadow.payload_changed((0,0),&previous,true);
    let state=shadow.state.as_mut().unwrap();assert_ne!(old,state.identities[&(0,0)]);
    assert_eq!(state.stats.active_gate_invalidations,2);
    state.sample(1,1,FORMAT,&previous,1,1,false,&data);
    assert!(!state.model.entries.is_empty());state.next_revision=u64::MAX;state.payload((99,0));
    assert_eq!(state.identities.len(),1);assert!(state.model.entries.is_empty());assert_eq!(state.stats.revision_wraps,1);
    let oversized=vec![((0,0),1);MAX_VISIBLE+1];
    shadow.payload_changed((0,0),&oversized,true);shadow.removed((0,0),&oversized,true);
    assert_eq!(shadow.state.as_ref().unwrap().stats.oversize_event_keys,2);
    shadow.bindings_changed();shadow.cleared();assert!(shadow.state.as_ref().unwrap().identities.is_empty());
}
