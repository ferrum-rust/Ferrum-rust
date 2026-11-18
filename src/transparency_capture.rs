//! Optional bounded sort-input evidence. No native objects or default-run I/O.
use super::chunk_transparency::Quad;
use crate::world::ChunkPos;
use std::{cell::Cell,path::Path};
const MAGIC:&[u8;8]=b"FRTCAP01";
const BUDGET:usize=4*1024*1024;
const RECORD_LIMIT:usize=512*1024;
const CASES:usize=16;
const HEADER:usize=16;
#[derive(Clone,Copy,PartialEq,Eq)]
struct Signature(ChunkPos,u16,u16);
#[derive(Default,serde::Serialize)]
struct Stats { captured:usize, outside_window:u64, occupied_second:u64, duplicate:u64,
    unsupported_wide:u64, invalid_shape:u64, oversized:u64, budget_exhausted:u64, invalid_result:u64 }
struct State { bytes:Vec<u8>, stats:Stats, used:[bool;CASES], signatures:[Option<Signature>;CASES],
    measured:Option<f64>, frame:u64, application:f32, last_frame:Option<u64>, written:Cell<bool> }
#[derive(Default)]
pub(super) struct Collector { state:Option<State> }
pub(super) struct Before<'a> {
    pub position:ChunkPos,pub previous_eye:Option<[u32;3]>,pub eye:[u32;3],
    pub old_mask:u16,pub mask:u16,pub index_stride:u8,pub solid_count:u32,
    // Quad/key/u32-index/u16-index element capacities, not byte counts.
    pub capacities:[usize;4],pub quads:&'a[Quad],pub keys:Option<&'a[u64]>,pub indices:&'a[u32],
}
pub(super) struct Ticket { start:usize,bound:usize,slot:usize,signature:Signature,n:usize,frame:u64 }
fn record_bound(n:usize,k:usize,i:usize)->Option<usize> {
    256usize.checked_add(n.checked_mul(69)?)?.checked_add(k.checked_mul(8)?)?.checked_add(i.checked_mul(4)?)
}
fn capacities_valid(c:[usize;4])->bool {
    [40usize,8,4,2].into_iter().zip(c).all(|(bytes,n)|n.checked_mul(bytes).is_some_and(|n|n<=RECORD_LIMIT))
}
fn u16_out(b:&mut Vec<u8>,v:u16){b.extend_from_slice(&v.to_le_bytes());}
fn u32_out(b:&mut Vec<u8>,v:u32){b.extend_from_slice(&v.to_le_bytes());}
fn u64_out(b:&mut Vec<u8>,v:u64){b.extend_from_slice(&v.to_le_bytes());}
impl Collector {
    pub fn new(enabled:bool)->Self {
        if !enabled {return Self::default();}
        let mut bytes=Vec::with_capacity(BUDGET);bytes.extend_from_slice(MAGIC);
        u32_out(&mut bytes,1);u32_out(&mut bytes,0);
        Self{state:Some(State{bytes,stats:Stats::default(),used:[false;CASES],signatures:[None;CASES],
            measured:None,frame:0,application:0.,last_frame:None,written:Cell::new(false)})}
    }
    pub fn enabled(&self)->bool{self.state.is_some()}
    pub fn measured_time(&mut self,time:Option<f64>){if let Some(s)=&mut self.state{s.measured=time;}}
    pub fn frame(&mut self,frame:u64,application:f32){if let Some(s)=&mut self.state{s.frame=frame;s.application=application;}}
    pub fn begin(&mut self,input:Before<'_>)->Option<Ticket> {
        let s=self.state.as_mut()?;
        let Some(time)=s.measured.filter(|t|t.is_finite()&&(100.0..116.0).contains(t)) else {
            s.stats.outside_window=s.stats.outside_window.saturating_add(1);return None;
        };
        let slot=time.floor() as usize-100;
        if s.used[slot]||s.last_frame==Some(s.frame)||s.stats.captured==CASES {
            s.stats.occupied_second=s.stats.occupied_second.saturating_add(1);return None;
        }
        let signature=Signature(input.position,input.old_mask,input.mask);
        if s.signatures.contains(&Some(signature)){s.stats.duplicate=s.stats.duplicate.saturating_add(1);return None;}
        let Some(keys)=input.keys else{s.stats.unsupported_wide=s.stats.unsupported_wide.saturating_add(1);return None;};
        let n=input.quads.len();
        if n==0||u32::try_from(n).is_err()||keys.len()>n||input.indices.len()%6!=0
            ||input.indices.len()>n.saturating_mul(6)||!matches!(input.index_stride,2|4)
            ||!capacities_valid(input.capacities)||input.capacities[0]<n
            ||input.capacities[1]<keys.len()||input.capacities[2]<input.indices.len() {
            s.stats.invalid_shape=s.stats.invalid_shape.saturating_add(1);return None;
        }
        let Some(bound)=record_bound(n,keys.len(),input.indices.len()).filter(|n|*n<=RECORD_LIMIT) else {
            s.stats.oversized=s.stats.oversized.saturating_add(1);return None;
        };
        if s.bytes.len().checked_add(bound).is_none_or(|n|n>BUDGET) {
            s.stats.budget_exhausted=s.stats.budget_exhausted.saturating_add(1);return None;
        }
        let b=&mut s.bytes;let start=b.len();u32_out(b,0);
        u64_out(b,s.frame);u32_out(b,s.application.to_bits());u64_out(b,time.to_bits());
        u32_out(b,input.position.0 as u32);u32_out(b,input.position.1 as u32);
        b.push(u8::from(input.previous_eye.is_some()));for v in input.previous_eye.unwrap_or([0;3]){u32_out(b,v);}
        for v in input.eye{u32_out(b,v);}u16_out(b,input.old_mask);u16_out(b,input.mask);
        b.push(input.index_stride);u32_out(b,input.solid_count);
        for n in input.capacities{u64_out(b,n as u64);}
        for count in [n,keys.len(),input.indices.len()]{u32_out(b,count as u32);}
        for q in input.quads {
            for v in q.center.to_array().map(f32::to_bits){u32_out(b,v);}
            for v in q.indices{u32_out(b,v);}b.push(q.section);
        }
        for &key in keys{u64_out(b,key);}for &index in input.indices{u32_out(b,index);}
        Some(Ticket{start,bound,slot,signature,n,frame:s.frame})
    }
    pub fn finish(&mut self,ticket:Ticket,result:u8,keys:&[u64],indices:&[u32]) {
        let Some(s)=&mut self.state else{return;};
        let post=9usize.checked_add(keys.len().saturating_mul(8)).and_then(|n|n.checked_add(indices.len().saturating_mul(4)));
        if result>2||keys.len()>ticket.n||indices.len()!=keys.len().saturating_mul(6)
            ||post.and_then(|n|s.bytes.len().checked_add(n)).is_none_or(|end|end>ticket.start+ticket.bound||end>BUDGET) {
            s.bytes.truncate(ticket.start);s.stats.invalid_result=s.stats.invalid_result.saturating_add(1);return;
        }
        let b=&mut s.bytes;b.push(result);u32_out(b,keys.len() as u32);u32_out(b,indices.len() as u32);
        for &key in keys{u64_out(b,key);}for &index in indices{u32_out(b,index);}
        let size=(b.len()-ticket.start) as u32;b[ticket.start..ticket.start+4].copy_from_slice(&size.to_le_bytes());
        s.signatures[s.stats.captured]=Some(ticket.signature);s.stats.captured+=1;
        s.used[ticket.slot]=true;s.last_frame=Some(ticket.frame);
        b[12..16].copy_from_slice(&(s.stats.captured as u32).to_le_bytes());
        debug_assert!(b.len()<=BUDGET);
    }
    pub fn report(&self)->serde_json::Value {
        match &self.state {
            None=>serde_json::json!({"enabled":false,"diagnostic_not_baseline":false}),
            Some(s)=>serde_json::json!({"enabled":true,"diagnostic_not_baseline":true,
                "file":"transparency-inputs.bin","version":1,"cases":s.stats.captured,"bytes":s.bytes.len(),
                "budget_bytes":BUDGET,"record_limit_bytes":RECORD_LIMIT,"stats":s.stats,
                "note":"Opt-in evidence copies contaminate timings; do not treat this run as a clean performance baseline. At most one distinct chunk/old-mask/new-mask case per measured second in [100,116), at most16 total. Rejected oversized/invalid/duplicate cases remain unsampled. Float bits are stored exactly; no raw Rust padding or native references."})
        }
    }
    pub fn write_once(&self,path:&Path)->anyhow::Result<()> {
        if let Some(s)=&self.state {if !s.written.get(){std::fs::write(path,&s.bytes)?;s.written.set(true);}}
        Ok(())
    }
}

