use super::*;
use super::super::near_arena::{Arena,Limits,PageFactory,PageKind,Request};

#[derive(Default)]struct Factory{count:usize}
impl PageFactory for Factory {type Handle=usize;type Error=();
    fn create(&mut self,_:PageKind,_:u64)->Result<usize,()>{let n=self.count;self.count+=1;Ok(n)}}
struct OwnedRow{backing:Backing,width:IndexWidth,ranges:Vec<Range<u32>>}
struct Rows(Vec<OwnedRow>);
impl Source for Rows {fn len(&self)->usize{self.0.len()}fn row(&self,i:usize)->Row<'_>{let r=&self.0[i];
    Row{backing:r.backing,width:r.width,ranges:&r.ranges}}}
fn request(width:IndexWidth)->Request {Request{vertices:4,indices:6,vertex_bytes:128,
    index_bytes:6*width.bytes(),vertex_reserved:128,index_reserved:if width==IndexWidth::U16{16}else{24},
    width,max_local_index:Some(3)}}
fn fixture()->(Arena<usize>,[Token;8]) {
    let mut arena=Arena::new(Mode::ArenaMdi,Limits{vertex_page:512,index_page:128,owners:8}).unwrap();
    let mut factory=Factory::default();
    let tokens=std::array::from_fn(|i|{let mut p=arena.begin_update(None,request(if i<4{IndexWidth::U16}else{IndexWidth::U32}),&mut factory).unwrap();
        let _=p.target();p.commit()});(arena,tokens)
}
fn limits(n:u32)->DrawLimits{DrawLimits::negotiated(true,true,true,Some(n))}
#[derive(Clone,Debug,PartialEq,Eq)]enum Flat{Dedicated(usize,Range<u32>),Arena(PageKey,Args)}
// Independent original forward visible-range walk. It does NOT use Binding::draw,
// Plan::validate, the planner's run state or its argument construction.
fn original(recording:&Recording<'_,usize>,rows:&Rows)->Vec<Flat> {
    let mut flat=Vec::new();
    for (i,row) in rows.0.iter().enumerate(){for range in &row.ranges {
        if range.is_empty(){continue;}
        flat.push(match row.backing {Backing::Dedicated=>Flat::Dedicated(i,range.clone()),Backing::Arena(token)=>{
            let b=recording.binding(token).unwrap();let bytes=if row.width==IndexWidth::U16{2}else{4};
            Flat::Arena(PageKey{arena_id:b.arena_id,vertex_page:b.vertex_page,index_page:b.index_page,width:row.width},
                Args{index_count:range.end-range.start,instance_count:1,
                    first_index:(b.index_range.start/bytes+u64::from(range.start)) as u32,
                    base_vertex:(b.vertex_range.start/32) as i32,first_instance:0})}});
    }}flat
}
fn expanded(prepared:&Prepared<'_,'_,'_,usize,Rows>,rows:&Rows)->Vec<Flat> {
    let mut flat=Vec::new();
    for command in prepared.commands(){match command {
        Command::Direct{row,range}=>{
            if let Some(b)=prepared.binding(*row).unwrap(){let bytes=if rows.0[*row].width==IndexWidth::U16{2}else{4};
                flat.push(Flat::Arena(PageKey{arena_id:b.arena_id,vertex_page:b.vertex_page,index_page:b.index_page,width:b.request.width},
                    Args{index_count:range.end-range.start,instance_count:1,first_index:(b.index_range.start/bytes+u64::from(range.start)) as u32,
                        base_vertex:(b.vertex_range.start/32) as i32,first_instance:0}));
            }else{flat.push(Flat::Dedicated(*row,range.clone()));}
        },Command::Multi{row,binding,first_arg,count}=>{
            let b=prepared.binding(*row).unwrap().unwrap();
            assert_eq!(*binding,PageKey{arena_id:b.arena_id,vertex_page:b.vertex_page,index_page:b.index_page,width:b.request.width});
            for arg in &prepared.args()[*first_arg as usize..(*first_arg+*count) as usize]{flat.push(Flat::Arena(*binding,*arg));}
        }
    }}flat
}

