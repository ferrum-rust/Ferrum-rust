//! Original sound event selection, category gain, 20 Hz background music timing,
//! and lock-free real-time WASAPI/Bluetooth mixer.
use crate::{resources::Resources, vanilla::random::JavaRandom};
use rodio::{
    Decoder, Source,
    cpal::{
        self, BufferSize, SampleFormat, SampleRate, StreamConfig, SupportedBufferSize,
        SupportedStreamConfig,
        traits::{DeviceTrait, HostTrait, StreamTrait},
    },
};
use std::{
    cell::UnsafeCell,
    collections::HashMap,
    io::Cursor,
    mem::MaybeUninit,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[path = "audio_events.rs"]
mod events;
#[path = "audio_cache.rs"]
mod pcm_cache;
use events::{Bank, Clip};

type VorbisSource = Decoder<Cursor<Arc<[u8]>>>;

const CMD_RING_CAP: usize = 512;
const RETIRE_RING_CAP: usize = 512;
const STREAM_RING_CAP: usize = 65_536;
const STREAM_PREFILL_SAMPLES: usize = 4_096;
const MAX_EFFECT_VOICES: usize = 64;
const MAX_FADE_TAILS: usize = 8;
const FADE_FRAMES: usize = 96;
const MICRO_BLOCK: usize = 64;
const DEVICE_POLL_INTERVAL: Duration = Duration::from_millis(2500);

/// Lock-free bounded single-producer / single-consumer ring buffer.
/// Capacity `cap` must be a power of two; all `cap` slots are usable via wrapping indices.
struct SpscRing<T> {
    slots: Box<[UnsafeCell<MaybeUninit<T>>]>,
    mask: usize,
    cap: usize,
    head: AtomicUsize,
    tail: AtomicUsize,
}
unsafe impl<T: Send> Send for SpscRing<T> {}
unsafe impl<T: Send> Sync for SpscRing<T> {}

impl<T> SpscRing<T> {
    fn new(cap: usize) -> Self {
        assert!(cap > 1 && cap.is_power_of_two());
        let mut vec = Vec::with_capacity(cap);
        for _ in 0..cap {
            vec.push(UnsafeCell::new(MaybeUninit::uninit()));
        }
        Self {
            slots: vec.into_boxed_slice(),
            mask: cap - 1,
            cap,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }
    #[inline]
    fn push(&self, value: T) -> Result<(), T> {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        if tail.wrapping_sub(head) >= self.cap {
            return Err(value);
        }
        unsafe {
            (*self.slots[tail & self.mask].get()).write(value);
        }
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        Ok(())
    }
    #[inline]
    fn pop(&self) -> Option<T> {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        if head == tail {
            return None;
        }
        let value = unsafe { (*self.slots[head & self.mask].get()).assume_init_read() };
        self.head.store(head.wrapping_add(1), Ordering::Release);
        Some(value)
    }
    #[inline]
    fn len_consumer(&self) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        tail.wrapping_sub(head)
    }
    #[inline]
    fn free_producer(&self) -> usize {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        self.cap.saturating_sub(tail.wrapping_sub(head))
    }
}
impl<T> Drop for SpscRing<T> {
    fn drop(&mut self) {
        let mut cur = *self.head.get_mut();
        let end = *self.tail.get_mut();
        while cur != end {
            unsafe {
                (*self.slots[cur & self.mask].get()).assume_init_drop();
            }
            cur = cur.wrapping_add(1);
        }
    }
}

struct StreamInner {
    ring: SpscRing<i16>,
    channels: u16,
    sample_rate: u32,
    eof: AtomicBool,
    aborted: AtomicBool,
}

/// Decoupled background Ogg/PCM feeder writing whole frames into a 65,536-sample lock-free ring.
struct StreamTrack {
    inner: Arc<StreamInner>,
    feeder: Option<std::thread::Thread>,
}
impl StreamTrack {
    fn new<S>(mut source: S) -> Option<Self>
    where
        S: Source<Item = i16> + Send + 'static,
    {
        let raw_ch = source.channels();
        let sample_rate = source.sample_rate();
        if raw_ch == 0 || sample_rate == 0 {
            return None;
        }
        let out_ch = raw_ch.min(2);
        let inner = Arc::new(StreamInner {
            ring: SpscRing::new(STREAM_RING_CAP),
            channels: out_ch,
            sample_rate,
            eof: AtomicBool::new(false),
            aborted: AtomicBool::new(false),
        });

        // Pre-fill up to ~4,096 samples synchronously before spawning the feeder
        // thread so playback starts immediately at sample 0 without an initial underrun.
        let prefill_frames = STREAM_PREFILL_SAMPLES / usize::from(out_ch);
        let mut reached_eof = false;
        for _ in 0..prefill_frames {
            if let Some((l, r)) = decode_stream_frame(&mut source, out_ch) {
                let _ = inner.ring.push(l);
                if out_ch == 2 {
                    let _ = inner.ring.push(r);
                }
            } else {
                reached_eof = true;
                break;
            }
        }
        if reached_eof {
            inner.eof.store(true, Ordering::Release);
            return Some(Self {
                inner,
                feeder: None,
            });
        }

        let worker_inner = Arc::clone(&inner);
        let feeder = match std::thread::Builder::new()
            .name("ferrum-audio-stream".into())
            .spawn(move || stream_feeder_loop(source, out_ch, worker_inner))
        {
            Ok(handle) => Some(handle.thread().clone()),
            Err(_) => {
                inner.eof.store(true, Ordering::Release);
                None
            }
        };
        Some(Self { inner, feeder })
    }
    #[inline]
    fn abort(&self) {
        self.inner.aborted.store(true, Ordering::Release);
        if let Some(thread) = &self.feeder {
            thread.unpark();
        }
    }
    #[inline]
    fn pop_frame(&self) -> Option<(f32, f32)> {
        let ch = usize::from(self.inner.channels);
        if self.inner.ring.len_consumer() < ch {
            return None;
        }
        let l = i16_to_f32(self.inner.ring.pop()?);
        let r = if ch == 2 {
            i16_to_f32(self.inner.ring.pop().unwrap_or(0))
        } else {
            l
        };
        Some((l, r))
    }
    #[inline]
    fn is_eof(&self) -> bool {
        self.inner.eof.load(Ordering::Acquire)
    }
}
impl Drop for StreamTrack {
    fn drop(&mut self) {
        self.abort();
    }
}

#[inline]
fn decode_stream_frame<S: Source<Item = i16>>(source: &mut S, out_ch: u16) -> Option<(i16, i16)> {
    let first = source.next()?;
    let cur_ch = source.channels().max(1);
    if cur_ch == 1 {
        return Some((first, first));
    }
    let second = source.next()?;
    for _ in 2..cur_ch {
        let _ = source.next()?;
    }
    if out_ch == 1 {
        let mono = ((i32::from(first) + i32::from(second)) / 2) as i16;
        Some((mono, mono))
    } else {
        Some((first, second))
    }
}

fn stream_feeder_loop<S>(mut source: S, out_ch: u16, inner: Arc<StreamInner>)
where
    S: Source<Item = i16>,
{
    let step = usize::from(out_ch);
    loop {
        if inner.aborted.load(Ordering::Acquire) {
            return;
        }
        let free = inner.ring.free_producer();
        if free < step {
            if inner.aborted.load(Ordering::Acquire) {
                return;
            }
            std::thread::park_timeout(Duration::from_millis(15));
            continue;
        }
        let burst_frames = (free / step).min(512);
        for _ in 0..burst_frames {
            if inner.aborted.load(Ordering::Acquire) {
                return;
            }
            match decode_stream_frame(&mut source, out_ch) {
                Some((l, r)) => {
                    let _ = inner.ring.push(l);
                    if out_ch == 2 {
                        let _ = inner.ring.push(r);
                    }
                }
                None => {
                    inner.eof.store(true, Ordering::Release);
                    return;
                }
            }
        }
    }
}

enum Command {
    PlayEffect {
        samples: Arc<[i16]>,
        channels: u16,
        sample_rate: u32,
        category: usize,
        base: f32,
        pitch: f32,
        source: Option<(glam::DVec3, f64)>,
        seq: u64,
    },
    PlayStream {
        track: Arc<StreamTrack>,
        category: usize,
        base: f32,
        pitch: f32,
        source: Option<(glam::DVec3, f64)>,
        seq: u64,
        music_id: u64,
    },
    StopMusic,
    StopAll,
}

enum Retired {
    Pcm(Arc<[i16]>),
    Stream(Arc<StreamTrack>),
}

struct SharedState {
    cmd_ring: SpscRing<Command>,
    retire_ring: SpscRing<Retired>,
    gains: [AtomicU32; 9],
    listener_pos: [AtomicU64; 3],
    listener_yaw: AtomicU32,
    play_seq: AtomicU64,
    paused_up_to_seq: AtomicU64,
    stop_all_seq: AtomicU64,
    music_paused: AtomicBool,
    active_music_id: AtomicU64,
}
impl SharedState {
    fn new() -> Self {
        Self {
            cmd_ring: SpscRing::new(CMD_RING_CAP),
            retire_ring: SpscRing::new(RETIRE_RING_CAP),
            gains: std::array::from_fn(|_| AtomicU32::new(1.0f32.to_bits())),
            listener_pos: std::array::from_fn(|_| AtomicU64::new(0.0f64.to_bits())),
            listener_yaw: AtomicU32::new(0.0f32.to_bits()),
            play_seq: AtomicU64::new(0),
            paused_up_to_seq: AtomicU64::new(0),
            stop_all_seq: AtomicU64::new(0),
            music_paused: AtomicBool::new(false),
            active_music_id: AtomicU64::new(0),
        }
    }
    #[inline]
    fn load_gains(&self) -> [f32; 9] {
        std::array::from_fn(|i| f32::from_bits(self.gains[i].load(Ordering::Relaxed)))
    }
    #[inline]
    fn load_listener(&self) -> (glam::DVec3, f32) {
        let pos = glam::DVec3::new(
            f64::from_bits(self.listener_pos[0].load(Ordering::Relaxed)),
            f64::from_bits(self.listener_pos[1].load(Ordering::Relaxed)),
            f64::from_bits(self.listener_pos[2].load(Ordering::Relaxed)),
        );
        let yaw = f32::from_bits(self.listener_yaw.load(Ordering::Relaxed));
        (pos, yaw)
    }
}

enum VoicePayload {
    Empty,
    Pcm {
        samples: Arc<[i16]>,
        frame_count: usize,
    },
    Stream {
        track: Arc<StreamTrack>,
        f0: (f32, f32),
        f1: (f32, f32),
        initialized: bool,
        eof_tail: u8,
    },
}

struct VoiceSlot {
    payload: VoicePayload,
    channels: u16,
    phase: f64,
    step: f64,
    category: usize,
    base: f32,
    source: Option<(glam::DVec3, f64)>,
    seq: u64,
    music_id: u64,
    cur_l: f32,
    cur_r: f32,
    last_out: (f32, f32),
    just_started: bool,
}
impl VoiceSlot {
    fn empty() -> Self {
        Self {
            payload: VoicePayload::Empty,
            channels: 1,
            phase: 0.0,
            step: 1.0,
            category: 0,
            base: 0.0,
            source: None,
            seq: 0,
            music_id: 0,
            cur_l: 0.0,
            cur_r: 0.0,
            last_out: (0.0, 0.0),
            just_started: false,
        }
    }
    #[inline]
    fn is_active(&self) -> bool {
        !matches!(self.payload, VoicePayload::Empty)
    }
    #[inline]
    fn take_retired(&mut self) -> Option<Retired> {
        let payload = std::mem::replace(&mut self.payload, VoicePayload::Empty);
        self.last_out = (0.0, 0.0);
        self.cur_l = 0.0;
        self.cur_r = 0.0;
        self.music_id = 0;
        match payload {
            VoicePayload::Empty => None,
            VoicePayload::Pcm { samples, .. } => Some(Retired::Pcm(samples)),
            VoicePayload::Stream { track, .. } => Some(Retired::Stream(track)),
        }
    }
}

#[derive(Clone, Copy, Default)]
struct FadeTail {
    active: bool,
    start_l: f32,
    start_r: f32,
    pos: usize,
}

#[inline]
fn push_fade_tail(tails: &mut [FadeTail; MAX_FADE_TAILS], (last_l, last_r): (f32, f32)) {
    if last_l.abs() < 1e-6 && last_r.abs() < 1e-6 {
        return;
    }
    if let Some(slot) = tails.iter_mut().find(|t| !t.active) {
        *slot = FadeTail {
            active: true,
            start_l: last_l,
            start_r: last_r,
            pos: 0,
        };
        return;
    }
    if let Some(slot) = tails.iter_mut().find(|t| t.pos == 0) {
        slot.start_l += last_l;
        slot.start_r += last_r;
        return;
    }
    let slot = tails.iter_mut().max_by_key(|t| t.pos).unwrap();
    let rem = (FADE_FRAMES.saturating_sub(slot.pos)) as f32 / (FADE_FRAMES as f32);
    slot.start_l = slot.start_l * rem + last_l;
    slot.start_r = slot.start_r * rem + last_r;
    slot.pos = 0;
    slot.active = true;
}

/// Zero-mutex, zero-allocation real-time software mixer owned by the WASAPI render thread.
struct Mixer {
    device_rate: f64,
    shared: Arc<SharedState>,
    effects: Box<[VoiceSlot; MAX_EFFECT_VOICES]>,
    music: VoiceSlot,
    tails: [FadeTail; MAX_FADE_TAILS],
    mix_buf: [(f32, f32); MICRO_BLOCK],
    dither_rng: u32,
    dither_sign: f32,
}
impl Mixer {
    fn new(device_rate: u32, shared: Arc<SharedState>) -> Self {
        Self {
            device_rate: f64::from(device_rate.max(1)),
            shared,
            effects: Box::new(std::array::from_fn(|_| VoiceSlot::empty())),
            music: VoiceSlot::empty(),
            tails: [FadeTail::default(); MAX_FADE_TAILS],
            mix_buf: [(0.0, 0.0); MICRO_BLOCK],
            dither_rng: 0xA341_316C,
            dither_sign: 1.0,
        }
    }
    #[inline]
    fn retire_slot(&mut self, is_music: bool, idx: usize, with_fade: bool) {
        let slot = if is_music {
            &mut self.music
        } else {
            &mut self.effects[idx]
        };
        if !slot.is_active() {
            return;
        }
        if with_fade {
            push_fade_tail(&mut self.tails, slot.last_out);
        }
        if is_music && slot.music_id != 0 {
            let _ = self.shared.active_music_id.compare_exchange(
                slot.music_id,
                0,
                Ordering::AcqRel,
                Ordering::Relaxed,
            );
        }
        if let Some(retired) = slot.take_retired() {
            // If the retire ring is full in a pathological scenario, let `retired`
            // drop gracefully rather than panicking or leaking.
            let _ = self.shared.retire_ring.push(retired);
        }
    }
    fn allocate_effect_slot(&mut self, paused_up_to: u64) -> usize {
        if let Some(idx) = self.effects.iter().position(|s| !s.is_active()) {
            return idx;
        }
        // Prefer stealing an already-paused slot first, otherwise steal the quietest/oldest voice.
        let mut best_idx = 0;
        let mut best_paused = false;
        let mut best_gain = f32::INFINITY;
        let mut best_seq = u64::MAX;
        for (i, slot) in self.effects.iter().enumerate() {
            let paused = paused_up_to != 0 && slot.seq <= paused_up_to;
            let peak = slot.cur_l.max(slot.cur_r);
            let better = match (paused, best_paused) {
                (true, false) => true,
                (false, true) => false,
                _ => {
                    peak < best_gain - 1e-4
                        || ((peak - best_gain).abs() <= 1e-4 && slot.seq < best_seq)
                }
            };
            if better {
                best_idx = i;
                best_paused = paused;
                best_gain = peak;
                best_seq = slot.seq;
            }
        }
        self.retire_slot(false, best_idx, true);
        best_idx
    }
    fn process_commands(&mut self) {
        let stop_all_seq = self.shared.stop_all_seq.load(Ordering::Acquire);
        if stop_all_seq != 0 {
            for i in 0..MAX_EFFECT_VOICES {
                if self.effects[i].is_active() && self.effects[i].seq <= stop_all_seq {
                    self.retire_slot(false, i, true);
                }
            }
        }
        let paused_up_to = self.shared.paused_up_to_seq.load(Ordering::Acquire);
        while let Some(cmd) = self.shared.cmd_ring.pop() {
            match cmd {
                Command::PlayEffect {
                    samples,
                    channels,
                    sample_rate,
                    category,
                    base,
                    pitch,
                    source,
                    seq,
                } => {
                    let ch = channels.max(1);
                    let frame_count = samples.len() / usize::from(ch);
                    let step =
                        (f64::from(sample_rate) * f64::from(pitch)) / self.device_rate;
                    if seq <= self.shared.stop_all_seq.load(Ordering::Acquire)
                        || frame_count == 0
                        || !(step > 0.0 && step.is_finite())
                    {
                        let _ = self.shared.retire_ring.push(Retired::Pcm(samples));
                        continue;
                    }
                    let idx = self.allocate_effect_slot(paused_up_to);
                    self.effects[idx] = VoiceSlot {
                        payload: VoicePayload::Pcm {
                            samples,
                            frame_count,
                        },
                        channels: ch,
                        phase: 0.0,
                        step,
                        category,
                        base,
                        source,
                        seq,
                        music_id: 0,
                        cur_l: 0.0,
                        cur_r: 0.0,
                        last_out: (0.0, 0.0),
                        just_started: true,
                    };
                }
                Command::PlayStream {
                    track,
                    category,
                    base,
                    pitch,
                    source,
                    seq,
                    music_id,
                } => {
                    let ch = track.inner.channels.max(1);
                    let step = (f64::from(track.inner.sample_rate) * f64::from(pitch))
                        / self.device_rate;
                    if !(step > 0.0 && step.is_finite()) {
                        let _ = self.shared.retire_ring.push(Retired::Stream(track));
                        continue;
                    }
                    if music_id != 0 {
                        if self.shared.active_music_id.load(Ordering::Acquire) == 0 {
                            let _ = self.shared.retire_ring.push(Retired::Stream(track));
                            continue;
                        }
                        self.retire_slot(true, 0, true);
                        self.music = VoiceSlot {
                            payload: VoicePayload::Stream {
                                track,
                                f0: (0.0, 0.0),
                                f1: (0.0, 0.0),
                                initialized: false,
                                eof_tail: 0,
                            },
                            channels: ch,
                            phase: 0.0,
                            step,
                            category,
                            base,
                            source: None,
                            seq: 0,
                            music_id,
                            cur_l: 0.0,
                            cur_r: 0.0,
                            last_out: (0.0, 0.0),
                            just_started: true,
                        };
                    } else {
                        if seq <= self.shared.stop_all_seq.load(Ordering::Acquire) {
                            let _ = self.shared.retire_ring.push(Retired::Stream(track));
                            continue;
                        }
                        let idx = self.allocate_effect_slot(paused_up_to);
                        self.effects[idx] = VoiceSlot {
                            payload: VoicePayload::Stream {
                                track,
                                f0: (0.0, 0.0),
                                f1: (0.0, 0.0),
                                initialized: false,
                                eof_tail: 0,
                            },
                            channels: ch,
                            phase: 0.0,
                            step,
                            category,
                            base,
                            source,
                            seq,
                            music_id: 0,
                            cur_l: 0.0,
                            cur_r: 0.0,
                            last_out: (0.0, 0.0),
                            just_started: true,
                        };
                    }
                }
                Command::StopMusic => {
                    self.retire_slot(true, 0, true);
                }
                Command::StopAll => {
                    for i in 0..MAX_EFFECT_VOICES {
                        self.retire_slot(false, i, true);
                    }
                    self.retire_slot(true, 0, true);
                }
            }
        }
    }

    fn mix_micro_block(&mut self, frames: usize) {
        debug_assert!(frames <= MICRO_BLOCK);
        for item in &mut self.mix_buf[..frames] {
            *item = (0.0, 0.0);
        }

        // Mix active 96-frame click-free fade-out tails first.
        const INV_FADE: f32 = 1.0 / (FADE_FRAMES as f32);
        for tail in &mut self.tails {
            if !tail.active {
                continue;
            }
            let n = frames.min(FADE_FRAMES.saturating_sub(tail.pos));
            for i in 0..n {
                let factor = (FADE_FRAMES - 1 - (tail.pos + i)) as f32 * INV_FADE;
                self.mix_buf[i].0 += tail.start_l * factor;
                self.mix_buf[i].1 += tail.start_r * factor;
            }
            tail.pos += n;
            if tail.pos >= FADE_FRAMES {
                tail.active = false;
            }
        }

        let gains = self.shared.load_gains();
        let (ear_pos, ear_yaw) = self.shared.load_listener();
        let paused_up_to = self.shared.paused_up_to_seq.load(Ordering::Acquire);
        let music_paused = self.shared.music_paused.load(Ordering::Acquire);

        for i in 0..MAX_EFFECT_VOICES {
            if !self.effects[i].is_active() {
                continue;
            }
            let is_paused = paused_up_to != 0 && self.effects[i].seq <= paused_up_to;
            let finished = render_voice_block(
                &mut self.effects[i],
                &mut self.mix_buf[..frames],
                &gains,
                ear_pos,
                ear_yaw,
                is_paused,
                true,
            );
            if finished {
                self.retire_slot(false, i, false);
            }
        }

        if self.music.is_active() {
            let finished = render_voice_block(
                &mut self.music,
                &mut self.mix_buf[..frames],
                &gains,
                ear_pos,
                ear_yaw,
                music_paused,
                false,
            );
            if finished {
                self.retire_slot(true, 0, false);
            }
        }
    }

    fn render_output<T: OutputSample>(&mut self, out: &mut [T], channels: usize) {
        if channels == 0 {
            for sample in out {
                *sample = T::from_mixer_f32(0.0);
            }
            return;
        }
        self.process_commands();
        let total_frames = out.len() / channels;
        let mut offset_frames = 0;
        while offset_frames < total_frames {
            let block_frames = (total_frames - offset_frames).min(MICRO_BLOCK);
            self.mix_micro_block(block_frames);
            let out_slice =
                &mut out[offset_frames * channels..(offset_frames + block_frames) * channels];
            if channels == 2 {
                for (i, pair) in out_slice.chunks_exact_mut(2).enumerate() {
                    self.dither_sign = -self.dither_sign;
                    let (dry_l, dry_r) = self.mix_buf[i];
                    let l = apply_keep_alive_dither(
                        soft_clip(dry_l),
                        self.dither_sign,
                        &mut self.dither_rng,
                    );
                    let r = apply_keep_alive_dither(
                        soft_clip(dry_r),
                        self.dither_sign,
                        &mut self.dither_rng,
                    );
                    pair[0] = T::from_mixer_f32(l);
                    pair[1] = T::from_mixer_f32(r);
                }
            } else if channels == 1 {
                for (i, slot) in out_slice.iter_mut().enumerate() {
                    self.dither_sign = -self.dither_sign;
                    let (dry_l, dry_r) = self.mix_buf[i];
                    let mono = apply_keep_alive_dither(
                        soft_clip((dry_l + dry_r) * 0.5),
                        self.dither_sign,
                        &mut self.dither_rng,
                    );
                    *slot = T::from_mixer_f32(mono);
                }
            } else {
                for (i, frame) in out_slice.chunks_exact_mut(channels).enumerate() {
                    self.dither_sign = -self.dither_sign;
                    let (dry_l, dry_r) = self.mix_buf[i];
                    frame[0] = T::from_mixer_f32(apply_keep_alive_dither(
                        soft_clip(dry_l),
                        self.dither_sign,
                        &mut self.dither_rng,
                    ));
                    frame[1] = T::from_mixer_f32(apply_keep_alive_dither(
                        soft_clip(dry_r),
                        self.dither_sign,
                        &mut self.dither_rng,
                    ));
                    for extra in &mut frame[2..] {
                        *extra = T::from_mixer_f32(apply_keep_alive_dither(
                            0.0,
                            self.dither_sign,
                            &mut self.dither_rng,
                        ));
                    }
                }
            }
            offset_frames += block_frames;
        }
    }
}

/// Renders up to `MICRO_BLOCK` frames for a single voice; returns `true` when the voice finishes.
fn render_voice_block(
    voice: &mut VoiceSlot,
    mix_buf: &mut [(f32, f32)],
    gains: &[f32; 9],
    ear_pos: glam::DVec3,
    ear_yaw: f32,
    is_paused: bool,
    retire_when_muted: bool,
) -> bool {
    let frames = mix_buf.len();
    if frames == 0 {
        return false;
    }
    let cat_gain = gain(gains, voice.category, voice.base);
    let (raw_target_l, raw_target_r) = match voice.source {
        Some((pos, radius)) => {
            let (pl, pr) = spatial_gains(pos, radius, ear_pos, ear_yaw);
            (cat_gain * pl, cat_gain * pr)
        }
        None => (cat_gain, cat_gain),
    };
    let (target_l, target_r) = if is_paused {
        (0.0, 0.0)
    } else {
        (raw_target_l, raw_target_r)
    };
    if voice.just_started {
        voice.just_started = false;
        voice.cur_l = target_l;
        voice.cur_r = target_r;
    }

    if is_paused && voice.cur_l == 0.0 && voice.cur_r == 0.0 {
        voice.last_out = (0.0, 0.0);
        return false;
    }
    if !is_paused && cat_gain <= 0.0 && voice.cur_l == 0.0 && voice.cur_r == 0.0 && retire_when_muted
    {
        voice.last_out = (0.0, 0.0);
        return true;
    }

    const INV_BLOCK: f32 = 1.0 / (MICRO_BLOCK as f32);
    let step_l = (target_l - voice.cur_l) * INV_BLOCK;
    let step_r = (target_r - voice.cur_r) * INV_BLOCK;
    let mut g_l = voice.cur_l;
    let mut g_r = voice.cur_r;
    let fold_3d_stereo = voice.source.is_some() && voice.channels >= 2;
    let ch = voice.channels;
    let phase_step = voice.step;

    match &mut voice.payload {
        VoicePayload::Empty => return true,
        VoicePayload::Pcm {
            samples,
            frame_count,
        } => {
            let fc = *frame_count;
            if !is_paused
                && voice.cur_l == 0.0
                && voice.cur_r == 0.0
                && target_l == 0.0
                && target_r == 0.0
            {
                voice.phase += phase_step * (frames as f64);
                voice.last_out = (0.0, 0.0);
                return voice.phase >= fc as f64;
            }
            let slice = &**samples;
            for out_frame in mix_buf.iter_mut() {
                if voice.phase >= fc as f64 {
                    voice.last_out = (0.0, 0.0);
                    return true;
                }
                g_l += step_l;
                g_r += step_r;
                let idx = voice.phase as usize;
                let frac = (voice.phase - idx as f64) as f32;
                let s0 = pcm_frame(slice, ch, idx);
                let s1 = if idx + 1 < fc {
                    pcm_frame(slice, ch, idx + 1)
                } else {
                    (0.0, 0.0)
                };
                let raw_l = s0.0 + (s1.0 - s0.0) * frac;
                let raw_r = s0.1 + (s1.1 - s0.1) * frac;
                let (sig_l, sig_r) = if fold_3d_stereo {
                    let m = (raw_l + raw_r) * 0.5;
                    (m, m)
                } else {
                    (raw_l, raw_r)
                };
                let out_l = sig_l * g_l;
                let out_r = sig_r * g_r;
                voice.last_out = (out_l, out_r);
                out_frame.0 += out_l;
                out_frame.1 += out_r;
                voice.phase += phase_step;
            }
        }
        VoicePayload::Stream {
            track,
            f0,
            f1,
            initialized,
            eof_tail,
        } => {
            for out_frame in mix_buf.iter_mut() {
                if !*initialized {
                    let eof_before = track.is_eof();
                    if let Some(first) = track.pop_frame() {
                        *f0 = first;
                        let eof_second = track.is_eof();
                        if let Some(second) = track.pop_frame() {
                            *f1 = second;
                            *eof_tail = 0;
                        } else if eof_second {
                            *f1 = (0.0, 0.0);
                            *eof_tail = 1;
                        } else {
                            *f1 = first;
                            *eof_tail = 0;
                        }
                        *initialized = true;
                    } else if eof_before {
                        voice.last_out = (0.0, 0.0);
                        return true;
                    } else {
                        // Transient feeder underrun before first sample; hold cleanly.
                        voice.last_out = (0.0, 0.0);
                        break;
                    }
                }

                let mut underrun = false;
                while voice.phase >= 1.0 {
                    if *eof_tail >= 1 {
                        *eof_tail = 2;
                        break;
                    }
                    let eof_observed = track.is_eof();
                    if let Some(next_frame) = track.pop_frame() {
                        *f0 = *f1;
                        *f1 = next_frame;
                        voice.phase -= 1.0;
                    } else if eof_observed {
                        *f0 = *f1;
                        *f1 = (0.0, 0.0);
                        *eof_tail = 1;
                        voice.phase -= 1.0;
                    } else {
                        voice.phase = 1.0;
                        underrun = true;
                        break;
                    }
                }
                if *eof_tail >= 2 {
                    voice.last_out = (0.0, 0.0);
                    return true;
                }
                if underrun {
                    voice.last_out = (0.0, 0.0);
                    break;
                }

                g_l += step_l;
                g_r += step_r;
                let frac = voice.phase as f32;
                let raw_l = f0.0 + (f1.0 - f0.0) * frac;
                let raw_r = f0.1 + (f1.1 - f0.1) * frac;
                let (sig_l, sig_r) = if fold_3d_stereo {
                    let m = (raw_l + raw_r) * 0.5;
                    (m, m)
                } else {
                    (raw_l, raw_r)
                };
                let out_l = sig_l * g_l;
                let out_r = sig_r * g_r;
                voice.last_out = (out_l, out_r);
                out_frame.0 += out_l;
                out_frame.1 += out_r;
                voice.phase += phase_step;
            }
        }
    }

    if frames == MICRO_BLOCK || (g_l - target_l).abs() < 1e-6 {
        voice.cur_l = target_l;
    } else {
        voice.cur_l = g_l;
    }
    if frames == MICRO_BLOCK || (g_r - target_r).abs() < 1e-6 {
        voice.cur_r = target_r;
    } else {
        voice.cur_r = g_r;
    }
    false
}

#[inline]
fn i16_to_f32(s: i16) -> f32 {
    f32::from(s) * (1.0 / 32768.0)
}

#[inline]
fn f32_to_i16(v: f32) -> i16 {
    (v * 32768.0).clamp(-32768.0, 32767.0) as i16
}

#[inline]
fn pcm_frame(samples: &[i16], channels: u16, idx: usize) -> (f32, f32) {
    if channels == 1 {
        let v = i16_to_f32(samples[idx]);
        (v, v)
    } else {
        let base = idx * usize::from(channels);
        (i16_to_f32(samples[base]), i16_to_f32(samples[base + 1]))
    }
}

/// Linear distance attenuation `(1 - dist / radius).clamp(0, 1)` and constant-power stereo pan.
#[inline]
fn spatial_gains(
    source: glam::DVec3,
    radius: f64,
    ear: glam::DVec3,
    yaw: f32,
) -> (f32, f32) {
    if !source.is_finite() || !ear.is_finite() || !yaw.is_finite() || !(radius > 0.0) {
        return (0.0, 0.0);
    }
    let delta = source - ear;
    let distance = delta.length();
    let atten = (1.0 - distance / radius).clamp(0.0, 1.0) as f32;
    if atten <= 0.0 {
        return (0.0, 0.0);
    }
    let right = glam::DVec3::new(f64::from(yaw.cos()), 0.0, f64::from(yaw.sin()));
    let pan = right.dot(delta.normalize_or_zero()).clamp(-1.0, 1.0) as f32;
    let pan_l = (1.0 - pan).min(1.0).sqrt();
    let pan_r = (1.0 + pan).min(1.0).sqrt();
    (atten * pan_l, atten * pan_r)
}

#[inline]
fn should_cull_3d(pos: glam::DVec3, listener_pos: glam::DVec3, volume: f32) -> bool {
    let range = 16.0 * f64::from(volume.max(1.0));
    !(volume > 0.0) || !(pos.distance_squared(listener_pos) < range * range)
}

/// Smooth C1 master soft-clip knee above `±0.95` asymptotically bounded by `±1.0`.
#[inline]
fn soft_clip(x: f32) -> f32 {
    if !x.is_finite() {
        return 0.0;
    }
    const KNEE: f32 = 0.95;
    const HEADROOM: f32 = 1.0 - KNEE;
    const INV_HEADROOM: f32 = 1.0 / HEADROOM;
    let abs_x = x.abs();
    if abs_x <= KNEE {
        x
    } else {
        let u = (abs_x - KNEE) * INV_HEADROOM;
        let compressed = KNEE + HEADROOM * (u / (1.0 + u));
        compressed.copysign(x)
    }
}

#[inline]
fn xorshift32(state: &mut u32) -> u32 {
    let mut x = *state;
    if x == 0 {
        x = 0x9E37_79B9;
    }
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}

/// Zero-mean DC-free TPDF dither + alternating `±2 / 32768` (`-84.3 dBFS`) Bluetooth keep-alive
/// floor so Galaxy Buds + Windows `BthA2dp.sys` never enter zero-PCM silence micro-sleep.
#[inline]
fn apply_keep_alive_dither(clipped: f32, sign: f32, rng: &mut u32) -> f32 {
    const LSB: f32 = 1.0 / 32768.0;
    const KEEP_ALIVE: f32 = 2.0 * LSB;
    const TPDF_SCALE: f32 = 0.5 * LSB;
    const MIN_NONZERO: f32 = 1.25 * LSB;
    const FALLBACK_NONZERO: f32 = 1.5 * LSB;
    const INV_U24: f32 = 1.0 / 16_777_216.0;

    let u1 = (xorshift32(rng) >> 8) as f32 * INV_U24;
    let u2 = (xorshift32(rng) >> 8) as f32 * INV_U24;
    let tpdf = (u1 - u2) * TPDF_SCALE;
    let mut out = (clipped + sign * KEEP_ALIVE + tpdf).clamp(-1.0, 1.0);
    if out.abs() < MIN_NONZERO {
        out = sign * FALLBACK_NONZERO;
    }
    out
}

trait OutputSample: cpal::SizedSample {
    fn from_mixer_f32(v: f32) -> Self;
}
impl OutputSample for f32 {
    #[inline]
    fn from_mixer_f32(v: f32) -> Self {
        v
    }
}
impl OutputSample for i16 {
    #[inline]
    fn from_mixer_f32(v: f32) -> Self {
        f32_to_i16(v)
    }
}
impl OutputSample for u16 {
    #[inline]
    fn from_mixer_f32(v: f32) -> Self {
        (i32::from(f32_to_i16(v)) + 32768) as u16
    }
}

/// Promotes the `cpal` render thread to `THREAD_PRIORITY_TIME_CRITICAL` + MMCSS `"Pro Audio"`
/// on its first invocation and reverts MMCSS cleanly on stream teardown.
struct AudioThreadGuard {
    initialized: bool,
    #[cfg(windows)]
    avrt_handle: isize,
}
impl AudioThreadGuard {
    fn new() -> Self {
        Self {
            initialized: false,
            #[cfg(windows)]
            avrt_handle: 0,
        }
    }
    #[inline]
    fn promote_once(&mut self) {
        if self.initialized {
            return;
        }
        self.initialized = true;
        #[cfg(windows)]
        unsafe {
            #[link(name = "kernel32")]
            extern "system" {
                fn GetCurrentThread() -> *mut std::ffi::c_void;
                fn SetThreadPriority(thread: *mut std::ffi::c_void, priority: i32) -> i32;
            }
            #[link(name = "avrt")]
            extern "system" {
                fn AvSetMmThreadCharacteristicsW(
                    task_name: *const u16,
                    task_index: *mut u32,
                ) -> isize;
            }
            const THREAD_PRIORITY_TIME_CRITICAL: i32 = 15;
            const PRO_AUDIO_UTF16: [u16; 10] = [
                b'P' as u16,
                b'r' as u16,
                b'o' as u16,
                b' ' as u16,
                b'A' as u16,
                b'u' as u16,
                b'd' as u16,
                b'i' as u16,
                b'o' as u16,
                0,
            ];
            let mut task_index = 0u32;
            let handle =
                AvSetMmThreadCharacteristicsW(PRO_AUDIO_UTF16.as_ptr(), &mut task_index);
            if handle != 0 {
                self.avrt_handle = handle;
            }
            let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
        }
    }
}
impl Drop for AudioThreadGuard {
    fn drop(&mut self) {
        #[cfg(windows)]
        if self.avrt_handle != 0 {
            unsafe {
                #[link(name = "avrt")]
                extern "system" {
                    fn AvRevertMmThreadCharacteristics(avrt_handle: isize) -> i32;
                }
                let _ = AvRevertMmThreadCharacteristics(self.avrt_handle);
            }
            self.avrt_handle = 0;
        }
    }
}

struct OutputHandle {
    _stream: cpal::Stream,
    device: cpal::Device,
    stream_failed: Arc<AtomicBool>,
}

fn same_device(a: &cpal::Device, b: &cpal::Device) -> bool {
    #[cfg(windows)]
    match (a.as_inner(), b.as_inner()) {
        (cpal::platform::DeviceInner::Wasapi(da), cpal::platform::DeviceInner::Wasapi(db)) => {
            da == db
        }
        #[allow(unreachable_patterns)]
        _ => a.name().ok() == b.name().ok(),
    }
    #[cfg(not(windows))]
    {
        a.name().ok() == b.name().ok()
    }
}

fn select_output_config(device: &cpal::Device) -> Result<SupportedStreamConfig, String> {
    let default_cfg = device
        .default_output_config()
        .map_err(|e| format!("default_output_config failed: {e}"))?;
    if matches!(
        default_cfg.sample_format(),
        SampleFormat::F32 | SampleFormat::I16 | SampleFormat::U16
    ) {
        return Ok(default_cfg);
    }
    if let Ok(configs) = device.supported_output_configs() {
        for range in configs {
            if matches!(
                range.sample_format(),
                SampleFormat::F32 | SampleFormat::I16 | SampleFormat::U16
            ) {
                let target_rate = SampleRate(48_000)
                    .max(range.min_sample_rate())
                    .min(range.max_sample_rate());
                if let Some(cfg) = range.try_with_sample_rate(target_rate) {
                    return Ok(cfg);
                }
            }
        }
    }
    Err(format!(
        "unsupported output sample format {:?}",
        default_cfg.sample_format()
    ))
}

fn compute_target_frames(supported: &SupportedStreamConfig) -> u32 {
    let sample_rate = supported.sample_rate().0;
    let raw_target = ((u64::from(sample_rate) * 45) / 1000).max(1024) as u32;
    match supported.buffer_size() {
        SupportedBufferSize::Range { min, max } if *min <= *max && *max > 0 => {
            raw_target.max(*min).min(*max)
        }
        _ => raw_target,
    }
}

fn build_typed_stream<T: OutputSample>(
    device: &cpal::Device,
    config: &StreamConfig,
    shared: &Arc<SharedState>,
    stream_failed: &Arc<AtomicBool>,
) -> Result<cpal::Stream, cpal::BuildStreamError> {
    let channels = usize::from(config.channels);
    let mut mixer = Mixer::new(config.sample_rate.0, Arc::clone(shared));
    let mut thread_guard = AudioThreadGuard::new();
    let err_flag = Arc::clone(stream_failed);
    device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            thread_guard.promote_once();
            mixer.render_output(data, channels);
        },
        move |err| {
            err_flag.store(true, Ordering::Release);
            eprintln!("Audio stream error: {err}");
        },
        None,
    )
}

