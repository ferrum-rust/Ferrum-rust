//! Actual compact View with packed Frustum versus independent original AoS
//! traversal with scalar Frustum. Child of dense_tests for original fixtures.
use super::*;

fn compare(actual:&mut View,expected:&mut OriginalView,scene:&Scene,
    eye:Vec3,direction:Vec3,radius:u8,revision:u8,packed:bool) {
    let matrix=Mat4::perspective_rh(1.1,1.3,0.05,if radius==128 {96.}else{600.})
        *Mat4::look_to_rh(eye,direction,Vec3::Y);
    let scalar=Frustum::new(matrix);
    let candidate=Frustum::new_with_packed(matrix,packed);
    assert!(!scalar.packed_for_test()); assert_eq!(candidate.packed_for_test(),packed);
    let ac=RefCell::new(Vec::new()); let ec=RefCell::new(Vec::new());
    // Fixed graph origin: the camera must not silently move scene contents.
    actual.update(matrix,&candidate,eye,direction,radius,|p|{
        ac.borrow_mut().push(p);scene.graph(p,(0,0),revision)
    });
    expected.update(matrix,&scalar,eye,direction,radius,|p|{
        ec.borrow_mut().push(p);scene.original_graph(p,(0,0),revision)
    });
    assert_eq!(*ac.borrow(),*ec.borrow(),"exact graph callback order");
    assert_eq!(actual.frustum_only,expected.frustum_only);
    assert_eq!(actual.camera_exits,expected.camera_exits);
    assert_eq!(actual.render_order,expected.render_order);
    assert!(actual.pending.iter().map(|&(p,f,t)|(p,f.map(usize::from),t))
        .eq(expected.pending.iter().copied()));
    let b=eye.floor().as_ivec3(); let origin=(b.x.div_euclid(16),b.z.div_euclid(16));
    let r=i32::from(radius); let span=r.min(25)+1;
    let mut columns=expected.reached.keys().copied().collect::<Vec<_>>();
    for z in -span..=span {for x in -span..=span {columns.push((origin.0+x,origin.1+z));}}
    for x in [-r-1,-r,0,r,r+1] {for z in [-r-1,-r,0,r,r+1] {columns.push((origin.0+x,origin.1+z));}}
    columns.extend([(i32::MIN,i32::MAX),(i32::MAX,i32::MIN)]);
    for p in columns {
        for present in [0,1,0x8000,0x5555,0xaaaa,u16::MAX] {
            assert_eq!(actual.mask(p,&candidate,present),expected.mask(p,&scalar,present),
                "column={p:?} radius={radius} revision={revision} packed={packed}");
        }
        for y in 0..16 {let p=IVec3::new(p.0,y,p.1);assert_eq!(actual.order(p),expected.order(p));}
    }
    if matrix.is_finite() && eye.is_finite() && direction.is_finite() {
        // The culling mode is not part of the exact View key: switching mode
        // must preserve cached masks, including frustum-only mask queries.
        let switched=Frustum::new_with_packed(matrix,!packed);
        actual.update(matrix,&switched,eye,direction,radius,|_|panic!("same-key actual callback"));
        expected.update(matrix,&scalar,eye,direction,radius,|_|panic!("same-key original callback"));
        for present in [0,0x5555,u16::MAX] {
            assert_eq!(actual.mask(origin,&switched,present),expected.mask(origin,&scalar,present));
        }
    }
}

#[test]
fn packed_frustum_view_matches_original_callbacks_masks_and_camera_boundaries() {
    let scene=Scene::new();
    for radius in [0,1,4,16,24,128] {
        let (mut actual,mut expected)=(View::default(),OriginalView::default());
        for revision in 0..=4 {
            actual.invalidate(); expected.invalidate();
            for (step,eye) in [Vec3::new(7.2,8.5,8.5),Vec3::new(8.5,8.5,8.5),
                Vec3::new(-0.01,15.99,-16.01),Vec3::new(16.,16.,-16.),
                Vec3::new(-32.,255.99,16.),Vec3::new(0.,-0.01,0.),Vec3::new(0.,256.,0.)]
                .into_iter().enumerate() {
                let tracking=(step+usize::from(revision))%2==0;
                actual.track_order(tracking); expected.track_order(tracking);
                let direction=if step%2==0 {Vec3::X}else{Vec3::new(0.3,-0.1,-1.).normalize()};
                compare(&mut actual,&mut expected,&scene,eye,direction,radius,revision,true);
            }
        }
    }
}

#[test]
fn packed_frustum_view_preserves_publication_reuse_and_nonfinite_histories() {
    let scene=Scene::new();
    let (mut actual,mut expected)=(View::default(),OriginalView::default());
    actual.track_order(true); expected.track_order(true);
    let start=Vec3::new(7.2,8.5,8.5);
    for revision in [0,4,1,2,3,0,4,0] {
        actual.invalidate(); expected.invalidate();
        compare(&mut actual,&mut expected,&scene,start,Vec3::X,4,revision,true);
        // Same exact key and mode changes: no graph invalidation is implied.
        compare(&mut actual,&mut expected,&scene,start,Vec3::X,4,revision,false);
        compare(&mut actual,&mut expected,&scene,start+Vec3::new(0.125,0.,0.125),Vec3::NEG_Z,4,revision,true);
    }
    for eye in [Vec3::new(-29_999_984.,255.99,29_999_984.),
        Vec3::new(29_999_984.,0.,-29_999_984.),Vec3::new(-0.01,f32::NAN,-16.01),
        Vec3::new(0.,f32::INFINITY,0.),Vec3::new(0.,f32::NEG_INFINITY,0.),start] {
        actual.invalidate(); expected.invalidate();
        compare(&mut actual,&mut expected,&scene,eye,Vec3::NEG_Z,4,0,true);
    }
}
