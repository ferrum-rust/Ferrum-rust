//! Actual Frustum dispatch compared with an independent original predicate.
use super::{frustum_packed::Planes, Frustum};
use glam::{Mat4, Vec3, Vec4};

fn original(planes: &[Vec4; 6], min: Vec3, max: Vec3) -> bool {
    planes.iter().all(|p| {
        let corner = Vec3::new(
            if p.x >= 0.0 { max.x } else { min.x },
            if p.y >= 0.0 { max.y } else { min.y },
            if p.z >= 0.0 { max.z } else { min.z },
        );
        p.truncate().dot(corner) + p.w >= 0.0
    })
}
fn check(planes: [Vec4; 6], min: Vec3, max: Vec3) {
    let expected = original(&planes, min, max);
    for packed in [false, true] {
        let actual = Frustum { planes, packed: packed.then(|| Planes::new(planes)) };
        assert_eq!(actual.visible(min, max), expected,
            "packed={packed} planes={planes:?} min={min:?} max={max:?}");
    }
}
fn word(state: &mut u64) -> u32 {
    *state ^= *state << 13; *state ^= *state >> 7; *state ^= *state << 17;
    (*state >> 16) as u32
}

#[test]
fn packed_frustum_preserves_accepted_nonfinite_witnesses_and_every_plane_boundary() {
    // These are deliberately ACCEPTED: unrelated zero coefficients in a
    // default filler plane must not make nonfinite coverage vacuously false.
    for (plane, min, max) in [
        (Vec4::new(-1.,0.,0.,10.),Vec3::new(1.,0.,0.),Vec3::new(f32::NAN,0.,0.)),
        (Vec4::new(-0.,0.,0.,1.),Vec3::new(f32::NAN,0.,0.),Vec3::new(1.,0.,0.)),
        (Vec4::new(1e20,-1e20,3.14,-3.),Vec3::ONE,Vec3::ONE),
        (Vec4::new(1.,0.,0.,0.),Vec3::new(f32::INFINITY,0.,0.),Vec3::new(f32::INFINITY,0.,0.)),
    ] {
        assert!(original(&[plane;6], min, max));
        check([plane;6], min, max);
    }
    for index in 0..6 {
        for x in [f32::from_bits(1f32.to_bits()-1),1.,f32::from_bits(1f32.to_bits()+1)] {
            let mut planes = [Vec4::W;6]; planes[index] = Vec4::new(1.,0.,0.,-1.);
            let p = Vec3::new(x,0.,0.);
            assert_eq!(original(&planes,p,p), x >= 1.);
            check(planes,p,p);
        }
        for zero in [0.,-0.] {
            let mut planes = [Vec4::W;6]; planes[index] = Vec4::new(zero,zero,zero,zero);
            assert!(original(&planes,Vec3::ZERO,Vec3::ZERO));
            check(planes,Vec3::ZERO,Vec3::ZERO);
        }
    }
}

#[test]
fn packed_frustum_matches_original_for_arbitrary_float_boxes_and_plane_coefficients() {
    let values = [0.,-0.,1.,-1.,f32::MIN_POSITIVE,f32::from_bits(1),-f32::from_bits(1),
        f32::MAX,-f32::MAX,f32::INFINITY,f32::NEG_INFINITY,
        f32::from_bits(0x7fc01234),f32::from_bits(0xffc05678)];
    for index in 0..6 { for &x in &values { for &a in &values {
        let mut planes = [Vec4::W;6]; planes[index] = Vec4::new(x,-0.,1.,a);
        check(planes,Vec3::new(a,-1.,-a),Vec3::new(-a,1.,a));
    } } }
    let mut random = 0x41ac_0987_fedc_1234u64;
    for i in 0..32_768 {
        let next = |s: &mut u64| { let n = word(s);
            if i%2 == 0 { f32::from_bits(n) } else { (n as i32) as f32/65536. } };
        let planes = std::array::from_fn(|_|Vec4::new(next(&mut random),next(&mut random),next(&mut random),next(&mut random)));
        let min = Vec3::new(next(&mut random),next(&mut random),next(&mut random));
        let max = Vec3::new(next(&mut random),next(&mut random),next(&mut random));
        check(planes,min,max); check(planes,max,min);
    }
}

#[test]
fn packed_frustum_is_explicit_and_preserves_constructor_planes_and_pixel_line_bits() {
    assert_eq!(std::mem::size_of::<Planes>(),128);
    assert!(std::mem::size_of::<Frustum>() <= 240, "bounded inline storage, no allocation");
    let mut random = 0x0820_4141_7cab_1234u64;
    let mut matrices = vec![Mat4::IDENTITY];
    for eye in [Vec3::ZERO,Vec3::new(-16.01,72.5,8.2),Vec3::new(29_999_984.,112.,-29_999_984.)] {
        for direction in [Vec3::NEG_Z,Vec3::X,Vec3::new(0.3,-0.1,-1.).normalize()] {
            matrices.push(Mat4::perspective_rh(1.1,16./9.,0.05,2896.3093)
                *Mat4::look_to_rh(eye,direction,Vec3::Y));
        }
    }
    for _ in 0..4096 {
        matrices.push(Mat4::from_cols_array(&std::array::from_fn(|_|f32::from_bits(word(&mut random)))));
    }
    for matrix in matrices {
        let ordinary = Frustum::new(matrix);
        let explicit = Frustum::new_with_packed(matrix,false);
        let packed = Frustum::new_with_packed(matrix,true);
        assert!(!ordinary.packed_for_test()); assert!(!explicit.packed_for_test()); assert!(packed.packed_for_test());
        let r0=matrix.row(0); let r1=matrix.row(1); let r2=matrix.row(2); let r3=matrix.row(3);
        let expected = [r3+r0,r3-r0,r3+r1,r3-r1,r2,r3-r2];
        for actual in [&ordinary,&explicit,&packed] {
            for (a,b) in actual.planes.into_iter().zip(expected) {
                for (a,b) in a.to_array().into_iter().zip(b.to_array()) {
                    if a.is_nan() || b.is_nan() { assert!(a.is_nan() && b.is_nan()); }
                    else { assert_eq!(a.to_bits(),b.to_bits()); }
                }
            }
        }
        for (min,max) in [(Vec3::splat(-1.),Vec3::ONE),
            (Vec3::new(-16.,64.,-32.),Vec3::new(0.,80.,-16.))] {
            assert_eq!(ordinary.visible(min,max),packed.visible(min,max));
        }
    }
    // pixel_line continues to consume only the unchanged original planes.
    let matrix=Mat4::perspective_rh(1.2,1.6,0.05,600.);
    let scalar=Frustum::new(matrix); let packed=Frustum::new_with_packed(matrix,true);
    let bits=|line:Option<[Vec3;4]>|line.map(|v|v.map(|p|p.to_array().map(f32::to_bits)));
    for (a,b) in [(Vec3::new(-1.,0.,-5.),Vec3::new(1.,0.,-3.)),
        (Vec3::new(0.,0.,0.1),Vec3::new(0.2,0.1,-4.)),
        (Vec3::new(0.,0.,1.),Vec3::new(1.,0.,3.)),
        (Vec3::new(0.,0.,-2.),Vec3::new(0.,0.,-2.))] {
        for height in [1,145,1001,1080,2160] {
            assert_eq!(bits(scalar.pixel_line(a,b,height)),bits(packed.pixel_line(a,b,height)));
        }
    }
}
