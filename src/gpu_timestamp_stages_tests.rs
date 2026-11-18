//! Independent shape/byte-ledger oracles; no GPU, clock, compiler or runtime fixture.
use super::*;

fn ticket(slot: usize) -> Ticket { Ticket { owner: 73, serial: slot as u64 + 101, slot: slot as u32 } }
fn eye_labels(eye: u8) -> [Label; 7] {
    [Label::WorldBegin(eye),Label::NearBegin(eye),Label::NearEnd(eye),Label::FarEnd(eye),
        Label::SkyDynamicEnd(eye),Label::AlphaEnd(eye),Label::WorldEnd(eye)]
}
fn shape(eyes: u8, post: bool, gui: bool) -> Vec<Label> {
    let mut labels = Vec::new();
    for eye in 0..eyes { labels.extend(eye_labels(eye)); }
    if post { labels.extend([Label::PostBegin,Label::PostEnd]); }
    if gui { labels.extend([Label::GuiBegin,Label::GuiEnd]); }
    labels
}
fn submit_shape(book: &mut Book, slot: usize, eyes: u8, post: bool, gui: bool) -> usize {
    let t = ticket(slot); book.begin(t).unwrap();
    let labels = shape(eyes,post,gui);
    for (i, label) in labels.into_iter().enumerate() {
        assert_eq!(book.mark(t,label,label.placement()).unwrap(), i as u32 + 1);
    }
    let last = book.end(t).unwrap();
    assert_eq!(last as usize, 1 + 7*eyes as usize + 2*usize::from(post) + 2*usize::from(gui));
    book.submitted(t).unwrap(); last as usize + 1
}
fn bytes(raw: &[u64], padding: u8) -> [u8; STRIDE as usize] {
    let mut out = [padding; STRIDE as usize];
    for (chunk,value) in out.chunks_exact_mut(8).zip(raw) { chunk.copy_from_slice(&value.to_ne_bytes()); }
    out
}

#[test]
fn optional_stage_shapes_have_contiguous_exact_prefixes_without_stale_padding() {
    let mut book = Book::new(12).unwrap();
    let mut expected = Vec::new();
    for eyes in [2,0,1] { for post in [true,false] { for gui in [true,false] {
        let slot = expected.len();
        let used = submit_shape(&mut book,slot,eyes,post,gui);
        let raw: Vec<_> = (0..used).map(|i| 10_000*slot as u64 + i as u64*11).collect();
        assert_eq!(book.decode(slot,&bytes(&raw,0xff)),[raw[0],raw[used-1]]);
        expected.push((eyes,post,gui,used,raw));
    }}}
    let report = book.report(true,2.);
    assert_eq!(report["submitted"],12);
    assert_eq!(report["query_count"],20); assert_eq!(report["readback_stride_bytes"],160);
    assert_eq!(report["readback_bytes"],20*1024*1024);
    assert_eq!(report["storage_shared_with_whole_frame"],true);
    for (slot,(eyes,post,gui,used,raw)) in expected.iter().enumerate() {
        let row=&report["rows"][slot];
        assert_eq!(row["row"],slot); assert_eq!(row["eyes"],*eyes);
        assert_eq!(row["mode"],if *eyes==0 {"no_world"} else {"world"});
        assert_eq!(row["postfx"],*post); assert_eq!(row["gui"],*gui);
        assert_eq!(row["used_query_count"],*used);
        let points=row["points"].as_array().unwrap(); assert_eq!(points.len(),*used);
        assert_eq!(points[0]["label"],"whole_begin"); assert!(points[0]["eye"].is_null());
        assert_eq!(points[used-1]["label"],"whole_end"); assert!(points[used-1]["eye"].is_null());
        // Expected labels are a literal protocol list, independent of Pending's state transition match.
        let names=["world_begin","near_begin","near_end","far_end","sky_dynamic_end","alpha_end","world_end"];
        for eye in 0..*eyes as usize { for (n,name) in names.iter().enumerate() {
            let point=&points[1+eye*7+n]; assert_eq!(point["label"],*name); assert_eq!(point["eye"],eye);
        }}
        let mut at=1+7*(*eyes as usize);
        if *post { assert_eq!(points[at]["label"],"post_begin"); assert_eq!(points[at+1]["label"],"post_end"); at+=2; }
        if *gui { assert_eq!(points[at]["label"],"gui_begin"); assert_eq!(points[at+1]["label"],"gui_end"); }
        for (index,point) in points.iter().enumerate() {
            assert_eq!(point["query_index"],index); assert_eq!(point["raw_ticks"],raw[index]);
        }
        let intervals=row["intervals"].as_array().unwrap(); assert_eq!(intervals.len(),used-1);
        for (i,interval) in intervals.iter().enumerate() {
            assert_eq!(interval["from"],i); assert_eq!(interval["to"],i+1);
            assert_eq!(interval["gpu_ms"],22./1_000_000.); assert!(interval["invalid_reason"].is_null());
        }
    }
    assert_eq!(report["invalid_intervals"],0); assert_eq!(report["quantized_zero_intervals"],0);
}