fn build_stream_with_format(
    device: &cpal::Device,
    sample_format: SampleFormat,
    config: &StreamConfig,
    shared: &Arc<SharedState>,
    stream_failed: &Arc<AtomicBool>,
) -> Result<cpal::Stream, cpal::BuildStreamError> {
    match sample_format {
        SampleFormat::F32 => build_typed_stream::<f32>(device, config, shared, stream_failed),
        SampleFormat::I16 => build_typed_stream::<i16>(device, config, shared, stream_failed),
        SampleFormat::U16 => build_typed_stream::<u16>(device, config, shared, stream_failed),
        _ => Err(cpal::BuildStreamError::StreamConfigNotSupported),
    }
}

fn open_device_output(
    device: cpal::Device,
    shared: &Arc<SharedState>,
) -> Result<OutputHandle, String> {
    let supported = select_output_config(&device)?;
    let sample_format = supported.sample_format();
    let target_frames = compute_target_frames(&supported);
    let stream_failed = Arc::new(AtomicBool::new(false));

    let fixed_config = StreamConfig {
        channels: supported.channels(),
        sample_rate: supported.sample_rate(),
        buffer_size: BufferSize::Fixed(target_frames),
    };
    let stream = match build_stream_with_format(
        &device,
        sample_format,
        &fixed_config,
        shared,
        &stream_failed,
    ) {
        Ok(s) => s,
        Err(_) => {
            stream_failed.store(false, Ordering::Relaxed);
            let default_config = StreamConfig {
                channels: supported.channels(),
                sample_rate: supported.sample_rate(),
                buffer_size: BufferSize::Default,
            };
            build_stream_with_format(
                &device,
                sample_format,
                &default_config,
                shared,
                &stream_failed,
            )
            .map_err(|e| format!("build_output_stream failed: {e}"))?
        }
    };
    stream
        .play()
        .map_err(|e| format!("stream play failed: {e}"))?;
    Ok(OutputHandle {
        _stream: stream,
        device,
        stream_failed,
    })
}

