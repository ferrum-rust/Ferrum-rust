use super::*;
use sha1::{Digest,Sha1};

struct Temporary(PathBuf);
impl Temporary {
    fn new()->Self {
        Self(std::env::temp_dir().join(format!("ferrum-native-journal-{}-{}",std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos())))
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let (Ok(root),Ok(path))=(fs::canonicalize(std::env::temp_dir()),fs::canonicalize(&self.0)) else {return;};
        if path.parent()==Some(root.as_path())
            && path.file_name().is_some_and(|name|name.to_string_lossy().starts_with("ferrum-native-journal-")) {
            let _=fs::remove_dir_all(&path);
        }
    }
}

fn digest(value:&impl Serialize)->String {
    format!("{:x}",Sha1::digest(serde_json::to_vec(value).unwrap()))
}

#[test]
fn native_session_untracked_journal_preserves_publications_and_durable_checkpoint() {
    let temp=Temporary::new();
    let config=Config::new(189,"native-journal-test",&temp.0);
    let path=config.checkpoint_path().unwrap();
    let mut expected=None;
    for recording in [true,false] {
        let (mut worker,_)=Worker::open(config.clone(),path.clone(),Some(1234)).unwrap();
        worker.runtime.backend.entities.math_state=Some(1234);
        worker.runtime.use_overlay_journal(recording);
        let mut publications=Vec::new();
        for positions in [vec![(0,0),(0,1)],vec![(1,0),(2,1),(1,1)]] {
            assert!(matches!(worker.execute(Command::Generate(positions)).unwrap().0,Reply::Generated));
            let update=worker.update(false);
            assert!(worker.runtime.backend.world.edits.is_empty());
            let hashes=update.chunks.iter().map(|(p,chunk)| {
                let mut bytes=Sha1::new();bytes.update(bytemuck::cast_slice(&chunk.blocks));
                bytes.update(&chunk.biomes);
                if let Some(light)=&chunk.light {bytes.update(&light.values);}
                (*p,format!("{:x}",bytes.finalize()))
            }).collect::<Vec<_>>();
            publications.push((hashes,update.remesh));
        }
        let position=[7,220,7];
        assert!(matches!(worker.execute(Command::Edits(vec![BlockEdit {
            position,state:57<<4,expected:None,notify_neighbors:false,
        }])).unwrap().0,Reply::Edited {rejected} if rejected.is_empty()));
        assert_eq!(worker.runtime.backend.world.edits.contains_key(&position),recording);
        worker.update(false);
        worker.gameplay=serde_json::json!({"edits":[[position,57<<4]],"inventory":[264,9]});
        worker.save().unwrap();
        let checkpoint=read_checkpoint(fs::File::open(&path).unwrap()).unwrap();
        assert!(checkpoint.runtime.edits.is_empty());
        assert_eq!(checkpoint.gameplay["edits"][0][1],57<<4);
        let actual=(publications,digest(&checkpoint));
        if let Some(expected)=&expected {assert_eq!(&actual,expected);}
        else {expected=Some(actual);}
        drop(worker);

        let (mut resumed,was_resumed)=Worker::open(config.clone(),path.clone(),None).unwrap();
        assert!(was_resumed);
        resumed.update(true);
        assert!(matches!(resumed.execute(Command::Edits(vec![BlockEdit {
            position,state:89<<4,expected:Some(57<<4),notify_neighbors:false,
        }])).unwrap().0,Reply::Edited {rejected} if rejected.is_empty()));
        assert!(resumed.runtime.backend.world.edits.is_empty(),"resumed owner must also disable its trace");
        assert_eq!(resumed.runtime.backend.world.get(IVec3::from_array(position)),89<<4);
        resumed.update(false);resumed.save().unwrap();
    }
}

#[test]
fn native_session_resume_discards_legacy_trace_without_losing_saved_blocks() {
    let temp=Temporary::new();
    let mut config=Config::new(189,"legacy-journal-test",&temp.0);config.map_features=false;
    let path=config.checkpoint_path().unwrap();
    let (mut worker,_)=Worker::open(config.clone(),path.clone(),Some(1234)).unwrap();
    worker.execute(Command::Generate(vec![(0,0)])).unwrap();worker.update(false);
    let position=IVec3::new(7,220,7);
    worker.runtime.use_overlay_journal(true);
    worker.runtime.access().set_block(position,57<<4).unwrap();
    assert_eq!(worker.runtime.backend.world.edits[&position.to_array()],57<<4);
    worker.save().unwrap();drop(worker);
    let (mut resumed,was_resumed)=Worker::open(config,path,None).unwrap();
    assert!(was_resumed);
    assert!(resumed.runtime.backend.world.edits.is_empty());
    assert_eq!(resumed.runtime.backend.world.get(position),57<<4);
    resumed.runtime.access().set_block(position,89<<4).unwrap();
    assert!(resumed.runtime.backend.world.edits.is_empty());
    assert_eq!(resumed.runtime.backend.world.get(position),89<<4);
}
