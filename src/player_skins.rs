//! Asynchronous multiplayer/local player skin downloader, 64x32 legacy converter, and GPU atlas.
use crate::resources::{Resources, Sprite};
use sha1::{Digest, Sha1};
use std::{
    collections::{HashMap, VecDeque},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, OnceLock},
    time::{Duration, Instant},
};

const SKIN_PIXELS: usize = 64 * 64 * 4;
const ATLAS_PAGES: u32 = 16;
const ATLAS_CAPACITY: usize = (ATLAS_PAGES as usize) * 16;
const MAX_SKIN_BYTES: u64 = 256 * 1024;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkinRegion {
    pub layer: u32,
    pub origin: [f32; 2],
    pub scale: [f32; 2],
}

impl SkinRegion {
    pub fn from_sprite(s: Sprite) -> Self {
        Self {
            layer: s.layer,
            origin: [0., 0.],
            scale: [s.width as f32 / 256., s.height as f32 / 256.],
        }
    }

    fn at_slot(first: u32, slot: usize) -> Self {
        Self {
            layer: first + (slot / 16) as u32,
            origin: [(slot % 4) as f32 * 0.25, ((slot / 4) % 4) as f32 * 0.25],
            scale: [0.25, 0.25],
        }
    }

    #[inline]
    pub fn uv(self, u01: f32, v01: f32) -> [f32; 2] {
        [
            self.origin[0] + u01 * self.scale[0],
            self.origin[1] + v01 * self.scale[1],
        ]
    }

    #[inline]
    pub fn pixel_uv(self, px: f32, py: f32) -> [f32; 2] {
        self.uv(px / 64., py / 64.)
    }

