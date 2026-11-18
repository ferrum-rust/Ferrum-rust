//! Lossless optional cold storage for a single native light byte array.
//! No caller compresses automatically: owner maintenance policy is separate.
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, io, ops::{Deref, DerefMut, Index, IndexMut}, slice::SliceIndex,
    sync::{Arc, OnceLock, atomic::{AtomicBool, Ordering}}};

const MAX_COLD_BYTES: usize = 65536;

#[derive(Clone)]
enum Encoding { Uniform(u8), Runs(Arc<[u8]>) }
impl Encoding {
    fn bytes(&self) -> usize { match self { Self::Uniform(_) => 1, Self::Runs(v) => v.len() } }
    fn allocation_bytes(&self) -> usize { match self { Self::Uniform(_) => 0, Self::Runs(v) => v.len() } }
}
enum Storage {
    Dense(Vec<u8>),
    Cold { encoded: Encoding, length: usize, read_cache: OnceLock<Vec<u8>> },
    // Client-only adoption. Boxing keeps this payload below the existing Cold
    // variant size and leaves authoritative native arrays as Dense/Cold.
    SharedPacked(Box<SharedPacked>),
}
struct SharedPacked {
    snapshot: Arc<super::ChunkLight>,
    kind: super::Kind,
    read_cache: OnceLock<Vec<u8>>,
}
impl SharedPacked {
    fn unpack(&self) -> Vec<u8> {
        match self.kind {
            super::Kind::Sky => self.snapshot.values.iter().map(|value| value >> 4).collect(),
            super::Kind::Block => self.snapshot.values.iter().map(|value| value & 15).collect(),
        }
    }
    #[cold]
    #[inline(never)]
    fn value_at(&self, index: usize) -> u8 {
        let packed = self.snapshot.values[index];
        match self.kind { super::Kind::Sky => packed >> 4, super::Kind::Block => packed & 15 }
    }
    #[cold]
    #[inline(never)]
    fn as_slice(&self) -> &[u8] { self.read_cache.get_or_init(|| self.unpack()) }
    #[cold]
    #[inline(never)]
    fn with_bytes<R>(&self, f: impl FnOnce(&[u8]) -> R) -> R {
        if let Some(bytes) = self.read_cache.get() { f(bytes) }
        else { let bytes = self.unpack(); f(&bytes) }
    }
}
pub(crate) struct LightBytes {
    storage: Storage,
    recent_cold_access: AtomicBool,
    next_eligible_generation: u64,
}
fn invalid(message: &'static str) -> io::Error { io::Error::new(io::ErrorKind::InvalidData, message) }
fn decode(encoded: &Encoding, length: usize) -> io::Result<Vec<u8>> {
    if length == 0 || length > MAX_COLD_BYTES { return Err(invalid("Invalid cold light output length")); }
    match encoded {
        Encoding::Uniform(value) => Ok(vec![*value; length]),
        Encoding::Runs(runs) => {
            // No external constructor admits Cold data. Check all structural
            // limits nevertheless: neither work nor output can exceed one array.
            if runs.is_empty() || runs.len() >= length || runs.len() % 3 != 0 {
                return Err(invalid("Invalid cold light run payload"));
            }
            let mut bytes = Vec::with_capacity(length);
            for run in runs.chunks_exact(3) {
                let count = usize::from(u16::from_le_bytes([run[0], run[1]]));
                if count == 0 || count > length - bytes.len() {
                    return Err(invalid("Cold light run exceeds exact output size"));
                }
                bytes.resize(bytes.len() + count, run[2]);
            }
            if bytes.len() != length { return Err(invalid("Cold light output length differs")); }
            Ok(bytes)
        }
    }
}
// Same bounded Uniform/RLE decision as the measured diagnostic. Raw fallback
// keeps the original Dense allocation; no compressed Raw representation exists.
fn encode(bytes: &[u8], minimum_saved_bytes: usize) -> Option<Encoding> {
    if bytes.is_empty() || bytes.len() > MAX_COLD_BYTES
        || minimum_saved_bytes >= bytes.len() { return None; }
    let mut runs = Vec::new();
    let mut start = 0;
    while start < bytes.len() {
        let value = bytes[start];
        let mut end = start + 1;
        while end < bytes.len() && bytes[end] == value { end += 1; }
        if start == 0 && end == bytes.len() {
            return (bytes.len() > 1 && bytes.len() - 1 >= minimum_saved_bytes)
                .then_some(Encoding::Uniform(value));
        }
        if start == 0 { runs.reserve(bytes.len().min(512)); }
        let mut remaining = end - start;
        while remaining != 0 {
            if runs.len() + 3 >= bytes.len() { return None; }
            let count = remaining.min(u16::MAX as usize) as u16;
            runs.extend_from_slice(&count.to_le_bytes()); runs.push(value);
            remaining -= usize::from(count);
        }
        start = end;
    }
    if bytes.len() - runs.len() < minimum_saved_bytes { return None; }
    Some(Encoding::Runs(runs.into()))
}

impl LightBytes {
    fn from_storage(storage: Storage) -> Self {
        Self { storage, recent_cold_access: AtomicBool::new(false), next_eligible_generation: 0 }
    }
    /// Only client snapshot adoption constructs this backing. Malformed lengths
    /// must retain the caller's original eager conversion rather than hydrate an
    /// unbounded array through this private cache.
    pub(crate) fn from_shared_packed(snapshot: Arc<super::ChunkLight>, kind: super::Kind) -> Self {
        assert_eq!(snapshot.values.len(), MAX_COLD_BYTES, "shared packed light must have exactly one column");
        Self::from_storage(Storage::SharedPacked(Box::new(SharedPacked {
            snapshot, kind, read_cache: OnceLock::new(),
        })))
    }
    pub(crate) fn shares_packed(&self, snapshot: &super::ChunkLight, kind: super::Kind) -> bool {
        matches!(&self.storage, Storage::SharedPacked(shared)
            if shared.kind == kind && std::ptr::eq(shared.snapshot.as_ref(), snapshot))
    }
    pub(crate) fn is_shared_packed(&self) -> bool { matches!(self.storage, Storage::SharedPacked(_)) }
    pub(crate) fn is_dense(&self) -> bool { matches!(self.storage, Storage::Dense(_)) }
    /// Nonlogical owner policy state, deliberately absent from Clone/serde.
    pub(crate) fn take_recent_cold_access(&mut self) -> bool {
        self.recent_cold_access.swap(false, Ordering::Relaxed)
    }
    pub(crate) fn next_eligible_generation(&self) -> u64 { self.next_eligible_generation }
    pub(crate) fn set_next_eligible_generation(&mut self, generation: u64) {
        self.next_eligible_generation = generation;
    }
    pub(crate) fn encoded_capacity_bytes(&self) -> usize {
        match &self.storage { Storage::Dense(_) | Storage::SharedPacked(_) => 0,
            Storage::Cold { encoded, .. } => encoded.allocation_bytes() }
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        match &self.storage {
            Storage::Dense(bytes) => bytes.len(), Storage::Cold { length, .. } => *length,
            Storage::SharedPacked(_) => MAX_COLD_BYTES,
        }
    }
    #[inline]
    pub(crate) fn is_empty(&self) -> bool { self.len() == 0 }
    /// Resident RAW capacity only; compressed allocation is reported separately.
    /// Cheap diagnostics must never hydrate a cold array through Deref.
    pub(crate) fn capacity(&self) -> usize {
        match &self.storage {
            Storage::Dense(bytes) => bytes.capacity(),
            Storage::Cold { read_cache, .. } => read_cache.get().map_or(0, Vec::capacity),
            Storage::SharedPacked(shared) => shared.read_cache.get().map_or(0, Vec::capacity),
        }
    }
    pub(crate) fn encoded_bytes(&self) -> usize {
        match &self.storage { Storage::Dense(_) | Storage::SharedPacked(_) => 0,
            Storage::Cold { encoded, .. } => encoded.bytes() }
    }
    pub(crate) fn is_cold(&self) -> bool { matches!(self.storage, Storage::Cold { .. }) }
    pub(crate) fn has_read_cache(&self) -> bool {
        match &self.storage {
            Storage::Cold { read_cache, .. } => read_cache.get().is_some(),
            Storage::SharedPacked(shared) => shared.read_cache.get().is_some(),
            Storage::Dense(_) => false,
        }
    }
    /// Scalar client-light reads can use an immutable packed nibble without
    /// materializing an entire raw plane. Native Dense/Cold access semantics
    /// (including Cold recency and read-cache initialization) stay unchanged.
    #[inline]
    pub(crate) fn value_at(&self, index: usize) -> u8 {
        match &self.storage {
            Storage::Dense(bytes) => bytes[index],
            Storage::Cold { encoded, length, read_cache } =>
                Self::cold_slice(encoded, *length, read_cache, &self.recent_cold_access)[index],
            Storage::SharedPacked(shared) => shared.value_at(index),
        }
    }
    #[inline]
    pub(crate) fn as_slice(&self) -> &[u8] {
        match &self.storage {
            Storage::Dense(bytes) => bytes,
            Storage::Cold { encoded, length, read_cache } =>
                Self::cold_slice(encoded, *length, read_cache, &self.recent_cold_access),
            Storage::SharedPacked(shared) => shared.as_slice(),
        }
    }
    // Keep cache initialization and decode failure paths out of the many
    // inlined lighting get/index callers. Even a cached Cold read is a notice.
    #[cold]
    #[inline(never)]
    fn cold_slice<'a>(encoded: &Encoding, length: usize,
        read_cache: &'a OnceLock<Vec<u8>>, recent_cold_access: &AtomicBool) -> &'a [u8] {
        recent_cold_access.store(true, Ordering::Relaxed);
        read_cache.get_or_init(|| decode(encoded, length)
            .expect("internally encoded light must decode exactly"))
    }
    #[inline]
    fn as_mut_slice(&mut self) -> &mut [u8] {
        match &mut self.storage {
            Storage::Dense(bytes) => bytes,
            cold @ Storage::Cold { .. } => Self::cold_mut_slice(cold, &self.recent_cold_access),
            shared @ Storage::SharedPacked(_) => Self::shared_mut_slice(shared),
        }
    }
    // Return the promoted slice from the out-of-line branch so the Dense branch
    // does not need to recheck the storage discriminant after a helper call.
    #[cold]
    #[inline(never)]
    fn cold_mut_slice<'a>(storage: &'a mut Storage, recent_cold_access: &AtomicBool) -> &'a mut [u8] {
        let Storage::Cold { encoded, length, read_cache } = storage else { unreachable!() };
        // Preserve original order: take cached bytes or finish decoding first,
        // then record the notice, replace storage, and return the Dense slice.
        let bytes = read_cache.take().unwrap_or_else(||
            decode(encoded, *length).expect("internally encoded light must decode exactly"));
        recent_cold_access.store(true, Ordering::Relaxed);
        *storage = Storage::Dense(bytes);
        match storage { Storage::Dense(bytes) => bytes, _ => unreachable!() }
    }
    #[cold]
    #[inline(never)]
    fn shared_mut_slice(storage: &mut Storage) -> &mut [u8] {
        let Storage::SharedPacked(shared) = storage else { unreachable!() };
        let bytes = shared.read_cache.take().unwrap_or_else(|| shared.unpack());
        *storage = Storage::Dense(bytes);
        // A client-only promotion is not a native compressed-array access.
        match storage { Storage::Dense(bytes) => bytes, _ => unreachable!() }
    }
    /// Borrow bytes for one synchronous operation without making cold arrays
    /// resident. R cannot borrow from the temporary slice. Used by serialization,
    /// checkpoint validation and packed render snapshots, never an owner callback.
    pub(crate) fn try_with_bytes<R>(&self, f: impl FnOnce(&[u8]) -> R) -> io::Result<R> {
        match &self.storage {
            Storage::Dense(bytes) => Ok(f(bytes)),
            Storage::Cold { encoded, length, read_cache } => {
                if let Some(bytes) = read_cache.get() { Ok(f(bytes)) }
                else { let bytes = decode(encoded, *length)?; Ok(f(&bytes)) }
            }
            Storage::SharedPacked(shared) => Ok(shared.with_bytes(f)),
        }
    }
    pub(crate) fn with_bytes<R>(&self, f: impl FnOnce(&[u8]) -> R) -> R {
        self.try_with_bytes(f).expect("internally encoded light must decode exactly")
    }
    /// Attempt at most one <=64-KiB array. The owner, not this type, chooses the
    /// eligibility, candidate/byte/time limits, minimum saving and thaw cooldown.
    /// Failed or unprofitable attempts leave the dense allocation unchanged.
    pub(crate) fn compress_if_saving(&mut self, minimum_saved_bytes: usize) -> io::Result<bool> {
        let Storage::Dense(bytes) = &self.storage else { return Ok(false); };
        if bytes.len() > MAX_COLD_BYTES || bytes.is_empty() { return Ok(false); }
        let Some(encoded) = encode(bytes, minimum_saved_bytes) else { return Ok(false); };
        let length = bytes.len();
        self.storage = Storage::Cold { encoded, length, read_cache: OnceLock::new() };
        Ok(true)
    }
    /// Only safe at an owner's exclusive maintenance boundary. The compressed
    /// representation remains valid until a mutable access promotes to Dense.
    pub(crate) fn release_read_cache(&mut self) -> usize {
        match &mut self.storage {
            Storage::Cold { read_cache, .. } => read_cache.take().map_or(0, |bytes| bytes.capacity()),
            Storage::SharedPacked(shared) => shared.read_cache.take().map_or(0, |bytes| bytes.capacity()),
            Storage::Dense(_) => 0,
        }
    }
}
impl Default for LightBytes { fn default() -> Self { Vec::new().into() } }
impl From<Vec<u8>> for LightBytes { fn from(bytes: Vec<u8>) -> Self { Self::from_storage(Storage::Dense(bytes)) } }
impl FromIterator<u8> for LightBytes {
    fn from_iter<T: IntoIterator<Item = u8>>(iter: T) -> Self { iter.into_iter().collect::<Vec<_>>().into() }
}
impl Clone for LightBytes {
    fn clone(&self) -> Self {
        Self::from_storage(match &self.storage {
            Storage::Dense(bytes) => Storage::Dense(bytes.clone()),
            Storage::Cold { encoded, length, .. } => Storage::Cold {
                encoded: encoded.clone(), length: *length, read_cache: OnceLock::new(),
            },
            Storage::SharedPacked(shared) => Storage::SharedPacked(Box::new(SharedPacked {
                snapshot: Arc::clone(&shared.snapshot), kind: shared.kind, read_cache: OnceLock::new(),
            })),
        })
    }
}
impl fmt::Debug for LightBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LightBytes").field("len", &self.len()).field("cold", &self.is_cold())
            .field("shared_packed", &self.is_shared_packed())
            .field("raw_capacity", &self.capacity()).field("encoded_bytes", &self.encoded_bytes()).finish()
    }
}
impl Deref for LightBytes { type Target = [u8]; #[inline] fn deref(&self) -> &[u8] { self.as_slice() } }
impl DerefMut for LightBytes { #[inline] fn deref_mut(&mut self) -> &mut [u8] { self.as_mut_slice() } }
impl AsRef<[u8]> for LightBytes { #[inline] fn as_ref(&self) -> &[u8] { self.as_slice() } }
impl<I: SliceIndex<[u8]>> Index<I> for LightBytes {
    type Output = I::Output;
    #[inline] fn index(&self, index: I) -> &Self::Output { &self.as_slice()[index] }
}
impl<I: SliceIndex<[u8]>> IndexMut<I> for LightBytes {
    #[inline] fn index_mut(&mut self, index: I) -> &mut Self::Output { &mut self.as_mut_slice()[index] }
}
impl<'a> IntoIterator for &'a LightBytes {
    type Item = &'a u8; type IntoIter = std::slice::Iter<'a, u8>;
    fn into_iter(self) -> Self::IntoIter { self.as_slice().iter() }
}
impl<'a> IntoIterator for &'a mut LightBytes {
    type Item = &'a mut u8; type IntoIter = std::slice::IterMut<'a, u8>;
    fn into_iter(self) -> Self::IntoIter { self.as_mut_slice().iter_mut() }
}
impl PartialEq for LightBytes {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other) || self.with_bytes(|a| other.with_bytes(|b| a == b))
    }
}
impl Eq for LightBytes {}
impl PartialEq<Vec<u8>> for LightBytes { fn eq(&self, other: &Vec<u8>) -> bool { self.with_bytes(|a| a == other.as_slice()) } }
impl PartialEq<LightBytes> for Vec<u8> { fn eq(&self, other: &LightBytes) -> bool { other == self } }
impl Serialize for LightBytes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Preserve the original JSON numeric array, including malformed dense
        // fixtures. Compression is an in-memory representation, not a wire type.
        self.try_with_bytes(|bytes| bytes.serialize(serializer))
            .map_err(<S::Error as serde::ser::Error>::custom)?
    }
}
impl<'de> Deserialize<'de> for LightBytes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Preserve Vec's existing malformed-length/nibble behavior. The native
        // checkpoint validator, and its existing file byte limit, remain owners
        // of admission. Never pad, mask, truncate, or accept a compressed tag.
        Vec::<u8>::deserialize(deserializer).map(Self::from)
    }
}

#[cfg(test)]
#[path = "lighting_bytes_simple_tests.rs"]
mod tests;
