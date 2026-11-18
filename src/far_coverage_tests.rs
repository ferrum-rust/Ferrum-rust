use super::*;
use crate::{distant_terrain::{TerrainTile,Surface,GRID},mesh::Vertex};

fn mesh_at(position:TilePos)->Mesh {
    let x=(i64::from(position.0)*128+2) as f32;
    let z=(i64::from(position.1)*128+2) as f32;
    let mut mesh=models::empty();
    for (dx,dz) in [(0.,0.),(4.,0.),(0.,4.)] {
        mesh.vertices.push(Vertex {position:[x+dx,64.,z+dz],uv:[0.;2],
            layer:super::super::distant_render::DISTANT|models::SOLID,
            color:[255;4],light:[0,240,(128.-dx) as u8,(128.-dz) as u8]});
    }
    mesh.indices=vec![0,1,2];mesh
}
fn set(mask:&mut Mask,x:usize,z:usize,value:bool) {
    let index=z*64+x;let word=&mut mask[index/128][index/32%4];
    if value {*word|=1u32<<(index%32);}else{*word&=!(1u32<<(index%32));}
}
fn full(origin:[i32;4],position:TilePos)->Mask {
    let mut mask=[[0;4];32];
    for z in 0..64 {for x in 0..64 {
        let wx=i64::from(origin[0])+x as i64;let wz=i64::from(origin[1])+z as i64;
        let tx=i64::from(position.0)*8;let tz=i64::from(position.1)*8;
        set(&mut mask,x,z,wx>=tx&&wx<tx+8&&wz>=tz&&wz<tz+8);
    }}
    mask
}
fn shader_owner(p:f32,byte:u8)->i32 {
    // Literal shader arithmetic reference, independent of integer proof math.
    let normalized=f32::from(byte)/255.;
    ((p+(normalized*255.).round()-128.)/16.).floor() as i32
}

#[test]
fn upload_proof_matches_shader_owners_for_actual_all_stride_tiles_and_exact_range() {
    assert_eq!(TILE_CHUNKS,8);
    let assets=crate::validation::assets();
    for position in [(0,0),(-1,-2),(131_069,-131_070),(-234_375,-234_375),(234_374,234_374)] {
        let tile=TerrainTile {position,heights:(0..GRID*GRID).map(|i|Surface {
            height:if i%29==0 {0}else{48+(i%23) as u16},
            state:if i%7==0 {9<<4}else{2<<4},biome:1}).collect()};
        for stride in [4,8,16] {
            let mesh=super::super::distant_render::build_with_stride(&tile,assets,stride);
            let proof=Proof::from_mesh(position,&mesh);
            if position.0.abs()>=234_374 {assert!(proof.is_none(),"outer world must be conservative KEEP");continue;}
            let proof=proof.expect("actual builder inside exact guard must be provable");
            for &index in &mesh.indices {
                let v=&mesh.vertices[index as usize];
                for (axis,byte) in [(0,2),(2,3)] {
                    let owner=shader_owner(v.position[axis],v.light[byte]);
                    assert!((proof.chunk_min[axis/2]..proof.chunk_min[axis/2]+8).contains(&owner));
                }
            }
        }
    }
    // All encoded byte values recover through actual normalized f32 math.
    // Both expression associations stay exact inside the chosen headroom.
    for p in [-EXACT_XZ_BOUND,-167.,-16.,-1.,-0.,0.,1.,16.,167.,EXACT_XZ_BOUND] {
        for byte in 0..=255u8 {
            assert_eq!(((f32::from(byte)/255.)*255.).round(),f32::from(byte));
            let exact=p as i64+i64::from(byte)-128;
            assert_eq!((p+f32::from(byte)-128.) as f64,exact as f64);
            assert_eq!((p+(f32::from(byte)-128.)) as f64,exact as f64);
            assert_eq!(shader_owner(p,byte),exact.div_euclid(16) as i32);
        }
    }
}

#[test]
fn arbitrary_invalid_transformed_fractional_and_foreign_payloads_stay_unproven() {
    assert!(Proof::from_mesh((0,0),&mesh_at((0,0))).is_some());
    for layer in [models::SOLID,super::super::distant_render::DISTANT|models::CLOUD,
        super::super::distant_render::DISTANT|models::SKY] {
        let mut mesh=mesh_at((0,0));mesh.vertices[0].layer=layer;
        assert!(Proof::from_mesh((0,0),&mesh).is_none());
    }
    for axis in 0..3 {for bad in [f32::NAN,f32::INFINITY,f32::NEG_INFINITY] {
        let mut mesh=mesh_at((0,0));mesh.vertices[0].position[axis]=bad;
        assert!(Proof::from_mesh((0,0),&mesh).is_none());
    }}
    for bad in [0.5,EXACT_XZ_BOUND+2.,-EXACT_XZ_BOUND-2.,30_000_000.,-30_000_000.] {
        let mut mesh=mesh_at((0,0));mesh.vertices[0].position[0]=bad;
        assert!(Proof::from_mesh((0,0),&mesh).is_none());
    }
    for invalid in [vec![],vec![0,1],vec![0,1,3],vec![0,1,u32::MAX]] {
        let mut mesh=mesh_at((0,0));mesh.indices=invalid;
        assert!(Proof::from_mesh((0,0),&mesh).is_none());
    }
    let mut mesh=mesh_at((0,0));mesh.vertices[0].light[2]=0;
    assert!(Proof::from_mesh((0,0),&mesh).is_none(),"flat owner is outside even though raw position is nominal");
    let mut mesh=mesh_at((0,0));let mut unused=mesh.vertices[0];unused.layer=0;mesh.vertices.push(unused);
    assert!(Proof::from_mesh((0,0),&mesh).is_none(),"all-vertex proof deliberately rejects unknown unused data");
    let mut mesh=mesh_at((0,0));mesh.vertices.resize(65_537,mesh.vertices[0]);mesh.indices=vec![65_536,1,2];
    assert!(Proof::from_mesh((0,0),&mesh).is_some(),"proof does not assume u16 indexing");
    assert!(Proof::from_mesh((i32::MIN,i32::MAX),&mesh).is_none());
    assert!(Proof::from_mesh((234_375,0),&mesh).is_none());
    assert!(Proof::from_mesh((1,0),&mesh).is_none(),"tile argument is provenance, not a trusted label");
}