    #[inline]
    pub fn subrect_uv(self, px: f32, py: f32, pw: f32, ph: f32) -> [f32; 4] {
        [
            self.origin[0] + (px / 64.) * self.scale[0],
            self.origin[1] + (py / 64.) * self.scale[1],
            (pw / 64.) * self.scale[0],
            (ph / 64.) * self.scale[1],
        ]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkinProfile {
    pub url: Option<Arc<str>>,
    pub slim: bool,
}

impl SkinProfile {
    pub fn default_for(uuid: [u8; 16]) -> Self {
        Self {
            url: None,
            slim: default_slim(uuid),
        }
    }
}

#[inline]
pub fn default_slim(uuid: [u8; 16]) -> bool {
    ((uuid[3] ^ uuid[7] ^ uuid[11] ^ uuid[15]) & 1) != 0
}

pub fn parse_uuid_hex(raw: &str) -> Option<[u8; 16]> {
    let mut out = [0u8; 16];
    let mut nibbles = raw.bytes().filter(|&b| b != b'-');
    for byte in &mut out {
        let hi = hex_nibble(nibbles.next()?)?;
        let lo = hex_nibble(nibbles.next()?)?;
        *byte = (hi << 4) | lo;
    }
    if nibbles.next().is_some() {
        return None;
    }
    Some(out)
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub fn decode_base64(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0u8;
    let mut padded = false;
    for b in input.bytes() {
        if b.is_ascii_whitespace() {
            continue;
        }
        if b == b'=' {
            padded = true;
            continue;
        }
        if padded {
            return None;
        }
        let val = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        };
        buf = (buf << 6) | u32::from(val);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    if bits >= 6 {
        return None;
    }
    Some(out)
}

pub fn normalize_skin_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let uri: ureq::http::Uri = trimmed.parse().ok()?;
    let scheme = uri.scheme_str()?;
    if !matches!(scheme, "http" | "https") {
        return None;
    }
    let authority = uri.authority()?;
    if authority.as_str().contains('@') {
        return None;
    }
    let host = uri.host()?.to_ascii_lowercase();
    let allowed = host == "textures.minecraft.net"
        || host.ends_with(".minecraft.net")
        || host == "mojang.com"
        || host.ends_with(".mojang.com");
    if !allowed {
        return None;
    }
    let path_and_query = uri.path_and_query().map_or("/", |pq| pq.as_str());
    Some(format!("https://{host}{path_and_query}"))
}

pub fn parse_properties(
    uuid: [u8; 16],
    properties: &[(String, String, Option<String>)],
) -> Option<SkinProfile> {
    let (_, encoded, _) = properties
        .iter()
        .find(|(name, _, _)| name.eq_ignore_ascii_case("textures"))?;
    let profile = parse_textures_value(uuid, encoded)?;
    if let Some(url) = profile.url.as_ref() {
        request_download(url);
    }
    Some(profile)
}

pub fn parse_textures_value(uuid: [u8; 16], encoded: &str) -> Option<SkinProfile> {
    let decoded = decode_base64(encoded)?;
    let json: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    let skin = &json["textures"]["SKIN"];
    if !skin.is_object() {
        return Some(SkinProfile::default_for(uuid));
    }
    let url = skin["url"]
        .as_str()
        .and_then(normalize_skin_url)
        .map(Arc::<str>::from);
    let slim = skin["metadata"]["model"]
        .as_str()
        .is_some_and(|m| m.eq_ignore_ascii_case("slim"));
    Some(SkinProfile { url, slim })
}

/// Exact port of Vanilla 1.8.9 `ImageBufferDownload.parseUserSkin`.
pub fn parse_user_skin(png_bytes: &[u8]) -> Option<[u8; SKIN_PIXELS]> {
    let img = image::load_from_memory_with_format(png_bytes, image::ImageFormat::Png)
        .ok()?
        .to_rgba8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    if w == 0 || w % 64 != 0 {
        return None;
    }
    let scale = w / 64;
    let norm_h = if h == 32 * scale {
        32
    } else if h == 64 * scale {
        64
    } else {
        return None;
    };
    let raw = img.as_raw();
    let mut out = [0u8; SKIN_PIXELS];
    for y in 0..norm_h {
        for x in 0..64 {
            let src_idx = ((y * scale) * w + (x * scale)) * 4;
            let dst_idx = (y * 64 + x) * 4;
            out[dst_idx..dst_idx + 4].copy_from_slice(&raw[src_idx..src_idx + 4]);
        }
    }
    if norm_h == 32 {
        const LEGACY_FLIPS: [[usize; 8]; 12] = [
            [24, 48, 20, 52, 4, 16, 8, 20],
            [28, 48, 24, 52, 8, 16, 12, 20],
            [20, 52, 16, 64, 8, 20, 12, 32],
            [24, 52, 20, 64, 4, 20, 8, 32],
            [28, 52, 24, 64, 0, 20, 4, 32],
            [32, 52, 28, 64, 12, 20, 16, 32],
            [40, 48, 36, 52, 44, 16, 48, 20],
            [44, 48, 40, 52, 48, 16, 52, 20],
            [36, 52, 32, 64, 48, 20, 52, 32],
            [40, 52, 36, 64, 44, 20, 48, 32],
            [44, 52, 40, 64, 40, 20, 44, 32],
            [48, 52, 44, 64, 52, 20, 56, 32],
        ];
        for [dx_hi, dy_lo, dx_lo, dy_hi, sx_lo, sy_lo, sx_hi, sy_hi] in LEGACY_FLIPS {
            debug_assert_eq!(dx_hi - dx_lo, sx_hi - sx_lo);
            debug_assert_eq!(dy_hi - dy_lo, sy_hi - sy_lo);
            let rw = sx_hi - sx_lo;
            let rh = sy_hi - sy_lo;
            for iy in 0..rh {
                for ix in 0..rw {
                    let s = ((sy_lo + iy) * 64 + (sx_lo + ix)) * 4;
                    let d = ((dy_lo + iy) * 64 + (dx_hi - 1 - ix)) * 4;
                    let px = [out[s], out[s + 1], out[s + 2], out[s + 3]];
                    out[d..d + 4].copy_from_slice(&px);
                }
            }
        }
    }
    set_area_opaque(&mut out, 0, 0, 32, 16);
    set_area_transparent(&mut out, 32, 0, 64, 32);
    set_area_opaque(&mut out, 0, 16, 64, 32);
    set_area_transparent(&mut out, 0, 32, 16, 48);
    set_area_transparent(&mut out, 16, 32, 40, 48);
    set_area_transparent(&mut out, 40, 32, 56, 48);
    set_area_transparent(&mut out, 0, 48, 16, 64);
    set_area_opaque(&mut out, 16, 48, 48, 64);
    set_area_transparent(&mut out, 48, 48, 64, 64);
    Some(out)
}

fn set_area_opaque(img: &mut [u8; SKIN_PIXELS], x1: usize, y1: usize, x2: usize, y2: usize) {
    for y in y1..y2 {
        for x in x1..x2 {
            img[(y * 64 + x) * 4 + 3] = 255;
        }
    }
}

fn set_area_transparent(img: &mut [u8; SKIN_PIXELS], x1: usize, y1: usize, x2: usize, y2: usize) {
    for y in y1..y2 {
        for x in x1..x2 {
            if img[(y * 64 + x) * 4 + 3] < 128 {
                return;
            }
        }
    }
    for y in y1..y2 {
        for x in x1..x2 {
            img[(y * 64 + x) * 4 + 3] = 0;
        }
    }
}

const FETCHER_MAP_CAP: usize = 512;

struct BoundedSkinMap<K, V> {
    map: HashMap<K, (V, u64)>,
    order: std::collections::BTreeMap<u64, K>,
    tick: u64,
}

impl<K, V> Default for BoundedSkinMap<K, V> {
    fn default() -> Self {
        Self {
            map: HashMap::new(),
            order: std::collections::BTreeMap::new(),
            tick: 0,
        }
    }
}

impl<K: Eq + std::hash::Hash + Clone, V> BoundedSkinMap<K, V> {
    fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: std::borrow::Borrow<Q>,
        Q: std::hash::Hash + Eq + ?Sized,
    {
        self.map.contains_key(key)
    }

    fn get<Q>(&mut self, key: &Q) -> Option<&V>
    where
        K: std::borrow::Borrow<Q>,
        Q: std::hash::Hash + Eq + ?Sized,
    {
        let (owned_key, old_tick) = {
            let (k, (_, t)) = self.map.get_key_value(key)?;
            (k.clone(), *t)
        };
        self.tick = self.tick.wrapping_add(1);
        let new_tick = self.tick;
        self.order.remove(&old_tick);
        self.order.insert(new_tick, owned_key);
        let (val, slot_tick) = self.map.get_mut(key)?;
        *slot_tick = new_tick;
        Some(val)
    }

    fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: std::borrow::Borrow<Q>,
        Q: std::hash::Hash + Eq + ?Sized,
    {
        let (val, old_tick) = self.map.remove(key)?;
        self.order.remove(&old_tick);
        Some(val)
    }

    fn insert(&mut self, key: K, value: V) {
        if let Some((_, old_tick)) = self.map.remove(&key) {
            self.order.remove(&old_tick);
        } else {
            while self.map.len() >= FETCHER_MAP_CAP {
                let Some((_, oldest_key)) = self.order.pop_first() else {
                    self.map.clear();
                    break;
                };
                self.map.remove(&oldest_key);
            }
        }
        self.tick = self.tick.wrapping_add(1);
        let new_tick = self.tick;
        self.order.insert(new_tick, key.clone());
        self.map.insert(key, (value, new_tick));
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.map.len()
    }
}

enum Job {
    Skin(Arc<str>),
    MojangProfile([u8; 16]),
    MojangUsername(String),
}

#[derive(Default)]
struct FetchState {
    ready: BoundedSkinMap<Arc<str>, Arc<[u8; SKIN_PIXELS]>>,
    in_flight: std::collections::HashSet<Arc<str>>,
    failed: BoundedSkinMap<Arc<str>, Instant>,
    profiles: BoundedSkinMap<[u8; 16], Option<SkinProfile>>,
    usernames: BoundedSkinMap<String, Option<SkinProfile>>,
    queue: VecDeque<Job>,
}

struct Fetcher {
    state: Mutex<FetchState>,
    wake: Condvar,
}

fn fetcher() -> &'static Arc<Fetcher> {
    static FETCHER: OnceLock<Arc<Fetcher>> = OnceLock::new();
    FETCHER.get_or_init(|| {
        let shared = Arc::new(Fetcher {
            state: Mutex::default(),
            wake: Condvar::new(),
        });
        for idx in 0..3 {
            let worker_shared = Arc::clone(&shared);
            let _ = std::thread::Builder::new()
                .name(format!("ferrum-skin-{idx}"))
                .spawn(move || worker_loop(worker_shared));
        }
        shared
    })
}

pub fn request_download(url: &Arc<str>) {
    let f = fetcher();
    let mut state = f.state.lock().unwrap();
    if state.ready.contains_key(url) || state.in_flight.contains(url) {
        return;
    }
    if let Some(&when) = state.failed.get(url) {
        if when.elapsed() < Duration::from_secs(60) {
            return;
        }
        state.failed.remove(url);
    }
    state.in_flight.insert(Arc::clone(url));
    state.queue.push_back(Job::Skin(Arc::clone(url)));
    f.wake.notify_one();
}

pub fn request_mojang_profile(uuid: [u8; 16]) {
    let f = fetcher();
    let mut state = f.state.lock().unwrap();
    if state.profiles.contains_key(&uuid) {
        return;
    }
    state.profiles.insert(uuid, None);
    state.queue.push_back(Job::MojangProfile(uuid));
    f.wake.notify_one();
}

pub fn poll_mojang_profile(uuid: [u8; 16]) -> Option<SkinProfile> {
    fetcher()
        .state
        .lock()
        .unwrap()
        .profiles
        .get(&uuid)
        .and_then(Clone::clone)
}

pub(crate) fn sanitize_username(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if !(1..=16).contains(&trimmed.len())
        || !trimmed
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "con"
            | "prn"
            | "aux"
            | "nul"
            | "com1"
            | "com2"
            | "com3"
            | "com4"
            | "com5"
            | "com6"
            | "com7"
            | "com8"
            | "com9"
            | "lpt1"
            | "lpt2"
            | "lpt3"
            | "lpt4"
            | "lpt5"
            | "lpt6"
            | "lpt7"
            | "lpt8"
            | "lpt9"
    ) {
        return None;
    }
    Some(lower)
}

