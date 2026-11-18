//! Test-only offline replay using the real production AoS and sorting method.
use super::*;
use super::super::transparency_capture::{self as capture,Before,Collector,Record};
fn output_code(result:Sort)->u8{match result{Sort::Skipped=>0,Sort::Identical=>1,Sort::Changed=>2}}
fn keys(order:&Order)->&[u64]{let DistanceKeys::Packed(keys)=&order.by_distance else{panic!("packed fixture")};keys}
fn clone_capacity<T:Clone>(values:&[T],capacity:usize)->Vec<T>{let mut v=Vec::with_capacity(capacity.max(values.len()));v.extend_from_slice(values);v}
fn restore(record:&Record)->Order {
    Order{quads:clone_capacity(&record.quads,record.capacities[0]),
        by_distance:DistanceKeys::Packed(clone_capacity(&record.old_keys,record.capacities[1])),
        indices:clone_capacity(&record.old_indices,record.capacities[2]),narrow_indices:Vec::with_capacity(record.capacities[3]),
        eye:record.previous_eye.map(|p|Vec3::from_array(p.map(f32::from_bits))),mask:record.old_mask,
        force_wide:false}
}
fn replay(record:&Record) {
    let mut order=restore(record);let eye=Vec3::from_array(record.eye.map(f32::from_bits));
    let result=order.sort(eye,record.mask);
    assert_eq!(output_code(result),record.result,"capture frame{} result",record.frame);
    assert_eq!(keys(&order),record.keys,"capture frame{} keys",record.frame);
    assert_eq!(order.indices,record.indices,"capture frame{} indices",record.frame);
    // Independent full original tuple sort, including skipped-eye threshold.
    let skipped=record.quads.is_empty()||(record.old_mask==record.mask&&record.previous_eye
        .map(|p|Vec3::from_array(p.map(f32::from_bits))).is_some_and(|p|p.distance_squared(eye)<=1.));
    if skipped {assert_eq!(record.result,0);assert_eq!(record.old_keys,record.keys);assert_eq!(record.old_indices,record.indices);}
    else {
        let mut original=record.quads.iter().enumerate().filter(|(_,q)|record.mask&(1<<q.section)!=0)
            .map(|(i,q)|(std::cmp::Reverse(q.center.distance_squared(eye).to_bits()),i)).collect::<Vec<_>>();
        original.sort_unstable();let wanted=original.iter().flat_map(|&(_,i)|record.quads[i].indices).collect::<Vec<_>>();
        let wanted_keys=original.iter().map(|&(d,i)|(u64::from(!d.0)<<32)|i as u64).collect::<Vec<_>>();
        assert_eq!(wanted_keys,record.keys);assert_eq!(wanted,record.indices);
        assert_eq!(record.result,if wanted==record.old_indices{1}else{2});
    }
}
#[test]
fn captured_real_aos_roundtrips_previous_order_membership_ties_and_gpu_widths(){
    assert_eq!(std::mem::size_of::<Quad>(),40,"profile the actual production AoS");
    let mut collector=Collector::new(true);
    for wide in [false,true] {
        let mut order=Order{quads:(0..768).map(|i|{
            let base=i as u32*4+if wide{65536}else{0};
            Quad{center:Vec3::new((i%16) as f32,(i%11) as f32,(i%23) as f32),
                indices:[base,base+1,base+2,base,base+2,base+3],section:(i%16) as u8}
        }).collect(),..Default::default()};
        order.by_distance.reserve(order.quads.len());order.indices.reserve(order.quads.len()*6);
        if !wide{order.narrow_indices.reserve(order.quads.len()*6);}
        let path=[(Vec3::ZERO,u16::MAX),(Vec3::X*2.,u16::MAX),(Vec3::X*2.,0),
            (Vec3::splat(3.),0xaaaa),(Vec3::splat(-2.),u16::MAX),
            (Vec3::splat(f32::NAN),u16::MAX),(Vec3::splat(f32::INFINITY),0x5555)];
        for (index,(eye,mask)) in path.into_iter().enumerate(){
            let slot=usize::from(wide)*7+index;collector.measured_time(Some(100.+slot as f64));collector.frame(slot as u64,slot as f32);
            let before=Before{position:(slot as i32,-3),previous_eye:order.eye.map(|p|p.to_array().map(f32::to_bits)),
                eye:eye.to_array().map(f32::to_bits),old_mask:order.mask,mask,index_stride:if wide{4}else{2},solid_count:12,
                capacities:[order.quads.capacity(),match &order.by_distance{DistanceKeys::Packed(k)=>k.capacity(),_=>0},order.indices.capacity(),order.narrow_indices.capacity()],
                quads:&order.quads,keys:Some(keys(&order)),indices:&order.indices};
            assert!(order.needs_sort(eye,mask));let ticket=collector.begin(before).unwrap();
            let result=order.sort(eye,mask);collector.finish(ticket,output_code(result),keys(&order),&order.indices);
        }
    }
    let records=capture::decode(capture::data(&collector)).unwrap();assert_eq!(records.len(),14);
    for record in records {replay(&record);assert!(matches!(record.index_stride,2|4));assert_eq!(record.solid_count,12);}
}
#[test]
#[ignore="offline exact replay only; set FERRUM_TRANSPARENCY_CAPTURE to an opt-in capture file"]
fn replay_actual_transparency_inputs_with_production_aos(){
    let path=std::path::PathBuf::from(std::env::var_os("FERRUM_TRANSPARENCY_CAPTURE").expect("capture path"));
    let metadata=std::fs::metadata(&path).unwrap();assert!(metadata.len()<=4*1024*1024,"bounded capture file");
    let bytes=std::fs::read(path).unwrap();let records=capture::decode(&bytes).unwrap();assert!(!records.is_empty(),"capture contains no admitted sort calls");
    for record in &records {replay(record);}
    eprintln!("Replayed{} exact production-AoS cases (Quad={}bytes); no sort algorithm changed",records.len(),std::mem::size_of::<Quad>());
}
