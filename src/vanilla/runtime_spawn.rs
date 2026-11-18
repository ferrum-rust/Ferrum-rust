//! Original DEFAULT overworld birth: choose the biome, probe grass at sea
//! level, then prepare the server's 25-by-25 initial region in X/Z order.
use super::*;

fn start_candidate(biomes: &BiomeGenerator, seed: i64, empty_list: bool) -> ([i32; 2], JavaRandom) {
    let mut random = JavaRandom::new(seed);
    let position = biomes.find_biome_position(0, 0, 256, &mut random, |id| {
        !empty_list && matches!(id, 1 | 4 | 5 | 18 | 19 | 21 | 22)
    });
    (position.unwrap_or([0, 0]), random)
}
fn choose_ground(
    [mut x, mut z]: [i32; 2],
    mut random: JavaRandom,
    mut ground: impl FnMut(i32, i32) -> Result<u16>,
) -> Result<IVec3> {
    for _ in 0..1000 {
        if ground(x, z)? >> 4 == 2 {
            break;
        }
        x += random.next_int(64) - random.next_int(64);
        z += random.next_int(64) - random.next_int(64);
    }
    Ok(IVec3::new(x, 64, z))
}
impl Runtime {
    fn spawn_ground(&mut self, x: i32, z: i32) -> Result<u16> {
        let access = self.access();
        let mut p = IVec3::new(x, 63, z);
        while access.get_block(p + IVec3::Y)? >> 4 != 0 {
            p.y += 1;
        }
        access.get_block(p)
    }
    /// World-spawn coordinates, distinct from the later randomized player
    /// placement. Reopening a checkpoint uses the already chosen world spawn.
    pub fn prepare_overworld_spawn(&mut self) -> Result<IVec3> {
        if let Some(point) = self.backend.world_spawn {
            return Ok(point);
        }
        let (candidate, random) = start_candidate(&self.backend.biomes, self.backend.seed, false);
        self.provider.finding_spawn = true;
        let point = choose_ground(candidate, random, |x, z| self.spawn_ground(x, z));
        self.provider.finding_spawn = false;
        let point = point?;
        self.backend.world.spawn = point.as_vec3();
        self.backend.world_spawn = Some(point);
        Ok(point)
    }
    pub fn load_initial_spawn_batch(&mut self, start: usize, count: usize) -> Result<usize> {
        ensure!(
            start <= 625 && (1..=256).contains(&count),
            "Invalid initial spawn load range"
        );
        let point = self.backend.world_spawn.ok_or_else(|| {
            anyhow::anyhow!("World spawn must be chosen before loading its region")
        })?;
        let end = (start + count).min(625);
        let region = initial_spawn_region(point);
        // Pure terrain for the next original requests runs while this owner
        // populates the current batch. Provider order remains X-major/Z-major.
        self.prefetch_terrain(&region[start..(start + 64).min(625)])?;
        self.prepare_terrain(&region[start..end])?;
        for &pos in &region[start..end] {
            self.provider.load(&mut self.backend, pos)?;
        }
        Ok(end)
    }
}
pub fn initial_spawn_region(point: IVec3) -> Vec<ChunkPos> {
    (-12..=12)
        .flat_map(|dx| (-12..=12).map(move |dz| ((point.x >> 4) + dx, (point.z >> 4) + dz)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    fn hash(bytes: impl IntoIterator<Item = u8>) -> String {
        format!(
            "{:x}",
            bytes
                .into_iter()
                .fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64)
                    .wrapping_mul(0x100000001b3))
        )
    }
    #[test]
    fn original_worldserver_spawn_probes_chunks_rng_and_region_order_match() {
        let rows: Vec<Value> = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-world-spawn.json"
        ))
        .unwrap();
        for row in rows {
            let seed = row["seed"].as_str().unwrap().parse().unwrap();
            let mode = row["mode"].as_u64().unwrap();
            let mut runtime = Runtime::for_overworld_verification(seed, 1234, true);
            let (p, random) = start_candidate(&runtime.backend.biomes, seed, mode == 2);
            let mut probes = Vec::new();
            runtime.provider.finding_spawn = true;
            let point = choose_ground(p, random, |x, z| {
                let state = if mode == 0 {
                    runtime.spawn_ground(x, z)?
                } else {
                    1 << 4
                };
                probes.push([x, z, (state >> 4) as i32]);
                Ok(state)
            })
            .unwrap();
            runtime.provider.finding_spawn = false;
            assert_eq!(
                json!(probes),
                row["probes"],
                "seed {seed} mode {mode} probes"
            );
            assert_eq!(
                json!(point.to_array()),
                row["spawn"],
                "seed {seed} mode {mode}"
            );
            assert_eq!(json!(initial_spawn_region(point)), row["warmup"]);
            assert_eq!(
                runtime.backend.world_random.state().to_string(),
                row["world_rand"].as_str().unwrap(),
                "seed {seed} world RNG"
            );
            assert_eq!(
                runtime.backend.provider_random.state().to_string(),
                row["provider_rand"].as_str().unwrap(),
                "seed {seed} provider RNG"
            );
            check_chunks(&runtime, &row["chunks"], seed);
        }
    }
    fn check_chunks(runtime: &Runtime, expected: &Value, seed: i64) {
        let rows = expected.as_array().unwrap();
        assert_eq!(
            runtime.provider.loaded_order().len(),
            rows.len(),
            "seed {seed} loaded count"
        );
        for (&(x, z), row) in runtime.provider.loaded_order().iter().zip(rows) {
            let c = &runtime.backend.world.chunks[&(x, z)];
            let record = runtime.provider.record((x, z)).unwrap();
            let mut light = Vec::with_capacity(131072);
            for y in 0..256 {
                for lz in 0..16 {
                    for lx in 0..16 {
                        let p = IVec3::new(x * 16 + lx, y, z * 16 + lz);
                        light.push(runtime.backend.light.get(Kind::Sky, p));
                        light.push(runtime.backend.light.get(Kind::Block, p));
                    }
                }
            }
            assert_eq!(
                json!([
                    x,
                    z,
                    record.terrain_populated,
                    record.light_populated,
                    hash(c.blocks.iter().flat_map(|b| b.to_le_bytes())),
                    hash(light)
                ]),
                *row,
                "seed {seed} chunk ({x},{z})"
            );
        }
    }
    #[test]
    fn original_default_spawn_region_all_625_requests_blocks_light_and_rng_match() {
        let row: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-world-start-region.json"
        ))
        .unwrap();
        let mut runtime = Runtime::for_overworld_verification(189, 1234, true);
        let point = runtime.prepare_overworld_spawn().unwrap();
        assert_eq!(json!(point.to_array()), row["spawn"]);
        let mut cursor = 0;
        while cursor < 625 {
            cursor = runtime.load_initial_spawn_batch(cursor, 32).unwrap();
        }
        check_chunks(&runtime, &row["region"], 189);
        assert_eq!(
            runtime.backend.world_random.state().to_string(),
            row["region_world_rand"].as_str().unwrap()
        );
        assert_eq!(
            runtime.backend.provider_random.state().to_string(),
            row["region_provider_rand"].as_str().unwrap()
        );
    }
}
