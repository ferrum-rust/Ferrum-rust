use super::*;
use clap::Parser;
#[test]
fn arena_cli_is_explicit_streaming_only_and_isolates_diagnostic_arms() {
    assert!(crate::Options::try_parse_from(["ferrum"]).unwrap().benchmark_near_arena.is_none());
    for (name,choice,mode) in [("dedicated",Choice::Dedicated,Mode::Dedicated),("direct",Choice::Direct,Mode::ArenaDirect),("mdi",Choice::Mdi,Mode::ArenaMdi)] {
        assert_eq!(choice.mode(),mode);
        assert!(crate::Options::try_parse_from(["ferrum","--benchmark-near-arena",name]).is_err());
        assert!(crate::Options::try_parse_from(["ferrum","--benchmark","--benchmark-near-arena",name]).is_err());
        let args=["ferrum","--benchmark","--benchmark-streaming","--benchmark-near-arena",name];
        assert_eq!(crate::Options::try_parse_from(args).unwrap().benchmark_near_arena,Some(choice));
        for conflict in ["--benchmark-near-opaque-runs","--benchmark-capture-uploads","--benchmark-capture-near-opaque","--benchmark-capture-transparency"] {
            let mut args=args.to_vec();args.push(conflict);assert!(crate::Options::try_parse_from(args).is_err());
        }
    }
    assert!(crate::Options::try_parse_from(["ferrum","--benchmark","--benchmark-streaming","--benchmark-near-arena","unknown"]).is_err());
}
#[test]
fn worker_index_provenance_uses_logical_values_excluding_upload_padding() {
    for values in [vec![],vec![0],vec![0,65535,1],vec![0,65536,3],vec![u32::MAX,1,0]] {
        let expected=values.iter().copied().max();let mut indices=geometry::Indices::new(values.clone());
        assert_eq!(indices.max_local_index(),expected);assert_eq!(indices.values(),values);
        assert_eq!(indices.bytes().len()%4,0);indices.wide();
        assert_eq!(indices.max_local_index(),expected);assert_eq!(indices.values(),values);
    }
}
#[test]
fn prepared_invalid_vertex_reach_rejects_arena_before_any_page_creation() {
    struct Never;impl PageFactory for Never {type Handle=usize;type Error=();
        fn create(&mut self,_:PageKind,_:u64)->std::result::Result<usize,()>{panic!("invalid provenance reached GPU page creation")}}
    let mut mesh=models::empty();
    models::face(&mut mesh,[Vec3::ZERO,Vec3::X,Vec3::ONE,Vec3::Y],[0.;4],models::SOLID,[255;4]);
    // The opaque partition reads the first vertex only. A later invalid index
    // can therefore survive into the dedicated GPU buffer's robust fetch path.
    mesh.indices[5]=500;
    let prepared=PreparedChunk::new(mesh);let request=prepared.arena_request([1<<20,1<<20]);
    assert_eq!(request.max_local_index,Some(500));assert_eq!(request.vertices,4);
    assert!(request.vertex_reserved>request.vertex_bytes);
    let mut arena=Arena::new(Mode::ArenaDirect,Limits::default()).unwrap();
    assert!(matches!(arena.begin_update(None,request,&mut Never),Err(near_arena::Failure::Admission(near_arena::Reject::InvalidRequest))));
}
