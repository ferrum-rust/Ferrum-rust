//! Layout oracle retains the original independent worker graph construction.
use super::*;

pub(super) fn original(chunk:&Chunk,fancy:bool)->Box<[Section;16]> {
    Box::new(std::array::from_fn(|y| {
        let mut bits = [0; 64];
        if (y * 16) < chunk.top as usize {
            for (i, &state) in chunk.blocks[y * 4096..(y + 1) * 4096].iter().enumerate() {
                let opaque = if matches!(state >> 4, 18 | 161) {
                    !fancy
                } else {
                    crate::vanilla::material::opaque_cube(state)
                };
                if opaque { bits[i / 64] |= 1 << (i % 64); }
            }
        }
        Section::new(bits)
    }))
}

#[test]
fn compact_graph_keeps_original_payload_bits_and_camera_exits_without_more_storage() {
    assert_eq!(std::mem::size_of::<Graph>(),std::mem::size_of::<[Section;16]>());
    assert_eq!(std::mem::size_of::<Graph>(),8320);
    let empty=Chunk::empty();
    let mut solid=Chunk::empty();solid.blocks.fill(16);solid.top=256;
    let mut patterned=Chunk::empty();
    for y in 0..256 {for z in 0..16 {
        patterned.set(8,y,z,16);
        if y%7==0 {patterned.set(3,y,z,18<<4);patterned.set(4,y,z,161<<4);}
    }}
    let mut cavity=solid.clone();
    for y in 1..15 {for z in 1..15 {for x in 1..15 {cavity.set(x,y,z,0);}}}
    for chunk in [&empty,&solid,&patterned,&cavity] {for fancy in [false,true] {
        let expected=original(chunk,fancy);
        let actual=TerrainSections::new(chunk,fancy);
        assert!(actual.owners.is_empty()&&actual.chests.is_empty());
        assert!(actual.graph.get(16).is_none());assert!(actual.graph.get(usize::MAX).is_none());
        for y in 0..16 {
            assert_eq!(actual.graph.connections[y],expected[y].connections);
            assert_eq!(actual.graph.occupied[y],expected[y].occupied);
            for local in [0,1,15,16,255,256,2048,4094,4095] {
                assert_eq!(actual.graph.get(y).unwrap().camera_exits(local),expected[y].camera_exits(local),
                    "y={y} local={local} fancy={fancy}");
            }
        }
    }}
}
