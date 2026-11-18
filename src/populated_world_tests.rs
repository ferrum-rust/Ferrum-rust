use super::*;
use crate::{gameplay::Mode, inventory::Stack};
use std::{fs, path::PathBuf, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};

struct Temporary(PathBuf);
impl Temporary {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("ferrum-autosave-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos())))
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let (Ok(root),Ok(path))=(fs::canonicalize(std::env::temp_dir()),fs::canonicalize(&self.0)) else {return;};
        if path.parent()==Some(root.as_path())
            && path.file_name().is_some_and(|name|name.to_string_lossy().starts_with("ferrum-autosave-")) {
            let _=fs::remove_dir_all(&path);
        }
    }
}
fn blank_world() -> World {
    World {
        chunks: Default::default(), spawn: Vec3::ZERO, label: "Autosave fixture".into(),
        save_path: Default::default(), edits: Default::default(), imported: true, light_revision: 0,
    }
}

#[test]
fn initial_view_batches_preserve_original_order_and_lookahead() {
    let plan=region((-7,3),5);
    let commands=initial_view_commands(&plan);
    let mut actual=Vec::new();
    for command in commands {
        let Command::GenerateAhead { positions,ahead }=command else {panic!("ordered generation");};
        assert!(!positions.is_empty() && positions.len()<=32);
        actual.extend(positions);
        assert_eq!(ahead,plan[actual.len()..(actual.len()+64).min(plan.len())]);
    }
    assert_eq!(actual,plan);
    assert!(initial_view_commands(&[]).is_empty());
}

#[test]
fn progressive_entry_requires_every_lit_neighbor_column() {
    let mut world=blank_world();
    let center=(-2,3);
    let mut chunk=crate::world::Chunk::empty();
    chunk.light=Some(Arc::new(crate::vanilla::lighting::ChunkLight {
        values:vec![0xf0;65536],height:vec![0;256],sections:[false;16],no_sky:false,
    }));
    let chunk=Arc::new(chunk);
    for x in -1..=1 {for z in -1..=1 {world.chunks.insert((center.0+x,center.1+z),chunk.clone());}}
    assert!(entry_neighborhood_ready(&world,center));
    world.chunks.remove(&(-1,4));
    assert!(!entry_neighborhood_ready(&world,center));
    world.chunks.insert((-1,4),Arc::new(crate::world::Chunk::empty()));
    assert!(!entry_neighborhood_ready(&world,center));
}

#[derive(Deserialize)]
struct RecordedCheckpoint {
    runtime:crate::vanilla::runtime::RuntimeSnapshot,
    gameplay:GameplaySave,
}

fn native_signature(path:&std::path::Path)->(serde_json::Value,GameplaySave) {
    use sha1::{Digest,Sha1};
    let file:RecordedCheckpoint=serde_json::from_reader(flate2::read::GzDecoder::new(
        std::io::BufReader::new(fs::File::open(path).unwrap()))).unwrap();
    let runtime=file.runtime;
    let mut terrain=Sha1::new();
    let mut light=Sha1::new();
    for chunk in &runtime.chunks {
        terrain.update(chunk.position.0.to_le_bytes());terrain.update(chunk.position.1.to_le_bytes());
        terrain.update(bytemuck::cast_slice(&chunk.blocks));terrain.update(&chunk.biomes);
        terrain.update(chunk.top.to_le_bytes());
        let tiles=chunk.tiles.iter().map(|(p,nbt)|(*p,nbt.as_ref())).collect::<Vec<_>>();
        terrain.update(serde_json::to_vec(&(&chunk.containers,&chunk.entities,&chunk.pistons,tiles)).unwrap());
        let column=&runtime.light.chunks[&chunk.position];
        light.update(&column.sky);light.update(&column.block);
        light.update(serde_json::to_vec(&(&column.height,column.minimum,column.sections,&column.gaps,column.queued_checks)).unwrap());
    }
    let mut tiles=runtime.tile_slots;
    tiles.sort_unstable_by_key(|tile|tile.position);
    let mut spawners=runtime.live_spawners;
    spawners.sort_unstable_by_key(|(p,_)|*p);
    (serde_json::json!({"terrain":format!("{:x}",terrain.finalize()),
        "light":format!("{:x}",light.finalize()),"provider":runtime.provider,
        "provider_random":runtime.provider_random,"world_random":runtime.world_random,
        "spawn":runtime.world_spawn,"initial_player":runtime.initial_player,
        "animals":runtime.animals,"containers":tiles,"spawners":spawners}),file.gameplay)
}