#[cfg(test)]
pub(super) struct Record {
    pub position:ChunkPos,pub frame:u64,pub application_bits:u32,pub measured_bits:u64,
    pub previous_eye:Option<[u32;3]>,pub eye:[u32;3],pub old_mask:u16,pub mask:u16,
    pub index_stride:u8,pub solid_count:u32,pub capacities:[usize;4],pub quads:Vec<Quad>,
    pub old_keys:Vec<u64>,pub old_indices:Vec<u32>,pub result:u8,pub keys:Vec<u64>,pub indices:Vec<u32>,
}
#[cfg(test)]
pub(super) fn decode(bytes:&[u8])->anyhow::Result<Vec<Record>> {
    use anyhow::{ensure,Context};
    struct Read<'a>{bytes:&'a[u8],at:usize}
    impl Read<'_>{
        fn take(&mut self,n:usize)->anyhow::Result<&[u8]>{let end=self.at.checked_add(n).context("capture offset")?;
            let b=self.bytes.get(self.at..end).context("truncated capture")?;self.at=end;Ok(b)}
        fn u8(&mut self)->anyhow::Result<u8>{Ok(self.take(1)?[0])}
        fn u16(&mut self)->anyhow::Result<u16>{Ok(u16::from_le_bytes(self.take(2)?.try_into()?))}
        fn u32(&mut self)->anyhow::Result<u32>{Ok(u32::from_le_bytes(self.take(4)?.try_into()?))}
        fn u64(&mut self)->anyhow::Result<u64>{Ok(u64::from_le_bytes(self.take(8)?.try_into()?))}
        fn eye(&mut self)->anyhow::Result<[u32;3]>{Ok([self.u32()?,self.u32()?,self.u32()?])}
        fn keys(&mut self,n:usize)->anyhow::Result<Vec<u64>>{ensure!(n<=self.bytes.len().saturating_sub(self.at)/8,"key count");(0..n).map(|_|self.u64()).collect()}
        fn indices(&mut self,n:usize)->anyhow::Result<Vec<u32>>{ensure!(n<=self.bytes.len().saturating_sub(self.at)/4,"index count");(0..n).map(|_|self.u32()).collect()}
    }
    ensure!((HEADER..=BUDGET).contains(&bytes.len()),"capture size");
    let mut r=Read{bytes,at:0};ensure!(r.take(8)?==MAGIC&&r.u32()?==1,"capture header");
    let count=r.u32()? as usize;ensure!(count<=CASES,"capture count");let mut records=Vec::with_capacity(count);
    for _ in 0..count {
        let start=r.at;let len=r.u32()? as usize;ensure!((4..=RECORD_LIMIT).contains(&len),"record size");
        let end=start.checked_add(len).context("record overflow")?;let payload=bytes.get(r.at..end).context("record end")?;
        let mut p=Read{bytes:payload,at:0};let frame=p.u64()?;let application_bits=p.u32()?;let measured_bits=p.u64()?;
        let position=(p.u32()? as i32,p.u32()? as i32);let has_eye=p.u8()?;ensure!(has_eye<=1,"eye tag");
        let old_eye=p.eye()?;let previous_eye=(has_eye==1).then_some(old_eye);let eye=p.eye()?;
        let old_mask=p.u16()?;let mask=p.u16()?;let index_stride=p.u8()?;ensure!(matches!(index_stride,2|4),"index format");
        let solid_count=p.u32()?;let mut capacities=[0;4];for c in &mut capacities{*c=usize::try_from(p.u64()?)?;}
        ensure!(capacities_valid(capacities),"capacity limit");
        let n=p.u32()? as usize;let k=p.u32()? as usize;let i=p.u32()? as usize;
        ensure!(n>0&&k<=n&&i%6==0&&i<=n.saturating_mul(6)&&n<=payload.len()/37,"input counts");
        ensure!(capacities[0]>=n&&capacities[1]>=k&&capacities[2]>=i,"input capacities");
        let mut quads=Vec::with_capacity(n);for _ in 0..n {
            let center=glam::Vec3::from_array(p.eye()?.map(f32::from_bits));
            let indices=[p.u32()?,p.u32()?,p.u32()?,p.u32()?,p.u32()?,p.u32()?];
            let section=p.u8()?;ensure!(section<16,"section");quads.push(Quad{center,indices,section});
        }
        let old_keys=p.keys(k)?;let old_indices=p.indices(i)?;let result=p.u8()?;ensure!(result<=2,"sort result");
        let after_k=p.u32()? as usize;let after_i=p.u32()? as usize;
        ensure!(after_k<=n&&after_i==after_k.saturating_mul(6),"output counts");
        let keys=p.keys(after_k)?;let indices=p.indices(after_i)?;
        ensure!(p.at==payload.len(),"record trailing data");
        ensure!(old_keys.iter().chain(&keys).all(|k|(*k as u32 as usize)<n),"quad index");
        records.push(Record{position,frame,application_bits,measured_bits,previous_eye,eye,old_mask,mask,index_stride,
            solid_count,capacities,quads,old_keys,old_indices,result,keys,indices});r.at=end;
    }
    ensure!(r.at==bytes.len(),"capture trailing data");Ok(records)
}
#[cfg(test)]
pub(super) fn data(capture:&Collector)->&[u8]{capture.state.as_ref().map_or(&[],|s|s.bytes.as_slice())}

