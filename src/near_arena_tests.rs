//! Independent byte occupancy and public transactional API oracles.
use super::*;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct FakeHandle {id:usize,kind:PageKind,bytes:u64}
#[derive(Default)]
struct Factory {calls:usize,fail_on:Option<usize>,panic_on:Option<usize>}
impl PageFactory for Factory {
    type Handle=FakeHandle;type Error=&'static str;
    fn create(&mut self,kind:PageKind,bytes:u64)->Result<FakeHandle,Self::Error>{
        self.calls+=1;if self.panic_on==Some(self.calls){panic!("injected factory unwind");}
        if self.fail_on==Some(self.calls){return Err("injected page creation failure");}
        Ok(FakeHandle{id:self.calls,kind,bytes})
    }
}
fn limits()->Limits {Limits{vertex_page:256,index_page:64,owners:16}}
fn request(v:u64,i:u64,width:IndexWidth)->Request {Request{vertex_bytes:v,index_bytes:i,
    vertex_reserved:v.max(32),index_reserved:i.max(4),vertices:(v/32)as u32,
    indices:(i/width.bytes())as u32,width,max_local_index:if i==0{None}else{Some((v/32).saturating_sub(1) as u32)}}}
fn admitted(arena:&mut Arena<FakeHandle>,factory:&mut Factory,old:Option<Token>,r:Request)->Token {
    arena.begin_update(old,r,factory).unwrap().commit()
}
fn summary(arena:&Arena<FakeHandle>,token:Token)->(FakeHandle,FakeHandle,Range<u64>,Range<u64>,Request) {
    let b=arena.binding(token).unwrap();(*b.vertex,*b.index,b.vertex_range,b.index_range,b.request)
}
#[test]
fn page_first_fit_and_coalescing_match_independent_byte_occupancy() {
    const N:usize=127;
    let mut page=Page{kind:PageKind::Vertex,bytes:N as u64,handle:(),holes:vec![Hole{offset:0,len:N as u64}],hole_limit:N+3};
    page.holes.reserve_exact(N+3-page.holes.len());
    let capacity=page.holes.capacity();let mut cells=[false;N];let mut live:Vec<Span>=Vec::new();let mut random=0x4d595df4d0f33173u64;
    for _ in 0..5000 {
        random=random.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        if !live.is_empty()&&random&3==0 {
            let at=(random as usize>>8)%live.len();let a=live.swap_remove(at);
            assert!(cells[a.offset as usize..(a.offset+a.len)as usize].iter().all(|b|*b));
            assert!(page.release(a));cells[a.offset as usize..(a.offset+a.len)as usize].fill(false);
        }else{
            let len=((random>>16)%19+1)as usize;
            let expected=(0..=N-len).find(|&start|cells[start..start+len].iter().all(|b|!*b));
            let actual=page.take(len as u64);assert_eq!(actual.map(|a|a.offset as usize),expected);
            if let Some(a)=actual {cells[a.offset as usize..(a.offset+a.len)as usize].fill(true);live.push(a);}
        }
        // Reconstruct free bytes without relying on allocator interval splitting.
        let mut free=[false;N];let mut previous=0;
        for h in &page.holes {assert!(h.offset>=previous&&h.len>0);let end=h.offset+h.len;
            assert!(end<=N as u64);for b in &mut free[h.offset as usize..end as usize]{assert!(!*b);*b=true;}previous=end;}
        assert!(free.iter().zip(cells).all(|(free,used)|*free!=used));
        assert_eq!(page.holes.capacity(),capacity,"metadata must not grow during allocation/release");
    }
    for a in live {assert!(page.release(a));}
    assert_eq!(page.holes,vec![Hole{offset:0,len:N as u64}]);
    let before=page.holes.clone();
    for invalid in [Span{page:0,offset:0,len:0},Span{page:0,offset:u64::MAX,len:2},Span{page:0,offset:126,len:2},Span{page:0,offset:3,len:2}] {
        assert!(!page.release(invalid));assert_eq!(page.holes,before,"rejected release is atomic");
    }
}
#[test]
fn failed_factory_and_abandoned_reservation_preserve_existing_owner_and_page_bounds() {
    let mut arena=Arena::new(Mode::ArenaDirect,limits()).unwrap();let mut factory=Factory::default();
    let old=admitted(&mut arena,&mut factory,None,request(32,4,IndexWidth::U16));
    let before=summary(&arena,old);let prior=arena.stats();
    // Vertex replacement first fits a different reserved range; U32 needs a new
    // index page, whose injected error must return that range without touching old.
    factory.fail_on=Some(factory.calls+1);
    assert!(matches!(arena.begin_update(Some(old),request(64,8,IndexWidth::U32),&mut factory),Err(Failure::Page(_))));
    assert_eq!(summary(&arena,old),before);assert_eq!(arena.stats(),prior);
    factory.fail_on=None;
    factory.panic_on=Some(factory.calls+1);
    let caught=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||{
        let _=arena.begin_update(Some(old),request(64,8,IndexWidth::U32),&mut factory);
    }));assert!(caught.is_err());
    assert_eq!(summary(&arena,old),before);assert_eq!(arena.stats(),prior,
        "second-factory unwind must return the newly reserved vertex interval too");
    factory.panic_on=None;
    {
        let pending=arena.begin_update(Some(old),request(64,8,IndexWidth::U32),&mut factory).unwrap();
        assert_eq!(pending.previous().unwrap().request,before.4);
        // No target() call: no GPU writes began, so Drop rolls back CPU ranges.
        drop(pending);
    }
    assert_eq!(summary(&arena,old),before);
    let after=arena.stats();assert_eq!(after.owners,1);assert_eq!(after.reserved_bytes,prior.reserved_bytes);
    assert_eq!(after.index_pages,2,"successfully created persistent empty page is retained");
    assert_eq!(after.committed_bytes,after.reserved_bytes+after.free_bytes);
    // A failed second page in an entirely new reservation retains the first
    // successfully created page as completely free, without a phantom owner.
    let mut empty=Arena::new(Mode::ArenaMdi,limits()).unwrap();let mut factory=Factory{calls:0,fail_on:Some(2),panic_on:None};
    assert!(matches!(empty.begin_update(None,request(32,4,IndexWidth::U16),&mut factory),Err(Failure::Page(_))));
    let s=empty.stats();assert_eq!((s.owners,s.vertex_pages,s.index_pages,s.free_bytes,s.reserved_bytes),(0,1,0,256,0));
    factory.fail_on=None;let token=admitted(&mut empty,&mut factory,None,request(32,4,IndexWidth::U16));
    assert_eq!(empty.binding(token).unwrap().vertex_range,0..32);assert_eq!(empty.stats().vertex_pages,1);
    // The same partial-creation failure during replacement must retain BOTH old
    // live owners, even though the new empty vertex page increases commitment.
    let mut arena=Arena::new(Mode::ArenaDirect,limits()).unwrap();let mut factory=Factory::default();
    let old=admitted(&mut arena,&mut factory,None,request(32,4,IndexWidth::U16));
    let neighbor=admitted(&mut arena,&mut factory,None,request(224,4,IndexWidth::U16));
    let old_before=summary(&arena,old);let neighbor_before=summary(&arena,neighbor);let before=arena.stats();
    factory.fail_on=Some(factory.calls+2); // new vertex succeeds, new U32 index fails
    assert!(matches!(arena.begin_update(Some(old),request(64,8,IndexWidth::U32),&mut factory),Err(Failure::Page(_))));
    assert_eq!(summary(&arena,old),old_before);assert_eq!(summary(&arena,neighbor),neighbor_before);
    let after=arena.stats();assert_eq!(after.owners,before.owners);assert_eq!(after.reserved_bytes,before.reserved_bytes);
    assert_eq!(after.used_bytes,before.used_bytes);assert_eq!((after.vertex_pages,after.index_pages),(2,1));
    assert_eq!(after.committed_bytes,before.committed_bytes+256);
    assert_eq!(after.free_bytes,before.free_bytes+256);
}
#[test]
fn generations_reuse_and_width_change_never_revalidate_old_or_foreign_tokens() {
    let mut arena=Arena::new(Mode::ArenaDirect,limits()).unwrap();let mut factory=Factory::default();
    let a=admitted(&mut arena,&mut factory,None,request(96,24,IndexWidth::U16));let original=summary(&arena,a);
    let b=admitted(&mut arena,&mut factory,Some(a),request(32,4,IndexWidth::U16));
    let next=summary(&arena,b);assert_eq!((&next.0,&next.1,&next.2,&next.3),(&original.0,&original.1,&original.2,&original.3));
    assert_eq!(factory.calls,2);assert_eq!(arena.binding(a).err(),Some(Reject::StaleToken));
    assert_ne!(a.identity(),b.identity());
    let c=admitted(&mut arena,&mut factory,Some(b),request(64,8,IndexWidth::U32));
    assert_eq!(summary(&arena,c).2,original.2);assert_eq!(arena.stats().index_pages,2);
    assert_ne!(summary(&arena,c).1,original.1);assert_eq!(arena.release(b),Err(Reject::StaleToken));
    let mut other=Arena::new(Mode::ArenaDirect,limits()).unwrap();
    assert_eq!(other.binding(c).err(),Some(Reject::StaleToken));
    let d=admitted(&mut other,&mut factory,None,request(32,4,IndexWidth::U16));assert_ne!(c.identity()[0],d.identity()[0]);
    arena.release(c).unwrap();assert_eq!(arena.release(c),Err(Reject::StaleToken));
    let e=admitted(&mut arena,&mut factory,None,request(32,4,IndexWidth::U16));
    assert_eq!(e.identity()[1],a.identity()[1]);assert_ne!(e.identity()[2],a.identity()[2]);
    assert_eq!(arena.binding(a).err(),Some(Reject::StaleToken));
    arena.next_generation=u64::MAX;
    assert!(matches!(arena.begin_update(Some(e),request(32,4,IndexWidth::U16),&mut factory),Err(Failure::Admission(Reject::GenerationExhausted))));
    assert!(arena.binding(e).is_ok());
}
#[test]
fn metadata_page_budgets_and_offsets_are_exact_without_cross_width_aliasing() {
    let mut arena=Arena::new(Mode::ArenaMdi,Limits{owners:3,..limits()}).unwrap();let mut factory=Factory::default();
    let a=admitted(&mut arena,&mut factory,None,request(32,4,IndexWidth::U16));
    let b=admitted(&mut arena,&mut factory,None,request(64,8,IndexWidth::U16));
    let view=arena.binding(b).unwrap();assert_eq!(view.vertex_range,32..96);assert_eq!(view.index_range,4..12);
    assert_eq!(view.draw(1..4).unwrap(),Draw{index_count:3,first_index:3,base_vertex:1});
    assert_eq!(view.draw(4..4).unwrap().index_count,0);assert!(view.draw(2..5).is_err());assert!(view.draw(3..2).is_err());
    let c=admitted(&mut arena,&mut factory,None,request(32,4,IndexWidth::U32));
    assert_ne!(arena.binding(c).unwrap().index_page,arena.binding(a).unwrap().index_page);
    let calls=factory.calls;
    assert!(matches!(arena.begin_update(None,request(32,4,IndexWidth::U16),&mut factory),Err(Failure::Admission(Reject::OwnerLimit))));
    assert_eq!(factory.calls,calls);arena.release(a).unwrap();arena.release(b).unwrap();arena.release(c).unwrap();
    let s=arena.stats();assert_eq!((s.vertex_pages,s.index_pages),(1,2));assert_eq!(s.free_bytes,s.committed_bytes);
    let mut full=Arena::new(Mode::ArenaDirect,limits()).unwrap();
    let x=admitted(&mut full,&mut factory,None,request(256,64,IndexWidth::U16));
    let y=admitted(&mut full,&mut factory,None,request(256,64,IndexWidth::U16));let before=full.stats();
    assert!(matches!(full.begin_update(None,request(32,4,IndexWidth::U16),&mut factory),Err(Failure::Admission(Reject::PageLimit))));
    assert_eq!(full.stats(),before);assert!(full.binding(x).is_ok()&&full.binding(y).is_ok());
    full.release(x).unwrap();
    // Empty persistent U16 pages are not silently retyped; total index budget is
    // shared across widths, so a U32 request fails closed with old owners intact.
    assert!(matches!(full.begin_update(None,request(32,4,IndexWidth::U32),&mut factory),Err(Failure::Admission(Reject::PageLimit))));
    assert!(full.binding(y).is_ok());assert_eq!(full.stats().owners,1);
}
#[test]
fn write_start_and_unfinished_recording_poison_instead_of_fake_gpu_rollback() {
    let mut factory=Factory::default();
    let mut arena=Arena::new(Mode::ArenaDirect,limits()).unwrap();let a=admitted(&mut arena,&mut factory,None,request(32,4,IndexWidth::U16));
    {
        let guard=arena.begin_recording().unwrap();assert!(guard.binding(a).is_ok());guard.submitted();
    }
    {arena.begin_recording().unwrap().discarded();}
    assert!(arena.binding(a).is_ok());
    let caught=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||{
        let _recording=arena.begin_recording().unwrap();panic!("encoder left unacknowledged");
    }));assert!(caught.is_err());
    assert_eq!(arena.binding(a).err(),Some(Reject::Poisoned));
    assert!(matches!(arena.begin_update(Some(a),request(32,4,IndexWidth::U16),&mut factory),Err(Failure::Admission(Reject::Poisoned))));
    assert_eq!(arena.release(a),Err(Reject::Poisoned));
    let mut arena=Arena::new(Mode::ArenaMdi,limits()).unwrap();let a=admitted(&mut arena,&mut factory,None,request(32,4,IndexWidth::U16));
    {
        let mut pending=arena.begin_update(Some(a),request(32,4,IndexWidth::U16),&mut factory).unwrap();
        assert_eq!(pending.target().vertex_range,0..32);
        // Even if a test merely borrows this target, API conservatively assumes
        // writes may already have overwritten the old reservation.
    }
    assert_eq!(arena.binding(a).err(),Some(Reject::Poisoned));
    let mut arena=Arena::new(Mode::ArenaMdi,limits()).unwrap();
    let mut pending=arena.begin_update(None,request(32,4,IndexWidth::U16),&mut factory).unwrap();
    let _=pending.target();let a=pending.commit();assert!(arena.binding(a).is_ok());
}
#[test]
fn malformed_requests_and_dedicated_mode_do_not_call_factory_or_mutate_owners() {
    let mut factory=Factory::default();let mut arena=Arena::new(Mode::ArenaDirect,limits()).unwrap();
    let good=request(32,4,IndexWidth::U16);let mut bad=Vec::new();
    for (v,i) in [(0,4),(31,4),(288,4),(32,0),(32,3),(32,68)] {let mut r=good;r.vertex_reserved=v;r.index_reserved=i;bad.push(r);}
    let mut r=good;r.vertices=u32::MAX;bad.push(r);let mut r=good;r.indices=u32::MAX;bad.push(r);
    let mut r=good;r.index_bytes=u64::MAX;bad.push(r);let mut r=good;r.vertex_bytes=u64::MAX;bad.push(r);
    let mut r=good;r.max_local_index=None;bad.push(r);
    let mut r=good;r.max_local_index=Some(1);bad.push(r); // reaches adjacent owner despite reservation padding
    let mut r=good;r.indices=0;r.index_bytes=0;bad.push(r); // nonempty maximum for no indices
    let mut r=good;r.vertices=0;r.vertex_bytes=0;r.max_local_index=Some(0);bad.push(r);
    for r in bad {assert!(matches!(arena.begin_update(None,r,&mut factory),Err(Failure::Admission(Reject::InvalidRequest))));}
    assert_eq!(factory.calls,0);assert_eq!(arena.stats().owners,0);
    let mut dedicated=Arena::new(Mode::Dedicated,limits()).unwrap();
    assert!(matches!(dedicated.begin_update(None,good,&mut factory),Err(Failure::Admission(Reject::Dedicated))));
    assert_eq!(factory.calls,0);assert_eq!(Mode::default(),Mode::Dedicated);
    for l in [Limits{owners:0,..limits()},Limits{owners:4097,..limits()},Limits{vertex_page:255,..limits()},Limits{index_page:65,..limits()},Limits{vertex_page:512*MIB,..limits()}] {
        assert!(matches!(Arena::<FakeHandle>::new(Mode::ArenaDirect,l),Err(Reject::InvalidLimits)));
    }
    let empty=request(0,0,IndexWidth::U16);
    let token=admitted(&mut arena,&mut factory,None,empty);
    assert_eq!(arena.binding(token).unwrap().draw(0..0).unwrap().index_count,0);
    let odd=Request{indices:1,..good};
    let token=admitted(&mut arena,&mut factory,Some(token),odd);
    assert_eq!(arena.binding(token).unwrap().request.index_bytes,4,"odd U16 count retains exact padding");
    assert_eq!(arena.binding(token).unwrap().draw(0..1).unwrap().index_count,1);
    let large=Request{vertices:65_537,vertex_bytes:65_537*32,vertex_reserved:65_537*32,
        indices:2,index_bytes:4,index_reserved:4,width:IndexWidth::U16,max_local_index:Some(65_536)};
    assert!(!large.valid(Limits::default()),"U16 proof must be representable even when vertex reach fits");
    assert!(Request{width:IndexWidth::U32,indices:1,..large}.valid(Limits::default()));
}
