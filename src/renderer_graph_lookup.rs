//! One immutable render-call lookup scope. View still invokes its graph
//! callback in the original order; only repeated map resolution is skipped.
use crate::{mesh::visibility::Graph, world::ChunkPos};
use glam::Vec3;
use std::cell::Cell;
const MAX_RADIUS: usize = 24;
const SLOTS: usize = (MAX_RADIUS * 2 + 1) * (MAX_RADIUS * 2 + 1);
type Entry<'a> = Option<Option<&'a Graph>>;

pub(super) struct Columns<'a> {
    origin: ChunkPos,
    radius: usize,
    // None=unqueried, Some(None)=missing; missing columns must also be cached.
    slots: [Cell<Entry<'a>>; SLOTS],
}
impl<'a> Columns<'a> {
    pub fn new(eye: Vec3, radius: u8) -> Self {
        // Same floor/cast/Euclidean conversion as View::update, including
        // extreme/nonfinite camera conversion. No subtraction is done in i32.
        let block=eye.floor().as_ivec3();
        Self { origin:(block.x.div_euclid(16),block.z.div_euclid(16)), radius:usize::from(radius),
            slots: [const { Cell::new(None) }; SLOTS] }
    }
    pub fn get(&self, position: ChunkPos,
        resolve: impl FnOnce() -> Option<&'a Graph>) -> Option<&'a Graph> {
        if self.radius > MAX_RADIUS { return resolve(); }
        let radius=self.radius as i64;
        let x=i64::from(position.0)-i64::from(self.origin.0)+radius;
        let z=i64::from(position.1)-i64::from(self.origin.1)+radius;
        let side=self.radius*2+1;
        if !(0..side as i64).contains(&x) || !(0..side as i64).contains(&z) { return resolve(); }
        let cell=&self.slots[z as usize*side+x as usize];
        if let Some(value)=cell.get() { return value; }
        let value=resolve();cell.set(Some(value));value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{mesh::{Frustum,visibility::{TerrainSections,View}},world::Chunk};
    use glam::{IVec3,Mat4};
    use std::{cell::RefCell,collections::HashSet};
    fn same(a:Option<&Graph>,b:Option<&Graph>)->bool {
        match(a,b){(None,None)=>true,(Some(a),Some(b))=>std::ptr::eq(a,b),_=>false}
    }
    #[test]
    fn scoped_columns_distinguish_missing_unqueried_and_fallback_boundaries() {
        let open=TerrainSections::new(&Chunk::empty(),true);
        for (eye,radius) in [(Vec3::new(-0.1,16.,-16.1),0),(Vec3::ZERO,24),
            (Vec3::new(29_999_984.,128.,-29_999_984.),24),(Vec3::splat(f32::INFINITY),24),
            (Vec3::ZERO,25),(Vec3::ZERO,128)] {
            let cache=Columns::new(eye,radius);let o=cache.origin;
            let queries=[o,(o.0+1,o.1),(o.0,o.1+24),(o.0,o.1+25),
                (i32::MIN,i32::MAX),(i32::MAX,i32::MIN),o,(o.0+1,o.1)];
            let mut expected_resolutions=0;let mut seen=HashSet::new();let resolutions=Cell::new(0);
            for p in queries.into_iter().cycle().take(queries.len()*3) {
                let value=if p==o {Some(open.graph.as_ref())}else{None};
                let actual=cache.get(p,||{resolutions.set(resolutions.get()+1);value});
                assert!(same(actual,value));
                let dx=i64::from(p.0)-i64::from(o.0);let dz=i64::from(p.1)-i64::from(o.1);
                let admitted=radius<=24&&dx.abs()<=i64::from(radius)&&dz.abs()<=i64::from(radius);
                if !admitted||seen.insert(p){expected_resolutions+=1;}
                assert_eq!(resolutions.get(),expected_resolutions);
            }
            // A new scope must resolve the changed/missing graph again.
            let next=Columns::new(eye,radius);
            assert!(next.get(o,||None).is_none());
        }
    }
    #[test]
    fn scoped_columns_preserve_complete_view_callback_sequence_masks_and_order() {
        let open=TerrainSections::new(&Chunk::empty(),true);
        let mut blocks=Chunk::empty();for y in 0..256 {for z in 0..16 {blocks.set(8,y,z,16);}}
        let wall=TerrainSections::new(&blocks,true);
        for radius in [0u8,1,4,16,24,25] {for eye in [Vec3::new(-0.1,47.9,-16.1),Vec3::new(24.5,64.,8.5)] {
            let block=eye.floor().as_ivec3();let origin=(block.x.div_euclid(16),block.z.div_euclid(16));
            for mode in 0..3 {
                let resolve=|p:ChunkPos| {
                    if p==origin {if mode==1 {None}else{Some(open.graph.as_ref())}}
                    else if (p.0^p.1)&3==0 {None}
                    else if mode==2&&(p.0&1)==0 {Some(wall.graph.as_ref())}
                    else{Some(open.graph.as_ref())}
                };
                let (mut actual,mut expected)=(View::default(),View::default());
                actual.track_order(true);expected.track_order(true);
                for step in 0..3 {
                    if step==2 {actual.invalidate();expected.invalidate();}
                    let eye=eye+Vec3::new(step as f32*0.25,0.,0.);
                    let matrix=Mat4::perspective_rh(1.1,1.3,0.05,600.)*Mat4::look_to_rh(eye,Vec3::X,Vec3::Y);
                    let frustum=Frustum::new(matrix);let columns=Columns::new(eye,radius);
                    let reference_calls=RefCell::new(Vec::new());let actual_calls=RefCell::new(Vec::new());
                    let misses=Cell::new(0);
                    expected.update(matrix,&frustum,eye,Vec3::X,radius,|p|{reference_calls.borrow_mut().push(p);resolve(p)});
                    actual.update(matrix,&frustum,eye,Vec3::X,radius,|p|{
                        actual_calls.borrow_mut().push(p);columns.get(p,||{misses.set(misses.get()+1);resolve(p)})});
                    assert_eq!(*reference_calls.borrow(),*actual_calls.borrow());
                    let unique=actual_calls.borrow().iter().copied().collect::<HashSet<_>>().len();
                    if radius<=24 {assert_eq!(misses.get(),unique);}else{assert_eq!(misses.get(),actual_calls.borrow().len());}
                    if radius>=4&&radius<=24&&actual_calls.borrow().len()>1 {assert!(misses.get()<actual_calls.borrow().len());}
                    for x in origin.0-i32::from(radius)-1..=origin.0+i32::from(radius)+1 {
                        for z in origin.1-i32::from(radius)-1..=origin.1+i32::from(radius)+1 {
                            for present in [0,1,0xaaaa,0xffff] {assert_eq!(actual.mask((x,z),&frustum,present),expected.mask((x,z),&frustum,present));}
                            for y in 0..16 {let p=IVec3::new(x,y,z);assert_eq!(actual.order(p),expected.order(p));}
                        }
                    }
                }
            }
        }}
    }
}