#[test]
fn stage_identity_placement_order_and_open_regions_reject_before_append() {
    let mut book=Book::new(2).unwrap(); let t=ticket(0); book.begin(t).unwrap();
    for bad in [Label::WorldBegin(1),Label::WorldBegin(2),Label::NearBegin(0),Label::PostEnd,Label::GuiEnd] {
        assert!(book.mark(t,bad,bad.placement()).is_err()); assert_eq!(book.pending.unwrap().used,1);
    }
    assert!(book.mark(Ticket{owner:74,..t},Label::GuiBegin,Placement::Encoder).is_err());
    assert!(book.mark(Ticket{serial:t.serial+1,..t},Label::GuiBegin,Placement::Encoder).is_err());
    assert!(book.mark(t,Label::WorldBegin(0),Placement::Pass).is_err());
    book.mark(t,Label::WorldBegin(0),Placement::Encoder).unwrap();
    assert!(book.mark(t,Label::WorldBegin(0),Placement::Encoder).is_err());
    assert!(book.mark(t,Label::NearBegin(0),Placement::Encoder).is_err());
    assert!(book.mark(t,Label::NearBegin(1),Placement::Pass).is_err());
    assert!(book.end(t).is_err()); assert!(book.submitted(t).is_err());
    for label in &eye_labels(0)[1..] { book.mark(t,*label,label.placement()).unwrap(); }
    book.mark(t,Label::PostBegin,Placement::Encoder).unwrap();
    assert!(book.mark(t,Label::GuiBegin,Placement::Encoder).is_err()); assert!(book.end(t).is_err());
    book.mark(t,Label::PostEnd,Placement::Encoder).unwrap();
    assert!(book.mark(t,Label::WorldBegin(1),Placement::Encoder).is_err());
    book.mark(t,Label::GuiBegin,Placement::Encoder).unwrap(); assert!(book.end(t).is_err());
    book.mark(t,Label::GuiEnd,Placement::Encoder).unwrap(); book.end(t).unwrap();
    assert!(book.end(t).is_err()); assert!(book.mark(t,Label::GuiBegin,Placement::Encoder).is_err());
    book.submitted(t).unwrap(); assert!(book.submitted(t).is_err());
    let next=ticket(1); book.begin(next).unwrap();
    assert!(book.mark(t,Label::GuiBegin,Placement::Encoder).is_err());
    assert_eq!(book.pending.unwrap().used,1);
}

#[test]
fn stage_storage_and_query_bounds_do_not_grow_or_publish_partial_rows() {
    assert_eq!(STRIDE,160); assert_eq!(READBACK_BYTES,20*1024*1024);
    assert_eq!((MAX_ROWS as u64-1)*STRIDE+STRIDE,READBACK_BYTES);
    assert_eq!(STRIDE%wgpu::MAP_ALIGNMENT,0);
    assert!(STRIDE<=super::super::RESOLVE_BYTES);
    assert!(Book::new(MAX_ROWS+1).is_err());
    let mut empty=Book::new(0).unwrap(); assert!(empty.begin(ticket(0)).is_err());
    let mut book=Book::new(3).unwrap(); let pointer=book.rows.as_ptr(); let capacity=book.rows.capacity();
    assert_eq!(submit_shape(&mut book,0,2,true,true),20);
    assert_eq!(submit_shape(&mut book,1,0,false,false),2);
    let t=ticket(2); book.begin(t).unwrap(); assert_eq!(book.rows.len(),2);
    let before=book.pending.unwrap(); book.pending.as_mut().unwrap().used=QUERY_COUNT as u8;
    assert!(book.end(t).is_err()); assert_eq!(book.rows.len(),2);
    book.pending=Some(before); book.end(t).unwrap(); book.submitted(t).unwrap();
    assert!(book.begin(ticket(3)).is_err());
    assert_eq!(book.rows.as_ptr(),pointer); assert_eq!(book.rows.capacity(),capacity);
    assert!(book.report(false,1.)["rows"].is_null());
}

