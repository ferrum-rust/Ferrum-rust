//! Exact original registry material/block properties. These are block-ID
//! properties, independent of legacy metadata. Unknown IDs resolve to air.
#[path = "material_data.rs"]
mod data;
pub fn light_opacity(state: u16) -> u8 {
    data::LIGHT_OPACITY
        .get((state >> 4) as usize)
        .copied()
        .unwrap_or(0) as u8
}
pub fn light_value(state: u16) -> u8 {
    data::LIGHT_VALUE
        .get((state >> 4) as usize)
        .copied()
        .unwrap_or(0) as u8
}
pub fn neighbor_brightness(state: u16) -> bool {
    data::NEIGHBOR_BRIGHTNESS
        .get((state >> 4) as usize)
        .copied()
        .unwrap_or(0)
        != 0
}
#[inline]
pub fn flags(state: u16) -> u16 {
    data::FLAGS
        .get((state >> 4) as usize)
        .copied()
        .unwrap_or(data::FLAGS[0])
}
#[inline]
pub fn blocks_movement(state: u16) -> bool {
    flags(state) & 1 != 0
}
#[inline]
pub fn is_solid(state: u16) -> bool {
    flags(state) & 2 != 0
}
#[inline]
pub fn is_liquid(state: u16) -> bool {
    flags(state) & 4 != 0
}
#[inline]
pub fn is_portal(state: u16) -> bool {
    flags(state) & 8 != 0
}
#[inline]
pub fn normal_cube(state: u16) -> bool {
    flags(state) & 16 != 0
}
#[inline]
pub fn full_block(state: u16) -> bool {
    flags(state) & 32 != 0
}
#[inline]
pub fn is_leaves(state: u16) -> bool {
    flags(state) & 64 != 0
}
#[inline]
pub fn full_cube(state: u16) -> bool {
    flags(state) & 128 != 0
}
#[inline]
pub fn replaceable(state: u16) -> bool {
    flags(state) & 256 != 0
}
#[inline]
pub fn opaque(state: u16) -> bool {
    flags(state) & 512 != 0
}
#[inline]
pub fn can_burn(state: u16) -> bool {
    flags(state) & 1024 != 0
}
#[inline]
pub fn blocks_light(state: u16) -> bool {
    flags(state) & 2048 != 0
}
#[inline]
pub fn opaque_cube(state: u16) -> bool {
    flags(state) & 4096 != 0
}
#[inline]
pub fn can_provide_power(state: u16) -> bool {
    flags(state) & 8192 != 0
}
#[inline]
pub fn is_vine(state: u16) -> bool {
    flags(state) & 16384 != 0
}
#[inline]
pub fn is_circuits(state: u16) -> bool {
    flags(state) & 32768 != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_registry_material_property_matches_original_bytecode() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-materials.json"
        ))
        .unwrap();
        let predicates: [fn(u16) -> bool; 16] = [
            blocks_movement,
            is_solid,
            is_liquid,
            is_portal,
            normal_cube,
            full_block,
            is_leaves,
            full_cube,
            replaceable,
            opaque,
            can_burn,
            blocks_light,
            opaque_cube,
            can_provide_power,
            is_vine,
            is_circuits,
        ];
        for row in fixture["rows"].as_array().unwrap() {
            let id = row[0].as_u64().unwrap() as u16;
            let original = row[2].as_u64().unwrap() as u16;
            for meta in 0..16 {
                let state = id << 4 | meta;
                assert_eq!(flags(state), original, "block {id}:{meta}");
                assert_eq!(
                    light_opacity(state) as u64,
                    row[4].as_u64().unwrap(),
                    "opacity {id}:{meta}"
                );
                assert_eq!(
                    light_value(state) as u64,
                    row[5].as_u64().unwrap(),
                    "emission {id}:{meta}"
                );
                assert_eq!(
                    neighbor_brightness(state),
                    row[6].as_u64().unwrap() != 0,
                    "neighbor brightness {id}:{meta}"
                );
                for (bit, predicate) in predicates.iter().enumerate() {
                    assert_eq!(
                        predicate(state),
                        original & (1 << bit) != 0,
                        "block {id}:{meta} {}",
                        fixture["bits"][bit]
                    );
                }
            }
        }
        assert_eq!(
            fixture["unknown_resolved_ids"],
            serde_json::json!([0, 0, 0])
        );
        for id in [256, 4095] {
            assert_eq!(flags(id << 4), flags(0));
        }
    }
}
