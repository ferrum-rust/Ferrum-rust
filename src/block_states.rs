//! Lossless native block storage with a Dense client/publication boundary.
//! Logical block arrays remain ordinary numeric u16 arrays on the wire.
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, io, ops::{Deref, DerefMut, Index, IndexMut}, slice::SliceIndex,
    sync::{Arc, OnceLock, atomic::{AtomicBool, Ordering}}};
const STATES: usize = 65536;
const RAW_BYTES: usize = STATES * std::mem::size_of::<u16>();
#[derive(Clone)]
enum Encoding { Uniform(u16), Runs(Arc<[u8]>) }
impl Encoding {
    fn payload_bytes(&self) -> usize { match self { Self::Uniform(_) => 2, Self::Runs(runs) => runs.len() } }
    fn allocation_bytes(&self) -> usize { match self { Self::Uniform(_) => 0, Self::Runs(runs) => runs.len() } }
}
struct ColdStates { encoded: Encoding, length: usize, read_cache: OnceLock<Vec<u16>> }
enum Storage { Dense(Vec<u16>), Cold(Box<ColdStates>) }
pub(crate) struct BlockStates {
    storage: Storage,
    // Cold accesses/promotions alone set this notice. It survives promotion.
    recent_cold_access: AtomicBool,
    next_eligible_generation: u64,
}
fn invalid(message: &'static str) -> io::Error { io::Error::new(io::ErrorKind::InvalidData,message) }
fn decode(encoded: &Encoding, length: usize) -> io::Result<Vec<u16>> {
    if length != STATES { return Err(invalid("Invalid cold block length")); }
    match encoded {
        Encoding::Uniform(value) => Ok(vec![*value;length]),
        Encoding::Runs(runs) => {
            if runs.is_empty() || runs.len() >= RAW_BYTES || runs.len()%4 != 0 {
                return Err(invalid("Invalid cold block runs"));
            }
            let mut output=Vec::with_capacity(length);
            for run in runs.chunks_exact(4) {
                let count=usize::from(u16::from_le_bytes([run[0],run[1]]));
                let value=u16::from_le_bytes([run[2],run[3]]);
                if count==0 || count>length-output.len() { return Err(invalid("Cold block run overflow")); }
                output.resize(output.len()+count,value);
            }
            if output.len()!=length { return Err(invalid("Short cold block output")); }
            Ok(output)
        }
    }
}
// Same decision and byte ordering as the separately measured shadow codec.
// Arc conversion adds immutable shared backing. Native owner maintenance bounds
// its measured allocation, encode and Dense-release work per publication.
fn encode(raw: &[u16], minimum_saved_bytes: usize) -> Option<Encoding> {
    if raw.len()!=STATES || minimum_saved_bytes>=RAW_BYTES { return None; }
    let mut runs=Vec::new(); let mut start=0;
    while start<raw.len() {
        let value=raw[start]; let mut end=start+1;
        while end<raw.len() && raw[end]==value { end+=1; }
        if start==0 && end==raw.len() {
            return (RAW_BYTES-2>=minimum_saved_bytes).then_some(Encoding::Uniform(value));
        }
        let mut remaining=end-start;
        while remaining!=0 {
            if runs.len()+4>=RAW_BYTES { return None; }
            if runs.len()+4>runs.capacity() {
                let target=runs.capacity().max(512).saturating_mul(2).min(RAW_BYTES);
                runs.reserve_exact(target-runs.len());
            }
            let count=remaining.min(usize::from(u16::MAX)) as u16;
            runs.extend_from_slice(&count.to_le_bytes()); runs.extend_from_slice(&value.to_le_bytes());
            remaining-=usize::from(count);
        }
        start=end;
    }
    if RAW_BYTES-runs.len()<minimum_saved_bytes { return None; }
    Some(Encoding::Runs(runs.into()))
}
impl BlockStates {
    fn from_storage(storage: Storage) -> Self {
        Self { storage, recent_cold_access:AtomicBool::new(false), next_eligible_generation:0 }
    }
    #[inline] pub(crate) fn is_dense(&self)->bool { matches!(&self.storage,Storage::Dense(_)) }
    #[inline] pub(crate) fn is_cold(&self)->bool { matches!(&self.storage,Storage::Cold(_)) }
    #[inline] pub(crate) fn len(&self)->usize {
        match &self.storage { Storage::Dense(values)=>values.len(),Storage::Cold(cold)=>cold.length }
    }
    #[inline] pub(crate) fn is_empty(&self)->bool { self.len()==0 }
    /// Raw resident capacity in states; never hydrates a compressed column.
    pub(crate) fn capacity(&self)->usize {
        match &self.storage {Storage::Dense(values)=>values.capacity(),
            Storage::Cold(cold)=>cold.read_cache.get().map_or(0,Vec::capacity)}
    }
    pub(crate) fn encoded_payload_bytes(&self)->usize {
        match &self.storage {Storage::Dense(_)=>0,Storage::Cold(cold)=>cold.encoded.payload_bytes()}
    }
    pub(crate) fn encoded_allocation_bytes(&self)->usize {
        match &self.storage {Storage::Dense(_)=>0,Storage::Cold(cold)=>cold.encoded.allocation_bytes()}
    }
    pub(crate) fn has_read_cache(&self)->bool {
        matches!(&self.storage,Storage::Cold(cold) if cold.read_cache.get().is_some())
    }
    pub(crate) fn take_recent_cold_access(&mut self)->bool {
        self.recent_cold_access.swap(false,Ordering::Relaxed)
    }
    pub(crate) fn next_eligible_generation(&self)->u64 {self.next_eligible_generation}
    pub(crate) fn set_next_eligible_generation(&mut self,generation:u64) {self.next_eligible_generation=generation;}
    #[inline] pub(crate) fn value_at(&self,index:usize)->u16 {
        match &self.storage {Storage::Dense(values)=>values[index],
            Storage::Cold(cold)=>Self::cold_slice(cold,&self.recent_cold_access)[index]}
    }
    #[inline] pub(crate) fn as_slice(&self)->&[u16] {
        match &self.storage {Storage::Dense(values)=>values,
            Storage::Cold(cold)=>Self::cold_slice(cold,&self.recent_cold_access)}
    }
    #[cold] #[inline(never)]
    fn cold_slice<'a>(cold:&'a ColdStates,notice:&AtomicBool)->&'a[u16] {
        notice.store(true,Ordering::Relaxed);
        cold.read_cache.get_or_init(||decode(&cold.encoded,cold.length)
            .expect("internally encoded blocks must decode exactly"))
    }
    #[inline] pub(crate) fn as_mut_slice(&mut self)->&mut[u16] {
        match &mut self.storage {Storage::Dense(values)=>values,
            storage @ Storage::Cold(_)=>Self::cold_mut_slice(storage,&self.recent_cold_access)}
    }
    #[cold] #[inline(never)]
    fn cold_mut_slice<'a>(storage:&'a mut Storage,notice:&AtomicBool)->&'a mut[u16] {
        let Storage::Cold(cold)=storage else {unreachable!()};
        // Decode first; a failed internal decode must leave storage/notice intact.
        let values=cold.read_cache.take().unwrap_or_else(||decode(&cold.encoded,cold.length)
            .expect("internally encoded blocks must decode exactly"));
        notice.store(true,Ordering::Relaxed);
        *storage=Storage::Dense(values);
        match storage {Storage::Dense(values)=>values,_=>unreachable!()}
    }
    /// Intended for owner-side publication only. Does not mark a block edit,
    /// advance RNG or modify any Chunk index, light state or revision.
    pub(crate) fn promote_to_dense(&mut self)->bool {
        if self.is_dense() {return false;}
        self.as_mut_slice(); true
    }
    /// A transient bulk borrow: neither creates a persistent cache nor marks
    /// recency. The result cannot borrow from the temporary decoded allocation.
    pub(crate) fn try_with_slice<R>(&self,f:impl FnOnce(&[u16])->R)->io::Result<R> {
        match &self.storage {Storage::Dense(values)=>Ok(f(values)),
            Storage::Cold(cold)=>Self::cold_with_slice(cold,f)}
    }
    #[cold] #[inline(never)]
    fn cold_with_slice<R>(cold:&ColdStates,f:impl FnOnce(&[u16])->R)->io::Result<R> {
        if let Some(values)=cold.read_cache.get(){Ok(f(values))}
        else {let values=decode(&cold.encoded,cold.length)?;Ok(f(&values))}
    }
    pub(crate) fn with_slice<R>(&self,f:impl FnOnce(&[u16])->R)->R {
        self.try_with_slice(f).expect("internally encoded blocks must decode exactly")
    }
    /// No automatic admission here. Native maintenance must first obtain
    /// Arc::get_mut on the OUTER Chunk and satisfy view/dirty/cooldown budgets.
    /// Failed/unprofitable attempts retain the exact Dense Vec allocation.
    pub(crate) fn compress_if_saving(&mut self,minimum_saved_bytes:usize)->bool {
        let Storage::Dense(values)=&self.storage else {return false;};
        let Some(encoded)=encode(values,minimum_saved_bytes) else {return false;};
        self.storage=Storage::Cold(Box::new(ColdStates{encoded,length:STATES,read_cache:OnceLock::new()}));
        true
    }
    /// Exclusive owner maintenance only; references cannot survive this borrow.
    pub(crate) fn release_read_cache(&mut self)->usize {
        match &mut self.storage {Storage::Dense(_)=>0,
            Storage::Cold(cold)=>cold.read_cache.take().map_or(0,|v|v.capacity()*2)}
    }
    pub(crate) fn into_vec(self)->Vec<u16> {
        match self.storage {Storage::Dense(values)=>values,Storage::Cold(cold)=>Self::cold_into_vec(*cold)}
    }
    #[cold] #[inline(never)]
    fn cold_into_vec(cold:ColdStates)->Vec<u16> {
        let ColdStates{encoded,length,read_cache}=cold;
        read_cache.into_inner().unwrap_or_else(||decode(&encoded,length)
            .expect("internally encoded blocks must decode exactly"))
    }
}
impl Default for BlockStates {fn default()->Self{Vec::new().into()}}
impl From<Vec<u16>> for BlockStates {fn from(values:Vec<u16>)->Self{Self::from_storage(Storage::Dense(values))}}
impl FromIterator<u16> for BlockStates {
    fn from_iter<T:IntoIterator<Item=u16>>(iter:T)->Self{iter.into_iter().collect::<Vec<_>>().into()}
}
impl Clone for BlockStates {
    fn clone(&self)->Self {
        Self::from_storage(match &self.storage {Storage::Dense(values)=>Storage::Dense(values.clone()),
            Storage::Cold(cold)=>Storage::Cold(Box::new(ColdStates{encoded:cold.encoded.clone(),
                length:cold.length,read_cache:OnceLock::new()}))})
    }
    fn clone_from(&mut self,source:&Self) {
        match (&mut self.storage,&source.storage) {
            (Storage::Dense(values),Storage::Dense(other))=>values.clone_from(other),
            (slot,Storage::Dense(other))=>*slot=Storage::Dense(other.clone()),
            (slot,Storage::Cold(cold))=>*slot=Storage::Cold(Box::new(ColdStates{
                encoded:cold.encoded.clone(),length:cold.length,read_cache:OnceLock::new()})),
        }
        self.recent_cold_access=AtomicBool::new(false);self.next_eligible_generation=0;
    }
}
impl fmt::Debug for BlockStates {
    fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result {
        f.debug_struct("BlockStates").field("len",&self.len()).field("cold",&self.is_cold())
            .field("raw_capacity_states",&self.capacity()).field("encoded_payload_bytes",&self.encoded_payload_bytes()).finish()
    }
}
impl Deref for BlockStates {type Target=[u16];#[inline]fn deref(&self)->&[u16]{self.as_slice()}}
impl DerefMut for BlockStates {#[inline]fn deref_mut(&mut self)->&mut[u16]{self.as_mut_slice()}}
impl AsRef<[u16]> for BlockStates {#[inline]fn as_ref(&self)->&[u16]{self.as_slice()}}
impl AsMut<[u16]> for BlockStates {#[inline]fn as_mut(&mut self)->&mut[u16]{self.as_mut_slice()}}
impl<I:SliceIndex<[u16]>> Index<I> for BlockStates {
    type Output=I::Output;#[inline]fn index(&self,index:I)->&Self::Output{&self.as_slice()[index]}
}
impl<I:SliceIndex<[u16]>> IndexMut<I> for BlockStates {
    #[inline]fn index_mut(&mut self,index:I)->&mut Self::Output{&mut self.as_mut_slice()[index]}
}
impl<'a> IntoIterator for &'a BlockStates {
    type Item=&'a u16;type IntoIter=std::slice::Iter<'a,u16>;
    fn into_iter(self)->Self::IntoIter{self.as_slice().iter()}
}
impl<'a> IntoIterator for &'a mut BlockStates {
    type Item=&'a mut u16;type IntoIter=std::slice::IterMut<'a,u16>;
    fn into_iter(self)->Self::IntoIter{self.as_mut_slice().iter_mut()}
}
impl IntoIterator for BlockStates {
    type Item=u16;type IntoIter=std::vec::IntoIter<u16>;
    fn into_iter(self)->Self::IntoIter{self.into_vec().into_iter()}
}
impl PartialEq for BlockStates {
    fn eq(&self,other:&Self)->bool{std::ptr::eq(self,other)||self.with_slice(|a|other.with_slice(|b|a==b))}
}
impl Eq for BlockStates {}
impl PartialEq<Vec<u16>> for BlockStates {fn eq(&self,other:&Vec<u16>)->bool{self.with_slice(|a|a==other.as_slice())}}
impl PartialEq<BlockStates> for Vec<u16> {fn eq(&self,other:&BlockStates)->bool{other==self}}
impl Serialize for BlockStates {
    fn serialize<S:Serializer>(&self,serializer:S)->Result<S::Ok,S::Error> {
        self.try_with_slice(|values|values.serialize(serializer)).map_err(<S::Error as serde::ser::Error>::custom)?
    }
}
impl<'de> Deserialize<'de> for BlockStates {
    fn deserialize<D:Deserializer<'de>>(deserializer:D)->Result<Self,D::Error> {
        Vec::<u16>::deserialize(deserializer).map(Self::from)
    }
}
#[cfg(test)]
#[path="block_states_tests.rs"]
mod tests;
