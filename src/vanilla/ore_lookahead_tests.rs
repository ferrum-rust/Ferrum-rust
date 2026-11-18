use super::*;
use std::{cell::{Cell, RefCell}, collections::BTreeMap};

fn original_minable_in(
    world: &mut impl FeatureWorld,
    random: &mut JavaRandom,
    p: IVec3,
    ore: u16,
    size: i32,
    replace_id: u16,
) -> bool {
    let angle = random.next_float() * std::f32::consts::PI;
    let a = [
        (p.x.wrapping_add(8) as f32 + math::sin(angle) * size as f32 / 8.) as f64,
        (p.y + random.next_int(3) - 2) as f64,
        (p.z.wrapping_add(8) as f32 + math::cos(angle) * size as f32 / 8.) as f64,
    ];
    let b = [
        (p.x.wrapping_add(8) as f32 - math::sin(angle) * size as f32 / 8.) as f64,
        (p.y + random.next_int(3) - 2) as f64,
        (p.z.wrapping_add(8) as f32 - math::cos(angle) * size as f32 / 8.) as f64,
    ];
    for step in 0..size {
        let fraction = step as f32 / size as f32;
        let center: [f64; 3] = std::array::from_fn(|i| a[i] + (b[i] - a[i]) * fraction as f64);
        let radius_random = random.next_double() * size as f64 / 16.;
        let radius =
            ((math::sin(std::f32::consts::PI * fraction) + 1.) as f64 * radius_random + 1.) / 2.;
        let low: [i32; 3] = center.map(|v| (v - radius).floor() as i32);
        let high: [i32; 3] = center.map(|v| (v + radius).floor() as i32);
        for x in low[0]..=high[0] {
            let dx = (x as f64 + 0.5 - center[0]) / radius;
            if dx * dx >= 1. { continue; }
            for y in low[1]..=high[1] {
                let dy = (y as f64 + 0.5 - center[1]) / radius;
                if dx * dx + dy * dy >= 1. { continue; }
                for z in low[2]..=high[2] {
                    let dz = (z as f64 + 0.5 - center[2]) / radius;
                    let p = IVec3::new(x, y, z);
                    if dx * dx + dy * dy + dz * dz < 1. && world.get(p) >> 4 == replace_id {
                        world.set(p, ore);
                    }
                }
            }
        }
    }
    true
}