pub fn request_mojang_username(raw: &str) {
    let Some(lower) = sanitize_username(raw) else {
        return;
    };
    let f = fetcher();
    let mut state = f.state.lock().unwrap();
    if state.usernames.contains_key(&lower) {
        return;
    }
    state.usernames.insert(lower.clone(), None);
    state.queue.push_back(Job::MojangUsername(lower));
    f.wake.notify_one();
}

pub fn poll_mojang_username(raw: &str) -> Option<SkinProfile> {
    let lower = sanitize_username(raw)?;
    fetcher()
        .state
        .lock()
        .unwrap()
        .usernames
        .get(&lower)
        .and_then(Clone::clone)
}

fn parse_textures_value_cached(uuid: [u8; 16], encoded: &str) -> Option<SkinProfile> {
    type RawTextures = Option<(Option<Arc<str>>, Option<bool>)>;
    static CACHE: OnceLock<Mutex<HashMap<String, RawTextures>>> = OnceLock::new();
    let trimmed = encoded.trim();
    if trimmed.is_empty() {
        return None;
    }
    let cache = CACHE.get_or_init(Mutex::default);
    if let Some(cached) = cache.lock().unwrap().get(trimmed).cloned() {
        let (url, slim) = cached?;
        return Some(SkinProfile {
            url,
            slim: slim.unwrap_or_else(|| default_slim(uuid)),
        });
    }
    let parsed: RawTextures = (|| {
        let decoded = decode_base64(trimmed)?;
        let json: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
        let skin = &json["textures"]["SKIN"];
        if !skin.is_object() {
            return Some((None, None));
        }
        let url = skin["url"]
            .as_str()
            .and_then(normalize_skin_url)
            .map(Arc::<str>::from);
        let slim = skin["metadata"]["model"]
            .as_str()
            .is_some_and(|m| m.eq_ignore_ascii_case("slim"));
        Some((url, Some(slim)))
    })();
    {
        let mut map = cache.lock().unwrap();
        if map.len() >= FETCHER_MAP_CAP {
            map.clear();
        }
        map.insert(trimmed.to_owned(), parsed.clone());
    }
    let (url, slim) = parsed?;
    Some(SkinProfile {
        url,
        slim: slim.unwrap_or_else(|| default_slim(uuid)),
    })
}

