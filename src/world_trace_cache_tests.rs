//! Independent pre-cache DDA oracle, intentionally unchanged traversal.
use super::*;

fn original_trace(
        world: &World,
        origin: Vec3,
        direction: Vec3,
        reach: f32,
        opaque_only: bool,
        stop_on_liquid: bool,
    ) -> Option<(IVec3, IVec3)> {
        let mut block = origin.floor().as_ivec3();
        let step = direction.signum().as_ivec3();
        let delta = Vec3::new(
            if direction.x == 0.0 {
                f32::INFINITY
            } else {
                1.0 / direction.x.abs()
            },
            if direction.y == 0.0 {
                f32::INFINITY
            } else {
                1.0 / direction.y.abs()
            },
            if direction.z == 0.0 {
                f32::INFINITY
            } else {
                1.0 / direction.z.abs()
            },
        );
        let mut next = Vec3::ZERO;
        for axis in 0..3 {
            next[axis] = if direction[axis] == 0.0 {
                f32::INFINITY
            } else {
                ((block[axis] as f32 + if step[axis] > 0 { 1.0 } else { 0.0 }) - origin[axis])
                    / direction[axis]
            };
        }
        let mut previous = block;
        for _ in 0..512 {
            let state = world.get(block);
            if if opaque_only {
                assets::solid(state) && !assets::see_through(state)
            } else {
                state != 0 && (!matches!(state >> 4, 8..=11) || (stop_on_liquid && state & 15 == 0))
            } {
                return Some((block, previous));
            }
            let axis = if next.x < next.y && next.x < next.z {
                0
            } else if next.y < next.z {
                1
            } else {
                2
            };
            if next[axis] > reach {
                break;
            }
            previous = block;
            block[axis] += step[axis];
            next[axis] += delta[axis];
        }
        None
    }