#[test]
fn every_mask_alignment_requires_all64_bits_and_exact_known_window() {
    for position in [(0,0),(-7,5)] {
        let proof=Proof::from_mesh(position,&mesh_at(position)).unwrap();
        for z in 0..=56 {for x in 0..=56 {
            let origin=[position.0*8-x,position.1*8-z,64,64];
            let mut mask=full(origin,position);assert!(proof.covered(origin,&mask));
            // An independent scalar shader lookup is checked at all64 owner
            // positions, spanning both32-bit words and packed128-bit rows.
            for dz in 0..8 {for dx in 0..8 {
                let xx=(x+dx) as usize;let zz=(z+dz) as usize;
                set(&mut mask,xx,zz,false);
                assert!(!proof.covered(origin,&mask),"one actual missing owner must KEEP");
                set(&mut mask,xx,zz,true);
            }}
        }}
        for (x,z) in [(-1,0),(0,-1),(57,0),(0,57),(64,64)] {
            assert!(!proof.covered([position.0*8-x,position.1*8-z,64,64],&[[u32::MAX;4];32]));
        }
        for origin in [[i32::MIN,i32::MAX,64,64],[i32::MAX,i32::MIN,64,64],
            [position.0*8,position.1*8,63,64],[position.0*8,position.1*8,64,65]] {
            assert!(!proof.covered(origin,&[[u32::MAX;4];32]));
        }
    }
}

#[test]
fn off_upload_late_enable_replacement_and_measured_only_counts_are_conservative() {
    let mesh=mesh_at((0,0));let origin=[-32,-32,64,64];let mask=full(origin,(0,0));
    let mut off=Trial::default();let old=off.prove_upload((0,0),&mesh);assert!(old.is_none());
    off.begin_frame(true);assert!(off.keep_visible(Proof::from_mesh((0,0),&mesh),origin,&mask,3,true));
    assert!(off.report().get("counts").is_none());
    let mut on=Trial::new(true);on.begin_frame(false);
    assert!(on.keep_visible(old,origin,&mask,3,false),"late enable cannot invent old provenance");
    let proof=on.prove_upload((0,0),&mesh);assert!(proof.is_some());
    assert!(!on.keep_visible(proof,origin,&mask,3,false));
    on.begin_frame(true);assert!(!on.keep_visible(proof,origin,&mask,3,true));
    assert!(on.keep_visible(None,origin,&mask,7,true));
    let mut hole=mask;set(&mut hole,32,32,false);assert!(on.keep_visible(proof,origin,&hole,3,true));
    let report=on.report();let c=&report["counts"];
    assert_eq!(c["measured_frames"],1);assert_eq!(c["visible_candidates_before"],3);
    assert_eq!(c["visible_culled"],1);assert_eq!(c["indices_omitted"],3);assert_eq!(c["unproven_kept"],1);
    assert!(off.prove_upload((0,0),&mesh).is_none(),"an OFF replacement discards any prior proof");
    on.counts.indices_omitted=u64::MAX-1;on.counts.visible_culled=u64::MAX;
    assert!(!on.keep_visible(proof,origin,&mask,u32::MAX,true));
    assert_eq!(on.counts.indices_omitted,u64::MAX);assert_eq!(on.counts.visible_culled,u64::MAX);
}

#[test]
fn filtering_after_original_admission_preserves_resident_slots_and_survivor_order() {
    let origin=[-32,-32,64,64];let mask=full(origin,(0,0));
    let proven=Proof::from_mesh((0,0),&mesh_at((0,0))).unwrap();
    let other=Proof::from_mesh((1,0),&mesh_at((1,0))).unwrap();
    let resident=(0..73usize).map(|i|(i,i%4!=0,match i%3 {0=>Some(proven),1=>Some(other),_=>None})).collect::<Vec<_>>();
    for enabled in [false,true] {
        let mut trial=Trial::new(enabled);trial.begin_frame(true);
        let actual=resident.iter().enumerate().filter_map(|(index,(id,old_visible,proof))| {
            if !old_visible {return None;}
            trial.keep_visible(*proof,origin,&mask,3,true).then_some((super::super::far_bundles::slot(index),*id))
        }).collect::<Vec<_>>();
        let expected=resident.iter().enumerate().filter(|(_,(_,old_visible,_))|*old_visible)
            .filter(|(_,(_,_,proof))|!enabled||*proof!=Some(proven))
            .map(|(index,(id,_,_))|(super::super::far_bundles::slot(index),*id)).collect::<Vec<_>>();
        assert_eq!(actual,expected);
        assert!(actual.windows(2).all(|v|v[0].0<=v[1].0));
    }
}
