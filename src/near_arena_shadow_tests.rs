use super::*;
use ahash::AHashMap;

// Independent byte occupancy oracle: no free-list/coalescing implementation.
struct Bytes {size:usize,max:usize,pages:Vec<Vec<bool>>}
impl Bytes {
    fn allocate(&mut self,len:usize,allow_new:bool)->Option<Allocation> {
        if len==0||len>self.size{return None;}
        for (page,bytes) in self.pages.iter_mut().enumerate() {
            if let Some(start)=(0..=self.size-len).find(|&at|bytes[at..at+len].iter().all(|b|!*b)) {
                bytes[start..start+len].fill(true);
                return Some(Allocation{page:page as u16,start:start as u64,len:len as u64});
            }
        }
        if !allow_new||self.pages.len()==self.max{return None;}
        let page=self.pages.len();let mut bytes=vec![false;self.size];bytes[..len].fill(true);self.pages.push(bytes);
        Some(Allocation{page:page as u16,start:0,len:len as u64})
    }
    fn release(&mut self,a:Allocation) {
        let bytes=&mut self.pages[usize::from(a.page)];let range=a.start as usize..(a.start+a.len)as usize;
        assert!(bytes[range.clone()].iter().all(|b|*b));bytes[range].fill(false);
    }
    fn assert_pool(&self,pool:&Pool) {
        assert_eq!(usize::from(pool.pages),self.pages.len());
        assert!(pool.holes.len()<=pool.hole_limit);
        let mut free=vec![vec![false;self.size];self.pages.len()];
        for h in &pool.holes {
            let range=h.start as usize..(h.start+h.len)as usize;
            for value in &mut free[usize::from(h.page)][range] {assert!(!*value);*value=true;}
        }
        for (used,free) in self.pages.iter().zip(free) {
            assert!(used.iter().zip(free).all(|(&u,f)|u!=f));
        }
        assert!(pool.holes.windows(2).all(|w|(w[0].page,w[0].start)<(w[1].page,w[1].start)));
    }
}
#[test]
fn first_fit_pool_matches_byte_occupancy_under_fragmentation_and_reuse() {
    let mut pool=Pool::new(31,3,16);let capacity=pool.holes.capacity();
    let mut reference=Bytes{size:31,max:3,pages:Vec::new()};let mut live=Vec::new();
    let mut random=0x8f42_01a1u32;
    for step in 0..2000 {
        random=random.wrapping_mul(1664525).wrapping_add(1013904223);
        if !live.is_empty()&&(random%3==0||live.len()==16) {
            let a=live.remove(random as usize%live.len());assert!(pool.release(a));reference.release(a);
        }else{
            let len=(random>>8)as usize%34;let allow=step%5!=0;
            let a=pool.allocate(len as u64,allow);assert_eq!(a,reference.allocate(len,allow));
            if let Some(a)=a{live.push(a);}
        }
        reference.assert_pool(&pool);assert_eq!(pool.holes.capacity(),capacity);
    }
    for a in live {assert!(pool.release(a));reference.release(a);}
    reference.assert_pool(&pool);
    assert!(pool.holes.iter().all(|h|h.start==0&&h.len==31));
    let before=pool.holes.clone();
    assert!(!pool.release(Allocation{page:pool.pages,start:0,len:1}));
    assert!(!pool.release(Allocation{page:0,start:u64::MAX,len:2}));
    assert_eq!(pool.holes,before);
    pool.clear();assert_eq!(pool.pages,0);assert!(pool.holes.is_empty());assert_eq!(pool.holes.capacity(),capacity);
    assert_eq!(pool.allocate(31,true),Some(Allocation{page:0,start:0,len:31}));
}