fn world() -> World {
    World { chunks:ChunkMap::new(),spawn:Vec3::ZERO,label:"trace cache oracle".into(),
        save_path:Default::default(),edits:Default::default(),imported:false,light_revision:0 }
}
fn compare(world:&World,origin:Vec3,direction:Vec3,reach:f32) {
    for opaque in [false,true] { for liquids in [false,true] {
        assert_eq!(world.trace(origin,direction,reach,opaque,liquids),
            original_trace(world,origin,direction,reach,opaque,liquids),
            "origin={origin:?}, dir={direction:?}, reach={reach}, opaque={opaque}, liquids={liquids}");
    } }
}
fn original_occluded(world:&World,from:Vec3,to:Vec3)->bool {
    let delta=to-from;
    let distance=delta.length();
    distance>0.01 && original_trace(world,from,delta/distance,(distance-0.05).max(0.),true,false).is_some()
}
#[test]
fn cached_trace_retains_original_shape_material_and_reach_endpoint_rules() {
    let mut world=world();
    world.chunks.insert((0,0),Arc::new(Chunk::empty()));
    let at=IVec3::new(4,64,4);
    // This private visual trace uses material predicates, not collision shapes.
    // Keep that behavior for glass/leaves/slabs/stairs/torches/fences/liquids.
    for state in [0,16,32,20<<4,18<<4,44<<4,53<<4,85<<4,50<<4,8<<4,8<<4|3,
        9<<4,10<<4,10<<4|8,11<<4,64<<4,71<<4,95<<4|7,102<<4,97<<4|2,166<<4,90<<4] {
        world.set_raw(at,state);
        for origin in [Vec3::new(0.5,64.5,4.5),Vec3::new(4.5,64.5,4.5),Vec3::new(8.5,64.5,4.5)] {
            for direction in [Vec3::X,-Vec3::X,Vec3::ZERO,Vec3::new(-0.,0.,-0.)] {
                for reach in [-1.,0.,0.01,3.49999,3.5,3.50001,16.,f32::INFINITY,f32::NAN] {
                    compare(&world,origin,direction,reach);
                }
            }
        }
    }
    world.set_raw(at,16);
    assert_eq!(world.trace(Vec3::new(0.5,64.5,4.5),Vec3::X,3.5,true,false),Some((at,at-IVec3::X)));
    assert_eq!(world.trace(Vec3::new(0.5,64.5,4.5),Vec3::X,3.49999,true,false),None);
    for distance in [0.,0.009999,0.01,0.010001,3.49999,3.5,3.55,3.55001,16.] {
        let from=Vec3::new(0.5,64.5,4.5);let to=from+Vec3::X*distance;
        assert_eq!(world.occluded(from,to),original_occluded(&world,from,to));
    }
}
#[test]
fn cached_trace_preserves_missing_columns_negative_bounds_vertical_limits_and_ties() {
    let mut world=world();
    for p in [(-2,0),(0,0),(2,0)] { world.chunks.insert(p,Arc::new(Chunk::empty())); }
    for at in [IVec3::new(-20,0,4),IVec3::new(4,255,4),IVec3::new(47,64,4)] {world.set_raw(at,16);}
    for origin in [Vec3::new(-31.5,64.5,4.5),Vec3::new(-16.,64.,0.),Vec3::new(0.,-1.,0.),
        Vec3::new(4.5,256.,4.5),Vec3::new(4.,255.,4.),Vec3::new(15.99999,64.,15.99999)] {
        for direction in [Vec3::X,-Vec3::X,Vec3::Y,-Vec3::Y,Vec3::Z,-Vec3::Z,
            Vec3::ONE, -Vec3::ONE,Vec3::new(1.,1.,0.),Vec3::new(0.,1.,1.),Vec3::new(1.,0.,1.),
            Vec3::new(-1.,1.,-1.),Vec3::new(f32::NAN,1.,0.),Vec3::new(f32::INFINITY,0.,1.)] {
            for reach in [0.,1.,16.,78.49999,78.5,78.50001,511.,512.,1024.,f32::INFINITY] {
                compare(&world,origin,direction,reach);
            }
        }
    }
    // No new +/-30M rule: World::get has only Y bounds, including loaded
    // columns just outside the logical play border. Preserve that old contract.
    for x in [-30_000_016_i32,30_000_016_i32] {
        world.chunks.insert((x.div_euclid(16),0),Arc::new(Chunk::empty()));
        world.set_raw(IVec3::new(x,64,4),16);
        compare(&world,Vec3::new(x as f32,64.5,4.5),Vec3::X,512.);
    }
    // The last-column cache must be local to one call, including cached absence.
    let origin=Vec3::new(17.5,64.5,4.5);let target=IVec3::new(19,64,4);
    assert!(world.trace(origin,Vec3::X,4.,true,false).is_none());
    world.chunks.insert((1,0),Arc::new(Chunk::empty()));world.set_raw(target,16);
    assert!(world.trace(origin,Vec3::X,4.,true,false).is_some());
    world.chunks.remove(&(1,0));
    assert!(world.trace(origin,Vec3::X,4.,true,false).is_none());
}
fn random_next(random:&mut u64)->u64 {
    *random=random.wrapping_mul(6364136223846793005).wrapping_add(1);*random
}
#[test]
fn cached_trace_matches_original_across_generated_terrain_and_random_octants() {
    use crate::vanilla::{terrain::Terrain,biome::BiomeGenerator};
    for seed in [189,8924706420471469774] {
        let mut world=world();let terrain=Terrain::new(seed);let biomes=BiomeGenerator::new(seed);
        for p in [(-1,-1),(-1,0),(0,-1),(0,0)] {
            world.chunks.insert(p,Arc::new(terrain.generate(p.0,p.1,&biomes).unwrap()));
        }
        let mut random=seed as u64;
        let mut hits=0;let mut misses=0;
        for _ in 0..4096 {
            let mut axis=|span:u64,base:f32| ((random_next(&mut random)>>32)%span) as f32 / 16.+base;
            let origin=Vec3::new(axis(1024,-32.),axis(4352,-8.),axis(1024,-32.));
            let to=Vec3::new(axis(2048,-64.),axis(4352,-8.),axis(2048,-64.));
            let delta=to-origin;
            let direction=delta.normalize_or_zero();
            let reach=delta.length();
            compare(&world,origin,direction,reach);
            let expected=original_occluded(&world,origin,to);
            assert_eq!(world.occluded(origin,to),expected);
            if expected {hits+=1;}else{misses+=1;}
        }
        assert!(hits>100 && misses>100,"generated fixture needs both outcomes");
    }
}
