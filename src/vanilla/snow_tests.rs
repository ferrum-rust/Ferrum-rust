use super::*;
use std::collections::BTreeMap;
struct SnowWorld {
    height: i32,
    light: u8,
    biome: u8,
    top_override: Option<u16>,
    changed: BTreeMap<(i32, i32, i32), u16>,
    writes: Vec<[i32; 4]>,
}
impl SnowWorld {
    fn initial(&self, p: IVec3) -> u16 {
        const TOP: [u16; 16] = [
            2 << 4,
            9 << 4,
            (9 << 4) | 1,
            8 << 4,
            79 << 4,
            174 << 4,
            18 << 4,
            161 << 4,
            (78 << 4) | 6,
            (78 << 4) | 7,
            (44 << 4) | 8,
            20 << 4,
            85 << 4,
            (31 << 4) | 1,
            0,
            10 << 4,
        ];
        if !(0..256).contains(&p.y) || p.y > self.height {
            0
        } else if p.y < self.height {
            1 << 4
        } else {
            self.top_override
                .unwrap_or(TOP[(p.x + p.z * 3).rem_euclid(16) as usize])
        }
    }
}
impl FeatureWorld for SnowWorld {
    fn get(&self, p: IVec3) -> u16 {
        self.changed
            .get(&(p.x, p.y, p.z))
            .copied()
            .unwrap_or_else(|| self.initial(p))
    }
    fn set(&mut self, p: IVec3, state: u16) {
        self.changed.insert((p.x, p.y, p.z), state);
        self.writes.push([p.x, p.y, p.z, i32::from(state)]);
    }
    fn biome(&self, _: IVec3) -> u8 {
        self.biome
    }
    fn light(&self, _: IVec3, sky: bool) -> u8 {
        if sky {
            15
        } else {
            self.light
        }
    }
    fn chest(&mut self, _: IVec3, _: Vec<Option<LootStack>>) {
        unreachable!()
    }
    fn spawner(&mut self, _: IVec3, _: &'static str) {
        unreachable!()
    }
    fn entity(&mut self, _: GeneratedEntity) {
        unreachable!()
    }
}
fn hash<const N: usize>(rows: impl IntoIterator<Item = [i32; N]>) -> String {
    let mut h = 0xcbf29ce484222325u64;
    for row in rows {
        for value in row {
            for byte in value.to_le_bytes() {
                h = (h ^ u64::from(byte)).wrapping_mul(0x100000001b3);
            }
        }
    }
    format!("{h:016x}")
}
#[test]
fn original_provider_ice_snow_and_precipitation_height_match() {
    let data: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/fixtures/vanilla189-snow.json")).unwrap();
    for (index, row) in data["cases"].as_array().unwrap().iter().enumerate() {
        let mut world = SnowWorld {
            height: row["height"].as_i64().unwrap() as i32,
            light: row["light"].as_u64().unwrap() as u8,
            biome: row["biome"].as_u64().unwrap() as u8,
            top_override: None,
            changed: BTreeMap::new(),
            writes: Vec::new(),
        };
        let origin = IVec3::new(
            row["chunk"][0].as_i64().unwrap() as i32 * 16,
            0,
            row["chunk"][1].as_i64().unwrap() as i32 * 16,
        );
        let mut precipitation = Vec::new();
        for x in 0..16 {
            for z in 0..16 {
                precipitation
                    .push([world.precipitation_height(origin.x + x + 8, origin.z + z + 8)]);
            }
        }
        assert_eq!(
            hash(precipitation),
            row["precipitation_hash"].as_str().unwrap(),
            "precipitation case{index}"
        );
        ice_and_snow(&mut world, origin);
        assert_eq!(
            world.writes.len() as u64,
            row["count"].as_u64().unwrap(),
            "write count case{index}"
        );
        assert_eq!(
            hash(world.writes),
            row["write_hash"].as_str().unwrap(),
            "ordered writes case{index}"
        );
    }
    assert_eq!(data["cases"].as_array().unwrap().len(), 732);
    for row in data["supports"].as_array().unwrap() {
        let state = row[0].as_u64().unwrap() as u16;
        let world = SnowWorld {
            height: 64,
            light: 0,
            biome: 12,
            top_override: Some(state),
            changed: BTreeMap::new(),
            writes: Vec::new(),
        };
        assert_eq!(
            snow_support(state),
            row[1].as_bool().unwrap(),
            "snow support {state}"
        );
        assert_eq!(
            world.can_snow(IVec3::new(0, 65, 0)),
            row[2].as_bool().unwrap(),
            "snow predicate {state}"
        );
        assert_eq!(
            world.precipitation_height(0, 0) as i64,
            row[3].as_i64().unwrap(),
            "precipitation {state}"
        );
    }
    assert_eq!(data["supports"].as_array().unwrap().len(), 1394);
}