#[test]
fn ordered_runs_match_original_for_modes_widths_singletons_and_native_splits() {
    let(mut arena,t)=fixture();let recording=arena.begin_recording().unwrap();
    let row=|i:usize,ranges|OwnedRow{backing:Backing::Arena(t[i]),width:if i<4{IndexWidth::U16}else{IndexWidth::U32},ranges};
    let rows=Rows(vec![row(0,vec![0..3,3..6]),row(1,vec![0..6]),
        OwnedRow{backing:Backing::Dedicated,width:IndexWidth::U16,ranges:vec![8..9]},
        row(0,vec![0..0,6..2,1..5]),row(4,vec![0..6]),row(5,vec![0..6]),row(2,vec![0..6]),row(6,vec![0..6])]);
    let expected=original(&recording,&rows);assert_eq!(expected.len(),9);
    let mut plan=Plan::new().unwrap();
    for mode in [Mode::Dedicated,Mode::ArenaDirect,Mode::ArenaMdi] {for count in [1,2,3,65_536,u32::MAX] {
        let ready=plan.build(mode,limits(count),&recording,&rows).unwrap();
        assert_eq!(expanded(&ready,&rows),expected);
        assert_eq!(ready.summary().records,expected.len());
        assert_eq!(ready.summary().direct_calls+ready.summary().multi_records,expected.len());
        if mode!=Mode::ArenaMdi||count==1 {assert_eq!(ready.summary().multi_calls,0);assert!(ready.args().is_empty());}
        else {assert!(ready.summary().multi_calls>=2);assert!(ready.commands().iter().all(|c|match c{
            Command::Multi{count:n,..}=>*n>=2&&*n<=count,Command::Direct{..}=>true}));}
    }}
    recording.discarded();
}

#[test]
fn native_capability_admission_is_conservative_and_does_not_guess_a_maximum() {
    for bits in 0..8 {for cap in [None,Some(0),Some(1),Some(2),Some(u32::MAX)] {
        let l=DrawLimits::negotiated(bits&1!=0,bits&2!=0,bits&4!=0,cap);
        assert_eq!(l.multi_draw_enabled(),bits==7&&cap.is_some_and(|n|n>=2));
        assert!(l.max_draws()<=MAX_RECORDS as u32);
    }}
    assert!(!DrawLimits::default().multi_draw_enabled());
    assert_eq!(std::mem::size_of::<Args>(),20);assert_eq!(std::mem::align_of::<Args>(),4);
    let a=Args{index_count:5,instance_count:1,first_index:9,base_vertex:-7,first_instance:0};
    assert_eq!(bytemuck::bytes_of(&a),bytemuck::cast_slice::<u32,u8>(&[5,1,9,(-7i32) as u32,0]));
}

#[test]
fn capacity_bound_and_recycled_storage_preserve_complete_order_without_growth() {
    let(mut arena,t)=fixture();let recording=arena.begin_recording().unwrap();let mut plan=Plan::new().unwrap();
    let pointers=(plan.commands.as_ptr(),plan.commands.capacity(),plan.args.as_ptr(),plan.args.capacity());
    let mut rows=Rows(vec![OwnedRow{backing:Backing::Arena(t[0]),width:IndexWidth::U16,ranges:vec![0..1;MAX_RECORDS]}]);
    {let ready=plan.build(Mode::ArenaMdi,limits(65_535),&recording,&rows).unwrap();
        assert_eq!(ready.summary().records,MAX_RECORDS);assert_eq!(ready.summary().multi_calls,1);assert_eq!(ready.summary().direct_calls,1);
        assert_eq!(ready.argument_bytes().len(),MAX_RECORDS*20);assert_eq!(expanded(&ready,&rows),original(&recording,&rows));}
    rows.0[0].ranges.push(0..1);
    assert!(matches!(plan.build(Mode::ArenaMdi,limits(65_535),&recording,&rows),Err(Error::TooManyRecords)));
    assert!(plan.commands.is_empty()&&plan.args.is_empty()&&plan.summary.bounded_fallback);
    rows.0[0].ranges.fill(0..0); // empty adversarial input is also CPU bounded
    assert!(matches!(plan.build(Mode::ArenaMdi,limits(4),&recording,&rows),Err(Error::TooManyRecords)));
    rows.0[0].ranges=vec![0..3,3..6];
    {let ready=plan.build(Mode::ArenaMdi,limits(2),&recording,&rows).unwrap();assert_eq!(expanded(&ready,&rows),original(&recording,&rows));}
    assert_eq!((plan.commands.as_ptr(),plan.commands.capacity(),plan.args.as_ptr(),plan.args.capacity()),pointers);
    recording.discarded();
}