fn copied_config(source:&Config,root:PathBuf)->Config {
    let mut target=source.clone();target.save_root=root;
    let path=target.checkpoint_path().unwrap();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::copy(source.checkpoint_path().unwrap(),&path).unwrap();
    target
}

fn open_recorded(config:Config,wanted:&[ChunkPos])->(PopulatedWorld,World,PlayerSave,Simulation) {
    let mut session=NativeSession::spawn(config).unwrap();
    let Event::Ready { gameplay,update,.. }=session.receive().unwrap() else {panic!("ready");};
    let saved:GameplaySave=serde_json::from_value(gameplay).unwrap();
    let mut host=PopulatedWorld::new(session,true);
    host.wanted.extend(wanted.iter().copied());
    host.seen_spawns=saved.seen_spawns;host.seen_containers=saved.seen_containers;
    host.pending_spawns.extend(saved.pending_spawns.into_iter().map(|s|((s.family,s.index),s)));
    host.pending_containers.extend(saved.pending_containers.into_iter().map(|c|(c.position,c)));
    host.initial_view_plan=saved.initial_view_plan;
    let mut world=blank_world();world.edits.extend(saved.edits);
    host.apply(&mut world,update);
    (host,world,PlayerSave {position:saved.position,motion:saved.motion},saved.simulation)
}