fn open_default_output(shared: &Arc<SharedState>) -> Result<OutputHandle, String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or_else(|| "no default output device".to_string())?;
    open_device_output(device, shared)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MusicMode {
    Menu,
    Game,
    Creative,
    Nether,
    End,
}
impl MusicMode {
    pub fn for_world(world: Option<(i32, bool)>) -> Self {
        match world {
            None => Self::Menu,
            Some((-1, _)) => Self::Nether,
            Some((1, _)) => Self::End,
            Some((_, true)) => Self::Creative,
            Some((_, false)) => Self::Game,
        }
    }
    fn event(self) -> (&'static str, i32, i32) {
        match self {
            Self::Menu => ("music.menu", 20, 600),
            Self::Game => ("music.game", 12000, 24000),
            Self::Creative => ("music.game.creative", 1200, 3600),
            Self::Nether => ("music.game.nether", 1200, 3600),
            Self::End => ("music.game.end", 6000, 24000),
        }
    }
}

struct MusicClock {
    delay: i32,
    current: Option<MusicMode>,
}
impl Default for MusicClock {
    fn default() -> Self {
        Self {
            delay: 100,
            current: None,
        }
    }
}
impl MusicClock {
    fn tick(&mut self, mode: MusicMode, playing: bool, rng: &mut JavaRandom) -> bool {
        let (_, min, max) = mode.event();
        if let Some(old) = self.current {
            if old != mode {
                self.delay = rng.next_int(min / 2 + 1);
            }
            if old != mode || !playing {
                self.current = None;
                self.delay = self.delay.min(min + rng.next_int(max - min + 1));
            }
        }
        if self.current.is_some() {
            return false;
        }
        if self.delay > 0 {
            self.delay -= 1;
            return false;
        }
        self.current = Some(mode);
        self.delay = i32::MAX;
        true
    }
}