// Deliberately independent default table and original imperative draw order.
fn original_default(world: &mut TraceWorld, random: &mut JavaRandom, origin: IVec3) {
    for (state, size, count, max) in [
        (3 << 4,33,10,256),(13 << 4,33,8,256),((1 << 4)|3,33,10,80),
        ((1 << 4)|1,33,10,80),((1 << 4)|5,33,10,80),(16 << 4,17,20,128),
        (15 << 4,9,20,64),(14 << 4,9,2,32),(73 << 4,8,8,16),(56 << 4,8,1,16),
    ] {
        for _ in 0..count {
            let p=origin+IVec3::new(random.next_int(16),random.next_int(max),random.next_int(16));
            world.ore.set(state); original_minable_in(world,random,p,state,size,1);
        }
    }
    let p=origin+IVec3::new(random.next_int(16),random.next_int(16)+random.next_int(16),random.next_int(16));
    world.ore.set(21 << 4); original_minable_in(world,random,p,21 << 4,7,1);
}
#[derive(Clone, Copy)]
enum Change { None, ReadReseed, WriteReseed, ReadRestore, FutureBlock }
#[derive(Debug,PartialEq)]
enum Event { Read([i32;3],u16,u64), Write([i32;3],u16,u64), Seed(u64), Edit([i32;3],u16) }
#[derive(Default)]
struct Trace { blocks:BTreeMap<[i32;3],u16>, events:Vec<Event>, batches:Vec<(u16,Vec<IVec3>,u64)> }
struct TraceWorld {
    state:RefCell<Trace>, random:RefCell<JavaRandom>, ore:Cell<u16>,
    change:Change, changed:Cell<bool>, owner:std::thread::ThreadId,
}
impl TraceWorld {
    fn new(random:JavaRandom,change:Change)->Self {
        Self {state:RefCell::new(Trace::default()),random:RefCell::new(random),ore:Cell::new(0),
            change,changed:Cell::new(false),owner:std::thread::current().id()}
    }
    fn callback(&self,p:IVec3,write:bool) {
        assert_eq!(std::thread::current().id(),self.owner,"all callbacks stay on owner");
        let trigger=match self.change { Change::None=>false, Change::WriteReseed=>write, _=>!write };
        // Diorite begins at vein18, after two prepared gravel veins16/17.
        if !trigger || self.ore.get()!=((1<<4)|3) || self.changed.replace(true) {return;}
        let mut random=self.random.borrow_mut(); let before=random.state();
        match self.change {
            Change::ReadReseed|Change::WriteReseed|Change::ReadRestore=> {
                let seed=random.next_long()^0x710189; random.set_seed(seed);
                self.state.borrow_mut().events.push(Event::Seed(random.state()));
                if matches!(self.change,Change::ReadRestore) {
                    // Restore exactly the shared Java state, without replacing
                    // the shared handle by an unrelated Owned snapshot.
                    random.set_seed((before^0x5deece66d) as i64);
                    self.state.borrow_mut().events.push(Event::Seed(random.state()));
                }
            }
            Change::FutureBlock=> {
                let q=(p+IVec3::Z).to_array();let mut state=self.state.borrow_mut();
                state.blocks.insert(q,0);state.events.push(Event::Edit(q,0));
            }
            Change::None=>unreachable!(),
        }
    }
}
impl FeatureWorld for TraceWorld {
    fn get(&self,p:IVec3)->u16 {
        self.callback(p,false);let rng=self.random.borrow().state();let mut state=self.state.borrow_mut();
        let value=state.blocks.get(&p.to_array()).copied().unwrap_or_else(|| {
            if !(0..256).contains(&p.y)||!(-30_000_000..30_000_000).contains(&p.x)||!(-30_000_000..30_000_000).contains(&p.z){0}
            else if (p.x+p.y+p.z).rem_euclid(13)==0{0}else{1<<4}
        });
        state.events.push(Event::Read(p.to_array(),value,rng));value
    }
    fn set(&mut self,p:IVec3,value:u16) {
        self.callback(p,true);let rng=self.random.borrow().state();let mut state=self.state.borrow_mut();
        state.blocks.insert(p.to_array(),value);state.events.push(Event::Write(p.to_array(),value,rng));
    }
    fn replace_ore_line(&mut self,positions:&[IVec3],ore:u16,replace_id:u16) {
        self.ore.set(ore);self.state.borrow_mut().batches.push((ore,positions.to_vec(),self.random.borrow().state()));
        for &p in positions {if self.get(p)>>4==replace_id{self.set(p,ore);}}
    }
    fn biome(&self,_:IVec3)->u8{1} fn light(&self,_:IVec3,_:bool)->u8{0}
    fn chest(&mut self,_:IVec3,_:Vec<Option<super::super::loot::LootStack>>){unreachable!()}
    fn spawner(&mut self,_:IVec3,_:&'static str){unreachable!()}
    fn entity(&mut self,_:super::super::GeneratedEntity){unreachable!()}
}
struct Immediate { corrupt_header:bool, corrupt_vein:bool, corrupt_sphere:bool, limit:usize }
impl Default for Immediate {fn default()->Self{Self{corrupt_header:false,corrupt_vein:false,corrupt_sphere:false,limit:COORDINATE_LIMIT}}}
impl Executor for Immediate {
    fn submit(&self,start:usize,random:JavaRandom,origin:IVec3,recycled:Option<Packet>)->mpsc::Receiver<Option<Packet>> {
        let mut result=prepare_bounded(start,random,origin,recycled,self.limit);
        if let Some(p)=&mut result {
            if self.corrupt_header {p.before^=1;}
            if self.corrupt_vein {p.veins[1].before^=1;}
            if self.corrupt_sphere {p.spheres[1].before^=1;}
        }
        let(tx,rx)=mpsc::sync_channel(1);tx.try_send(result).ok().unwrap();rx
    }
}
#[derive(Default)]
struct NotReady { retained:RefCell<Vec<mpsc::SyncSender<Option<Packet>>>> }
impl Executor for NotReady {
    fn submit(&self,_:usize,_:JavaRandom,_:IVec3,_:Option<Packet>)->mpsc::Receiver<Option<Packet>> {
        let(tx,rx)=mpsc::sync_channel(1);self.retained.borrow_mut().push(tx);rx
    }
}
fn compare(seed:i64,origin:IVec3,change:Change,executor:&impl Executor)->Stats {
    let mut expected_rng=JavaRandom::new(seed);let mut actual_rng=JavaRandom::new(seed);
    let mut expected=TraceWorld::new(expected_rng.share(),change);
    let mut actual=TraceWorld::new(actual_rng.share(),change);
    original_default(&mut expected,&mut expected_rng,origin);
    let constructors=crate::vanilla::random::independent_seed_calls_for_thread();
    let stats=run(&mut actual,&mut actual_rng,origin,executor);
    assert_eq!(constructors,crate::vanilla::random::independent_seed_calls_for_thread());
    assert_eq!(expected_rng.state(),actual_rng.state());
    assert_eq!(expected.changed.get(),actual.changed.get());
    let expected=expected.state.into_inner();let actual=actual.state.into_inner();
    assert_eq!(expected.blocks,actual.blocks);assert_eq!(expected.events.len(),actual.events.len());
    for(index,(a,b))in expected.events.iter().zip(&actual.events).enumerate(){assert_eq!(a,b,"access {index}");}
    assert_eq!(stats.prepared_spheres+stats.inline_spheres,2201);
    assert!(stats.maximum_packet_coordinates<=COORDINATE_LIMIT);stats
}
#[test]
fn ready_packets_match_original_every_access_shared_rng_and_reentrant_mutations() {
    for change in [Change::None,Change::ReadReseed,Change::WriteReseed,Change::ReadRestore,Change::FutureBlock] {
        let stats=compare(189,IVec3::new(-16,0,16),change,&Immediate::default());
        assert!(stats.prepared_spheres>0);
        if matches!(change,Change::ReadReseed|Change::WriteReseed){assert_eq!(stats.rng_rejections,1);}
        else{assert_eq!(stats.rng_rejections,0);assert_eq!(stats.ready,6);}
    }
    for(seed,origin)in[(0,IVec3::ZERO),(-1,IVec3::new(29_999_968,0,-29_999_984)),(8924706420471469774,IVec3::new(-128,0,-32))] {
        compare(seed,origin,Change::None,&Immediate::default());
    }
}
#[test]
fn not_ready_bad_state_and_bounded_packet_failure_keep_original_remaining_path() {
    let pending=NotReady::default();let stats=compare(189,IVec3::ZERO,Change::None,&pending);
    assert_eq!(stats.submitted,1);assert_eq!(stats.unavailable,1);assert_eq!(stats.prepared_spheres,0);
    assert_eq!(pending.retained.borrow().len(),1,"no queue growth when the pool is busy");
    let stats=compare(189,IVec3::ZERO,Change::None,&Immediate{corrupt_header:true,..Default::default()});
    assert_eq!(stats.rng_rejections,1);assert_eq!(stats.prepared_spheres,0);
    let stats=compare(189,IVec3::ZERO,Change::None,&Immediate{corrupt_vein:true,..Default::default()});
    assert_eq!(stats.rng_rejections,1);assert_eq!(stats.prepared_spheres,33,"next vein header resumes without replay");
    let stats=compare(189,IVec3::ZERO,Change::None,&Immediate{corrupt_sphere:true,..Default::default()});
    assert_eq!(stats.rng_rejections,1);assert_eq!(stats.prepared_spheres,1,"continue after already committed sphere");
    let stats=compare(189,IVec3::ZERO,Change::None,&Immediate{limit:0,..Default::default()});
    assert_eq!(stats.unavailable,1);assert_eq!(stats.prepared_spheres,0);
}
#[test]
fn packet_recycling_keeps_two_flat_buffers_bounded_and_prediction_owned() {
    let mut rng=JavaRandom::new(189);let shared=rng.share();let before=shared.state();
    let next=predict_after(0,&rng,IVec3::ZERO);assert_eq!(shared.state(),before);
    let first=prepare(16,next,IVec3::ZERO,None).unwrap();
    let pointers=(first.positions.as_ptr(),first.spheres.as_ptr(),first.veins.as_ptr());
    let next=predict_after(0,&rng,IVec3::ZERO);
    let second=prepare(16,next,IVec3::ZERO,Some(first)).unwrap();
    assert_eq!(pointers,(second.positions.as_ptr(),second.spheres.as_ptr(),second.veins.as_ptr()));
    assert!(second.positions.capacity() >= second.positions.len());
    assert!(second.positions.capacity() <= COORDINATE_LIMIT);
    assert_eq!(second.veins.len(),PACKET_VEINS);assert!(second.spheres.len()<=PACKET_VEINS*33);
    assert_eq!(shared.state(),before);
}

#[test]
fn sphere_and_sixty_four_candidate_flushes_match_established_batch_adapter() {
    let mut a_rng=JavaRandom::new(189);let mut b_rng=JavaRandom::new(189);
    let mut a=TraceWorld::new(a_rng.share(),Change::ReadReseed);
    let mut b=TraceWorld::new(b_rng.share(),Change::ReadReseed);
    super::super::default_ores(&mut a,&mut a_rng,IVec3::new(-16,0,16));
    let stats=run(&mut b,&mut b_rng,IVec3::new(-16,0,16),&Immediate::default());
    let a=a.state.into_inner();let b=b.state.into_inner();
    assert_eq!(a.batches,b.batches);assert_eq!(a.events,b.events);assert_eq!(a_rng.state(),b_rng.state());
    assert!(a.batches.iter().all(|(_,p,_)|!p.is_empty()&&p.len()<=64));
    assert!(a.batches.iter().any(|(_,p,_)|p.len()==64));assert!(stats.prepared_spheres>0);
}
#[test]
fn cancelled_queued_task_keeps_single_lease_until_completion_and_unwinds_cleanly() {
    use std::time::Duration;
    static ISOLATED:AtomicBool=AtomicBool::new(false);
    let pool=rayon::ThreadPoolBuilder::new().num_threads(1).build().unwrap();
    let(start_tx,start_rx)=mpsc::sync_channel(1);let(release_tx,release_rx)=mpsc::sync_channel(1);
    pool.spawn_fifo(move||{let _=start_tx.send(());let _=release_rx.recv_timeout(Duration::from_secs(5));});
    start_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let executor=Pool{pool:&pool,lease:acquire_from(&ISOLATED).unwrap()};
    assert!(acquire_from(&ISOLATED).is_none(),"nested producer cannot acquire a second lease");
    let ticket=executor.submit(16,JavaRandom::new(189),IVec3::ZERO,None);
    drop(ticket);drop(executor);
    assert!(acquire_from(&ISOLATED).is_none(),"queued pure work retains the cancellation bound");
    release_tx.send(()).unwrap();
    let(done_tx,done_rx)=mpsc::sync_channel(1);pool.spawn_fifo(move||{let _=done_tx.send(());});
    done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let lease=acquire_from(&ISOLATED).expect("completed cancelled work releases lease");drop(lease);
    let panic=catch_unwind(||{let _lease=acquire_from(&ISOLATED).unwrap();panic!("owner fixture unwind");});
    assert!(panic.is_err());assert!(acquire_from(&ISOLATED).is_some());
}

#[test]
fn grown_packet_matches_normal_geometry_rng_and_recycles_within_hard_cap() {
    fn same_packet(a: &Packet, b: &Packet) {
        assert_eq!((a.start, a.before), (b.start, b.before));
        assert_eq!(a.positions, b.positions);
        assert_eq!(a.spheres.iter().map(|s| (s.before, s.positions.clone())).collect::<Vec<_>>(),
            b.spheres.iter().map(|s| (s.before, s.positions.clone())).collect::<Vec<_>>());
        assert_eq!(a.veins.iter().map(|v| (v.before, v.spheres.clone())).collect::<Vec<_>>(),
            b.veins.iter().map(|v| (v.before, v.spheres.clone())).collect::<Vec<_>>());
    }
    let origin = IVec3::new(-16, 0, 16);
    let initial = predict_after(0, &JavaRandom::new(189), origin);
    let expected_after = predict_after(16, &initial, origin).state();
    let mut normal_rng = initial.clone();
    let normal_observer = normal_rng.share();
    let normal = prepare(16, normal_rng, origin, None).unwrap();
    assert_eq!(normal_observer.state(), expected_after);

    let mut small = Packet {
        start: usize::MAX, before: u64::MAX, positions: Vec::with_capacity(1),
        spheres: Vec::with_capacity(PACKET_VEINS * 33), veins: Vec::with_capacity(PACKET_VEINS),
    };
    // These stale entries must be cleared by ordinary recycled preparation.
    small.positions.push(IVec3::splat(i32::MIN));
    small.spheres.push(PreparedSphere { before: 0, positions: 0..1 });
    small.veins.push(PreparedVein { before: 0, spheres: 0..1 });
    let initial_capacity = small.positions.capacity();
    let mut growth_rng = initial.clone();
    let growth_observer = growth_rng.share();
    let grown = prepare(16, growth_rng, origin, Some(small)).unwrap();
    assert!(grown.positions.len() > initial_capacity, "fixture must actually grow the allocation");
    assert!(grown.positions.capacity() > initial_capacity);
    assert!(grown.positions.capacity() >= grown.positions.len());
    assert!(grown.positions.capacity() <= COORDINATE_LIMIT);
    same_packet(&normal, &grown);
    assert_eq!(growth_observer.state(), expected_after);

    let capacity = grown.positions.capacity();
    let pointers = (grown.positions.as_ptr(), grown.spheres.as_ptr(), grown.veins.as_ptr());
    let mut recycled_rng = initial.clone();
    let recycled_observer = recycled_rng.share();
    let recycled = prepare(16, recycled_rng, origin, Some(grown)).unwrap();
    same_packet(&normal, &recycled);
    assert_eq!(recycled_observer.state(), expected_after);
    assert_eq!(pointers, (recycled.positions.as_ptr(), recycled.spheres.as_ptr(), recycled.veins.as_ptr()));
    assert_eq!(recycled.positions.capacity(), capacity);
    assert!(recycled.positions.capacity() <= COORDINATE_LIMIT);
}