pub(crate) fn item_skull_owner(
    nbt: Option<&crate::vanilla::drops::DropNbt>,
) -> Option<&crate::vanilla::drops::DropNbt> {
    use crate::vanilla::drops::DropNbt as Tag;
    let Tag::Compound(fields) = nbt? else {
        return None;
    };
    fields
        .get("SkullOwner")
        .or_else(|| fields.get("Owner"))
        .or_else(|| fields.get("ExtraType"))
        .or_else(|| {
            let Tag::Compound(be) = fields.get("BlockEntityTag")? else {
                return None;
            };
            be.get("Owner").or_else(|| be.get("ExtraType"))
        })
}

fn resolve_skull_skin_by_name(
    name: &str,
    r: &Resources,
    presentation: Option<&crate::net_presentation::State>,
) -> SkinRegion {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return r.skins.resolve(r, None, false);
    }
    if let Some(p) = presentation {
        if let Some((uuid, _)) = p
            .players
            .iter()
            .find(|(_, e)| e.name.eq_ignore_ascii_case(trimmed))
        {
            if let Some(profile) = p.skins.get(uuid) {
                return r.skins.resolve(r, profile.url.as_ref(), profile.slim);
            }
        }
    }
    if let Some(profile) = poll_mojang_username(trimmed) {
        return r.skins.resolve(r, profile.url.as_ref(), profile.slim);
    }
    request_mojang_username(trimmed);
    r.skins.resolve(r, None, false)
}

