//! Exact optional omission of far tiles whose every possible flat owner is
//! covered by the same near-mask snapshot sent to the terrain shader.
use crate::{distant_terrain::{TilePos,TILE_CHUNKS},mesh::Mesh,models};

// The original world supports farther coordinates. They deliberately remain
// unproven here: this guard makes BOTH shader additions exact for all byte
// offsets, including negative coordinates and reassociation. No float guess.
const EXACT_XZ_BOUND:f32=16_776_960.; // 2^24 - 256
const WORLD_TILE_LIMIT:i32=234_375;
type Mask=[[u32;4];32];

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(super) struct Proof { chunk_min:[i32;2] }
impl Proof {
    pub(super) fn from_mesh(position:TilePos,mesh:&Mesh)->Option<Self> {
        if !(-WORLD_TILE_LIMIT..WORLD_TILE_LIMIT).contains(&position.0)
            || !(-WORLD_TILE_LIMIT..WORLD_TILE_LIMIT).contains(&position.1)
            || mesh.indices.is_empty() || mesh.indices.len()%3!=0
            || u32::try_from(mesh.indices.len()).is_err()
            || mesh.indices.iter().any(|&i|i as usize>=mesh.vertices.len()) {
            return None;
        }
        // Widen before arithmetic. This proof's nominal tile remains exactly
        // eight chunks on each axis; actual owner subsets never weaken it.
        let chunk_min=[i64::from(position.0)*i64::from(TILE_CHUNKS),
            i64::from(position.1)*i64::from(TILE_CHUNKS)];
        // Validate all vertices, even unreferenced ones. This is conservative
        // and independent of topology/provoking-vertex/backend choices.
        for vertex in &mesh.vertices {
            if vertex.layer & super::distant_render::DISTANT==0
                || vertex.layer & (models::CLOUD|models::SKY)!=0
                || vertex.position.iter().any(|v|!v.is_finite()) {return None;}
            for (axis,byte) in [(0,2),(2,3)] {
                let p=vertex.position[axis];
                if p.abs()>EXACT_XZ_BOUND || p.fract()!=0. {return None;}
                // Input light is Unorm8x4. round(light*255) recovers the byte;
                // its conversion/multiplication error is far below 0.5.
                // Integral |p|<=2^24-256 keeps p+byte and p+byte-128 within
                // exactly representable integers. Division by16 is exact.
                let owner=(p as i64+i64::from(vertex.light[byte])-128).div_euclid(16);
                let min=chunk_min[axis/2];
                if owner<min || owner>=min+i64::from(TILE_CHUNKS) {return None;}
            }
        }
        Some(Self {chunk_min:[i32::try_from(chunk_min[0]).ok()?,i32::try_from(chunk_min[1]).ok()?]})
    }

    pub(super) fn covered(self,origin:[i32;4],mask:&Mask)->bool {
        if origin[2]!=64 || origin[3]!=64 {return false;}
        let x=i64::from(self.chunk_min[0])-i64::from(origin[0]);
        let z=i64::from(self.chunk_min[1])-i64::from(origin[1]);
        // Partial/outside windows are KEEP, including old halo uploads. The
        // actual shader mask already omits out-of-detail-radius residents.
        if !(0..=56).contains(&x) || !(0..=56).contains(&z) {return false;}
        let wanted=0xffu64 << x as u32;
        for row in z as usize..z as usize+8 {
            let words=&mask[row/2];let word=(row%2)*2;
            let bits=u64::from(words[word]) | (u64::from(words[word+1])<<32);
            if bits & wanted != wanted {return false;}
        }
        true
    }
}

#[derive(Default)]
struct Counts {
    measured_frames:u64,
    visible_candidates_before:u64,
    visible_culled:u64,
    indices_omitted:u64,
    unproven_kept:u64,
}

#[derive(Default)]
pub(super) struct Trial { enabled:bool, counts:Counts }
impl Trial {
    pub(super) fn new(enabled:bool)->Self {Self {enabled,..Default::default()}}
    pub(super) fn prove_upload(&self,position:TilePos,mesh:&Mesh)->Option<Proof> {
        // Ordinary OFF uploads never enter the new vertex/index scan.
        if self.enabled {Proof::from_mesh(position,mesh)} else {None}
    }
    pub(super) fn begin_frame(&mut self,measured:bool) {
        if self.enabled && measured {self.counts.measured_frames=self.counts.measured_frames.saturating_add(1);}
    }
    pub(super) fn keep_visible(&mut self,proof:Option<Proof>,origin:[i32;4],mask:&Mask,
        index_count:u32,measured:bool)->bool {
        if !self.enabled {return true;}
        let culled=proof.is_some_and(|proof|proof.covered(origin,mask));
        if measured {
            let c=&mut self.counts;
            c.visible_candidates_before=c.visible_candidates_before.saturating_add(1);
            c.unproven_kept=c.unproven_kept.saturating_add(u64::from(proof.is_none()));
            if culled {
                c.visible_culled=c.visible_culled.saturating_add(1);
                c.indices_omitted=c.indices_omitted.saturating_add(u64::from(index_count));
            }
        }
        !culled
    }
    pub(super) fn report(&self)->serde_json::Value {
        let mut report=serde_json::json!({
            "enabled":self.enabled,"mode":if self.enabled {"omit-proven-fully-covered"}else{"keep-all"},
            "proof_scan_on_upload":self.enabled,
            "proof_option_bytes_per_tile":std::mem::size_of::<Option<Proof>>(),
            "proof_scope":"When enabled, every ordinary distant payload upload replaces provenance after scanning all indices/vertices. DISTANT is required, CLOUD/SKY forbidden, finite XYZ and integral X/Z within +/- (2^24-256); every reconstructed flat owner must lie in the nominal8x8 tile. Other/malformed/outer-world meshes remain unproven and are kept. No scan, migration or forced upload on late enable.",
            "scope":"Only candidates surviving original nonempty/bounds/frustum tests are checked. Omission requires all64 bits in the exact Camera origin/mask snapshot. Enumeration, group slots, surviving order and payload revisions remain original. This changes no resident data, geometry, density, shader, synchronization or graphics settings.",
            "cost_scope":"Explicit candidate includes proof CPU work at upload plus per-frame coverage checks. OFF does neither scan nor coverage work/counters. Both same-binary arms contain the additional inline Tile Option layout; no performance or memory gain versus an older binary is inferred.",
            "counter_scope":"Measured world-preparation frames only, once before either eye. Candidate and index counts are per tile preparation, not multiplied by eyes, draw calls or GPU execution. Empty/menu frames absent. Saturating totals; visible survivors=visible_candidates_before-visible_culled."});
        if self.enabled {
            let c=&self.counts;
            report["counts"]=serde_json::json!({"measured_frames":c.measured_frames,
                "visible_candidates_before":c.visible_candidates_before,"visible_culled":c.visible_culled,
                "indices_omitted":c.indices_omitted,"unproven_kept":c.unproven_kept});
        }
        report
    }
}

#[cfg(test)]
#[path="far_coverage_tests.rs"]
mod tests;