#[test]
fn stale_generations_foreign_tokens_width_and_range_mismatches_fail_closed() {
    let(mut arena,t)=fixture();let old=t[0];let mut factory=Factory::default();
    let mut pending=arena.begin_update(Some(old),request(IndexWidth::U16),&mut factory).unwrap();let _=pending.target();let new=pending.commit();
    assert_ne!(new.identity(),old.identity());
    let(mut foreign,other)=fixture();let other_recording=foreign.begin_recording().unwrap();
    let recording=arena.begin_recording().unwrap();let mut plan=Plan::new().unwrap();
    let row=|token,width,range|Rows(vec![OwnedRow{backing:Backing::Arena(token),width,ranges:vec![range]}]);
    assert!(matches!(plan.build(Mode::ArenaMdi,limits(8),&recording,&row(old,IndexWidth::U16,0..6)),Err(Error::StaleArena{row:0})));
    assert!(matches!(plan.build(Mode::ArenaMdi,limits(8),&recording,&row(other[0],IndexWidth::U16,0..6)),Err(Error::StaleArena{row:0})));
    assert!(matches!(plan.build(Mode::ArenaMdi,limits(8),&recording,&row(new,IndexWidth::U32,0..6)),Err(Error::UnsafeArena{row:0})));
    assert!(matches!(plan.build(Mode::ArenaMdi,limits(8),&recording,&row(new,IndexWidth::U16,0..7)),Err(Error::InvalidRange{row:0})));
    assert!(plan.commands.is_empty()&&plan.args.is_empty());
    let rows=row(new,IndexWidth::U16,0..6);
    {let ready=plan.build(Mode::ArenaMdi,limits(8),&recording,&rows).unwrap();assert_eq!(expanded(&ready,&rows),original(&recording,&rows));}
    recording.discarded();other_recording.discarded();
}

#[test]
fn shared_full_page_addressing_requires_used_vertex_provenance_and_aligned_bounds() {
    let handle=0usize;let mut binding=Binding{arena_id:1,allocation_generation:1,vertex:&handle,index:&handle,
        vertex_page:0,index_page:1,vertex_range:128..256,index_range:16..32,request:request(IndexWidth::U16)};
    assert!(validate(&binding,IndexWidth::U16).is_ok());
    for max in [None,Some(4),Some(u32::MAX)]{binding.request.max_local_index=max;assert!(validate(&binding,IndexWidth::U16).is_err());}
    binding.request=request(IndexWidth::U16);binding.request.vertex_reserved=256;
    assert!(validate(&binding,IndexWidth::U16).is_err());binding.request=request(IndexWidth::U16);
    binding.vertex_range=129..257;assert!(validate(&binding,IndexWidth::U16).is_err());
    binding.vertex_range=128..256;binding.index_range=18..34;assert!(validate(&binding,IndexWidth::U16).is_err());
    binding.index_range=MAX_INDEX_PAGE..MAX_INDEX_PAGE+16;assert!(validate(&binding,IndexWidth::U16).is_err());
    binding.index_range=16..32;binding.vertex_range=MAX_VERTEX_PAGE..MAX_VERTEX_PAGE+128;
    assert!(validate(&binding,IndexWidth::U16).is_err());
}