pub(crate) fn resolve_skull_skin(
    owner_tag: Option<&crate::vanilla::drops::DropNbt>,
    r: &Resources,
    presentation: Option<&crate::net_presentation::State>,
) -> SkinRegion {
    use crate::vanilla::drops::DropNbt as Tag;
    match owner_tag {
        Some(Tag::Compound(fields)) => {
            let get_ci = |upper: &str, lower: &str| {
                fields.get(upper).or_else(|| fields.get(lower)).or_else(|| {
                    fields
                        .iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case(upper))
                        .map(|(_, v)| v)
                })
            };
            let parsed_uuid = match get_ci("Id", "id") {
                Some(Tag::String(s)) => parse_uuid_hex(s),
                Some(Tag::Ints(ints)) if ints.len() == 4 => {
                    let mut bytes = [0u8; 16];
                    for (i, &w) in ints.iter().enumerate() {
                        bytes[i * 4..i * 4 + 4].copy_from_slice(&w.to_be_bytes());
                    }
                    Some(bytes)
                }
                _ => None,
            };
            if let Some(Tag::Compound(props)) = get_ci("Properties", "properties") {
                let textures = props.get("textures").or_else(|| {
                    props
                        .iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case("textures"))
                        .map(|(_, v)| v)
                });
                if let Some(Tag::List { values, .. }) = textures {
                    let encoded = values.iter().find_map(|entry| {
                        let Tag::Compound(tex) = entry else {
                            return None;
                        };
                        let val = tex.get("Value").or_else(|| tex.get("value")).or_else(|| {
                            tex.iter()
                                .find(|(k, _)| k.eq_ignore_ascii_case("Value"))
                                .map(|(_, v)| v)
                        })?;
                        match val {
                            Tag::String(s) if !s.trim().is_empty() => Some(s.as_str()),
                            _ => None,
                        }
                    });
                    if let Some(val) = encoded {
                        if let Some(profile) =
                            parse_textures_value_cached(parsed_uuid.unwrap_or([0; 16]), val)
                        {
                            return r.skins.resolve(r, profile.url.as_ref(), profile.slim);
                        }
                    }
                }
            }
            let non_empty_name = match get_ci("Name", "name") {
                Some(Tag::String(s)) if !s.trim().is_empty() => Some(s.as_str()),
                _ => None,
            };
            if let Some(uuid) = parsed_uuid {
                if let Some(profile) = presentation.and_then(|p| p.skins.get(&uuid)) {
                    return r.skins.resolve(r, profile.url.as_ref(), profile.slim);
                }
                if let Some(profile) = poll_mojang_profile(uuid) {
                    return r.skins.resolve(r, profile.url.as_ref(), profile.slim);
                }
                if (uuid[6] >> 4) == 4 {
                    request_mojang_profile(uuid);
                    return r.skins.resolve(r, None, default_slim(uuid));
                }
                if non_empty_name.is_none() {
                    return r.skins.resolve(r, None, default_slim(uuid));
                }
            }
            if let Some(name) = non_empty_name {
                return resolve_skull_skin_by_name(name, r, presentation);
            }
            r.skins.resolve(r, None, false)
        }
        Some(Tag::String(name)) => resolve_skull_skin_by_name(name, r, presentation),
        _ => r.skins.resolve(r, None, false),
    }
}

fn ready_skin(url: &Arc<str>) -> Option<Arc<[u8; SKIN_PIXELS]>> {
    let f = fetcher();
    let mut state = f.state.lock().unwrap();
    if let Some(rgba) = state.ready.get(url) {
        return Some(Arc::clone(rgba));
    }
    if !state.in_flight.contains(url)
        && state
            .failed
            .get(url)
            .is_none_or(|t| t.elapsed() >= Duration::from_secs(60))
    {
        state.failed.remove(url);
        state.in_flight.insert(Arc::clone(url));
        state.queue.push_back(Job::Skin(Arc::clone(url)));
        f.wake.notify_one();
    }
    None
}

fn cache_file_for_url(url: &str) -> PathBuf {
    let digest = format!("{:x}", Sha1::digest(url.as_bytes()));
    Path::new(".local/cache/skins")
        .join(&digest[..2])
        .join(format!("{digest}.png"))
}

