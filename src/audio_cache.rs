//! Bounded, shared PCM for short Vorbis effects. Streaming music and oversized
//! Ogg tracks use the background streaming ring buffer instead.
use rodio::Source;
use std::{collections::HashMap, sync::Arc};

const MAX_CLIP_BYTES: usize = 1024 * 1024;
const MAX_CACHE_BYTES: usize = 32 * 1024 * 1024;
const MAX_ENTRIES: usize = 256;

#[derive(Clone, Debug)]
pub(super) struct Pcm {
    pub samples: Arc<[i16]>,
    pub channels: u16,
    pub rate: u32,
}
impl Pcm {
    #[inline]
    pub fn bytes(&self) -> usize {
        self.samples.len() * size_of::<i16>()
    }
}

struct Entry {
    pcm: Pcm,
    used: u64,
}
pub(super) struct Cache {
    entries: HashMap<String, Entry>,
    bytes: usize,
    clock: u64,
    budget: usize,
}
impl Default for Cache {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            bytes: 0,
            clock: 0,
            budget: MAX_CACHE_BYTES,
        }
    }
}
impl Cache {
    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
        self.clock = 0;
    }
    pub fn get(&mut self, path: &str, streaming: bool) -> Option<Pcm> {
        // A resource can appear in both streaming and non-streaming events.
        if streaming {
            return None;
        }
        let entry = self.entries.get_mut(path)?;
        self.clock = self.clock.saturating_add(1);
        entry.used = self.clock;
        Some(entry.pcm.clone())
    }
    pub fn insert(&mut self, path: String, pcm: &Pcm) {
        let bytes = pcm.bytes();
        if bytes == 0 || bytes > MAX_CLIP_BYTES || bytes > self.budget {
            return;
        }
        if let Some(old) = self.entries.remove(&path) {
            self.bytes -= old.pcm.bytes();
        }
        while self.bytes + bytes > self.budget || self.entries.len() >= MAX_ENTRIES {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.used)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            let old = self.entries.remove(&oldest).unwrap();
            self.bytes -= old.pcm.bytes();
        }
        self.clock = self.clock.saturating_add(1);
        self.entries.insert(
            path,
            Entry {
                pcm: pcm.clone(),
                used: self.clock,
            },
        );
        self.bytes += bytes;
        // The bound covers cache-owned samples. Already-playing voices may keep
        // an evicted Arc alive until their playback ends.
    }
}

/// Structural admission, not a second decoder or an integrity validator. Unknown
/// lengths, chained/multiplexed streams and large clips fall back to `capture_any`
/// or streaming without unbounded allocation.
pub(super) fn admissible_ogg(bytes: &[u8]) -> bool {
    if bytes.len() > MAX_CLIP_BYTES {
        return false;
    }
    let mut at = 0;
    let mut serial = None;
    let mut sequence = 0u32;
    let mut continued = false;
    let mut channels = 0usize;
    let mut finished = false;
    while at < bytes.len() {
        let Some(header) = bytes.get(at..at + 27) else {
            return false;
        };
        if &header[..4] != b"OggS" || header[4] != 0 || header[5] & !7 != 0 {
            return false;
        }
        let flags = header[5];
        let id = u32::from_le_bytes(header[14..18].try_into().unwrap());
        let seq = u32::from_le_bytes(header[18..22].try_into().unwrap());
        let Some(laces) = bytes.get(at + 27..at + 27 + usize::from(header[26])) else {
            return false;
        };
        let body_start = at + 27 + laces.len();
        let body_len: usize = laces.iter().map(|&v| usize::from(v)).sum();
        let Some(body) = bytes.get(body_start..body_start + body_len) else {
            return false;
        };
        let end = body_start + body_len;
        if seq != sequence || (flags & 1 != 0) != continued {
            return false;
        }
        sequence = sequence.wrapping_add(1);
        if serial.is_none() {
            // A normal Vorbis identification packet is exactly 30 bytes.
            if flags != 2
                || laces.first() != Some(&30)
                || body.len() < 30
                || &body[..7] != b"\x01vorbis"
                || body[7..11] != [0; 4]
                || !matches!(body[11], 1 | 2)
                || body[29] & 1 == 0
                || u32::from_le_bytes(body[12..16].try_into().unwrap()) == 0
            {
                return false;
            }
            channels = usize::from(body[11]);
            serial = Some(id);
        } else if Some(id) != serial || flags & 2 != 0 {
            return false;
        }
        continued = laces.last() == Some(&255);
        if flags & 4 != 0 {
            let frames = u64::from_le_bytes(header[6..14].try_into().unwrap());
            if continued
                || end != bytes.len()
                || frames == 0
                || frames == u64::MAX
                || frames
                    .checked_mul(channels as u64)
                    .and_then(|v| v.checked_mul(2))
                    .is_none_or(|size| size > MAX_CLIP_BYTES as u64)
            {
                return false;
            }
            finished = true;
        }
        at = end;
    }
    finished
}