fn upload(width:u8,vertex_reserved:u64,index_reserved:u64)->Upload {
    let index_count=6;let bytes=index_count*u64::from(width);
    Upload{vertex_count:4,vertex_used:4*VERTEX_STRIDE,index_count,index_used:(bytes+3)&!3,
        vertex_reserved,index_reserved,width}
}
#[test]
fn replacement_respects_reuse_width_budget_and_resident_limit_without_leaks() {
    let mut m=Model::with_limits(16*VERTEX_STRIDE,32*VERTEX_STRIDE,32,64,3);
    let a=upload(2,8*VERTEX_STRIDE,16);
    m.upload((0,0),1,a);m.upload((1,0),2,a);
    let first=m.entries[&(0,0)];let other=m.entries[&(1,0)];
    assert_eq!(first.vertex,Some(Allocation{page:0,start:0,len:8*VERTEX_STRIDE}));
    assert_eq!(other.vertex,Some(Allocation{page:0,start:8*VERTEX_STRIDE,len:8*VERTEX_STRIDE}));
    assert_eq!((m.narrow.pages,m.wide.pages),(1,0));
    m.upload((0,0),3,upload(2,4*VERTEX_STRIDE,12));
    assert_eq!(m.entries[&(0,0)].vertex,first.vertex);assert_eq!(m.entries[&(0,0)].index,first.index);
    m.upload((0,0),4,upload(4,4*VERTEX_STRIDE,24));
    assert_eq!(m.entries[&(0,0)].vertex,first.vertex);assert_eq!(m.entries[&(0,0)].index.unwrap().page,0);
    assert_eq!((m.narrow.pages,m.wide.pages),(1,1));assert_eq!(m.events.width_changes,1);
    // The second wide reservation cannot use narrow free bytes or add a third index page.
    m.upload((2,0),5,upload(4,8*VERTEX_STRIDE,24));
    assert!(m.entries[&(2,0)].vertex.is_none()&&m.entries[&(2,0)].index.is_none());
    assert_eq!(m.events.placement_failures,1);assert_eq!(m.entries[&(1,0)].vertex,other.vertex);
    m.upload((3,0),6,a);assert!(!m.entries.contains_key(&(3,0)));assert_eq!(m.events.resident_limit_rejections,1);
    m.removed((0,0));m.upload((2,0),7,upload(4,8*VERTEX_STRIDE,24));
    assert!(m.entries[&(2,0)].vertex.is_some()&&m.entries[&(2,0)].index.is_some());
    assert_eq!(m.narrow.pages+m.wide.pages,2);
    let mut malformed=a;malformed.vertex_count=u64::MAX;
    m.upload((2,0),8,malformed);assert!(m.entries[&(2,0)].vertex.is_none());
    assert_eq!(m.events.invalid_uploads,1);assert!(!m.faulted);
    m.removed((1,0));m.removed((2,0));
    assert_eq!(m.vertices.holes.iter().map(|h|h.len).sum::<u64>(),u64::from(m.vertices.pages)*m.vertices.page_size);
    assert_eq!(m.narrow.holes.iter().map(|h|h.len).sum::<u64>(),u64::from(m.narrow.pages)*32);
    assert_eq!(m.wide.holes.iter().map(|h|h.len).sum::<u64>(),u64::from(m.wide.pages)*32);
    m.clear();assert!(m.entries.is_empty());assert_eq!((m.vertices.pages,m.narrow.pages,m.wide.pages),(0,0,0));
}