#[test]
fn progressive_initial_view_preserves_eager_state_and_autosaved_request_order() {
    let temp=Temporary::new();
    let mut config=Config::new(189,"view-plan-test",temp.0.join("base"));
    config.map_features=false;
    let mut session=NativeSession::spawn(config.clone()).unwrap();
    let Event::Ready { update,.. }=session.receive().unwrap() else {panic!("ready");};
    let mut host=PopulatedWorld::new(session,true);
    let mut world=blank_world();host.apply(&mut world,update);
    host.wanted.insert((0,0));
    let (_,update)=host.exchange(Command::Generate(vec![(0,0)])).unwrap();
    host.apply(&mut world,update);
    let mut sim=Simulation::new(Mode::Creative,189);sim.mob_spawning=false;
    let pose=PlayerSave {position:[1.5,200.,1.5],motion:None};
    let edited=IVec3::new(7,200,7);
    world.set(edited,57<<4);
    host.save(&mut world,pose,&mut sim).unwrap();drop(host);
    let wanted=region((0,0),4);
    let eager_config=copied_config(&config,temp.0.join("eager"));
    let progressive_config=copied_config(&config,temp.0.join("progressive"));
    let (mut eager,mut ew,ep,mut es)=open_recorded(eager_config.clone(),&wanted);
    let plan=eager.missing_generation_requests(&ew,&wanted);
    assert!(plan.len()>64);
    eager.generate_initial_view_loading(&mut ew,&plan).unwrap();
    eager.poll(&mut ew,&mut es).unwrap();
    eager.save(&mut ew,ep,&mut es).unwrap();drop(eager);
    let expected=native_signature(&eager_config.checkpoint_path().unwrap()).0;

    let (mut progressive,mut pw,pp,mut ps)=open_recorded(progressive_config.clone(),&wanted);
    assert_eq!(progressive.missing_generation_requests(&pw,&wanted),plan);
    progressive.begin_initial_view(plan.clone()).unwrap();
    assert!(progressive.initial_view_pending() && progressive.busy());
    assert!(progressive.autosave(&mut pw,pp,&ps).unwrap());
    let deadline=Instant::now()+Duration::from_secs(60);
    while progressive.saving {
        progressive.poll(&mut pw,&mut ps).unwrap();
        assert!(Instant::now()<deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let resumed_config=copied_config(&progressive_config,temp.0.join("resumed"));
    let (_,captured)=native_signature(&resumed_config.checkpoint_path().unwrap());
    assert_eq!(captured.initial_view_plan,plan);
    assert!(captured.edits.contains(&(edited.to_array(),57<<4)));
    // Old playable checkpoints deserialize without a plan and keep eager load.
    let mut legacy=serde_json::to_value(captured).unwrap();legacy.as_object_mut().unwrap().remove("initial_view_plan");
    assert!(serde_json::from_value::<GameplaySave>(legacy).unwrap().initial_view_plan.is_empty());
    progressive.drain(&mut pw,&mut ps).unwrap();
    progressive.save(&mut pw,pp,&mut ps).unwrap();
    assert!(!progressive.initial_view_pending());
    assert_eq!(pw.get(edited),57<<4);
    let ids=ps.mobs.iter().map(|mob|mob.id).collect::<HashSet<_>>();
    assert_eq!(ids.len(),ps.mobs.len(),"repeated publication cannot duplicate native mobs");
    drop(progressive);
    assert_eq!(native_signature(&progressive_config.checkpoint_path().unwrap()).0,expected);

    // A different current view must not reorder the unfinished original plan.
    let smaller=region((0,0),1);
    let (mut resumed,mut rw,rp,mut rs)=open_recorded(resumed_config.clone(),&smaller);
    assert_eq!(resumed.initial_view_plan,plan);
    resumed.restore_initial_view(&mut rw).unwrap();
    assert!(resumed.initial_view_plan.is_empty());
    resumed.poll(&mut rw,&mut rs).unwrap();
    resumed.save(&mut rw,rp,&mut rs).unwrap();drop(resumed);
    assert_eq!(native_signature(&resumed_config.checkpoint_path().unwrap()).0,expected,
        "saved-prefix resume must preserve blocks/light, provider order, RNG and generated metadata");
}

#[test]
fn manual_save_keeps_initial_view_queued_and_resumes_the_saved_prefix_exactly() {
    let temp=Temporary::new();
    let mut config=Config::new(189,"manual-prefix-test",temp.0.join("live"));
    config.map_features=false;
    let mut session=NativeSession::spawn(config.clone()).unwrap();
    let Event::Ready { update,.. }=session.receive().unwrap() else {panic!("ready");};
    let mut host=PopulatedWorld::new(session,true);
    let wanted=region((0,0),4);
    host.wanted.extend(wanted.iter().copied());
    let mut world=blank_world();host.apply(&mut world,update);
    let (_,update)=host.exchange(Command::Generate(vec![(0,0)])).unwrap();
    host.apply(&mut world,update);
    let mut sim=Simulation::new(Mode::Creative,189);sim.mob_spawning=false;
    let first_pose=PlayerSave {position:[1.5,201.,1.5],motion:None};
    let edited=IVec3::new(7,200,7);
    world.set(edited,41<<4);
    host.save(&mut world,first_pose,&mut sim).unwrap();

    let plan=host.missing_generation_requests(&world,&wanted);
    assert!(plan.len()>2*GENERATE_BATCH);
    host.begin_initial_view(plan.clone()).unwrap();
    // Keep an older autosave barrier in front of the remaining generation.
    // Manual saving must honor it, then persist the newer gameplay and edit.
    world.set(edited,57<<4);sim.health=12.;
    assert!(host.autosave(&mut world,first_pose,&sim).unwrap());
    assert!(host.saving);
    assert!(host.queued.iter().any(|command|matches!(command,Command::Snapshot {..})));
    world.set(edited,89<<4);sim.health=3.;
    let latest_pose=PlayerSave {position:[4.5,203.,5.5],motion:None};
    host.save(&mut world,latest_pose,&mut sim).unwrap();

    assert!(host.initial_view_pending() && host.busy());
    assert!(host.in_flight.is_none(),"closing after save must not join a new generation batch");
    assert!(!host.saving);
    assert!(!host.adoption_dirty,"generated metadata must be adopted before capture");
    let queued=host.queued.iter().flat_map(|command| match command {
        Command::GenerateAhead {positions,..}=>positions.iter().copied(),
        _=>panic!("only the ordered remaining initial view should be queued"),
    }).collect::<Vec<_>>();
    assert_eq!(queued,plan[GENERATE_BATCH..],"manual saving must not drain or reorder distant generation");
    assert_eq!(world.get(edited),89<<4);
    assert_eq!(host.sent_edits.get(&edited.to_array()),Some(&(89<<4)));
    let (_,capture)=native_signature(&config.checkpoint_path().unwrap());
    assert_eq!(capture.initial_view_plan,plan);
    assert_eq!(capture.position,latest_pose.position);
    assert_eq!(capture.simulation.health,3.);
    assert!(capture.edits.contains(&(edited.to_array(),89<<4)));
    assert_eq!(capture.seen_spawns,host.seen_spawns);
    assert_eq!(capture.seen_containers,host.seen_containers);
    let resumed_config=copied_config(&config,temp.0.join("resumed"));

    // Death saves can continue playing: the ordinary poll restarts the queue.
    host.poll(&mut world,&mut sim).unwrap();
    assert!(host.in_flight.is_some());
    host.drain(&mut world,&mut sim).unwrap();
    host.save(&mut world,latest_pose,&mut sim).unwrap();
    let expected=native_signature(&config.checkpoint_path().unwrap()).0;
    drop(host);

    let (mut resumed,mut rw,rp,mut rs)=open_recorded(resumed_config.clone(),&region((0,0),1));
    assert_eq!(resumed.initial_view_plan,plan);
    assert_eq!(rw.get(edited),89<<4);
    resumed.restore_initial_view(&mut rw).unwrap();
    resumed.poll(&mut rw,&mut rs).unwrap();
    resumed.save(&mut rw,rp,&mut rs).unwrap();drop(resumed);
    assert_eq!(native_signature(&resumed_config.checkpoint_path().unwrap()).0,expected,
        "manual prefix save/resume must retain the final terrain, lighting, provider order and RNG");
}

#[test]
#[ignore="full original625 startup, persistent checkpoint and eager/deferred view comparison"]
fn fresh_entry_finishes_original_spawn_then_defers_view_and_survives_immediate_save() {
    use clap::Parser;
    let temp=Temporary::new();
    let mut options=Options::parse_from(["ferrum189","--seed","189","--render-distance","16",
        "--save-id","fresh-entry-test"]);
    options.project_dir=temp.0.clone();options.world_rules.structures=false;
    let mut loaded=PopulatedWorld::load(&options,&mut |_|{}).unwrap();
    assert!(loaded.host.startup.progressive_view);
    assert_eq!(loaded.host.startup.deferred_spawn_publications, 19);
    assert!(loaded.host.startup.deferred_view_requests>0);
    assert!(loaded.host.initial_view_pending());
    let pose=loaded.pose.unwrap();
    assert!(entry_neighborhood_ready(&loaded.world,chunk_of(pose.position)));
    for p in crate::vanilla::runtime::initial_spawn_region(loaded.world.spawn.as_ivec3()) {
        assert!(loaded.host.known_chunks.contains(&p));
    }
    let mut sim=Simulation::new(Mode::Creative,189);sim.mob_spawning=false;
    loaded.host.poll(&mut loaded.world,&mut sim).unwrap();
    loaded.host.save(&mut loaded.world,pose,&mut sim).unwrap();
    assert!(loaded.host.initial_view_pending());
    let path=loaded.world.save_path.clone();
    let prefix=fs::read(&path).unwrap();
    loaded.host.drain(&mut loaded.world,&mut sim).unwrap();
    loaded.host.save(&mut loaded.world,pose,&mut sim).unwrap();drop(loaded);
    let expected=native_signature(&path).0;
    fs::write(&path,prefix).unwrap();
    options.render_distance=3;
    let mut resumed=PopulatedWorld::load(&options,&mut |_|{}).unwrap();
    assert!(resumed.host.startup.resumed);
    assert!(!resumed.host.startup.progressive_view);
    assert_eq!(resumed.host.startup.deferred_spawn_publications, 0);
    assert!(!resumed.host.initial_view_pending());
    let mut sim=resumed.simulation.take().unwrap();
    resumed.host.save(&mut resumed.world,resumed.pose.unwrap(),&mut sim).unwrap();drop(resumed);
    assert_eq!(native_signature(&path).0,expected);
}

#[test]
fn retained_chunk_requests_reuse_snapshots_and_republish_only_after_eviction() {
    let mut session = NativeSession::in_memory(189, 1234).unwrap();
    let Event::Ready { update, .. } = session.receive().unwrap() else { panic!("native owner ready"); };
    let mut host = PopulatedWorld::new(session, false);
    let mut world = blank_world();
    let mut sim = Simulation::new(Mode::Creative, 189);
    sim.mob_spawning = false;
    host.apply(&mut world, update);
    host.request_chunks(&mut world, vec![(0, 0)]).unwrap();
    assert!(host.busy());
    host.drain(&mut world, &mut sim).unwrap();
    let first = world.chunks[&(0, 0)].clone();
    let revision = world.light_revision;
    // A visible retained target and its generated population halo are done.
    host.request_chunks(&mut world, vec![(0, 0)]).unwrap();
    assert!(!host.busy(), "cached target must not queue another worker publication");
    assert!(host.poll(&mut world, &mut sim).unwrap().is_empty());
    assert_eq!(world.light_revision, revision);
    assert!(Arc::ptr_eq(&first, &world.chunks[&(0, 0)]));
    // A duplicate immutable result should not trigger lighting or meshing.
    host.apply(&mut world, Update { chunks: vec![((0, 0), first.clone())],
        remesh: vec![(0, 0)], ..Default::default() });
    assert_eq!(world.light_revision, revision);
    assert!(host.poll(&mut world, &mut sim).unwrap().is_empty());
    // Once the client evicts it, the owner must still publish the retained
    // blocks/light without regenerating the provider or copying their arrays.
    world.chunks.remove(&(0, 0));
    host.request_chunks(&mut world, vec![(0, 0)]).unwrap();
    assert!(host.busy());
    assert!(host.drain(&mut world, &mut sim).unwrap().contains(&(0, 0)));
    assert!(Arc::ptr_eq(&first, &world.chunks[&(0, 0)]));
}

#[test]
fn empty_and_discarded_publications_keep_metadata_and_overlay_visible_replacements() {
    let mut session = NativeSession::in_memory(189, 1234).unwrap();
    assert!(matches!(session.receive().unwrap(), Event::Ready { .. }));
    let mut host = PopulatedWorld::new(session, false);
    let mut world = blank_world();
    let visible = Arc::new(crate::world::Chunk::empty());
    world.chunks.insert((1, 0), visible.clone());
    world.light_revision = 7;
    host.wanted.insert((0, 0));
    host.dirty.insert((1, 0));
    host.adoption_dirty = false;
    let removed = [130, 70, 130];
    host.pending_containers.insert(removed, NativeContainer::empty(removed, 54, 1, 27));
    let pending = [131, 70, 130];
    let spawn = SpawnRecord { family: 3, index: 7, spawn: GeneratedSpawn::Witch([130, 70, 130]) };

    // Both a metadata-only acknowledgement and an invisible population halo
    // must take the shortcut without losing discoveries or dirty notifications.
    for chunks in [Vec::new(), vec![((8, 8), visible.clone())]] {
        host.apply(&mut world, Update {
            chunks,
            generation_timings: crate::native_session::GenerationTimings {
                generate_ms: 17., ..Default::default()
            },
            remesh: vec![(1, 0), (8, 8)],
            metadata: MetadataUpdate {
                containers: vec![NativeContainer::empty(pending, 54, 2, 27)],
                removed_containers: vec![removed],
                spawned: vec![spawn.clone()],
                ..Default::default()
            },
            ..Default::default()
        });
        assert_eq!(world.chunks.len(), 1);
        assert!(Arc::ptr_eq(&world.chunks[&(1, 0)], &visible));
        assert_eq!(world.light_revision, 7);
        assert_eq!(host.dirty, BTreeSet::from([(1, 0)]));
        assert_eq!(host.generation_timings.generate_ms, 17.);
        assert!(host.adoption_dirty);
        assert!(!host.pending_containers.contains_key(&removed));
        assert_eq!(host.pending_containers[&pending].revision, 2);
        assert_eq!(host.pending_spawns.len(), 1);
        assert!(host.pending_spawns.contains_key(&(3, 7)));
    }
    assert!(host.known_chunks.contains(&(8, 8)), "discarded halo is still generated on the owner");

    // Retain both requested chunks and already-visible chunks outside wanted.
    // Latest local edits must still win over a racing owner publication.
    world.edits.extend([([2, 70, 2], 41 << 4), ([18, 70, 2], 57 << 4),
        ([130, 70, 130], 89 << 4)]);
    let edits = world.edits.clone();
    host.apply(&mut world, Update {
        chunks: vec![((0, 0), visible.clone()), ((1, 0), visible.clone()), ((8, 8), visible)],
        remesh: vec![(1, 0), (8, 8)],
        ..Default::default()
    });
    assert_eq!(world.get(IVec3::new(2, 70, 2)), 41 << 4);
    assert_eq!(world.get(IVec3::new(18, 70, 2)), 57 << 4);
    assert!(!world.chunks.contains_key(&(8, 8)));
    assert_eq!(world.edits, edits);
    assert_eq!(world.light_revision, 8);
    assert_eq!(host.dirty, BTreeSet::from([(0, 0), (1, 0)]));
    assert!(host.known_chunks.is_superset(&HashSet::from([(0, 0), (1, 0), (8, 8)])));
    assert!(host.pending_containers.contains_key(&pending));
    assert!(host.pending_spawns.contains_key(&(3, 7)));
}

fn reopen(config: Config, expected_block: u16, expected_health: f32, expected_item: u16,
    expected_position: [f64;3], far_loaded: bool) {
    let mut session = NativeSession::spawn(config).unwrap();
    let Event::Ready { resumed: true, gameplay, update, .. } = session.receive().unwrap() else {
        panic!("durable snapshot must reopen");
    };
    let save: GameplaySave = serde_json::from_value(gameplay).unwrap();
    assert_eq!(save.position, expected_position);
    assert_eq!(save.simulation.health, expected_health);
    assert_eq!(save.simulation.inventory.held().unwrap().id, expected_item);
    assert!(save.edits.contains(&([7, 200, 7], expected_block)));
    let chunk = update.chunks.iter().find(|(p,_)| *p == (0,0)).unwrap().1.clone();
    assert_eq!(chunk.get(7,200,7), expected_block, "native block and gameplay overlay must share capture instant");
    assert_eq!(update.chunks.iter().any(|(p,_)| *p == (12,12)), far_loaded);
    session.join().unwrap();
}

#[test]
fn autosave_barrier_preserves_capture_without_waiting_or_losing_generation() {
    let temp = Temporary::new();
    fs::create_dir_all(&temp.0).unwrap();
    let legacy = temp.0.join("world-autosave-test.json");
    fs::write(&legacy, b"original terrain overlay remains untouched").unwrap();
    let original = fs::read(&legacy).unwrap();
    let mut config = Config::new(189, "autosave-test", &temp.0);
    config.map_features = false;
    let path = config.checkpoint_path().unwrap();
    let mut session = NativeSession::spawn(config.clone()).unwrap();
    let Event::Ready { update, .. } = session.receive().unwrap() else { panic!("new owner ready"); };
    let mut host = PopulatedWorld::new(session, true);
    let mut world = blank_world();
    host.wanted.extend([(0,0), (4,4), (12,12)]);
    host.apply(&mut world, update);
    let (reply, update) = host.exchange(Command::Generate(vec![(0,0)])).unwrap();
    PopulatedWorld::successful(&reply).unwrap();
    host.apply(&mut world, update);
    let mut sim = Simulation::new(Mode::Creative, 189);
    sim.mob_spawning = false;
    sim.health = 7.;
    sim.inventory.slots[0] = Some(Stack::new(266, 3));
    let at = IVec3::new(7,200,7);
    world.set(at, 41 << 4);
    host.save(&mut world, PlayerSave { position:[1.5,201.,1.5], motion:None }, &mut sim).unwrap();
    let previous_checkpoint = fs::read(&path).unwrap();

    // Real worker generation is submitted, with another request and a stale
    // unsent edit queued behind it. No large spawn preload is needed.
    host.queued.push_back(Command::Generate(vec![(4,4)]));
    host.queued.push_back(Command::Edits(vec![BlockEdit { position:at.to_array(),
        state:1 << 4, expected:None, notify_neighbors:false }]));
    host.queued.push_back(Command::Generate(vec![(12,12)]));
    host.start_next().unwrap();
    let in_flight = host.in_flight.unwrap();
    world.set(at, 57 << 4);
    sim.health = 12.;
    sim.inventory.slots[0] = Some(Stack::new(264,9));
    let captured_position = [2.75,202.125,3.25];
    let start = Instant::now();
    assert!(host.autosave(&mut world, PlayerSave { position:captured_position, motion:None }, &sim).unwrap());
    let capture_time = start.elapsed();
    // This is a nonblocking protocol assertion, independent of worker speed:
    // the caller has not consumed the generation completion or submitted save.
    assert_eq!(host.in_flight, Some(in_flight));
    assert!(!host.known_chunks.contains(&(4,4)));
    assert!(capture_time < Duration::from_secs(1), "gameplay capture blocked for {capture_time:?}");
    assert_eq!(fs::read(&path).unwrap(), previous_checkpoint,
        "the previous complete checkpoint stays intact before the queued barrier");
    assert!(matches!(host.queued.front(), Some(Command::Edits(edits)) if edits.len()==1 && edits[0].state==57<<4));
    assert!(matches!(host.queued.get(1), Some(Command::Snapshot { .. })));
    assert!(matches!(host.queued.get(2), Some(Command::Generate(v)) if v==&vec![(12,12)]));
    assert_eq!(host.queued.len(),3, "stale queued edits must not be replayed");

    // Input continues after the capture; these changes belong to the next save.
    world.set(at, 89 << 4);
    sim.health = 3.;
    sim.inventory.slots[0] = Some(Stack::new(265,2));
    let next_position = [4.5,203.,5.5];
    assert!(!host.autosave(&mut world, PlayerSave { position:next_position, motion:None }, &sim).unwrap());
    let deadline = Instant::now()+Duration::from_secs(30);
    while host.busy() {
        host.poll(&mut world,&mut sim).unwrap();
        assert!(Instant::now()<deadline, "queued save/generation failed to complete");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(!host.saving);
    assert!(host.known_chunks.contains(&(4,4)) && host.known_chunks.contains(&(12,12)),
        "generation requests behind the save barrier must resume");
    assert_eq!(world.get(at),89<<4);
    assert_eq!(host.sent_edits.get(&at.to_array()), Some(&(89<<4)),
        "post-capture edits must reach the owner before pending generation resumes");
    let first_checkpoint = fs::read(&path).unwrap();
    assert_ne!(first_checkpoint,previous_checkpoint);
    assert_eq!(fs::read(&legacy).unwrap(),original);

    // Open a byte-for-byte copy while the live owner still holds its own lock.
    let mut copy = config.clone();
    copy.save_root = temp.0.join("captured-copy");
    let copied_path = copy.checkpoint_path().unwrap();
    fs::create_dir_all(copied_path.parent().unwrap()).unwrap();
    fs::write(copied_path,&first_checkpoint).unwrap();
    reopen(copy,57<<4,12.,264,captured_position,false);

    host.save(&mut world, PlayerSave { position:next_position, motion:None }, &mut sim).unwrap();
    assert_ne!(fs::read(&path).unwrap(),first_checkpoint);
    assert_eq!(fs::read(&legacy).unwrap(),original);
    assert!(fs::read_dir(path.parent().unwrap()).unwrap().all(|entry|
        !entry.unwrap().file_name().to_string_lossy().ends_with(".tmp")),
        "atomic checkpoint replacement must not leave a partial temporary file");
    drop(host);
    reopen(config,89<<4,3.,265,next_position,true);
    eprintln!("Autosave gameplay capture returned in {capture_time:?}; both durable generations reopened");
}

#[test]
fn manual_save_reconciles_after_an_older_autosave_with_and_without_a_generation_tail() {
    for trailing_generation in [false, true] {
        let temp = Temporary::new();
        fs::create_dir_all(&temp.0).unwrap();
        let mut config = Config::new(189, "manual-after-autosave", &temp.0);
        config.map_features = false;
        let mut session = NativeSession::spawn(config.clone()).unwrap();
        let Event::Ready { update, .. } = session.receive().unwrap() else { panic!("new owner ready"); };
        let mut host = PopulatedWorld::new(session, true);
        let mut world = blank_world();
        host.wanted.extend([(0, 0), (4, 4), (12, 12)]);
        host.apply(&mut world, update);
        let (reply, update) = host.exchange(Command::Generate(vec![(0, 0)])).unwrap();
        PopulatedWorld::successful(&reply).unwrap();
        host.apply(&mut world, update);
        let mut sim = Simulation::new(Mode::Creative, 189);
        sim.mob_spawning = false;
        sim.health = 7.;
        sim.inventory.slots[0] = Some(Stack::new(266, 3));
        let p = IVec3::new(7, 200, 7);
        let original = 41 << 4;
        let captured = 57 << 4;
        assert!(world.set(p, original));
        host.save(&mut world, PlayerSave { position: [1.5, 201., 1.5], motion: None }, &mut sim).unwrap();
        assert_eq!(host.sent_edits.get(&p.to_array()), Some(&original));
        assert!(!host.initial_view_pending());

        // Submit one real request but deliberately do not consume its reply.
        // This retains the autosave barrier regardless of worker completion
        // speed. The no-tail case ends exactly at the captured Snapshot.
        host.queued.push_back(Command::Generate(vec![(4, 4)]));
        if trailing_generation { host.queued.push_back(Command::Generate(vec![(12, 12)])); }
        host.start_next().unwrap();
        assert!(host.in_flight.is_some());
        assert!(world.set(p, captured));
        sim.health = 12.;
        sim.inventory.slots[0] = Some(Stack::new(264, 9));
        assert!(host.autosave(&mut world,
            PlayerSave { position: [2.5, 202., 2.5], motion: None }, &sim).unwrap());
        assert!(host.saving);
        assert!(matches!(host.queued.front(), Some(Command::Edits(edits))
            if edits.len() == 1 && edits[0].state == captured));
        if trailing_generation {
            assert!(matches!(host.queued.back(), Some(Command::Generate(v)) if v == &vec![(12, 12)]));
        } else {
            assert!(matches!(host.queued.back(), Some(Command::Snapshot { .. })));
            assert_eq!(host.queued.len(), 2);
        }

        // The latest value equals the OLD submission baseline, so the first
        // manual-save queue pass has nothing to append for this position.
        assert!(world.set(p, original));
        sim.health = 3.;
        sim.inventory.slots[0] = Some(Stack::new(265, 2));
        let latest_position = [4.5, 203., 5.5];
        let unknown = [320, 200, 320];
        world.edits.insert(unknown, 48);
        assert_eq!(host.sent_edits.get(&p.to_array()), Some(&original));
        host.save(&mut world, PlayerSave { position: latest_position, motion: None }, &mut sim).unwrap();
        assert!(!host.busy() && !host.saving);
        assert_eq!(world.get(p), original);
        assert_eq!(host.sent_edits.get(&p.to_array()), Some(&original));
        assert!(!host.known_chunks.contains(&(20, 20)), "reconciliation must not generate unknown overlays");
        assert_eq!(host.deferred_edits[&(20, 20)].get(&unknown), Some(&48));
        drop(host);

        // Reopen the actual durable checkpoint, independently checking its
        // native block snapshot and gameplay overlay/player state agree.
        reopen(config, original, 3., 265, latest_position, trailing_generation);
    }
}

#[test]
fn nether_death_autosave_and_overworld_respawn_keeps_native_worker_healthy() {
    use crate::vanilla::drops::DropNbt as Tag;
    let temp = Temporary::new();
    let mut config = Config::new(189, "nether-respawn-test", temp.0.join("live"));
    config.map_features = false;
    let mut session = NativeSession::spawn(config).unwrap();
    let Event::Ready { update, .. } = session.receive().unwrap() else {
        panic!("ready");
    };
    let mut host = PopulatedWorld::new(session, true);
    let mut ow_world = blank_world();
    ow_world.spawn = Vec3::new(8.5, 70.0, 8.5);
    host.apply(&mut ow_world, update);
    let (_, update) = host.exchange(Command::Generate(vec![(0, 0)])).unwrap();
    host.apply(&mut ow_world, update);

    let mut sim = Simulation::new(Mode::Survival, 189);
    sim.mob_spawning = false;
    let spawner_pos = IVec3::new(4, 70, 4);
    ow_world.set(spawner_pos, 52 << 4);
    sim.world_commands.edit_tile(
        &mut ow_world,
        spawner_pos,
        52 << 4,
        Some(Tag::Compound(std::collections::BTreeMap::from([
            ("id".into(), Tag::String("MobSpawner".into())),
            ("EntityId".into(), Tag::String("Zombie".into())),
            ("x".into(), Tag::Int(spawner_pos.x)),
            ("y".into(), Tag::Int(spawner_pos.y)),
            ("z".into(), Tag::Int(spawner_pos.z)),
        ]))),
    );
    host.flush_overworld_edits(&ow_world).unwrap();

    // Enter Nether (-1) with tile edits present
    sim.stash_active_dimension(&ow_world);
    let mut nether_world = blank_world();
    sim.activate_dimension(-1, &mut nether_world);
    let nether_spawner = IVec3::new(2, 60, 2);
    nether_world.set(nether_spawner, 52 << 4);
    sim.world_commands.edit_tile(
        &mut nether_world,
        nether_spawner,
        52 << 4,
        Some(Tag::Compound(std::collections::BTreeMap::from([
            ("id".into(), Tag::String("MobSpawner".into())),
            ("EntityId".into(), Tag::String("Blaze".into())),
            ("x".into(), Tag::Int(nether_spawner.x)),
            ("y".into(), Tag::Int(nether_spawner.y)),
            ("z".into(), Tag::Int(nether_spawner.z)),
        ]))),
    );

    // Trigger /kill autosave in Nether
    sim.health = 0.0;
    sim.snapshot_active_dimension_for_save(&nether_world);
    assert!(host
        .autosave(
            &mut nether_world,
            PlayerSave {
                position: [2.5, 61.0, 2.5],
                motion: None
            },
            &sim,
        )
        .unwrap());

    // Respawn back to Overworld (0) and ensure chunks + worker remain healthy
    sim.stash_active_dimension(&nether_world);
    sim.activate_dimension(0, &mut ow_world);
    host.ensure_priority_chunks_blocking(&mut ow_world, &mut sim, &[(0, 0), (1, 0)])
        .expect("Overworld respawn priority chunks must succeed without worker disconnect");
    assert!(ow_world.chunks.contains_key(&(0, 0)));
    assert!(ow_world.chunks.contains_key(&(1, 0)));

    // Subsequent streaming request and Save & Quit must also succeed cleanly
    host.request_chunks(&mut ow_world, vec![(0, 1)]).unwrap();
    host.drain(&mut ow_world, &mut sim).unwrap();
    assert!(ow_world.chunks.contains_key(&(0, 1)));
    host.save(
        &mut ow_world,
        PlayerSave {
            position: [8.5, 70.0, 8.5],
            motion: None,
        },
        &mut sim,
    )
    .expect("Save & Quit after Nether respawn must succeed");
}