#[test]
fn stage_tick_decode_preserves_zero_reversal_large_tick_precision_and_absence() {
    let mut book=Book::new(3).unwrap();
    for row in 0..2 { assert_eq!(submit_shape(&mut book,row,0,false,true),4); }
    assert_eq!(submit_shape(&mut book,2,0,false,false),2);
    let raw=[u64::MAX-10,u64::MAX-4,u64::MAX-5,u64::MAX];
    assert_eq!(book.decode(0,&bytes(&raw,0xab)),[raw[0],raw[3]]);
    book.decode(1,&bytes(&[8,8,8,8],0xcd));
    book.decode(2,&bytes(&[70,60],0xef));
    let report=book.report(true,0.5);
    assert_eq!(report["valid_intervals"],5); assert_eq!(report["invalid_intervals"],2);
    assert_eq!(report["quantized_zero_intervals"],3);
    let row=&report["rows"][0];
    assert_eq!(row["intervals"][0]["gpu_ms"],0.000003);
    assert!(row["intervals"][1]["gpu_ms"].is_null());
    assert_eq!(row["intervals"][1]["invalid_reason"],"reversed_or_unknown_wrap");
    assert_eq!(row["intervals"][2]["gpu_ms"],0.0000025);
    assert_eq!(report["rows"][2]["used_query_count"],2);
    assert_eq!(report["rows"][2]["gui"],false);
    assert_eq!(report["rows"][2]["points"].as_array().unwrap().len(),2);
    assert_eq!(report["rows"][2]["intervals"][0]["invalid_reason"],"reversed_or_unknown_wrap");
}

#[test]
fn stages_keep_whole_ticket_submission_and_capabilities_authoritative() {
    use super::super::{Book as WholeBook,CpuTimes,Phase,Terminal};
    let flags=wgpu::DownlevelFlags::NONBLOCKING_QUERY_RESOLVE;
    let base=wgpu::Features::TIMESTAMP_QUERY|wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
    assert!(super::super::supported_stages(base,flags).is_err());
    let all=base|wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES;
    assert!(super::super::supported_stages(all,wgpu::DownlevelFlags::empty()).is_err());
    assert_eq!(super::super::supported_stages(all|wgpu::Features::MULTI_DRAW_INDIRECT,flags).unwrap(),all);
    for close in [false,true] {
        let mut whole=WholeBook::new(1).unwrap(); let mut stage=Book::new(1).unwrap();
        let t=whole.reserve(9,1.).unwrap(); whole.started(t).unwrap(); stage.begin(t).unwrap();
        stage.mark(t,Label::GuiBegin,Placement::Encoder).unwrap();
        if close {stage.mark(t,Label::GuiEnd,Placement::Encoder).unwrap();stage.end(t).unwrap();whole.ended(t).unwrap();}
        whole.clear_pending_present(); assert_eq!(whole.terminal,Terminal::Failed);
        assert!(whole.start_finish().is_err()); assert!(whole.rows.is_empty()); assert!(stage.rows.is_empty());
    }
    let mut whole=WholeBook::new(1).unwrap();let mut stage=Book::new(1).unwrap();
    let canceled=whole.reserve(1,0.).unwrap();whole.clear_pending_present();
    let t=whole.reserve(3,1.).unwrap(); assert_ne!(t,canceled); assert_eq!(t.slot,canceled.slot);
    assert!(whole.validate(canceled,Phase::Reserved).is_err());
    whole.started(t).unwrap();stage.begin(t).unwrap();stage.end(t).unwrap();whole.ended(t).unwrap();
    stage.validate_submission(t).unwrap();whole.submitted(t,CpuTimes::default()).unwrap();stage.submitted(t).unwrap();
    whole.presented(Some(0),Some(100.),107.).unwrap();
    assert_eq!(whole.rows.len(),stage.rows.len()); assert_eq!(whole.rows[0].stamp.unwrap().sample_index,Some(0));
    assert!(whole.reserve(4,1.).is_none());assert_eq!(whole.overflow,1);
    assert_eq!(stage.rows.len(),1);assert!(stage.pending.is_none());
}