#[derive(Clone)]
struct Row {position:ChunkPos,width:u8,ranges:Vec<Range<u32>>,expected:Option<Binding>}
struct Rows(Vec<Row>);
impl Source for Rows {
    fn len(&self)->usize{self.0.len()}
    fn key(&self,i:usize)->(ChunkPos,u16,u8){let r=&self.0[i];(r.position,7,r.width)}
    fn ranges(&self,i:usize)->&[Range<u32>]{&self.0[i].ranges}
}
fn b(vertex_page:u16,index_page:u16,width:u8)->Option<Binding>{Some(Binding{vertex_page,index_page,width})}
// Flatten the original per-column range stream first; a separate adjacent-key
// scan computes run boundaries. No call to Model::binding or allocator state.
fn exact(model:&Model,rows:&Rows,identities:&AHashMap<ChunkPos,u64>)->Counts {
    let flat:Vec<_>=rows.0.iter().flat_map(|row|row.ranges.iter().filter(|r|!r.is_empty())
        .map(move|r|((row.position,row.width,r.start,r.end),row.expected))).collect();
    let mut expected:Vec<(Range<usize>,Option<Binding>)>=Vec::new();
    for (i,(_,binding)) in flat.iter().enumerate() {
        if let Some((range,previous))=expected.last_mut() {
            if binding.is_some()&&binding==previous {range.end=i+1;continue;}
        }
        expected.push((i..i+1,*binding));
    }
    let mut emitted=Vec::new();let mut expanded=Vec::new();let mut cursor=0;
    let counts=model.walk(rows,identities,|range,binding|{
        assert_eq!(range.start,cursor);cursor=range.end;
        expanded.extend(flat[range.clone()].iter().map(|(draw,_)|*draw));emitted.push((range,binding));
    });
    assert_eq!(emitted,expected);assert_eq!(cursor,flat.len());
    assert_eq!(expanded,flat.iter().map(|(draw,_)|*draw).collect::<Vec<_>>());
    assert_eq!(counts.records,flat.len()as u64);assert_eq!(counts.calls,expected.len()as u64);
    assert_eq!(counts.fallback_records,flat.iter().filter(|(_,binding)|binding.is_none()).count()as u64);
    assert_eq!(counts.records_in_mdi,expected.iter().filter(|(r,_)|r.len()>1).map(|(r,_)|r.len()as u64).sum::<u64>());
    counts
}
#[test]
fn consecutive_binding_runs_preserve_exact_range_order_and_every_fallback_boundary() {
    let mut m=Model::with_limits(8*VERTEX_STRIDE,32*VERTEX_STRIDE,32,64,8);
    let small=upload(2,4*VERTEX_STRIDE,12);let wide=upload(4,4*VERTEX_STRIDE,24);
    let mut ids=AHashMap::new();
    for (p,u) in [((0,0),small),((1,0),small),((2,0),wide),((3,0),small)] {
        let rev=p.0 as u64+1;m.upload(p,rev,u);ids.insert(p,rev);
    }
    let mut invalid=small;invalid.index_used=1;m.upload((4,0),5,invalid);ids.insert((4,0),5);
    m.upload((5,0),6,wide);ids.insert((5,0),6); // wide page cannot fit another24-byte reservation.
    let mut rows=Rows(vec![
        Row{position:(0,0),width:2,ranges:vec![0..3,3..3,3..6],expected:b(0,0,2)},
        Row{position:(1,0),width:2,ranges:vec![0..6],expected:b(0,0,2)},
        Row{position:(2,0),width:4,ranges:vec![0..6],expected:b(1,0,4)},
        Row{position:(99,0),width:2,ranges:vec![0..3,3..6],expected:None},
        Row{position:(0,0),width:2,ranges:vec![0..6],expected:b(0,0,2)},
        Row{position:(4,0),width:2,ranges:vec![0..6],expected:None},
        Row{position:(5,0),width:4,ranges:vec![0..6],expected:None},
        Row{position:(1,0),width:4,ranges:vec![0..6],expected:None},
        Row{position:(0,0),width:2,ranges:vec![0..7],expected:None},
        Row{position:(0,0),width:2,ranges:vec![0..6],expected:b(0,0,2)},
    ]);
    let c=exact(&m,&rows,&ids);
    assert_eq!((c.unknown_records,c.invalid_metadata_records,c.unplaced_records,c.width_mismatch_records,c.invalid_range_records),(2,1,1,1,1));
    assert_eq!((c.mdi_calls,c.max_run,c.run_histogram[1]),(1,3,1));
    assert_eq!((c.vertex_page_transitions,c.index_page_transitions,c.width_transitions),(1,0,1));
    // ABA/revision changes fail closed even if a virtual allocation is reused.
    m.removed((0,0));m.upload((0,0),20,small);
    for row in &mut rows.0 {if row.position==(0,0){row.expected=None;}}
    assert!(exact(&m,&rows,&ids).unknown_records>=6);
    ids.insert((0,0),20);
    for row in &mut rows.0 {if row.position==(0,0)&&row.ranges[0].end<=6{row.expected=b(0,0,2);}}
    exact(&m,&rows,&ids);
    rows.0.reverse();exact(&m,&rows,&ids);
}