fn download_bytes(agent: &ureq::Agent, url: &str) -> Option<Vec<u8>> {
    let try_url = |target: &str| -> Option<Vec<u8>> {
        let resp = agent.get(target).call().ok()?;
        let mut bytes = Vec::new();
        resp.into_body()
            .into_reader()
            .take(MAX_SKIN_BYTES + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        (bytes.len() as u64 <= MAX_SKIN_BYTES).then_some(bytes)
    };
    try_url(url).or_else(|| {
        let http_fallback = url.strip_prefix("https://").map(|rest| format!("http://{rest}"))?;
        try_url(&http_fallback)
    })
}

fn load_or_fetch_skin(agent: &ureq::Agent, url: &str) -> Option<[u8; SKIN_PIXELS]> {
    let cache_path = cache_file_for_url(url);
    if let Ok(cached) = fs::read(&cache_path) {
        if let Some(parsed) = parse_user_skin(&cached) {
            return Some(parsed);
        }
    }
    let bytes = download_bytes(agent, url)?;
    let parsed = parse_user_skin(&bytes)?;
    if let Some(parent) = cache_path.parent() {
        if fs::create_dir_all(parent).is_ok() {
            let tmp = parent.join(format!("skin-{:016x}.part", rand::random::<u64>()));
            if let Ok(mut f) = fs::OpenOptions::new().create_new(true).write(true).open(&tmp) {
                if f.write_all(&bytes).is_ok() {
                    drop(f);
                    let _ = fs::rename(&tmp, &cache_path);
                }
                let _ = fs::remove_file(&tmp);
            }
        }
    }
    Some(parsed)
}

fn fetch_mojang_profile(agent: &ureq::Agent, uuid: [u8; 16]) -> Option<SkinProfile> {
    let mut hex = String::with_capacity(32);
    for b in uuid {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{b:02x}");
    }
    let cache_dir = Path::new(".local/cache/skins/profiles");
    let cache_path = cache_dir.join(format!("{hex}.json"));
    let fresh_cached = fs::metadata(&cache_path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < Duration::from_secs(3600))
        .then(|| fs::read(&cache_path).ok())
        .flatten();
    let body = fresh_cached
        .or_else(|| {
            download_bytes(
                agent,
                &format!("https://sessionserver.mojang.com/session/minecraft/profile/{hex}"),
            )
            .inspect(|bytes| {
                if fs::create_dir_all(cache_dir).is_ok() {
                    let _ = fs::write(&cache_path, bytes);
                }
            })
        })
        .or_else(|| fs::read(&cache_path).ok())?;
    let json: serde_json::Value = serde_json::from_slice(&body).ok()?;
    let props = json["properties"].as_array()?;
    let encoded = props.iter().find_map(|entry| {
        entry["name"]
            .as_str()?
            .eq_ignore_ascii_case("textures")
            .then(|| entry["value"].as_str())
            .flatten()
    })?;
    let profile = parse_textures_value(uuid, encoded)?;
    if let Some(url) = profile.url.as_ref() {
        request_download(url);
    }
    Some(profile)
}

fn fetch_mojang_username(agent: &ureq::Agent, lower_name: &str) -> Option<([u8; 16], SkinProfile)> {
    let lower_name = sanitize_username(lower_name)?;
    let cache_dir = Path::new(".local/cache/skins/usernames");
    let cache_path = cache_dir.join(format!("{lower_name}.json"));
    let fresh_cached = fs::metadata(&cache_path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < Duration::from_secs(3600))
        .then(|| fs::read(&cache_path).ok())
        .flatten();
    let mut from_network = false;
    let body = fresh_cached
        .or_else(|| {
            let bytes = download_bytes(
                agent,
                &format!("https://api.mojang.com/users/profiles/minecraft/{lower_name}"),
            )?;
            from_network = true;
            Some(bytes)
        })
        .or_else(|| fs::read(&cache_path).ok())?;
    let json: serde_json::Value = serde_json::from_slice(&body).ok()?;
    let uuid = parse_uuid_hex(json["id"].as_str()?)?;
    if from_network && fs::create_dir_all(cache_dir).is_ok() {
        let _ = fs::write(&cache_path, &body);
    }
    let profile = fetch_mojang_profile(agent, uuid)?;
    Some((uuid, profile))
}

fn worker_loop(shared: Arc<Fetcher>) {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(8)))
        .build()
        .new_agent();
    loop {
        let job = {
            let mut state = shared.state.lock().unwrap();
            loop {
                if let Some(job) = state.queue.pop_front() {
                    break job;
                }
                state = shared.wake.wait(state).unwrap();
            }
        };
        match job {
            Job::Skin(url) => {
                let decoded = load_or_fetch_skin(&agent, &url).map(Arc::new);
                let mut state = shared.state.lock().unwrap();
                state.in_flight.remove(&url);
                if let Some(rgba) = decoded {
                    state.ready.insert(url, rgba);
                } else {
                    state.failed.insert(url, Instant::now());
                }
            }
            Job::MojangProfile(uuid) => {
                if let Some(profile) = fetch_mojang_profile(&agent, uuid) {
                    shared.state.lock().unwrap().profiles.insert(uuid, Some(profile));
                }
            }
            Job::MojangUsername(name) => {
                if let Some((uuid, profile)) = fetch_mojang_username(&agent, &name) {
                    let mut state = shared.state.lock().unwrap();
                    state.profiles.insert(uuid, Some(profile.clone()));
                    state.usernames.insert(name, Some(profile));
                }
            }
        }
    }
}

struct SlotEntry {
    url: Arc<str>,
    rgba: Arc<[u8; SKIN_PIXELS]>,
    revision: u64,
    last_used: Instant,
}

#[derive(Default)]
struct Slots {
    entries: Vec<SlotEntry>,
    revision: u64,
}

pub(crate) struct Atlas {
    first: u32,
    slots: Mutex<Slots>,
}