#[inline]
fn gain(gains: &[f32; 9], category: usize, base: f32) -> f32 {
    base.clamp(0., 1.) * gains[0] * if category == 0 { 1. } else { gains[category] }
}

/// Window-thread work only; decoder playback on the mixer thread is excluded.
#[derive(Clone, Default, serde::Serialize)]
pub(crate) struct ProfileStats {
    pub play_calls: u64,
    pub play_ms: f64,
    pub bank_builds: u64,
    pub bank_ms: f64,
    pub resource_reads: u64,
    pub read_bytes: u64,
    pub read_ms: f64,
    pub decoder_calls: u64,
    pub decoder_ms: f64,
    pub pcm_hits: u64,
    pub pcm_misses: u64,
    pub pcm_preparations: u64,
    pub pcm_prepare_ms: f64,
    pub sink_calls: u64,
    pub sink_ms: f64,
}

enum PreparedClip {
    Pcm(pcm_cache::Pcm),
    Stream(Arc<StreamTrack>),
}

struct ActiveMusic {
    track: Arc<StreamTrack>,
    category: usize,
    base: f32,
    pitch: f32,
    music_id: u64,
}

pub struct Audio {
    output: Option<OutputHandle>,
    shared: Arc<SharedState>,
    cache: HashMap<String, Arc<[u8]>>,
    pcm_cache: pcm_cache::Cache,
    bank: Option<Bank>,
    listener: (glam::DVec3, f32),
    gains: [f32; 9],
    current_music: Option<ActiveMusic>,
    next_music_id: u64,
    clock: MusicClock,
    last_tick: Instant,
    last_device_check: Instant,
    rng: JavaRandom,
    profile_enabled: bool,
    profile: ProfileStats,
}
impl Audio {
    pub fn new() -> Self {
        let shared = Arc::new(SharedState::new());
        let output = open_default_output(&shared)
            .map_err(|e| eprintln!("Audio unavailable: {e}"))
            .ok();
        let now = Instant::now();
        Self {
            output,
            shared,
            cache: HashMap::new(),
            pcm_cache: Default::default(),
            bank: None,
            listener: (glam::DVec3::ZERO, 0.),
            gains: [1.; 9],
            current_music: None,
            next_music_id: 1,
            clock: MusicClock::default(),
            last_tick: now,
            last_device_check: now,
            rng: JavaRandom::new(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as i64,
            ),
            profile_enabled: false,
            profile: Default::default(),
        }
    }
    pub(crate) fn set_profiling(&mut self, enabled: bool) {
        if self.profile_enabled != enabled {
            self.profile = Default::default();
        }
        self.profile_enabled = enabled;
    }
    pub(crate) fn take_profile(&mut self) -> ProfileStats {
        std::mem::take(&mut self.profile)
    }
    #[inline]
    fn profile_timer(&self) -> Option<Instant> {
        self.profile_enabled.then(Instant::now)
    }
    #[inline]
    fn drain_retired(&mut self) {
        while let Some(retired) = self.shared.retire_ring.pop() {
            match retired {
                Retired::Pcm(samples) => drop(samples),
                Retired::Stream(track) => drop(track),
            }
        }
        if self.current_music.is_some()
            && self.shared.active_music_id.load(Ordering::Acquire) == 0
        {
            self.current_music = None;
        }
    }
    fn stop_music(&mut self) {
        self.shared.active_music_id.store(0, Ordering::Release);
        if let Some(active) = self.current_music.take() {
            active.track.abort();
        }
        let _ = self.shared.cmd_ring.push(Command::StopMusic);
        self.drain_retired();
    }
    fn stop_all_voices(&mut self) {
        let latest_seq = self.shared.play_seq.load(Ordering::Acquire);
        self.shared.stop_all_seq.store(latest_seq, Ordering::Release);
        self.shared.paused_up_to_seq.store(0, Ordering::Release);
        self.shared.music_paused.store(false, Ordering::Release);
        self.shared.active_music_id.store(0, Ordering::Release);
        if let Some(active) = self.current_music.take() {
            active.track.abort();
        }
        let _ = self.shared.cmd_ring.push(Command::StopAll);
        self.drain_retired();
    }
    fn poll_device_hot_swap(&mut self, now: Instant) {
        let failed = self
            .output
            .as_ref()
            .is_some_and(|out| out.stream_failed.load(Ordering::Acquire));
        if failed {
            // Dropping `output` synchronously terminates and joins the old WASAPI thread
            // before we attach a new consumer to the SPSC rings.
            self.output = None;
            self.drain_retired();
            self.last_device_check = now;
            if let Ok(new_out) = open_default_output(&self.shared) {
                self.output = Some(new_out);
                self.requeue_music_after_swap();
            }
            return;
        }
        if now.duration_since(self.last_device_check) < DEVICE_POLL_INTERVAL {
            return;
        }
        self.last_device_check = now;
        let default_dev = cpal::default_host().default_output_device();
        match (&self.output, default_dev) {
            (Some(cur), Some(new_dev)) if !same_device(&cur.device, &new_dev) => {
                self.output = None;
                self.drain_retired();
                if let Ok(new_out) = open_device_output(new_dev, &self.shared)
                    .or_else(|_| open_default_output(&self.shared))
                {
                    self.output = Some(new_out);
                    self.requeue_music_after_swap();
                }
            }
            (Some(_), None) => {
                self.output = None;
                self.drain_retired();
            }
            (None, Some(new_dev)) => {
                self.drain_retired();
                if let Ok(new_out) = open_device_output(new_dev, &self.shared) {
                    self.output = Some(new_out);
                    self.requeue_music_after_swap();
                }
            }
            _ => {}
        }
    }
    fn requeue_music_after_swap(&mut self) {
        if let Some(active) = &self.current_music {
            if self.shared.active_music_id.load(Ordering::Acquire) == active.music_id {
                let _ = self.shared.cmd_ring.push(Command::PlayStream {
                    track: Arc::clone(&active.track),
                    category: active.category,
                    base: active.base,
                    pitch: active.pitch,
                    source: None,
                    seq: 0,
                    music_id: active.music_id,
                });
            }
        }
    }
    pub fn clear_resources(&mut self) {
        self.cache.clear();
        self.pcm_cache.clear();
        self.bank = None;
        self.stop_all_voices();
    }
    pub fn pause_sounds(&self) {
        let watermark = self.shared.play_seq.load(Ordering::Acquire);
        self.shared
            .paused_up_to_seq
            .store(watermark, Ordering::Release);
    }
    pub fn resume_sounds(&self) {
        self.shared.paused_up_to_seq.store(0, Ordering::Release);
    }
    pub fn leave_world(&mut self) {
        self.stop_all_voices();
    }
    fn load_bank(&mut self, r: &Resources) {
        if self.bank.is_none() {
            let started = self.profile_timer();
            let mut bank = Bank::default();
            match r.sound_manifests() {
                Ok(layers) => {
                    for layer in layers {
                        if let Err(e) = bank.append(&layer, |s| r.has_sound(s)) {
                            eprintln!("Invalid sounds.json: {e}");
                        }
                    }
                }
                Err(e) => eprintln!("Missing sounds.json: {e}"),
            }
            bank.finish();
            self.bank = Some(bank);
            if let Some(started) = started {
                self.profile.bank_builds += 1;
                self.profile.bank_ms += started.elapsed().as_secs_f64() * 1000.;
            }
        }
    }
    pub fn set_levels(&mut self, master: f32, levels: &[f32; 8]) {
        self.drain_retired();
        if self.gains[0] == master && self.gains[1..] == levels[..] {
            return;
        }
        self.gains[0] = master;
        self.gains[1..].copy_from_slice(levels);
        for (slot, &g) in self.shared.gains.iter().zip(&self.gains) {
            slot.store(g.to_bits(), Ordering::Relaxed);
        }
        if let Some(active) = &self.current_music {
            if gain(&self.gains, active.category, active.base) <= 0. {
                self.stop_music();
            }
        }
    }
    pub fn music(&mut self, r: &Resources, mode: MusicMode, paused: bool) {
        let now = Instant::now();
        self.poll_device_hot_swap(now);
        self.drain_retired();
        if self.output.is_none() {
            return;
        }
        if paused {
            self.shared.music_paused.store(true, Ordering::Release);
            self.last_tick = now;
            return;
        }
        self.shared.music_paused.store(false, Ordering::Release);
        let ticks = now.duration_since(self.last_tick).as_millis() / 50;
        if ticks == 0 {
            return;
        }
        self.last_tick += Duration::from_millis(ticks as u64 * 50);
        // Like Timer.elapsedTicks, limit catch-up after a stalled frame.
        for _ in 0..ticks.min(10) {
            if self.clock.current.is_some_and(|old| old != mode) {
                self.stop_music();
            }
            let playing = self.shared.active_music_id.load(Ordering::Acquire) != 0;
            if !playing {
                self.current_music = None;
            }
            if self.clock.tick(mode, playing, &mut self.rng) {
                self.load_bank(r);
                if let Some((clip, category)) = self
                    .bank
                    .as_ref()
                    .unwrap()
                    .pick(mode.event().0, &mut self.rng)
                {
                    self.start_music(r, clip, category);
                }
            }
        }
    }
    pub fn play_ui(&mut self, r: &Resources) {
        self.load_bank(r);
        let choice = self
            .bank
            .as_ref()
            .unwrap()
            .pick("gui.button.press", &mut self.rng);
        self.play_choice(r, choice, 0.25, 1.);
    }
    pub fn play_secret(&mut self, r: &Resources) {
        self.load_bank(r);
        let choice = self.bank.as_ref().unwrap().secret(&mut self.rng);
        self.play_choice(r, choice, 0.25, 0.5);
    }
    pub fn play(&mut self, r: &Resources, name: &str, volume: f32) {
        self.play_pitched(r, name, volume, 1.);
    }
    pub fn play_pitched(&mut self, r: &Resources, name: &str, volume: f32, pitch: f32) {
        self.play_event(r, name, volume, pitch, None);
    }
    pub fn listener(&mut self, position: glam::DVec3, yaw: f32) {
        self.listener = (position, yaw);
        self.shared.listener_pos[0].store(position.x.to_bits(), Ordering::Relaxed);
        self.shared.listener_pos[1].store(position.y.to_bits(), Ordering::Relaxed);
        self.shared.listener_pos[2].store(position.z.to_bits(), Ordering::Relaxed);
        self.shared
            .listener_yaw
            .store(yaw.to_bits(), Ordering::Relaxed);
        self.drain_retired();
    }
    pub fn play_world(
        &mut self,
        r: &Resources,
        name: &str,
        volume: f32,
        pitch: f32,
        source: glam::DVec3,
    ) {
        self.play_event(r, name, volume, pitch, Some(source));
    }
    fn play_event(
        &mut self,
        r: &Resources,
        name: &str,
        volume: f32,
        pitch: f32,
        source: Option<glam::DVec3>,
    ) {
        if let Some(pos) = source {
            if should_cull_3d(pos, self.listener.0, volume) {
                return;
            }
        }
        let started = self.profile_timer();
        self.drain_retired();
        self.load_bank(r);
        let bank = self.bank.as_ref().unwrap();
        let (clip, category) = bank.pick(name, &mut self.rng).unwrap_or_else(|| {
            (
                Clip {
                    path: name.into(),
                    pitch: 1.,
                    volume: 1.,
                    stream: false,
                },
                bank.category(name),
            )
        });
        self.start_effect(r, clip, category, volume, pitch, source);
        if let Some(started) = started {
            self.profile.play_calls += 1;
            self.profile.play_ms += started.elapsed().as_secs_f64() * 1000.;
        }
    }
    fn play_choice(
        &mut self,
        r: &Resources,
        choice: Option<(Clip, usize)>,
        volume: f32,
        pitch: f32,
    ) {
        let started = self.profile_timer();
        self.drain_retired();
        if let Some((clip, category)) = choice {
            self.start_effect(r, clip, category, volume, pitch, None);
        }
        if let Some(started) = started {
            self.profile.play_calls += 1;
            self.profile.play_ms += started.elapsed().as_secs_f64() * 1000.;
        }
    }
    fn start_effect(
        &mut self,
        r: &Resources,
        clip: Clip,
        category: usize,
        volume: f32,
        pitch: f32,
        location: Option<glam::DVec3>,
    ) {
        let base = volume * clip.volume;
        if !(gain(&self.gains, category, base) > 0.) || self.output.is_none() {
            return;
        }
        let Some(prepared) = self.prepare_clip(r, &clip) else {
            return;
        };
        let raw_speed = pitch * clip.pitch;
        let speed = if raw_speed.is_finite() {
            raw_speed.clamp(0.5, 2.0)
        } else {
            1.0
        };
        let spatial = location.map(|pos| (pos, 16.0 * f64::from(volume.max(1.0))));
        let seq = self.shared.play_seq.fetch_add(1, Ordering::AcqRel) + 1;
        let started = self.profile_timer();
        match prepared {
            PreparedClip::Pcm(pcm) => {
                let _ = self.shared.cmd_ring.push(Command::PlayEffect {
                    samples: pcm.samples,
                    channels: pcm.channels,
                    sample_rate: pcm.rate,
                    category,
                    base,
                    pitch: speed,
                    source: spatial,
                    seq,
                });
            }
            PreparedClip::Stream(track) => {
                let _ = self.shared.cmd_ring.push(Command::PlayStream {
                    track,
                    category,
                    base,
                    pitch: speed,
                    source: spatial,
                    seq,
                    music_id: 0,
                });
            }
        }
        if let Some(started) = started {
            self.profile.sink_calls += 1;
            self.profile.sink_ms += started.elapsed().as_secs_f64() * 1000.;
        }
    }
    fn start_music(&mut self, r: &Resources, clip: Clip, category: usize) {
        let base = clip.volume;
        if !(gain(&self.gains, category, base) > 0.) || self.output.is_none() {
            return;
        }
        let Some(bytes) = self.read_ogg_bytes(r, &clip.path, false) else {
            return;
        };
        let Some(decoder) = self.decode(bytes) else {
            return;
        };
        let started = self.profile_timer();
        let Some(track) = StreamTrack::new(decoder).map(Arc::new) else {
            return;
        };
        let pitch = if clip.pitch.is_finite() {
            clip.pitch.clamp(0.5, 2.0)
        } else {
            1.0
        };
        if let Some(old) = self.current_music.take() {
            old.track.abort();
        }
        let music_id = self.next_music_id;
        self.next_music_id = self.next_music_id.wrapping_add(1).max(1);
        self.shared
            .active_music_id
            .store(music_id, Ordering::Release);
        if self
            .shared
            .cmd_ring
            .push(Command::PlayStream {
                track: Arc::clone(&track),
                category,
                base,
                pitch,
                source: None,
                seq: 0,
                music_id,
            })
            .is_ok()
        {
            self.current_music = Some(ActiveMusic {
                track,
                category,
                base,
                pitch,
                music_id,
            });
        } else {
            self.shared.active_music_id.store(0, Ordering::Release);
            track.abort();
        }
        if let Some(started) = started {
            self.profile.sink_calls += 1;
            self.profile.sink_ms += started.elapsed().as_secs_f64() * 1000.;
        }
    }
    fn read_ogg_bytes(&mut self, r: &Resources, path: &str, cache_bytes: bool) -> Option<Arc<[u8]>> {
        if let Some(bytes) = self.cache.get(path) {
            return Some(Arc::clone(bytes));
        }
        let started = self.profile_timer();
        let result = r.bytes(&format!("sounds/{path}.ogg"));
        if let Some(started) = started {
            self.profile.resource_reads += 1;
            self.profile.read_bytes += result.as_ref().map_or(0, |b| b.len() as u64);
            self.profile.read_ms += started.elapsed().as_secs_f64() * 1000.;
        }
        let bytes: Arc<[u8]> = result.ok()?.into();
        if cache_bytes {
            self.cache.insert(path.to_owned(), Arc::clone(&bytes));
        }
        Some(bytes)
    }
    fn prepare_clip(&mut self, r: &Resources, clip: &Clip) -> Option<PreparedClip> {
        if let Some(pcm) = self.pcm_cache.get(&clip.path, clip.stream) {
            if self.profile_enabled {
                self.profile.pcm_hits += 1;
            }
            return Some(PreparedClip::Pcm(pcm));
        }
        if self.profile_enabled && !clip.stream {
            self.profile.pcm_misses += 1;
        }
        let bytes = self.read_ogg_bytes(r, &clip.path, !clip.stream)?;
        if !clip.stream {
            if pcm_cache::admissible_ogg(&bytes) {
                let source = self.decode(Arc::clone(&bytes))?;
                let started = self.profile_timer();
                let captured = pcm_cache::capture(source);
                if let Some(started) = started {
                    self.profile.pcm_preparations += 1;
                    self.profile.pcm_prepare_ms += started.elapsed().as_secs_f64() * 1000.;
                }
                if let Some(pcm) = captured {
                    self.pcm_cache.insert(clip.path.clone(), &pcm);
                    return Some(PreparedClip::Pcm(pcm));
                }
            }
            // Non-streaming clip with non-standard Ogg framing, unknown granule length,
            // >2 channels, or format change: try bounded capture up to MAX_CLIP_BYTES first.
            let fallback_source = self.decode(Arc::clone(&bytes))?;
            let started = self.profile_timer();
            let fallback = pcm_cache::capture_any(fallback_source);
            if let Some(started) = started {
                self.profile.pcm_preparations += 1;
                self.profile.pcm_prepare_ms += started.elapsed().as_secs_f64() * 1000.;
            }
            if let Some(pcm) = fallback {
                self.pcm_cache.insert(clip.path.clone(), &pcm);
                return Some(PreparedClip::Pcm(pcm));
            }
        }
        let stream_source = self.decode(bytes)?;
        let started = self.profile_timer();
        let track = StreamTrack::new(stream_source).map(Arc::new);
        if let Some(started) = started {
            self.profile.sink_calls += 1;
            self.profile.sink_ms += started.elapsed().as_secs_f64() * 1000.;
        }
        track.map(PreparedClip::Stream)
    }
    fn decode(&mut self, bytes: Arc<[u8]>) -> Option<VorbisSource> {
        let started = self.profile_timer();
        let source = Decoder::new(Cursor::new(bytes));
        if let Some(started) = started {
            self.profile.decoder_calls += 1;
            self.profile.decoder_ms += started.elapsed().as_secs_f64() * 1000.;
        }
        source.ok()
    }
}
impl Drop for Audio {
    fn drop(&mut self) {
        self.output = None;
        if let Some(active) = self.current_music.take() {
            active.track.abort();
        }
        self.drain_retired();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::buffer::SamplesBuffer;

    fn render_dry_frames(mixer: &mut Mixer, total_frames: usize) -> Vec<(f32, f32)> {
        mixer.process_commands();
        let mut out = Vec::with_capacity(total_frames);
        let mut remaining = total_frames;
        while remaining > 0 {
            let n = remaining.min(MICRO_BLOCK);
            mixer.mix_micro_block(n);
            out.extend_from_slice(&mixer.mix_buf[..n]);
            remaining -= n;
        }
        out
    }

    #[test]
    fn music_delay_track_transitions_and_category_gain() {
        let mut state = MusicClock::default();
        let mut rng = JavaRandom::new(626);
        for _ in 0..100 {
            assert!(!state.tick(MusicMode::Menu, false, &mut rng));
        }
        assert!(state.tick(MusicMode::Menu, false, &mut rng));
        assert!(!state.tick(MusicMode::Menu, true, &mut rng));
        assert!(!state.tick(MusicMode::Menu, false, &mut rng));
        assert!((19..600).contains(&state.delay));
        for _ in 0..state.delay {
            assert!(!state.tick(MusicMode::Menu, false, &mut rng));
        }
        assert!(state.tick(MusicMode::Menu, false, &mut rng));
        state.tick(MusicMode::Creative, true, &mut rng);
        assert!((0..600).contains(&state.delay) || state.current == Some(MusicMode::Creative));
        assert_eq!(MusicMode::for_world(Some((-1, true))), MusicMode::Nether);
        assert_eq!(MusicMode::for_world(None), MusicMode::Menu);
        let mut gains = [1.; 9];
        gains[0] = 0.4;
        gains[5] = 0.5;
        assert_eq!(gain(&gains, 0, 0.25), 0.1);
        assert_eq!(gain(&gains, 5, 2.), 0.2);
    }

    #[test]
    fn short_clip_fractional_resampler_preserves_every_sample_across_pitches() {
        // 37-sample (<100 samples) transient clip tested at 0.5x, 1.0x, and 2.0x pitch,
        // plus cross-rate 24 kHz -> 48 kHz, verifying zero bootstrap truncation and exact interpolation.
        let raw: Vec<i16> = (0..37i32)
            .map(|i| ((i * 1973 + 4096) % 30000 - 15000) as i16)
            .collect();
        let samples: Arc<[i16]> = raw.clone().into();

        // 1.0x pitch at matching 48 kHz -> exact 37 frames followed by clean silence.
        {
            let shared = Arc::new(SharedState::new());
            let mut mixer = Mixer::new(48_000, Arc::clone(&shared));
            assert!(shared
                .cmd_ring
                .push(Command::PlayEffect {
                    samples: Arc::clone(&samples),
                    channels: 1,
                    sample_rate: 48_000,
                    category: 0,
                    base: 1.0,
                    pitch: 1.0,
                    source: None,
                    seq: 1,
                })
                .is_ok());
            let rendered = render_dry_frames(&mut mixer, 64);
            for i in 0..37 {
                let expected = i16_to_f32(raw[i]);
                assert!(
                    (rendered[i].0 - expected).abs() < 1e-6,
                    "1.0x mismatch at frame {i}: got {}, expected {expected}",
                    rendered[i].0
                );
                assert_eq!(rendered[i].0, rendered[i].1);
            }
            for i in 37..64 {
                assert_eq!(rendered[i], (0.0, 0.0), "expected silence after tail at {i}");
            }
        }

        // 0.5x pitch at 48 kHz -> exact 74 frames with midpoint linear interpolation (and 0.0 at idx == 37).
        {
            let shared = Arc::new(SharedState::new());
            let mut mixer = Mixer::new(48_000, Arc::clone(&shared));
            assert!(shared
                .cmd_ring
                .push(Command::PlayEffect {
                    samples: Arc::clone(&samples),
                    channels: 1,
                    sample_rate: 48_000,
                    category: 0,
                    base: 1.0,
                    pitch: 0.5,
                    source: None,
                    seq: 1,
                })
                .is_ok());
            let rendered = render_dry_frames(&mut mixer, 96);
            for out_idx in 0..74 {
                let src_idx = out_idx / 2;
                let s0 = i16_to_f32(raw[src_idx]);
                let s1 = if src_idx + 1 < 37 {
                    i16_to_f32(raw[src_idx + 1])
                } else {
                    0.0
                };
                let expected = if out_idx % 2 == 0 {
                    s0
                } else {
                    0.5 * (s0 + s1)
                };
                assert!(
                    (rendered[out_idx].0 - expected).abs() < 1e-6,
                    "0.5x mismatch at frame {out_idx}: got {}, expected {expected}",
                    rendered[out_idx].0
                );
            }
            for out_idx in 74..96 {
                assert_eq!(rendered[out_idx], (0.0, 0.0));
            }
        }

        // 2.0x pitch at 48 kHz -> exact 19 frames (indices 0, 2, ..., 36).
        {
            let shared = Arc::new(SharedState::new());
            let mut mixer = Mixer::new(48_000, Arc::clone(&shared));
            assert!(shared
                .cmd_ring
                .push(Command::PlayEffect {
                    samples: Arc::clone(&samples),
                    channels: 1,
                    sample_rate: 48_000,
                    category: 0,
                    base: 1.0,
                    pitch: 2.0,
                    source: None,
                    seq: 1,
                })
                .is_ok());
            let rendered = render_dry_frames(&mut mixer, 32);
            for out_idx in 0..19 {
                let expected = i16_to_f32(raw[out_idx * 2]);
                assert!(
                    (rendered[out_idx].0 - expected).abs() < 1e-6,
                    "2.0x mismatch at frame {out_idx}"
                );
            }
            for out_idx in 19..32 {
                assert_eq!(rendered[out_idx], (0.0, 0.0));
            }
        }

        // Streaming decoder ring buffer matches direct PCM fractional resampling bit-for-bit.
        {
            let shared = Arc::new(SharedState::new());
            let mut mixer = Mixer::new(48_000, Arc::clone(&shared));
            let track =
                Arc::new(StreamTrack::new(SamplesBuffer::new(1, 24_000, raw.clone())).unwrap());
            shared.active_music_id.store(1, Ordering::Release);
            assert!(shared
                .cmd_ring
                .push(Command::PlayStream {
                    track,
                    category: 0,
                    base: 1.0,
                    pitch: 1.0,
                    source: None,
                    seq: 0,
                    music_id: 1,
                })
                .is_ok());
            let stream_rendered = render_dry_frames(&mut mixer, 96);

            let shared_pcm = Arc::new(SharedState::new());
            let mut mixer_pcm = Mixer::new(48_000, Arc::clone(&shared_pcm));
            assert!(shared_pcm
                .cmd_ring
                .push(Command::PlayEffect {
                    samples,
                    channels: 1,
                    sample_rate: 24_000,
                    category: 0,
                    base: 1.0,
                    pitch: 1.0,
                    source: None,
                    seq: 1,
                })
                .is_ok());
            let pcm_rendered = render_dry_frames(&mut mixer_pcm, 96);
            assert_eq!(stream_rendered, pcm_rendered);
            assert_eq!(shared.active_music_id.load(Ordering::Acquire), 0);
        }
    }

    #[test]
    fn distance_culling_stereo_pan_and_stereo_3d_mono_folddown() {
        let ear = glam::DVec3::new(10.0, 64.0, -20.0);
        assert!(should_cull_3d(ear, ear, 0.0));
        assert!(should_cull_3d(ear, ear, -0.5));
        assert!(!should_cull_3d(ear + glam::DVec3::new(15.99, 0.0, 0.0), ear, 0.5));
        assert!(should_cull_3d(ear + glam::DVec3::new(16.0, 0.0, 0.0), ear, 0.5));
        assert!(!should_cull_3d(ear + glam::DVec3::new(31.9, 0.0, 0.0), ear, 2.0));
        assert!(should_cull_3d(ear + glam::DVec3::new(32.0, 0.0, 0.0), ear, 2.0));

        // At yaw = 0, listener right vector is +X.
        // Source 8 blocks to +X with radius 16: atten = 0.5, pan = +1.0 -> L = 0.0, R = 0.5.
        let (gl_right, gr_right) =
            spatial_gains(ear + glam::DVec3::new(8.0, 0.0, 0.0), 16.0, ear, 0.0);
        assert!(gl_right.abs() < 1e-6);
        assert!((gr_right - 0.5).abs() < 1e-6);

        // Source 4 blocks to -X with radius 16: atten = 0.75, pan = -1.0 -> L = 0.75, R = 0.0.
        let (gl_left, gr_left) =
            spatial_gains(ear + glam::DVec3::new(-4.0, 0.0, 0.0), 16.0, ear, 0.0);
        assert!((gl_left - 0.75).abs() < 1e-6);
        assert!(gr_left.abs() < 1e-6);

        // Source 8 blocks ahead (-Z) at yaw = 0: atten = 0.5, pan = 0.0 -> L = 0.5, R = 0.5.
        let (gl_front, gr_front) =
            spatial_gains(ear + glam::DVec3::new(0.0, 0.0, -8.0), 16.0, ear, 0.0);
        assert!((gl_front - 0.5).abs() < 1e-6);
        assert!((gr_front - 0.5).abs() < 1e-6);

        // 2-channel 3D clip with Left-only PCM [16384, 0] panned hard right (+X) must fold
        // (L + R) * 0.5 = 8192 before panning so it remains audible in the Right ear!
        let shared = Arc::new(SharedState::new());
        let mut mixer = Mixer::new(48_000, Arc::clone(&shared));
        let stereo_left_only: Arc<[i16]> = [16384i16, 0i16].repeat(16).into();
        assert!(shared
            .cmd_ring
            .push(Command::PlayEffect {
                samples: stereo_left_only,
                channels: 2,
                sample_rate: 48_000,
                category: 0,
                base: 1.0,
                pitch: 1.0,
                source: Some((glam::DVec3::new(8.0, 0.0, 0.0), 16.0)),
                seq: 1,
            })
            .is_ok());
        let out = render_dry_frames(&mut mixer, 4);
        // Folded mono = (0.5 + 0.0) * 0.5 = 0.25; atten = 0.5, right pan = 1.0 -> R = 0.125.
        assert!(out[0].0.abs() < 1e-6);
        assert!((out[0].1 - 0.125).abs() < 1e-6);
    }

    #[test]
    fn pause_and_resume_sequence_watermark_holds_world_sounds_while_ui_plays() {
        let shared = Arc::new(SharedState::new());
        let mut mixer = Mixer::new(48_000, Arc::clone(&shared));

        // World sound (seq = 1): 256 constant samples at +0.5 (16384).
        let world_pcm: Arc<[i16]> = vec![16384i16; 256].into();
        shared.play_seq.store(1, Ordering::Release);
        assert!(shared
            .cmd_ring
            .push(Command::PlayEffect {
                samples: world_pcm,
                channels: 1,
                sample_rate: 48_000,
                category: 0,
                base: 1.0,
                pitch: 1.0,
                source: None,
                seq: 1,
            })
            .is_ok());

        // Render 64 frames running normally -> phase advances 0..64 at gain 1.0.
        let run1 = render_dry_frames(&mut mixer, 64);
        assert!((run1[0].0 - 0.5).abs() < 1e-6);
        assert!((run1[63].0 - 0.5).abs() < 1e-6);

        // Pause sounds up to current play_seq (1).
        shared.paused_up_to_seq.store(
            shared.play_seq.load(Ordering::Acquire),
            Ordering::Release,
        );

        // Next 64 frames ramp world sound smoothly down to 0.0 (advancing phase 64..128),
        // and the following 128 frames hold world sound completely frozen at phase 128!
        let ramp_down = render_dry_frames(&mut mixer, 64);
        assert!(ramp_down[0].0 < 0.5 && ramp_down[0].0 > 0.48);
        assert!(ramp_down[63].0.abs() < 1e-6);

        // While paused, enqueue a UI click sound (seq = 2 > paused_up_to_seq = 1).
        let ui_pcm: Arc<[i16]> = vec![8192i16; 32].into();
        shared.play_seq.store(2, Ordering::Release);
        assert!(shared
            .cmd_ring
            .push(Command::PlayEffect {
                samples: ui_pcm,
                channels: 1,
                sample_rate: 48_000,
                category: 0,
                base: 1.0,
                pitch: 1.0,
                source: None,
                seq: 2,
            })
            .is_ok());

        let while_paused = render_dry_frames(&mut mixer, 128);
        // UI sound plays cleanly for its 32 frames at 0.25 while world sound stays silent and held!
        for i in 0..32 {
            assert!((while_paused[i].0 - 0.25).abs() < 1e-6, "frame {i}");
        }
        for i in 32..128 {
            assert_eq!(while_paused[i], (0.0, 0.0), "frame {i}");
        }
        assert!((mixer.effects[0].phase - 128.0).abs() < 1e-9);

        // Resume sounds -> world sound ramps back up over 64 frames (phase 128..192)
        // and plays its remaining 64 frames (phase 192..256) before finishing cleanly!
        shared.paused_up_to_seq.store(0, Ordering::Release);
        let resumed = render_dry_frames(&mut mixer, 192);
        assert!(resumed[0].0 > 0.0 && resumed[0].0 < 0.02);
        assert!((resumed[63].0 - 0.5).abs() < 1e-6);
        assert!((resumed[127].0 - 0.5).abs() < 1e-6);
        assert_eq!(resumed[128], (0.0, 0.0));
    }

    #[test]
    fn bluetooth_keep_alive_tpdf_dither_stays_nonzero_inaudible_and_dc_free() {
        let shared = Arc::new(SharedState::new());
        let mut mixer = Mixer::new(48_000, Arc::clone(&shared));
        let mut f32_buf = vec![0.0f32; 20_000];
        mixer.render_output(&mut f32_buf, 2);

        let mut sum = 0.0f64;
        for (idx, &sample) in f32_buf.iter().enumerate() {
            // Verify non-zero quantization under both 32768 and 32767 i16 scalings.
            let q32768 = f32_to_i16(sample);
            let q32767 = (sample * 32767.0) as i16;
            assert_ne!(q32768, 0, "zero PCM sample at index {idx}");
            assert_ne!(q32767, 0, "zero WASAPI i16 sample at index {idx}");
            assert!(q32768.abs() <= 3, "inaudible bound exceeded: {q32768}");
            sum += f64::from(sample);
        }
        let mean_dc = (sum / f32_buf.len() as f64).abs();
        assert!(mean_dc < 5e-6, "unexpected DC bias: {mean_dc}");

        // Also check I16 and U16 output sample formats directly.
        let mut i16_buf = vec![0i16; 512];
        mixer.render_output(&mut i16_buf, 2);
        assert!(i16_buf.iter().all(|&s| s != 0 && s.abs() <= 3));

        let mut u16_buf = vec![32768u16; 512];
        mixer.render_output(&mut u16_buf, 2);
        assert!(u16_buf.iter().all(|&s| s != 32768 && s.abs_diff(32768) <= 3));
    }

    #[test]
    fn fade_tail_soft_clip_and_main_thread_arc_retirement() {
        let shared = Arc::new(SharedState::new());
        let mut mixer = Mixer::new(48_000, Arc::clone(&shared));

        let pcm: Arc<[i16]> = vec![16384i16; 512].into();
        let weak = Arc::downgrade(&pcm);
        assert!(shared
            .cmd_ring
            .push(Command::PlayEffect {
                samples: pcm,
                channels: 1,
                sample_rate: 48_000,
                category: 0,
                base: 1.0,
                pitch: 1.0,
                source: None,
                seq: 1,
            })
            .is_ok());

        let active_out = render_dry_frames(&mut mixer, 64);
        assert!((active_out[63].0 - 0.5).abs() < 1e-6);

        // Stopping all voices retires the Arc to `retire_ring` (without freeing it on the
        // mixer thread!) and emits a smooth 96-frame linear fade-out tail to 0.0.
        assert!(shared.cmd_ring.push(Command::StopAll).is_ok());
        let tail_out = render_dry_frames(&mut mixer, 128);
        assert!(
            weak.upgrade().is_some(),
            "Arc must remain alive in retire_ring until main thread drains it"
        );
        assert!((tail_out[0].0 - 0.5 * (95.0 / 96.0)).abs() < 1e-5);
        assert!(tail_out[48].0 > 0.2 && tail_out[48].0 < 0.3);
        assert_eq!(tail_out[95], (0.0, 0.0));
        assert_eq!(tail_out[96], (0.0, 0.0));

        while let Some(retired) = shared.retire_ring.pop() {
            drop(retired);
        }
        assert!(weak.upgrade().is_none());

        // Soft-clip is exact identity within ±0.95 and bounded within ±1.0 above ±0.95.
        assert_eq!(soft_clip(0.95), 0.95);
        assert_eq!(soft_clip(-0.95), -0.95);
        assert!(soft_clip(4.0) > 0.99 && soft_clip(4.0) <= 1.0);
        assert!(soft_clip(-4.0) < -0.99 && soft_clip(-4.0) >= -1.0);
    }
}