struct Repeat {len:usize,ranges:Vec<Range<u32>>}
impl Source for Repeat {
    fn len(&self)->usize{self.len}
    fn key(&self,i:usize)->(ChunkPos,u16,u8){assert!(i<self.len);((0,0),1,2)}
    fn ranges(&self,i:usize)->&[Range<u32>]{assert!(i<self.len);&self.ranges}
}
#[test]
fn census_caps_maximum_runs_and_counts_cached_direct_and_abandoned_frames_separately() {
    let mut c=Census::new();let ids=AHashMap::from_iter([((0,0),1)]);
    c.upload((0,0),Some(1),upload(2,4*VERTEX_STRIDE,12));
    let one=Repeat{len:1,ranges:vec![0..3,3..6]};
    c.sample(&one,&ids,2);c.completed(true);c.completed(false);
    for m in &c.models {assert_eq!((m.all.frames,m.all.records,m.all.calls,m.all.all_record_argument_bytes),(1,4,2,40));assert_eq!(m.direct.frames,0);}
    c.sample(&one,&ids,1);c.completed(false);
    c.sample(&one,&ids,1);c.cancel_pending();c.completed(false);
    for m in &c.models {assert_eq!(m.all.frames,3);assert_eq!((m.direct.frames,m.direct.records),(1,2));}
    let too_many=Repeat{len:MAX_VISIBLE+1,ranges:vec![0..6]};
    let too_many_ranges=Repeat{len:1,ranges:vec![0..6;RANGE_LIMIT+1]};
    for (input,eyes) in [(&too_many,1),(&too_many_ranges,1),(&one,0),(&one,3)] {
        c.sample(input,&ids,eyes);assert!(c.pending.is_none());c.completed(false);
    }
    assert_eq!(c.incomplete_frames,4);assert!(c.models.iter().all(|m|m.all.frames==3));
    let bound=Repeat{len:MAX_VISIBLE,ranges:vec![0..6;RANGE_LIMIT]};
    c.sample(&bound,&ids,1);c.completed(false);
    for m in &c.models {assert_eq!(m.all.max_run,(MAX_VISIBLE*RANGE_LIMIT)as u64);assert_eq!(m.all.run_histogram[16],1);}
    c.upload((0,0),None,upload(2,4*VERTEX_STRIDE,12));assert_eq!(c.unknown_uploads,1);
    assert!(c.models.iter().all(|m|!m.entries.contains_key(&(0,0))));
    c.sample(&one,&ids,1);c.clear();c.completed(false);
    assert!(c.models.iter().all(|m|m.entries.is_empty()&&m.vertices.pages==0&&m.direct.frames==2));
}

#[test]
fn actual_shadow_keeps_warmup_placement_but_never_counts_disabled_or_uncompleted_draws() {
    use super::super::Shadow;
    let metadata=upload(2,4*VERTEX_STRIDE,12);
    let mut disabled=Shadow::default();
    disabled.payload_changed((0,0),&[],false);disabled.geometry_uploaded((0,0),metadata);
    disabled.removed((0,0),&[],false);disabled.cleared();
    assert!(!disabled.begin_frame(true));assert!(disabled.state.is_none());
    let mut shadow=Shadow::new(true);
    shadow.payload_changed((0,0),&[],false);shadow.geometry_uploaded((0,0),metadata);
    let allocation=shadow.state.as_ref().unwrap().arena.models[0].entries[&(0,0)].vertex;
    assert!(!shadow.begin_frame(false));
    assert_eq!(shadow.state.as_ref().unwrap().arena.models[0].entries[&(0,0)].vertex,allocation);
    shadow.bindings_changed();
    assert_eq!(shadow.state.as_ref().unwrap().arena.models[0].entries[&(0,0)].vertex,allocation);
    let data=Rows(vec![Row{position:(0,0),width:2,ranges:vec![0..3,3..6],expected:b(0,0,2)}]);
    let format=wgpu::TextureFormat::Rgba8Unorm;
    assert!(shadow.begin_frame(true));
    shadow.state.as_mut().unwrap().sample(0,1,format,&[],1,0,false,&data);
    // A following warmup/disabled begin abandons only completion, not placements.
    assert!(!shadow.begin_frame(false));shadow.completed(false,false);
    let arena=&shadow.state.as_ref().unwrap().arena;
    assert_eq!((arena.models[0].all.frames,arena.models[0].direct.frames),(1,0));
    assert_eq!(arena.models[0].entries[&(0,0)].vertex,allocation);
    assert!(shadow.begin_frame(true));
    shadow.state.as_mut().unwrap().sample(1,2,format,&[],1,0,false,&data);
    shadow.completed(false,false);
    assert_eq!(shadow.state.as_ref().unwrap().arena.models[0].direct.records,4);
    // Missing identity rejects the older reuse model; the arena must still
    // account for this exact direct fallback before that model's early return.
    let unknown=Rows(vec![Row{position:(99,0),width:2,ranges:vec![0..6],expected:None}]);
    assert!(shadow.begin_frame(true));
    shadow.state.as_mut().unwrap().sample(2,1,format,&[],1,0,false,&unknown);
    assert!(shadow.state.as_ref().unwrap().pending.is_none());shadow.completed(false,false);
    assert_eq!(shadow.state.as_ref().unwrap().arena.models[0].direct.unknown_records,1);
    shadow.removed((0,0),&[],false);
    assert!(shadow.state.as_ref().unwrap().arena.models.iter().all(|m|m.entries.is_empty()));
    shadow.payload_changed((0,0),&[],false);shadow.geometry_uploaded((0,0),metadata);
    shadow.cleared();
    assert!(shadow.state.as_ref().unwrap().arena.models.iter().all(|m|m.entries.is_empty()&&m.vertices.pages==0));
}