impl Atlas {
    pub(crate) fn reserve(pixels: &mut Vec<u8>) -> Self {
        let page = 256 * 256 * 4;
        let first = (pixels.len() / page) as u32;
        pixels.resize(pixels.len() + (ATLAS_PAGES as usize) * page, 0);
        Self {
            first,
            slots: Mutex::default(),
        }
    }

    pub(crate) fn resolve(
        &self,
        r: &Resources,
        url: Option<&Arc<str>>,
        slim: bool,
    ) -> SkinRegion {
        if let Some(url) = url {
            if let Some(rgba) = ready_skin(url) {
                return self.allocate_slot(url, rgba);
            }
        }
        SkinRegion::from_sprite(r.sprite(if slim {
            "textures/entity/alex.png"
        } else {
            "textures/entity/steve.png"
        }))
    }

    fn allocate_slot(&self, url: &Arc<str>, rgba: Arc<[u8; SKIN_PIXELS]>) -> SkinRegion {
        let mut cache = self.slots.lock().unwrap();
        let now = Instant::now();
        let slot = if let Some(i) = cache.entries.iter().position(|e| e.url == *url) {
            cache.entries[i].last_used = now;
            if !Arc::ptr_eq(&cache.entries[i].rgba, &rgba) {
                cache.revision += 1;
                let rev = cache.revision;
                cache.entries[i].rgba = rgba;
                cache.entries[i].revision = rev;
            }
            i
        } else {
            let i = if cache.entries.len() < ATLAS_CAPACITY {
                cache.entries.len()
            } else {
                cache
                    .entries
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, e)| e.last_used)
                    .map_or(0, |(idx, _)| idx)
            };
            cache.revision += 1;
            let entry = SlotEntry {
                url: Arc::clone(url),
                rgba,
                revision: cache.revision,
                last_used: now,
            };
            if i == cache.entries.len() {
                cache.entries.push(entry);
            } else {
                cache.entries[i] = entry;
            }
            i
        };
        SkinRegion::at_slot(self.first, slot)
    }

    pub(crate) fn upload(&self, queue: &wgpu::Queue, texture: &wgpu::Texture, last: &mut u64) {
        let cache = self.slots.lock().unwrap();
        if *last == cache.revision {
            return;
        }
        for (i, e) in cache
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.revision > *last)
        {
            let origin = wgpu::Origin3d {
                x: ((i % 4) * 64) as u32,
                y: ((i / 4 % 4) * 64) as u32,
                z: self.first + (i / 16) as u32,
            };
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin,
                    aspect: wgpu::TextureAspect::All,
                },
                &e.rgba[..],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(64),
                },
                wgpu::Extent3d {
                    width: 64,
                    height: 64,
                    depth_or_array_layers: 1,
                },
            );
        }
        *last = cache.revision;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_base64(bytes: &[u8]) -> String {
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let b0 = chunk[0];
            let b1 = *chunk.get(1).unwrap_or(&0);
            let b2 = *chunk.get(2).unwrap_or(&0);
            out.push(TABLE[(b0 >> 2) as usize] as char);
            out.push(TABLE[(((b0 & 3) << 4) | (b1 >> 4)) as usize] as char);
            out.push(if chunk.len() > 1 {
                TABLE[(((b1 & 15) << 2) | (b2 >> 6)) as usize] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                TABLE[(b2 & 63) as usize] as char
            } else {
                '='
            });
        }
        out
    }

    #[test]
    fn parses_mojang_textures_property_and_validates_host() {
        let uuid = parse_uuid_hex("069a79f4-44e9-4726-a5be-fca90e38aaf5").unwrap();
        let slim_json = br#"{"textures":{"SKIN":{"url":"http://textures.minecraft.net/texture/abcdef012345","metadata":{"model":"slim"}}}}"#;
        let encoded = encode_base64(slim_json);
        let parsed = parse_textures_value(uuid, &encoded).unwrap();
        assert!(parsed.slim);
        assert_eq!(
            parsed.url.as_deref(),
            Some("https://textures.minecraft.net/texture/abcdef012345")
        );

        let classic_json =
            br#"{"textures":{"SKIN":{"url":"https://textures.minecraft.net/texture/99887766"}}}"#;
        let parsed_classic = parse_textures_value([1; 16], &encode_base64(classic_json)).unwrap();
        assert!(!parsed_classic.slim);
        assert_eq!(
            parsed_classic.url.as_deref(),
            Some("https://textures.minecraft.net/texture/99887766")
        );

        assert!(normalize_skin_url("http://evil.example.com/skin.png").is_none());
        assert!(normalize_skin_url("https://user@textures.minecraft.net/texture/abc").is_none());
    }

    #[test]
    fn legacy_64x32_skin_converts_limbs_and_strips_opaque_hat_box() {
        let mut legacy = image::RgbaImage::from_pixel(64, 32, image::Rgba([0, 0, 0, 255]));
        // Distinct pixels on right leg front (4..8, 20..32) and right arm front (44..48, 20..32)
        legacy.put_pixel(4, 20, image::Rgba([200, 10, 20, 128]));
        legacy.put_pixel(7, 20, image::Rgba([30, 210, 40, 255]));
        legacy.put_pixel(44, 20, image::Rgba([50, 60, 220, 255]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(legacy)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();

        let out = parse_user_skin(&png).unwrap();
        let px = |x: usize, y: usize| -> [u8; 4] {
            let i = (y * 64 + x) * 4;
            [out[i], out[i + 1], out[i + 2], out[i + 3]]
        };
        // Base body forced opaque
        assert_eq!(px(4, 20), [200, 10, 20, 255]);
        // Left leg front (20..24, 52..64) horizontally flipped from right leg front (4..8, 20..32)
        assert_eq!(px(23, 52), [200, 10, 20, 255]);
        assert_eq!(px(20, 52), [30, 210, 40, 255]);
        // Left arm front (36..40, 52..64) horizontally flipped from right arm front (44..48, 20..32)
        assert_eq!(px(39, 52), [50, 60, 220, 255]);
        // Solid alpha=255 hat area (32..64, 0..32) stripped to alpha=0
        assert_eq!(px(40, 8)[3], 0);
    }

    #[test]
    fn atlas_allocates_distinct_4x4_cells_and_reuses_url() {
        let mut pixels = vec![0u8; 256 * 256 * 4 * 2];
        let atlas = Atlas::reserve(&mut pixels);
        assert_eq!(atlas.first, 2);
        assert_eq!(pixels.len(), 256 * 256 * 4 * (2 + ATLAS_PAGES as usize));
        let sample = Arc::new([255u8; SKIN_PIXELS]);
        let u1: Arc<str> = Arc::from("https://textures.minecraft.net/texture/one");
        let u2: Arc<str> = Arc::from("https://textures.minecraft.net/texture/two");
        let r1 = atlas.allocate_slot(&u1, Arc::clone(&sample));
        let r2 = atlas.allocate_slot(&u2, Arc::clone(&sample));
        let r1_again = atlas.allocate_slot(&u1, sample);
        assert_eq!(r1, r1_again);
        assert_eq!((r1.layer, r1.origin), (2, [0.0, 0.0]));
        assert_eq!((r2.layer, r2.origin), (2, [0.25, 0.0]));
        assert_eq!(r2.subrect_uv(8., 8., 8., 8.), [0.25 + 8. / 256., 8. / 256., 8. / 256., 8. / 256.]);
    }

    #[test]
    fn sanitizes_usernames_and_extracts_skull_owner_tags() {
        use crate::vanilla::drops::DropNbt as Tag;
        use std::collections::BTreeMap;

        assert_eq!(sanitize_username("  Notch_123 "), Some("notch_123".into()));
        assert_eq!(sanitize_username("a"), Some("a".into()));
        assert_eq!(sanitize_username("SixteenCharsLong"), Some("sixteencharslong".into()));
        assert_eq!(sanitize_username(""), None);
        assert_eq!(sanitize_username("   "), None);
        assert_eq!(sanitize_username("SeventeenCharsLon"), None);
        assert_eq!(sanitize_username("../etc/passwd"), None);
        assert_eq!(sanitize_username("bad name"), None);
        assert_eq!(sanitize_username("CON"), None);
        assert_eq!(sanitize_username("lpt1"), None);

        let str_nbt = Tag::Compound(BTreeMap::from([(
            "SkullOwner".into(),
            Tag::String("Notch".into()),
        )]));
        assert_eq!(
            item_skull_owner(Some(&str_nbt)),
            Some(&Tag::String("Notch".into()))
        );

        let custom_json = br#"{"textures":{"SKIN":{"url":"http://textures.minecraft.net/texture/deadbeefcafe"}}}"#;
        let encoded = encode_base64(custom_json);
        let parsed = parse_textures_value_cached([0; 16], &encoded).unwrap();
        assert_eq!(
            parsed.url.as_deref(),
            Some("https://textures.minecraft.net/texture/deadbeefcafe")
        );
    }

    #[test]
    fn fetcher_skin_maps_cap_at_512_entries_with_lru_eviction() {
        let mut map = BoundedSkinMap::<u32, u32>::default();
        for i in 0..512 {
            map.insert(i, i * 10);
        }
        assert_eq!(map.len(), 512);
        // Touch key 0 so it becomes MRU and key 1 becomes the oldest.
        assert_eq!(map.get(&0), Some(&0));
        map.insert(512, 5120);
        assert_eq!(map.len(), 512);
        assert!(map.contains_key(&0));
        assert!(!map.contains_key(&1));
        assert_eq!(map.get(&512), Some(&5120));
    }
}
