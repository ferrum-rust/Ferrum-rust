//! Original 1.8.9 CPU lightmap, sampled by the Vulkan terrain shader.
//! The caller supplies the world brightness, torch flicker and active effects.
use super::math;

#[derive(Default)]
pub struct TorchFlicker {
    pub value: f32,
    velocity: f32,
}
impl TorchFlicker {
    pub fn tick(&mut self, random: &mut super::random::JavaRandom) {
        let perturbation = (random.next_double() - random.next_double())
            * random.next_double()
            * random.next_double();
        self.velocity = (self.velocity as f64 + perturbation) as f32;
        self.velocity = (self.velocity as f64 * 0.9) as f32;
        self.value += self.velocity - self.value;
    }
}
#[derive(Clone, Copy, PartialEq)]
pub struct Input {
    pub dimension: i32,
    pub sun: f32,
    pub torch_flicker: f32,
    pub gamma: f32,
    pub boss: f32,
    pub previous_boss: f32,
    pub partial: f32,
    pub lightning: bool,
    pub night_vision: Option<i32>,
}
impl Default for Input {
    fn default() -> Self {
        Self {
            dimension: 0,
            sun: 1.0,
            torch_flicker: 0.0,
            gamma: 0.0,
            boss: 0.0,
            previous_boss: 0.0,
            partial: 1.0,
            lightning: false,
            night_vision: None,
        }
    }
}
pub fn brightness_table(dimension: i32) -> [f32; 16] {
    let ambient = if dimension == -1 { 0.1 } else { 0.0 };
    std::array::from_fn(|i| {
        let inverse = 1.0 - i as f32 / 15.0;
        (1.0 - inverse) / (inverse * 3.0 + 1.0) * (1.0 - ambient) + ambient
    })
}
pub fn colors(input: &Input) -> [[u8; 4]; 256] {
    let table = brightness_table(input.dimension);
    std::array::from_fn(|i| {
        let sky = table[i / 16]
            * if input.lightning {
                1.0
            } else {
                input.sun * 0.95 + 0.05
            };
        let block = table[i % 16] * (input.torch_flicker * 0.1 + 1.5);
        let warm = sky * (input.sun * 0.65 + 0.35);
        let green = block * ((block * 0.6 + 0.4) * 0.6 + 0.4);
        let blue = block * (block * block * 0.6 + 0.4);
        let mut rgb = [
            (warm + block) * 0.96 + 0.03,
            (warm + green) * 0.96 + 0.03,
            (sky + blue) * 0.96 + 0.03,
        ];
        if input.boss > 0.0 {
            let blend = input.previous_boss + (input.boss - input.previous_boss) * input.partial;
            for (component, factor) in rgb.iter_mut().zip([0.7, 0.6, 0.6]) {
                *component = *component * (1.0 - blend) + *component * factor * blend;
            }
        }
        if input.dimension == 1 {
            rgb = [0.22 + block * 0.75, 0.28 + green * 0.75, 0.25 + blue * 0.75];
        }
        if let Some(ticks) = input.night_vision {
            let strength = if ticks > 200 {
                1.0
            } else {
                0.7 + math::sin((ticks as f32 - input.partial) * std::f32::consts::PI * 0.2) * 0.3
            };
            let mut inverse = 1.0 / rgb[0];
            if inverse > 1.0 / rgb[1] {
                inverse = 1.0 / rgb[1];
            }
            if inverse > 1.0 / rgb[2] {
                inverse = 1.0 / rgb[2];
            }
            for c in &mut rgb {
                *c = *c * (1.0 - strength) + *c * inverse * strength;
            }
        }
        let mut result = [255; 4];
        for (out, mut c) in result.iter_mut().zip(rgb) {
            if c > 1.0 {
                c = 1.0;
            }
            let inverse = 1.0 - c;
            let bright = 1.0 - inverse * inverse * inverse * inverse;
            c = c * (1.0 - input.gamma) + bright * input.gamma;
            c = c * 0.96 + 0.03;
            *out = (c.clamp(0.0, 1.0) * 255.0) as u8;
        }
        result
    })
}
pub fn celestial_angle(time: i64, partial: f32, dimension: i32) -> f32 {
    if dimension == -1 {
        return 0.5;
    }
    if dimension == 1 {
        return 0.0;
    }
    let mut angle = ((time % 24000) as f32 + partial) / 24000.0 - 0.25;
    if angle < 0.0 {
        angle += 1.0;
    }
    if angle > 1.0 {
        angle -= 1.0;
    }
    let original = angle;
    angle = 1.0 - (((angle as f64 * std::f64::consts::PI).cos() + 1.0) / 2.0) as f32;
    original + (angle - original) / 3.0
}
pub fn sun_brightness(time: i64, partial: f32, dimension: i32, rain: f32, thunder: f32) -> f32 {
    let angle = celestial_angle(time, partial, dimension);
    let mut value = 1.0 - (math::cos(angle * std::f32::consts::PI * 2.0) * 2.0 + 0.2);
    value = 1.0 - value.clamp(0.0, 1.0);
    value = (value as f64 * (1.0 - (rain * 5.0) as f64 / 16.0)) as f32;
    // World.getThunderStrength already multiplies by the interpolated rain.
    value = (value as f64 * (1.0 - (thunder * rain * 5.0) as f64 / 16.0)) as f32;
    value * 0.8 + 0.2
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_torch_flicker_6000_ticks_and_random_draws_match() {
        let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-torch-flicker.json"
        ))
        .unwrap();
        for row in rows {
            let mut random =
                super::super::random::JavaRandom::new(row[0].as_str().unwrap().parse().unwrap());
            let mut flicker = TorchFlicker::default();
            for frame in row[1].as_array().unwrap() {
                flicker.tick(&mut random);
                assert_eq!(flicker.value.to_bits(), frame[0].as_u64().unwrap() as u32);
                assert_eq!(
                    flicker.velocity.to_bits(),
                    frame[1].as_u64().unwrap() as u32
                );
            }
            assert_eq!(random.next_long().to_string(), row[2].as_str().unwrap());
        }
    }
    #[test]
    fn original_renderer_lightmap_and_world_sun_match() {
        let data: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-lightmap.json"
        ))
        .unwrap();
        let f = |v: &serde_json::Value| f32::from_bits(v.as_u64().unwrap() as u32);
        for row in data["tables"].as_array().unwrap() {
            for (actual, expected) in brightness_table(row[0].as_i64().unwrap() as i32)
                .iter()
                .zip(row[1].as_array().unwrap())
            {
                assert_eq!(actual.to_bits(), f(expected).to_bits());
            }
        }
        for (index, row) in data["colors"].as_array().unwrap().iter().enumerate() {
            let input = Input {
                dimension: row[0].as_i64().unwrap() as i32,
                sun: f(&row[1]),
                torch_flicker: f(&row[2]),
                gamma: f(&row[3]),
                boss: f(&row[4]),
                previous_boss: f(&row[5]),
                partial: f(&row[6]),
                lightning: row[7] == 1,
                night_vision: match row[8].as_i64().unwrap() {
                    -1 => None,
                    n => Some(n as i32),
                },
            };
            let expected = row[9].as_str().unwrap();
            for (pixel, actual) in colors(&input).iter().enumerate() {
                let rgb = u32::from_str_radix(&expected[pixel * 6..pixel * 6 + 6], 16).unwrap();
                assert_eq!(
                    *actual,
                    [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 255],
                    "case={index} pixel={pixel}"
                );
            }
        }
        for row in data["sun"].as_array().unwrap() {
            let dimension = row[0].as_i64().unwrap() as i32;
            let time = row[1].as_str().unwrap().parse().unwrap();
            assert_eq!(
                celestial_angle(time, f(&row[2]), dimension).to_bits(),
                f(&row[5]).to_bits(),
                "angle {row}"
            );
            assert_eq!(
                sun_brightness(time, f(&row[2]), dimension, f(&row[3]), f(&row[4])).to_bits(),
                f(&row[6]).to_bits(),
                "sun {row}"
            );
        }
    }
}