#[cfg(test)]
mod tests {
    use super::*;use glam::Vec3;
    fn input<'a>(p:ChunkPos,q:&'a[Quad],keys:&'a[u64],indices:&'a[u32])->Before<'a>{Before{
        position:p,previous_eye:Some([0x80000000,0x7fc01234,0x7f800000]),eye:[1,2,3],old_mask:3,mask:5,
        index_stride:2,solid_count:18,capacities:[q.len(),keys.len(),indices.len(),indices.len()],quads:q,keys:Some(keys),indices}}
    fn quads(n:usize)->Vec<Quad>{(0..n).map(|i|Quad{center:Vec3::from_array([f32::from_bits(0x7fc00123),-0.,i as f32]),indices:[i as u32;6],section:(i%16) as u8}).collect()}
    #[test]
    fn capture_option_defaults_off_and_requires_streaming_benchmark(){
        use clap::Parser;
        for args in [vec!["ferrum","--benchmark-capture-transparency"],
            vec!["ferrum","--benchmark","--benchmark-capture-transparency"],
            vec!["ferrum","--benchmark-streaming","--benchmark-capture-transparency"]] {
            assert!(crate::Options::try_parse_from(args).is_err());
        }
        let plain=crate::Options::try_parse_from(["ferrum","--benchmark","--benchmark-streaming"]).unwrap();
        assert!(!plain.benchmark_capture_transparency);
        let enabled=crate::Options::try_parse_from(["ferrum","--benchmark","--benchmark-streaming","--benchmark-capture-transparency"]).unwrap();
        assert!(enabled.benchmark_capture_transparency);
    }
    #[test]
    fn capture_disabled_is_empty_and_float_bit_wire_roundtrips_without_padding(){
        let q=quads(3);let keys=vec![0x1234567800000002,0xffffffff00000000,1];let indices=vec![17;18];
        let mut disabled=Collector::new(false);disabled.measured_time(Some(100.));disabled.frame(7,2.5);
        assert!(disabled.begin(input((1,-2),&q,&keys,&indices)).is_none());assert!(disabled.state.is_none());
        assert!(disabled.write_once(Path::new("nonexistent-capture-parent/disabled.bin")).is_ok());
        let mut c=Collector::new(true);c.measured_time(Some(100.25));c.frame(7,2.5);
        let cap=c.state.as_ref().unwrap().bytes.capacity();let ticket=c.begin(input((1,-2),&q,&keys,&indices)).unwrap();
        c.finish(ticket,2,&keys,&indices);assert_eq!(c.state.as_ref().unwrap().bytes.capacity(),cap);
        let rows=decode(data(&c)).unwrap();let r=&rows[0];assert_eq!((r.position,r.frame,r.application_bits,r.measured_bits),((1,-2),7,2.5f32.to_bits(),100.25f64.to_bits()));
        assert_eq!(r.previous_eye,Some([0x80000000,0x7fc01234,0x7f800000]));assert_eq!(r.eye,[1,2,3]);
        for(a,b)in q.iter().zip(&r.quads){assert_eq!(a.center.to_array().map(f32::to_bits),b.center.to_array().map(f32::to_bits));assert_eq!((a.indices,a.section),(b.indices,b.section));}
        assert_eq!(r.old_keys,keys);assert_eq!(r.old_indices,indices);assert_eq!(r.keys,keys);assert_eq!(r.indices,indices);
        for len in [0,1,15,data(&c).len()-1]{assert!(decode(&data(&c)[..len]).is_err());}
        let mut bad=data(&c).to_vec();bad.push(0);assert!(decode(&bad).is_err());
        let mut bad=data(&c).to_vec();bad[12..16].copy_from_slice(&17u32.to_le_bytes());assert!(decode(&bad).is_err());
    }
    #[test]
    fn capture_time_unique_case_record_and_total_budgets_are_hard_bounds(){
        let q=quads(4000);let keys=(0..q.len() as u64).collect::<Vec<_>>();let indices=vec![0;q.len()*6];let mut c=Collector::new(true);
        let cap=c.state.as_ref().unwrap().bytes.capacity();
        for time in [99.999,116.,f64::NAN,f64::INFINITY]{c.measured_time(Some(time));assert!(c.begin(input((0,0),&q,&keys,&indices)).is_none());}
        for slot in 0..16 {c.measured_time(Some(100.+slot as f64));c.frame(slot as u64,0.);
            if let Some(t)=c.begin(input((slot,0),&q,&keys,&indices)){c.finish(t,1,&keys,&indices);}}
        assert!(data(&c).len()<=BUDGET);assert_eq!(c.state.as_ref().unwrap().bytes.capacity(),cap);
        assert!(c.state.as_ref().unwrap().stats.budget_exhausted>0);let decoded=decode(data(&c)).unwrap();assert!(!decoded.is_empty()&&decoded.len()<16);
        let huge=quads(8192);let large_keys=(0..8192u64).collect::<Vec<_>>();let large_indices=vec![0;8192*6];let mut fresh=Collector::new(true);
        fresh.measured_time(Some(100.));assert!(fresh.begin(input((0,0),&huge,&large_keys,&large_indices)).is_none());assert_eq!(data(&fresh).len(),HEADER);
        let q=quads(1);let mut c=Collector::new(true);
        for slot in 0..16 {c.measured_time(Some(100.+slot as f64));c.frame(slot as u64,0.);
            let t=c.begin(input((slot,0),&q,&[0],&[0;6])).unwrap();c.finish(t,1,&[0],&[0;6]);
            assert!(c.begin(input((slot+1,0),&q,&[0],&[0;6])).is_none());}
        assert_eq!(decode(data(&c)).unwrap().len(),16);
    }
    #[test]
    fn invalid_result_rolls_back_and_duplicate_signature_does_not_consume_next_slot(){
        let q=quads(1);let mut c=Collector::new(true);c.measured_time(Some(100.));c.frame(1,0.);
        let mut invalid=input((0,0),&q,&[0],&[0;6]);invalid.capacities[0]=0;
        assert!(c.begin(invalid).is_none());assert_eq!(data(&c).len(),HEADER);
        let t=c.begin(input((0,0),&q,&[0],&[0;6])).unwrap();c.finish(t,2,&[0,0],&[0;12]);assert_eq!(data(&c).len(),HEADER);
        let t=c.begin(input((0,0),&q,&[0],&[0;6])).unwrap();c.finish(t,1,&[0],&[0;6]);
        c.measured_time(Some(101.));c.frame(2,0.);assert!(c.begin(input((0,0),&q,&[0],&[0;6])).is_none());
        let t=c.begin(input((1,0),&q,&[0],&[0;6])).unwrap();c.finish(t,1,&[0],&[0;6]);assert_eq!(decode(data(&c)).unwrap().len(),2);
    }
}
