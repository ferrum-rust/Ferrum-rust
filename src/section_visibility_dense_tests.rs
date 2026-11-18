//! Independent original HashMap traversal, retained to check exact callbacks,
//! queue/order/mask results rather than deriving expected masks from new slots.
use super::*;
use std::cell::RefCell;
#[derive(Default)]
struct OriginalView {
    key: Option<(Mat4, Vec3, Vec3, u8)>,
    reached: HashMap<ChunkPos, u16>,
    record_order:bool,
    render_order:HashMap<IVec3,u32>,
    frustum_only: bool,
    camera_exits: Option<(IVec3, u8)>,
    pending: VecDeque<(IVec3, Option<usize>, u8)>,
    seen: Vec<u16>,
}
impl OriginalView {
    pub fn track_order(&mut self,on:bool){if self.record_order!=on{self.key=None;self.record_order=on;}}
    pub fn order(&self,p:IVec3)->Option<u32>{self.render_order.get(&p).copied()}
    pub fn invalidate(&mut self) {
        self.key = None;
        self.camera_exits = None;
    }
    pub fn update<'a>(
        &mut self,
        matrix: Mat4,
        frustum: &Frustum,
        eye: Vec3,
        direction: Vec3,
        radius: u8,
        graph: impl Fn(ChunkPos) -> Option<&'a [Section; 16]>,
    ) {
        let key = (matrix, eye, direction, radius);
        if self.key == Some(key) {
            return;
        }
        self.key = Some(key);
        self.reached.clear();self.render_order.clear();
        self.frustum_only = true;
        let block = eye.floor().as_ivec3();
        let start = IVec3::new(
            block.x.div_euclid(16),
            block.y.div_euclid(16),
            block.z.div_euclid(16),
        );
        let Some(first) = usize::try_from(start.y)
            .ok()
            .and_then(|y| graph((start.x, start.z))?.get(y))
        else {
            return;
        };
        let local = Chunk::index(
            block.x.rem_euclid(16),
            block.y.rem_euclid(16),
            block.z.rem_euclid(16),
        );
        // Third-person cameras, spectators and corrections may place the eye
        // inside solid geometry. Retain all frustum-visible sections there.
        if occupied(&first.occupied, local) {
            return;
        }
        self.frustum_only = false;
        self.reached.insert((start.x, start.z), 1 << start.y);
        if self.record_order{self.render_order.insert(start,0);}
        let mut visible_exits = match self.camera_exits {
            Some((cached, exits)) if cached == block => exits,
            _ => {
                let exits = first.camera_exits(local);
                self.camera_exits = Some((block, exits));
                exits
            }
        };
        if visible_exits.count_ones() == 1 {
            let mut best = f32::MIN_POSITIVE;
            let mut facing = 2;
            for (index, dir) in DIRS.iter().enumerate() {
                let dot = dir.as_vec3().dot(direction);
                if dot > best {
                    best = dot;
                    facing = index;
                }
            }
            visible_exits &= !(1 << (facing ^ 1));
        }
        if visible_exits == 0 {
            return;
        }
        let pending = &mut self.pending;
        pending.clear();
        pending.push_back((start, None, 0));
        let radius = i32::from(radius);
        let side = (radius * 2 + 1) as usize;
        let seen = &mut self.seen;
        seen.resize(side * side, 0);
        seen.fill(0);
        seen[radius as usize * (side + 1)] = 1 << start.y;
        while let Some((current, incoming, travel)) = pending.pop_front() {
            // Unpublished chunk meshes are traversable until the corresponding
            // opaque mask arrives. Their arrival invalidates the cached view.
            let links =
                graph((current.x, current.z)).map_or(ALL, |g| g[current.y as usize].connections);
            for (face, delta) in DIRS.iter().enumerate() {
                if travel & (1 << (face ^ 1)) != 0
                    || incoming.is_some_and(|f| links & (1 << ((f ^ 1) * 6 + face)) == 0)
                {
                    continue;
                }
                let next = current + *delta;
                let dx = next.x - start.x;
                let dz = next.z - start.z;
                if !(0..16).contains(&next.y) || dx.abs() > radius || dz.abs() > radius {
                    continue;
                }
                let mark = &mut seen[(dz + radius) as usize * side + (dx + radius) as usize];
                let bit = 1u16 << next.y;
                if *mark & bit != 0 {
                    continue;
                }
                *mark |= bit;
                let min = (next * 16).as_vec3();
                if !frustum.visible(min, min + Vec3::splat(16.)) {
                    continue;
                }
                *self.reached.entry((next.x, next.z)).or_default() |= bit;
                if self.record_order{self.render_order.insert(next,self.render_order.len() as u32);}
                pending.push_back((next, Some(face), travel | 1 << face));
            }
        }
    }
    pub fn mask(&self, pos: ChunkPos, frustum: &Frustum, present: u16) -> u16 {
        if self.frustum_only {
            let Some((_, eye, _, radius)) = self.key else {
                return 0;
            };
            let block = eye.floor().as_ivec3();
            if pos.0.abs_diff(block.x.div_euclid(16)) > u32::from(radius)
                || pos.1.abs_diff(block.z.div_euclid(16)) > u32::from(radius)
            {
                return 0;
            }
            let mut mask = 0;
            for y in 0..16 {
                let bit = 1 << y;
                if present & bit == 0 {
                    continue;
                }
                let min = Vec3::new((pos.0 * 16) as f32, (y * 16) as f32, (pos.1 * 16) as f32);
                if frustum.visible(min, min + Vec3::splat(16.)) {
                    mask |= bit;
                }
            }
            mask
        } else {
            self.reached.get(&pos).copied().unwrap_or_default() & present
        }
    }
}

