//! Overworld World.getSkyColor and EntityRenderer.updateFogColor/setupFog.
//! Colors are the legacy GL color arguments, before framebuffer conversion.
use super::{lightmap, math};
use std::f32::consts::PI;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Medium {
    #[default]
    Air,
    Water,
    Lava,
    Cloud,
}

#[derive(Clone, Copy, Debug)]
pub struct Input {
    pub dimension: i32,
    pub time: i64,
    pub partial: f32,
    pub feet_y: f64,
    pub temperature: f32,
    /// X component of Entity.getLook(partial), even in third person.
    pub look_x: f32,
    pub radius: u32,
    pub brightness: f32,
    pub flat: bool,
    pub blindness: Option<i32>,
    pub night_vision: Option<i32>,
    pub medium: Medium,
    pub respiration: i32,
    pub water_breathing: bool,
    /// World.getRainStrength and World.getThunderStrength, already interpolated.
    pub rain: f32,
    pub thunder: f32,
    pub lightning_ticks: i32,
    pub boss: f32,
    pub anaglyph: bool,
}
impl Default for Input {
    fn default() -> Self {
        Self {
            dimension: 0,
            time: 6000,
            partial: 0.,
            feet_y: 64.,
            temperature: 0.8,
            look_x: 0.,
            radius: 8,
            brightness: 1.,
            flat: false,
            blindness: None,
            night_vision: None,
            medium: Medium::Air,
            respiration: 0,
            water_breathing: false,
            rain: 0.,
            thunder: 0.,
            lightning_ticks: 0,
            boss: 0.,
            anaglyph: false,
        }
    }
}
pub struct Atmosphere {
    pub sky: [f32; 3],
    pub fog: [f32; 3],
    pub lower_sky: [f32; 3],
    pub sky_range: [f32; 2],
    pub terrain_range: [f32; 2],
    pub density: Option<f32>,
    pub sunrise: Option<[f32; 4]>,
    pub sunset_west: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct FogBrightness {
    previous: f32,
    current: f32,
}
impl FogBrightness {
    pub fn tick(&mut self, light_level: u8, radius: u32) {
        self.tick_dimension(light_level, radius, 0);
    }
    pub fn tick_dimension(&mut self, light_level: u8, radius: u32, dimension: i32) {
        self.previous = self.current;
        let light = lightmap::brightness_table(dimension)[usize::from(light_level.min(15))];
        let distance = radius as f32 / 32.;
        let target = light * (1. - distance) + distance;
        self.current += (target - self.current) * 0.1;
    }
    pub fn sample(&self, partial: f32) -> f32 {
        self.previous + (self.current - self.previous) * partial
    }
}
fn biome_sky(temperature: f32) -> [f32; 3] {
    let t = (temperature / 3.).clamp(-1., 1.);
    let hue = (0.62222224 - t * 0.05) * 6.;
    let sat = 0.5 + t * 0.1;
    let frac = hue - hue as i32 as f32;
    let low = 1. - sat;
    let rgb = if hue < 4. {
        [low, 1. - frac * sat, 1.]
    } else {
        [1. - (1. - frac) * sat, low, 1.]
    };
    rgb.map(|v| (v * 255.) as u8 as f32 / 255.)
}
fn gray(rgb: [f32; 3], brightness: f32, amount: f32) -> [f32; 3] {
    let gray = (rgb[0] * 0.3 + rgb[1] * 0.59 + rgb[2] * 0.11) * brightness;
    let keep = 1. - amount;
    rgb.map(|channel| channel * keep + gray * (1. - keep))
}
fn anaglyph([r, g, b]: [f32; 3]) -> [f32; 3] {
    [
        (r * 30. + g * 59. + b * 11.) / 100.,
        (r * 30. + g * 70.) / 100.,
        (r * 30. + b * 70.) / 100.,
    ]
}
pub fn sample(i: Input) -> Atmosphere {
    if matches!(i.dimension, -1 | 1) {
        return sample_non_overworld(i);
    }
    let angle = lightmap::celestial_angle(i.time, i.partial, 0);
    let rad = angle * PI * 2.;
    let cosine = math::cos(rad);
    let daylight = (cosine * 2. + 0.5).clamp(0., 1.);
    let mut sky = biome_sky(i.temperature).map(|v| v * daylight);
    if i.rain > 0. {
        sky = gray(sky, 0.6, i.rain * 0.75);
    }
    if i.thunder > 0. {
        sky = gray(sky, 0.2, i.thunder * 0.75);
    }
    if i.lightning_ticks > 0 {
        let flash = (i.lightning_ticks as f32 - i.partial).min(1.) * 0.45;
        sky = [
            sky[0] * (1. - flash) + 0.8 * flash,
            sky[1] * (1. - flash) + 0.8 * flash,
            sky[2] * (1. - flash) + flash,
        ];
    }
    let sunrise = if (-0.4..=0.4).contains(&cosine) {
        let phase = cosine / 0.4 * 0.5 + 0.5;
        let alpha = 1. - (1. - math::sin(phase * PI)) * 0.99;
        Some([
            phase * 0.3 + 0.7,
            phase * phase * 0.7 + 0.2,
            0.2,
            alpha * alpha,
        ])
    } else {
        None
    };
    let sunset_west = math::sin(rad) > 0.;
    let mut fog = [
        0.7529412 * (daylight * 0.94 + 0.06),
        0.84705883 * (daylight * 0.94 + 0.06),
        daylight * 0.91 + 0.09,
    ];
    if let Some(sun) = sunrise.filter(|_| i.radius >= 4) {
        let amount = (if sunset_west { -i.look_x } else { i.look_x }).max(0.) * sun[3];
        for n in 0..3 {
            fog[n] = fog[n] * (1. - amount) + sun[n] * amount;
        }
    }
    let sky_weight = 1. - f64::from(0.25 + 0.75 * i.radius as f32 / 32.).powf(0.25) as f32;
    for n in 0..3 {
        fog[n] += (sky[n] - fog[n]) * sky_weight;
    }
    fog[0] *= 1. - i.rain * 0.5;
    fog[1] *= 1. - i.rain * 0.5;
    fog[2] *= 1. - i.rain * 0.4;
    fog = fog.map(|v| v * (1. - i.thunder * 0.5));
    let mut density = None;
    match i.medium {
        Medium::Cloud => {
            let colors = if i.rain > 0. {
                gray([1.; 3], 0.6, i.rain * 0.95)
            } else {
                [1.; 3]
            };
            fog = [
                colors[0] * (daylight * 0.9 + 0.1),
                colors[1] * (daylight * 0.9 + 0.1),
                colors[2] * (daylight * 0.85 + 0.15),
            ];
            if i.thunder > 0. {
                fog = gray(fog, 0.2, i.thunder * 0.95);
            }
            density = Some(0.1);
        }
        Medium::Water => {
            let mut breath = i.respiration as f32 * 0.2;
            if i.water_breathing {
                breath = breath * 0.3 + 0.6;
            }
            fog = [0.02 + breath, 0.02 + breath, 0.2 + breath];
            density = Some(if i.water_breathing {
                0.01
            } else {
                0.1 - i.respiration as f32 * 0.03
            });
        }
        Medium::Lava => {
            fog = [0.6, 0.1, 0.];
            density = Some(2.);
        }
        Medium::Air => {}
    }
    fog = fog.map(|v| v * i.brightness);
    let mut darkness = i.feet_y * if i.flat { 1. } else { 0.03125 };
    if let Some(ticks) = i.blindness {
        darkness = if ticks < 20 {
            darkness * f64::from(1. - ticks as f32 / 20.)
        } else {
            0.
        };
    }
    if darkness < 1. {
        let factor = darkness.max(0.) * darkness.max(0.);
        fog = fog.map(|v| (f64::from(v) * factor) as f32);
    }
    if i.boss > 0. {
        for (c, factor) in fog.iter_mut().zip([0.7, 0.6, 0.6]) {
            *c = *c * (1. - i.boss) + *c * factor * i.boss;
        }
    }
    if let Some(ticks) = i.night_vision {
        let strength = if ticks > 200 {
            1.
        } else {
            0.7 + math::sin((ticks as f32 - i.partial) * PI * 0.2) * 0.3
        };
        let intensity = (1. / fog[0]).min(1. / fog[1]).min(1. / fog[2]);
        fog = fog.map(|v| v * (1. - strength) + v * intensity * strength);
    }
    if i.anaglyph {
        sky = anaglyph(sky);
        fog = anaglyph(fog);
    }
    let distance = i.radius as f32 * 16.;
    let (sky_range, terrain_range) = if let Some(ticks) = i.blindness {
        let end = if ticks < 20 {
            5. + (distance - 5.) * (1. - ticks as f32 / 20.)
        } else {
            5.
        };
        density = None;
        ([0., end * 0.8], [end * 0.25, end])
    } else {
        ([0., distance], [distance * 0.75, distance])
    };
    Atmosphere {
        sky,
        lower_sky: [sky[0] * 0.2 + 0.04, sky[1] * 0.2 + 0.04, sky[2] * 0.6 + 0.1],
        fog,
        sky_range,
        terrain_range,
        density,
        sunrise,
        sunset_west,
    }
}
fn sample_non_overworld(i: Input) -> Atmosphere {
    let mut fog = if i.dimension == -1 {
        [0.2, 0.03, 0.03]
    } else {
        [
            (160.0 / 255.0) * 0.15,
            (128.0 / 255.0) * 0.15,
            (160.0 / 255.0) * 0.15,
        ]
    };
    let mut density = None;
    match i.medium {
        Medium::Cloud => {
            density = Some(0.1);
        }
        Medium::Water => {
            let mut breath = i.respiration as f32 * 0.2;
            if i.water_breathing {
                breath = breath * 0.3 + 0.6;
            }
            fog = [0.02 + breath, 0.02 + breath, 0.2 + breath];
            density = Some(if i.water_breathing {
                0.01
            } else {
                0.1 - i.respiration as f32 * 0.03
            });
        }
        Medium::Lava => {
            fog = [0.6, 0.1, 0.];
            density = Some(2.);
        }
        Medium::Air => {}
    }
    fog = fog.map(|v| v * i.brightness);
    // Nether (-1) and The End (1) have `hasNoSky = true`, so `getVoidFogYFactor()` is 1.0
    // (no void fog darkening at low Y), while Blindness still scales `d` from 1.0 down to 0.0.
    if let Some(ticks) = i.blindness {
        let darkness = if ticks < 20 {
            f64::from(1. - ticks as f32 / 20.).max(0.)
        } else {
            0.
        };
        let factor = darkness * darkness;
        fog = fog.map(|v| (f64::from(v) * factor) as f32);
    }
    if i.boss > 0. {
        for (c, factor) in fog.iter_mut().zip([0.7, 0.6, 0.6]) {
            *c = *c * (1. - i.boss) + *c * factor * i.boss;
        }
    }
    if let Some(ticks) = i.night_vision {
        let strength = if ticks > 200 {
            1.
        } else {
            0.7 + math::sin((ticks as f32 - i.partial) * PI * 0.2) * 0.3
        };
        let max_channel = fog[0].max(fog[1]).max(fog[2]);
        if max_channel > 0. {
            let intensity = 1. / max_channel;
            fog = fog.map(|v| v * (1. - strength) + v * intensity * strength);
        }
    }
    let mut sky = if i.dimension == -1 {
        fog
    } else {
        [0.0, 0.0, 0.0]
    };
    if i.anaglyph {
        sky = anaglyph(sky);
        fog = anaglyph(fog);
    }
    let far = i.radius as f32 * 16.;
    let (sky_range, terrain_range) = if let Some(ticks) = i.blindness {
        let end = if ticks < 20 {
            5. + (far - 5.) * (1. - ticks as f32 / 20.)
        } else {
            5.
        };
        density = None;
        ([0., end * 0.8], [end * 0.25, end])
    } else if i.dimension == -1 {
        let nether_end = far.min(192.0) * 0.5;
        ([0., nether_end], [far * 0.05, nether_end])
    } else {
        ([0., far], [far * 0.75, far])
    };
    Atmosphere {
        sky,
        lower_sky: if i.dimension == -1 {
            sky
        } else {
            [0.0, 0.0, 0.0]
        },
        fog,
        sky_range,
        terrain_range,
        density,
        sunrise: None,
        sunset_west: false,
    }
}
/// World.updateSkylightSubtracted always samples the end of the current tick.
pub fn skylight_subtracted(time: i64, rain: f32, thunder: f32) -> u8 {
    let angle = lightmap::celestial_angle(time, 1., 0);
    let mut sky = 1. - (1. - (math::cos(angle * PI * 2.) * 2. + 0.5)).clamp(0., 1.);
    for strength in [rain, thunder] {
        sky = (f64::from(sky) * (1. - f64::from(strength * 5.) / 16.)) as f32;
    }
    ((1. - sky) * 11.) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::GzDecoder;
    use serde_json::Value;
    fn corpus() -> Value {
        serde_json::from_reader(GzDecoder::new(
            &include_bytes!("../../tests/fixtures/vanilla189-atmosphere.json.gz")[..],
        ))
        .unwrap()
    }
    fn expect(arr: &[f32], row: &Value, case: usize, field: &str) {
        let actual = arr
            .iter()
            .map(|n| {
                if n.is_nan() {
                    f32::NAN.to_bits()
                } else {
                    n.to_bits()
                }
            })
            .collect::<Vec<_>>();
        let java = row
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_u64().unwrap() as u32)
            .collect::<Vec<_>>();
        assert_eq!(actual, java, "original atmosphere {field} case {case}");
    }
    #[test]
    fn original_overworld_sky_fog_color_density_and_linear_ranges_match() {
        let fixture = corpus();
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 1536);
        for (case, r) in cases.iter().enumerate() {
            let x = &r[0];
            let n = |j: usize| x[j].as_i64().unwrap() as i32;
            let f = |j: usize| x[j].as_f64().unwrap() as f32;
            let opt = |j| Some(n(j)).filter(|v| *v >= 0);
            let i = Input {
                dimension: 0,
                time: i64::from(n(0)),
                partial: f(1),
                feet_y: x[3].as_f64().unwrap(),
                temperature: f(6),
                look_x: f(7),
                radius: n(8) as u32,
                flat: x[9].as_bool().unwrap(),
                brightness: f(10),
                medium: [Medium::Air, Medium::Water, Medium::Lava, Medium::Cloud][n(11) as usize],
                respiration: n(12),
                water_breathing: x[13].as_bool().unwrap(),
                blindness: opt(14),
                night_vision: opt(15),
                rain: f(16),
                thunder: f(17),
                lightning_ticks: n(18),
                boss: f(19),
                anaglyph: x[20].as_bool().unwrap(),
            };
            assert_eq!(
                crate::assets::biome_colors::temperature(
                    n(5) as u8,
                    glam::IVec3::new(n(2), i.feet_y.floor() as i32, n(4))
                )
                .to_bits(),
                i.temperature.to_bits(),
                "temperature {case}"
            );
            let a = sample(i);
            if !i.anaglyph {
                expect(&a.sky, &r[1], case, "sky");
            }
            expect(&a.fog, &r[2], case, "fog");
            expect(
                a.sunrise.as_ref().map_or(&[][..], |s| &s[..]),
                &r[3],
                case,
                "sunrise",
            );
            if let Some(density) = a.density {
                expect(&[density], &r[4], case, "density");
            } else {
                expect(
                    &[
                        a.sky_range[0],
                        a.sky_range[1],
                        a.terrain_range[0],
                        a.terrain_range[1],
                    ],
                    &r[4],
                    case,
                    "linear range",
                );
            }
            assert_eq!(
                u64::from(skylight_subtracted(i.time, i.rain, i.thunder)),
                r[5].as_u64().unwrap(),
                "skylight {case}"
            );
        }
    }
    #[test]
    fn original_fog_brightness_tick_history_matches() {
        let data = corpus();
        let mut brightness = FogBrightness::default();
        let cases = data["brightness"].as_array().unwrap();
        assert_eq!(cases.len(), 256);
        for (idx, row) in cases.iter().enumerate() {
            brightness.tick(
                row[0].as_u64().unwrap() as u8,
                row[1].as_u64().unwrap() as u32,
            );
            expect(
                &[brightness.previous, brightness.current],
                &row[2],
                idx,
                "brightness tick",
            );
        }
    }
    #[test]
    fn nether_thick_red_fog_and_no_void_fog_preserve_medium_and_effects() {
        let a = sample(Input {
            dimension: -1,
            radius: 16, // far = 256 -> min(256, 192) * 0.5 = 96
            feet_y: 4.0,
            ..Default::default()
        });
        assert_eq!(a.fog, [0.2, 0.03, 0.03]);
        assert_eq!(a.sky, a.fog);
        assert_eq!(a.sunrise, None);
        assert_eq!(a.density, None);
        assert_eq!(a.terrain_range, [256.0 * 0.05, 96.0]);
        assert_eq!(a.sky_range, [0.0, 96.0]);

        let short = sample(Input {
            dimension: -1,
            radius: 8, // far = 128 -> min(128, 192) * 0.5 = 64
            ..Default::default()
        });
        assert_eq!(short.terrain_range, [128.0 * 0.05, 64.0]);
        assert_eq!(short.sky_range, [0.0, 64.0]);

        let lava = sample(Input {
            dimension: -1,
            medium: Medium::Lava,
            ..Default::default()
        });
        assert_eq!(lava.fog, [0.6, 0.1, 0.0]);
        assert_eq!(lava.density, Some(2.0));

        let blind = sample(Input {
            dimension: -1,
            blindness: Some(40),
            ..Default::default()
        });
        assert_eq!(blind.fog, [0.0, 0.0, 0.0]);
        assert_eq!(blind.terrain_range, [1.25, 5.0]);
        assert_eq!(blind.sky_range, [0.0, 4.0]);

        let nv = sample(Input {
            dimension: -1,
            night_vision: Some(400),
            ..Default::default()
        });
        assert!((nv.fog[0] - 1.0).abs() < 1e-6);
        assert!((nv.fog[1] - 0.15).abs() < 1e-6);
        assert!((nv.fog[2] - 0.15).abs() < 1e-6);

        let mut nether_br = FogBrightness::default();
        let mut overworld_br = FogBrightness::default();
        nether_br.tick_dimension(0, 8, -1);
        overworld_br.tick_dimension(0, 8, 0);
        assert!(nether_br.sample(1.0) > overworld_br.sample(1.0));
    }
    #[test]
    fn end_dark_violet_fog_black_sky_and_no_void_fog_preserve_medium_and_effects() {
        let a = sample(Input {
            dimension: 1,
            radius: 12, // far = 192
            feet_y: 2.0,
            ..Default::default()
        });
        assert!((a.fog[0] - 0.09411765).abs() < 1e-7);
        assert!((a.fog[1] - 0.07529412).abs() < 1e-7);
        assert!((a.fog[2] - 0.09411765).abs() < 1e-7);
        assert_eq!(a.sky, [0.0, 0.0, 0.0]);
        assert_eq!(a.sunrise, None);
        assert_eq!(a.density, None);
        assert_eq!(a.terrain_range, [192.0 * 0.75, 192.0]);
        assert_eq!(a.sky_range, [0.0, 192.0]);

        let water = sample(Input {
            dimension: 1,
            medium: Medium::Water,
            water_breathing: true,
            ..Default::default()
        });
        assert_eq!(water.density, Some(0.01));
        assert!(water.fog[2] > a.fog[2]);

        let anaglyph_end = sample(Input {
            dimension: 1,
            anaglyph: true,
            ..Default::default()
        });
        assert_eq!(anaglyph_end.sky, [0.0, 0.0, 0.0]);
        assert_ne!(anaglyph_end.fog[0], anaglyph_end.fog[1]);
    }
}
