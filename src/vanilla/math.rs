//! MathHelper's 65536-entry float sine table and truncating lookup.
use std::sync::OnceLock;
#[path = "strict_trig.rs"]
mod strict_trig;
pub use strict_trig::unit_circle;
fn sine_table() -> &'static [f32; 65536] {
    static TABLE: OnceLock<Box<[f32; 65536]>> = OnceLock::new();
    TABLE.get_or_init(|| {
        Box::new(std::array::from_fn(|i| {
            unit_circle(i as f64 * std::f64::consts::PI * 2.0 / 65536.0).1 as f32
        }))
    })
}
pub fn sin(angle: f32) -> f32 {
    sine_table()[((angle * 10430.378) as i32 & 65535) as usize]
}
pub fn cos(angle: f32) -> f32 {
    sine_table()[((angle * 10430.378 + 16384.0) as i32 & 65535) as usize]
}
#[cfg(test)]
pub fn table_bits() -> impl Iterator<Item = u32> {
    sine_table().iter().map(|f| f.to_bits())
}