/// Capture with an independent hard sample limit; an untrusted granule position
/// is never enough to permit unbounded decoding/allocation.
pub(super) fn capture(source: impl Source<Item = i16>) -> Option<Pcm> {
    capture_bounded(source, MAX_CLIP_BYTES / size_of::<i16>())
}
fn capture_bounded(mut source: impl Source<Item = i16>, max_samples: usize) -> Option<Pcm> {
    let channels = source.channels();
    let rate = source.sample_rate();
    if !matches!(channels, 1 | 2) || rate == 0 {
        return None;
    }
    let mut samples = Vec::new();
    while let Some(sample) = source.next() {
        if samples.len() >= max_samples
            || source.channels() != channels
            || source.sample_rate() != rate
        {
            return None;
        }
        samples.push(sample);
    }
    let rem = samples.len() % usize::from(channels);
    if rem != 0 {
        samples.truncate(samples.len() - rem);
    }
    if samples.is_empty() {
        return None;
    }
    Some(Pcm {
        samples: samples.into(),
        channels,
        rate,
    })
}

/// Bounded fallback capture up to `MAX_CLIP_BYTES` for non-streaming clips whose
/// Ogg container was not admitted by `admissible_ogg` (e.g. unknown granule
/// lengths, trailing padding, >2 channels, or chained sub-streams).
pub(super) fn capture_any(source: impl Source<Item = i16>) -> Option<Pcm> {
    capture_any_bounded(source, MAX_CLIP_BYTES / size_of::<i16>())
}
fn capture_any_bounded(mut source: impl Source<Item = i16>, max_samples: usize) -> Option<Pcm> {
    let init_ch = source.channels();
    let out_rate = source.sample_rate();
    if init_ch == 0 || out_rate == 0 || max_samples == 0 {
        return None;
    }
    let mut out_ch: u16 = init_ch.min(2);
    let mut samples: Vec<i16> = Vec::new();
    let mut frame_buf = Vec::<i16>::with_capacity(usize::from(init_ch).max(2));

    while let Some(first) = source.next() {
        let cur_ch = source.channels();
        let cur_rate = source.sample_rate();
        if cur_ch == 0 || cur_rate == 0 {
            return None;
        }
        frame_buf.clear();
        frame_buf.push(first);
        let mut incomplete = false;
        for _ in 1..cur_ch {
            if let Some(s) = source.next() {
                frame_buf.push(s);
            } else {
                incomplete = true;
                break;
            }
        }
        if incomplete {
            break;
        }
        if cur_ch >= 2 && out_ch == 1 {
            if samples.len().saturating_mul(2) > max_samples {
                return None;
            }
            let mut stereo = Vec::with_capacity(samples.len() * 2);
            for &s in &samples {
                stereo.push(s);
                stereo.push(s);
            }
            samples = stereo;
            out_ch = 2;
        }
        let (l, r) = if cur_ch == 1 {
            (frame_buf[0], frame_buf[0])
        } else {
            (frame_buf[0], frame_buf[1])
        };
        let repeats = if cur_rate == out_rate {
            1usize
        } else {
            ((u64::from(out_rate) + u64::from(cur_rate) / 2) / u64::from(cur_rate)).max(1) as usize
        };
        let needed = repeats.saturating_mul(usize::from(out_ch));
        if samples.len().saturating_add(needed) > max_samples {
            return None;
        }
        for _ in 0..repeats {
            samples.push(l);
            if out_ch == 2 {
                samples.push(r);
            }
        }
    }
    if samples.is_empty() {
        return None;
    }
    Some(Pcm {
        samples: samples.into(),
        channels: out_ch,
        rate: out_rate,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::{Decoder, buffer::SamplesBuffer};
    use std::{io::Cursor, time::Duration};

    fn sound(samples: Vec<i16>, channels: u16, rate: u32) -> Pcm {
        capture(SamplesBuffer::new(channels, rate, samples)).unwrap()
    }
    #[derive(Clone)]
    struct PacketSource {
        position: usize,
        change_format: bool,
    }
    impl Iterator for PacketSource {
        type Item = i16;
        fn next(&mut self) -> Option<i16> {
            if self.position == 51 {
                return None;
            }
            let value = ((self.position * 7919) % 50000) as i32 - 25000;
            self.position += 1;
            Some(value as i16)
        }
    }
    impl Source for PacketSource {
        fn current_frame_len(&self) -> Option<usize> {
            Some(if self.position <= 7 {
                7
            } else if self.position <= 18 {
                11
            } else {
                33
            })
        }
        fn channels(&self) -> u16 {
            1
        }
        fn sample_rate(&self) -> u32 {
            if self.change_format && self.position > 7 {
                44100
            } else {
                22050
            }
        }
        fn total_duration(&self) -> Option<Duration> {
            None
        }
    }
    #[test]
    fn packet_capture_preserves_exact_samples_and_capture_any_rescues_nonstandard_streams() {
        let source = PacketSource {
            position: 0,
            change_format: false,
        };
        let pcm = capture(source.clone()).unwrap();
        assert_eq!((pcm.channels, pcm.rate), (1, 22050));
        assert_eq!(&*pcm.samples, source.clone().collect::<Vec<_>>().as_slice());
        assert_eq!(
            &*capture_any(source.clone()).unwrap().samples,
            &*pcm.samples
        );
        assert!(
            capture(PacketSource {
                position: 0,
                change_format: true
            })
            .is_none()
        );
        let rescued = capture_any(PacketSource {
            position: 0,
            change_format: true,
        })
        .unwrap();
        assert_eq!((rescued.channels, rescued.rate), (1, 22050));
        assert!(!rescued.samples.is_empty());
    }
    #[test]
    fn shared_arc_entries_preserve_samples_and_source_metadata() {
        let expected = vec![10, -20, 30, -40, 50, -60];
        let original = SamplesBuffer::new(2, 48000, expected.clone());
        let pcm = capture(original).unwrap();
        let mut cache = Cache::default();
        cache.insert("effect".into(), &pcm);
        let a = cache.get("effect", false).unwrap();
        let b = cache.get("effect", false).unwrap();
        assert!(Arc::ptr_eq(&a.samples, &b.samples));
        assert_eq!((a.channels, a.rate), (2, 48000));
        assert_eq!(&*a.samples, expected.as_slice());
        assert_eq!(&*b.samples, expected.as_slice());
        assert!(cache.get("effect", true).is_none());
    }
    #[test]
    fn lru_budget_eviction_and_resource_invalidation_keep_live_voices_valid() {
        let per_clip = 3 * size_of::<i16>();
        let mut cache = Cache {
            budget: per_clip * 2,
            ..Default::default()
        };
        cache.insert("a".into(), &sound(vec![1, 2, 3], 1, 22050));
        cache.insert("b".into(), &sound(vec![4, 5, 6], 1, 22050));
        let playing = cache.get("a", false).unwrap();
        cache.insert("c".into(), &sound(vec![7, 8, 9], 1, 22050));
        assert!(cache.get("b", false).is_none());
        assert_eq!(cache.bytes, per_clip * 2);
        assert!(cache.get("a", false).is_some());
        cache.clear();
        assert_eq!((cache.bytes, cache.entries.len()), (0, 0));
        assert!(cache.get("a", false).is_none());
        assert_eq!(&*playing.samples, &[1, 2, 3]);
        cache.insert("a".into(), &sound(vec![9, 10], 2, 44100));
        assert_eq!(&*cache.get("a", false).unwrap().samples, &[9, 10]);
    }
    #[test]
    fn tiny_clips_and_replacements_still_obey_entry_and_byte_bounds() {
        let mut cache = Cache::default();
        for i in 0..MAX_ENTRIES + 5 {
            cache.insert(i.to_string(), &sound(vec![i as i16], 1, 8000));
        }
        let per_clip = size_of::<i16>();
        assert_eq!(
            (cache.entries.len(), cache.bytes),
            (MAX_ENTRIES, MAX_ENTRIES * per_clip)
        );
        assert!(cache.get("0", false).is_none());
        cache.insert("5".into(), &sound(vec![7, 8], 1, 8000));
        assert_eq!(cache.bytes, MAX_ENTRIES * per_clip + size_of::<i16>());
        assert!(capture_bounded(SamplesBuffer::new(1, 8000, vec![0; 9]), 8).is_none());
        assert!(capture_bounded(SamplesBuffer::new(1, 8000, vec![0; 8]), 8).is_some());
        assert!(capture_any_bounded(SamplesBuffer::new(1, 8000, vec![0; 9]), 8).is_none());
        assert!(capture_any_bounded(SamplesBuffer::new(1, 8000, vec![0; 8]), 8).is_some());
    }

    fn page(serial: u32, sequence: u32, flags: u8, frames: u64, body: &[u8]) -> Vec<u8> {
        assert!(body.len() < 255);
        let mut result = vec![0; 28];
        result[..4].copy_from_slice(b"OggS");
        result[5] = flags;
        result[6..14].copy_from_slice(&frames.to_le_bytes());
        result[14..18].copy_from_slice(&serial.to_le_bytes());
        result[18..22].copy_from_slice(&sequence.to_le_bytes());
        result[26] = 1;
        result[27] = body.len() as u8;
        result.extend(body);
        result
    }
    fn small_ogg() -> Vec<u8> {
        let mut ident = vec![0; 30];
        ident[..7].copy_from_slice(b"\x01vorbis");
        ident[11] = 2;
        ident[12..16].copy_from_slice(&44100u32.to_le_bytes());
        ident[29] = 1;
        let mut result = page(17, 0, 2, 0, &ident);
        result.extend(page(17, 1, 4, 64, &[0]));
        result
    }
    #[test]
    fn ogg_admission_rejects_unknown_oversized_malformed_and_chained_streams() {
        let valid = small_ogg();
        assert!(admissible_ogg(&valid));
        for len in 0..valid.len() {
            assert!(!admissible_ogg(&valid[..len]), "length {len}");
        }
        let last_page = 58;
        for granule in [0, u64::MAX, MAX_CLIP_BYTES as u64] {
            let mut bad = valid.clone();
            bad[last_page + 6..last_page + 14].copy_from_slice(&granule.to_le_bytes());
            assert!(!admissible_ogg(&bad));
        }
        for (offset, value) in [
            (last_page + 14, 18),
            (last_page + 5, 6),
            (last_page + 18, 9),
            (last_page + 5, 0),
            (28 + 11, 0),
        ] {
            let mut bad = valid.clone();
            bad[offset] = value;
            assert!(!admissible_ogg(&bad));
        }
        let mut chained = valid.clone();
        chained.extend(valid);
        assert!(!admissible_ogg(&chained));
        assert!(!admissible_ogg(&vec![0; MAX_CLIP_BYTES + 1]));
    }
    #[test]
    #[ignore = "requires locally imported Minecraft sounds"]
    fn imported_vorbis_effects_match_decoder_samples() {
        let r = crate::validation::resources();
        for path in [
            "random/click",
            "mob/sheep/say1",
            "step/grass1",
            "tile/piston/out",
        ] {
            let bytes: Arc<[u8]> = r.bytes(&format!("sounds/{path}.ogg")).unwrap().into();
            assert!(admissible_ogg(&bytes), "{path}");
            let decoder = || Decoder::new(Cursor::new(bytes.clone())).unwrap();
            let original = decoder();
            let metadata = (original.channels(), original.sample_rate());
            let pcm = capture(original).unwrap();
            assert_eq!((pcm.channels, pcm.rate), metadata, "{path}");
            assert_eq!(
                &*pcm.samples,
                decoder().collect::<Vec<_>>().as_slice(),
                "{path}"
            );
        }
    }
}