struct SceneGraph {
    graph: Box<Graph>,
    original: Box<[Section;16]>,
}
impl SceneGraph {
    fn new(chunk:&Chunk,fancy:bool)->Self {
        Self { graph:TerrainSections::new(chunk,fancy).graph,
            original:super::graph_layout_tests::original(chunk,fancy) }
    }
}
struct Scene {
    open: SceneGraph,
    wall: SceneGraph,
    solid: SceneGraph,
    cavity: SceneGraph,
}
impl Scene {
    fn new() -> Self {
        let mut wall=Chunk::empty();
        for y in 0..256 { for z in 0..16 {wall.set(8,y,z,16);} }
        let mut solid=Chunk::empty();
        solid.blocks.fill(16); solid.top=256;
        let mut cavity=solid.clone();
        for y in 1..15 { for z in 1..15 {for x in 1..15 {cavity.set(x,y,z,0);}} }
        Self {open:SceneGraph::new(&Chunk::empty(),true),
            wall:SceneGraph::new(&wall,true), solid:SceneGraph::new(&solid,true),
            cavity:SceneGraph::new(&cavity,true)}
    }
    fn select(&self,p:ChunkPos,origin:ChunkPos,mode:u8)->Option<&SceneGraph> {
        if p==origin {
            return match mode {1=>None,2=>Some(&self.solid),3=>Some(&self.cavity),
                4=>Some(&self.wall),_=>Some(&self.open)};
        }
        let code=(i64::from(p.0)*31+i64::from(p.1)*17).rem_euclid(11);
        match code {0|1=>None,2|3=>Some(&self.wall),4=>Some(&self.solid),
            _=>Some(&self.open)}
    }
    fn graph(&self,p:ChunkPos,origin:ChunkPos,mode:u8)->Option<&Graph> {
        self.select(p,origin,mode).map(|entry|entry.graph.as_ref())
    }
    fn original_graph(&self,p:ChunkPos,origin:ChunkPos,mode:u8)->Option<&[Section;16]> {
        self.select(p,origin,mode).map(|entry|entry.original.as_ref())
    }
}
fn compare_queries(actual:&View,expected:&OriginalView,frustum:&Frustum,origin:ChunkPos,radius:u8) {
    let r=i32::from(radius);
    let mut columns=expected.reached.keys().copied().collect::<Vec<_>>();
    // Exhaust the small/normal view and all section bits; for radius128 the
    // projection is deliberately short so the normal test stays bounded.
    let span=r.min(25)+1;
    for z in -span..=span { for x in -span..=span {columns.push((origin.0+x,origin.1+z));} }
    for dx in [-r-1,-r,0,r,r+1] {for dz in [-r-1,-r,0,r,r+1] {
        columns.push((origin.0+dx,origin.1+dz));
    }}
    columns.extend([(i32::MIN,i32::MAX),(i32::MAX,i32::MIN)]);
    for p in columns {
        for present in [0,1,0x8000,0x5555,0xaaaa,u16::MAX] {
            assert_eq!(actual.mask(p,frustum,present),expected.mask(p,frustum,present),
                "mask column={p:?} present={present:x} radius={radius}");
        }
        for y in 0..16 {let p=IVec3::new(p.0,y,p.1);
            assert_eq!(actual.order(p),expected.order(p),"boss visit order {p:?}");
        }
    }
    assert_eq!(actual.frustum_only,expected.frustum_only);
    assert_eq!(actual.camera_exits,expected.camera_exits);
    assert!(actual.pending.iter().map(|&(p,f,t)|(p,f.map(usize::from),t))
        .eq(expected.pending.iter().copied()),"the exact traversal queue must be unchanged");
    assert_eq!(actual.render_order,expected.render_order);
    if !actual.frustum_only && actual.reached_side!=0 {
        let center=actual.reached_side/2;
        let first=actual.seen[center*(actual.reached_side+1)];
        assert_ne!(first>>16,0,"camera section must survive the no-exits early return");
    }
}
fn compare_update(actual:&mut View,expected:&mut OriginalView,scene:&Scene,
    eye:Vec3,direction:Vec3,radius:u8,mode:u8) {
    let matrix=Mat4::perspective_rh(1.1,1.3,0.05,if radius==128 {96.}else{600.})
        *Mat4::look_to_rh(eye,direction,Vec3::Y);
    let frustum=Frustum::new(matrix);
    let block=eye.floor().as_ivec3();
    let origin=(block.x.div_euclid(16),block.z.div_euclid(16));
    let actual_calls=RefCell::new(Vec::new());
    let expected_calls=RefCell::new(Vec::new());
    actual.update(matrix,&frustum,eye,direction,radius,|p|{
        actual_calls.borrow_mut().push(p);scene.graph(p,origin,mode)
    });
    expected.update(matrix,&frustum,eye,direction,radius,|p|{
        expected_calls.borrow_mut().push(p);scene.original_graph(p,origin,mode)
    });
    assert_eq!(actual_calls.into_inner(),expected_calls.into_inner(),
        "no graph callback may be reordered, elided or added");
    compare_queries(actual,expected,&frustum,origin,radius);
    // An unchanged camera key must retain the existing cache even if the graph
    // supplied by the caller would now differ, until invalidate is requested.
    if matrix.is_finite() && eye.is_finite() && direction.is_finite() {
        actual.update(matrix,&frustum,eye,direction,radius,|_|panic!("cached actual view"));
        expected.update(matrix,&frustum,eye,direction,radius,|_|panic!("cached original view"));
    }
}
#[test]
fn packed_reached_masks_match_original_traversal_calls_order_and_camera_boundaries() {
    let scene=Scene::new();
    let (mut actual,mut expected)=(View::default(),OriginalView::default());
    // X/Z and Y boundaries, negative columns, inside-solid F5/spectator camera,
    // absent camera graph, sealed cavity, and normal16/24 near-ring traversal.
    for radius in [0,1,4,16,24,128] {
        for (i,eye) in [Vec3::new(8.5,8.5,8.5),Vec3::new(-0.01,15.99,-16.01),
            Vec3::new(16.,16.,-16.),Vec3::new(-32.,255.99,16.),
            Vec3::new(0.,-0.01,0.),Vec3::new(0.,256.,0.)].into_iter().enumerate() {
            for mode in 0..=4 {
                actual.invalidate();expected.invalidate();
                let tracking=(i+mode as usize)%2==0;
                actual.track_order(tracking);expected.track_order(tracking);
                let direction=[Vec3::X,Vec3::NEG_Z,Vec3::new(0.5,0.1,-1.).normalize()][i%3];
                compare_update(&mut actual,&mut expected,&scene,eye,direction,radius,mode);
            }
        }
    }
}
#[test]
fn packed_reached_cache_preserves_invalidation_radius_changes_and_extreme_queries() {
    let scene=Scene::new();
    let (mut actual,mut expected)=(View::default(),OriginalView::default());
    let eye=Vec3::new(8.5,8.5,8.5);
    let matrix=Mat4::perspective_rh(1.1,1.3,0.05,600.)*Mat4::look_to_rh(eye,Vec3::X,Vec3::Y);
    let frustum=Frustum::new(matrix);
    compare_queries(&actual,&expected,&frustum,(0,0),0);
    for (radius,mode,tracking) in [(24,0,true),(1,1,false),(0,3,true),(16,2,true),(4,4,false)] {
        actual.track_order(tracking);expected.track_order(tracking);
        // invalidate historically clears the key/exits, not the last observable
        // mask/order. Keep that intermediate API behavior exactly.
        actual.invalidate();expected.invalidate();
        compare_queries(&actual,&expected,&frustum,(0,0),24);
        compare_update(&mut actual,&mut expected,&scene,eye,Vec3::X,radius,mode);
    }
    for eye in [Vec3::new(-29_999_984.,64.,29_999_984.),Vec3::new(29_999_984.,255.,-29_999_984.)] {
        actual.invalidate();expected.invalidate();
        compare_update(&mut actual,&mut expected,&scene,eye,Vec3::NEG_Z,4,0);
    }
    // Invalid/nonfinite height selects the original conservative fallback.
    // No new clamping, coordinate wrapping, or changed frustum logic is added.
    for y in [f32::NEG_INFINITY,f32::INFINITY,f32::NAN] {
        actual.invalidate();expected.invalidate();
        compare_update(&mut actual,&mut expected,&scene,Vec3::new(0.,y,0.),Vec3::Z,1,1);
    }
}

#[path="section_visibility_pre_acquire_tests.rs"]
mod pre_acquire_tests;

#[path = "section_visibility_packed_frustum_tests.rs"]
mod packed_frustum_tests;
