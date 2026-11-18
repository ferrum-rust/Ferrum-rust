//! Reuse chunk draw commands while the ordered visible set and bindings stay
//! unchanged. Dynamic meshes remain outside the two bundles for each eye.
use super::{ChunkMesh, ChunkPos, DEPTH};

pub(super) struct Eye<'a> {
    pub opaque: &'a wgpu::RenderPipeline,
    pub translucent: &'a wgpu::RenderPipeline,
    pub bind: &'a wgpu::BindGroup,
}

pub(super) struct Batches {
    pub opaque: wgpu::RenderBundle,
    pub translucent: wgpu::RenderBundle,
}

#[derive(Default)]
pub(super) struct TerrainBundles {
    order: Vec<(ChunkPos, u16)>,
    eye_count: usize,
    matching_frames: u8,
    pub eyes: Vec<Batches>,
}

impl TerrainBundles {
    // Borrow the actual cache key for opt-in diagnostics; never mutate it.
    pub(super) fn diagnostic_key(&self) -> (&[(ChunkPos, u16)], usize, u8, bool) {
        (&self.order, self.eye_count, self.matching_frames, !self.eyes.is_empty())
    }

    pub fn invalidate(&mut self) {
        self.eyes.clear();
        self.matching_frames = 0;
    }

    pub fn invalidate_position(&mut self, position: ChunkPos) {
        // The remembered order also covers the three-frame preparation gate.
        // Other uploads cannot change these recorded buffers/ranges. If their
        // visibility changes, prepare still compares the exact order and masks.
        if self.order.iter().any(|&(p, _)| p == position) {
            self.invalidate();
        }
    }

    pub(super) fn invalidate_position_captured(&mut self,position:ChunkPos,trace:&mut super::upload_capture::Trace) {
        trace.observation.remembered_key_len=self.order.len();
        trace.observation.cached_eyes_before=self.eyes.len();
        let scanning=std::time::Instant::now();
        let matched=self.order.iter().any(|&(p,_)|p==position);
        trace.phase(super::upload_capture::Phase::CacheScan,scanning);
        trace.observation.cache_position_matched=matched;
        if matched {
            trace.observation.released_bundle_handles=self.eyes.len()*2;
            let releasing=std::time::Instant::now();
            self.eyes.clear();
            trace.phase(super::upload_capture::Phase::BundleDrop,releasing);
            self.matching_frames=0;
        }
        trace.observation.cached_eyes_after=self.eyes.len();
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        eyes: &[Eye<'_>],
        chunks: &[(ChunkPos, &mut ChunkMesh)],
    ) -> (bool, usize) {
        let same = self.eye_count == eyes.len()
            && self.order.len() == chunks.len()
            && self
                .order
                .iter()
                .copied()
                .eq(chunks.iter().map(|(pos, chunk)| (*pos, chunk.mask)));
        if !same {
            self.invalidate();
            self.eye_count = eyes.len();
            self.order.clear();
            self.order
                .extend(chunks.iter().map(|(pos, chunk)| (*pos, chunk.mask)));
        }
        if chunks.is_empty() || !self.eyes.is_empty() {
            return (false, 0);
        }
        self.matching_frames += 1;
        if self.matching_frames < 3 {
            return (false, 0);
        }
        let formats = [Some(format)];
        let descriptor = wgpu::RenderBundleEncoderDescriptor {
            label: Some("ordered terrain draws"),
            color_formats: &formats,
            depth_stencil: Some(wgpu::RenderBundleDepthStencil {
                format: DEPTH,
                depth_read_only: false,
                stencil_read_only: true,
            }),
            sample_count: 1,
            multiview: None,
        };
        let mut draws = 0;
        for eye in eyes {
            let mut opaque = device.create_render_bundle_encoder(&descriptor);
            opaque.set_pipeline(eye.opaque);
            opaque.set_bind_group(0, eye.bind, &[]);
            for (_, chunk) in chunks {
                for range in &chunk.solid_ranges {
                    draws += usize::from(!range.is_empty());
                    record(&mut opaque, chunk, range.clone());
                }
            }
            let mut translucent = device.create_render_bundle_encoder(&descriptor);
            translucent.set_pipeline(eye.translucent);
            translucent.set_bind_group(0, eye.bind, &[]);
            for (_, chunk) in chunks.iter().rev() {
                draws += usize::from(!chunk.translucent_range().is_empty());
                record(&mut translucent, chunk, chunk.translucent_range());
            }
            self.eyes.push(Batches {
                opaque: opaque.finish(&wgpu::RenderBundleDescriptor {
                    label: Some("opaque terrain"),
                }),
                translucent: translucent.finish(&wgpu::RenderBundleDescriptor {
                    label: Some("translucent terrain"),
                }),
            });
        }
        (true, draws)
    }
}

fn record<'a>(
    encoder: &mut wgpu::RenderBundleEncoder<'a>,
    chunk: &'a ChunkMesh,
    range: std::ops::Range<u32>,
) {
    if !range.is_empty() {
        encoder.set_vertex_buffer(0, chunk.gpu.vertex_slice());
        encoder.set_index_buffer(chunk.gpu.index_slice(), chunk.gpu.index_format);
        encoder.draw_indexed(range, 0, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invisible_payload_changes_preserve_preparation_gate_but_visible_changes_reset_it() {
        let mut cache = TerrainBundles { order: vec![((0, 0), 3), ((-1, 5), 8)],
            matching_frames: 2, ..Default::default() };
        cache.invalidate_position((100, 100));
        assert_eq!(cache.matching_frames, 2);
        cache.invalidate_position((-1, 5));
        assert_eq!(cache.matching_frames, 0);
        assert_eq!(cache.order, vec![((0, 0), 3), ((-1, 5), 8)]);
    }
}
