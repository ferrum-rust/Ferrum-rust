//! 1.8.9 noise operations, preserving Java evaluation order and the Improved
//! generator's Y-cell gradient cache. Standard noise crates do not match it.
use super::random::JavaRandom;

fn permutation(random: &mut JavaRandom) -> ([usize; 512], [f64; 3]) {
    let offset = [
        random.next_double() * 256.0,
        random.next_double() * 256.0,
        random.next_double() * 256.0,
    ];
    let mut p = [0; 512];
    for (i, v) in p[..256].iter_mut().enumerate() {
        *v = i;
    }
    for i in 0..256 {
        let index = random.next_int((256 - i) as i32) as usize + i;
        p.swap(i, index);
        p[i + 256] = p[i];
    }
    (p, offset)
}
fn floor_int(value: f64) -> i32 {
    let n = value as i32;
    if value < (n as f64) {
        n.wrapping_sub(1)
    } else {
        n
    }
}
fn floor_long(value: f64) -> i64 {
    let n = value as i64;
    if value < (n as f64) {
        n.wrapping_sub(1)
    } else {
        n
    }
}
fn fade(x: f64) -> f64 {
    x * x * x * (x * (x * 6.0 - 15.0) + 10.0)
}
fn lerp(t: f64, a: f64, b: f64) -> f64 {
    a + t * (b - a)
}
const GX: [f64; 16] = [
    1., -1., 1., -1., 1., -1., 1., -1., 0., 0., 0., 0., 1., 0., -1., 0.,
];
const GY: [f64; 16] = [
    1., 1., -1., -1., 0., 0., 0., 0., 1., -1., 1., -1., 1., -1., 1., -1.,
];
const GZ: [f64; 16] = [
    0., 0., 0., 0., 1., 1., -1., -1., 1., 1., -1., -1., 0., 1., 0., -1.,
];
fn grad(hash: usize, x: f64, y: f64, z: f64) -> f64 {
    let i = hash & 15;
    GX[i] * x + GY[i] * y + GZ[i] * z
}
fn grad2(hash: usize, x: f64, z: f64) -> f64 {
    let i = hash & 15;
    GX[i] * x + GZ[i] * z
}

#[derive(Clone)]
pub struct Improved {
    permutation: [usize; 512],
    pub offset: [f64; 3],
}
impl Improved {
    pub fn new(random: &mut JavaRandom) -> Self {
        let (permutation, offset) = permutation(random);
        Self {
            permutation,
            offset,
        }
    }
    /// Output order is x, then z, then y (y is contiguous). Adds to output.
    pub fn add(
        &self,
        output: &mut [f64],
        origin: [f64; 3],
        size: [usize; 3],
        scale: [f64; 3],
        divisor: f64,
    ) {
        assert!(output.len() >= size[0] * size[1] * size[2]);
        let p = &self.permutation;
        let inverse = 1.0 / divisor;
        let mut index = 0;
        if size[1] == 1 {
            for x in 0..size[0] {
                let mut dx = origin[0] + x as f64 * scale[0] + self.offset[0];
                let ix = floor_int(dx);
                let px = (ix & 255) as usize;
                dx -= ix as f64;
                let fx = fade(dx);
                for z in 0..size[2] {
                    let mut dz = origin[2] + z as f64 * scale[2] + self.offset[2];
                    let iz = floor_int(dz);
                    let pz = (iz & 255) as usize;
                    dz -= iz as f64;
                    let fz = fade(dz);
                    let a = p[p[px]] + pz;
                    let b = p[p[px + 1]] + pz;
                    let low = lerp(fx, grad2(p[a], dx, dz), grad(p[b], dx - 1.0, 0.0, dz));
                    let high = lerp(
                        fx,
                        grad(p[a + 1], dx, 0.0, dz - 1.0),
                        grad(p[b + 1], dx - 1.0, 0.0, dz - 1.0),
                    );
                    output[index] += lerp(fz, low, high) * inverse;
                    index += 1;
                }
            }
            return;
        }
        let mut previous_y = -1;
        let (mut g00, mut g10, mut g01, mut g11) = (0.0, 0.0, 0.0, 0.0);
        for x in 0..size[0] {
            let mut dx = origin[0] + x as f64 * scale[0] + self.offset[0];
            let ix = floor_int(dx);
            let px = (ix & 255) as usize;
            dx -= ix as f64;
            let fx = fade(dx);
            for z in 0..size[2] {
                let mut dz = origin[2] + z as f64 * scale[2] + self.offset[2];
                let iz = floor_int(dz);
                let pz = (iz & 255) as usize;
                dz -= iz as f64;
                let fz = fade(dz);
                for y in 0..size[1] {
                    let mut dy = origin[1] + y as f64 * scale[1] + self.offset[1];
                    let iy = floor_int(dy);
                    let py = iy & 255;
                    dy -= iy as f64;
                    let fy = fade(dy);
                    if y == 0 || py != previous_y {
                        previous_y = py;
                        let a = p[px] + py as usize;
                        let aa = p[a] + pz;
                        let ab = p[a + 1] + pz;
                        let b = p[px + 1] + py as usize;
                        let ba = p[b] + pz;
                        let bb = p[b + 1] + pz;
                        g00 = lerp(fx, grad(p[aa], dx, dy, dz), grad(p[ba], dx - 1.0, dy, dz));
                        g10 = lerp(
                            fx,
                            grad(p[ab], dx, dy - 1.0, dz),
                            grad(p[bb], dx - 1.0, dy - 1.0, dz),
                        );
                        g01 = lerp(
                            fx,
                            grad(p[aa + 1], dx, dy, dz - 1.0),
                            grad(p[ba + 1], dx - 1.0, dy, dz - 1.0),
                        );
                        g11 = lerp(
                            fx,
                            grad(p[ab + 1], dx, dy - 1.0, dz - 1.0),
                            grad(p[bb + 1], dx - 1.0, dy - 1.0, dz - 1.0),
                        );
                    }
                    let low = lerp(fy, g00, g10);
                    let high = lerp(fy, g01, g11);
                    output[index] += lerp(fz, low, high) * inverse;
                    index += 1;
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct Octaves {
    generators: Vec<Improved>,
}
impl Octaves {
    pub fn new(random: &mut JavaRandom, count: usize) -> Self {
        Self {
            generators: (0..count).map(|_| Improved::new(random)).collect(),
        }
    }
    pub fn generate(&self, origin: [i32; 3], size: [usize; 3], scale: [f64; 3]) -> Vec<f64> {
        let mut out = vec![0.0; size[0] * size[1] * size[2]];
        self.fill(&mut out, origin, size, scale);
        out
    }
    pub fn fill(&self, out: &mut [f64], origin: [i32; 3], size: [usize; 3], scale: [f64; 3]) {
        out.fill(0.0);
        let mut factor = 1.0;
        for generator in &self.generators {
            let mut x = origin[0] as f64 * factor * scale[0];
            let y = origin[1] as f64 * factor * scale[1];
            let mut z = origin[2] as f64 * factor * scale[2];
            let ix = floor_long(x);
            let iz = floor_long(z);
            x -= ix as f64;
            z -= iz as f64;
            x += (ix % 16777216) as f64;
            z += (iz % 16777216) as f64;
            generator.add(
                out,
                [x, y, z],
                size,
                [scale[0] * factor, scale[1] * factor, scale[2] * factor],
                factor,
            );
            factor /= 2.0;
        }
    }
    /// Vanilla's 2D overload ignores its final argument and fixes Y=10/scaleY=1.
    pub fn generate_2d(
        &self,
        x: i32,
        z: i32,
        width: usize,
        depth: usize,
        scale_x: f64,
        scale_z: f64,
    ) -> Vec<f64> {
        self.generate([x, 10, z], [width, 1, depth], [scale_x, 1.0, scale_z])
    }
}

const SIMPLEX_GRAD: [[i32; 2]; 12] = [
    [1, 1],
    [-1, 1],
    [1, -1],
    [-1, -1],
    [1, 0],
    [-1, 0],
    [1, 0],
    [-1, 0],
    [0, 1],
    [0, -1],
    [0, 1],
    [0, -1],
];
fn simplex_floor(v: f64) -> i32 {
    if v > 0.0 {
        v as i32
    } else {
        (v as i32).wrapping_sub(1)
    }
}
fn simplex_corner(hash: usize, x: f64, y: f64) -> f64 {
    let mut attenuation = 0.5 - x * x - y * y;
    if attenuation < 0.0 {
        0.0
    } else {
        attenuation *= attenuation;
        let grad = SIMPLEX_GRAD[hash];
        attenuation * attenuation * (grad[0] as f64 * x + grad[1] as f64 * y)
    }
}
#[derive(Clone)]
pub struct Simplex {
    permutation: [usize; 512],
    pub offset: [f64; 3],
}
impl Simplex {
    pub fn new(random: &mut JavaRandom) -> Self {
        let (permutation, offset) = permutation(random);
        Self {
            permutation,
            offset,
        }
    }
    /// Original point sampler does not add the random offsets. Region sampling does.
    pub fn sample(&self, x: f64, y: f64) -> f64 {
        let sqrt3 = 3.0f64.sqrt();
        let skew = 0.5 * (sqrt3 - 1.0);
        let unskew = (3.0 - sqrt3) / 6.0;
        let s = (x + y) * skew;
        let ix = simplex_floor(x + s);
        let iy = simplex_floor(y + s);
        let t = ix.wrapping_add(iy) as f64 * unskew;
        let dx = x - (ix as f64 - t);
        let dy = y - (iy as f64 - t);
        let (a, b) = if dx > dy { (1, 0) } else { (0, 1) };
        let mx = dx - a as f64 + unskew;
        let my = dy - b as f64 + unskew;
        let hx = dx - 1.0 + 2.0 * unskew;
        let hy = dy - 1.0 + 2.0 * unskew;
        let px = (ix & 255) as usize;
        let py = (iy & 255) as usize;
        let p = &self.permutation;
        let n0 = simplex_corner(p[px + p[py]] % 12, dx, dy);
        let n1 = simplex_corner(p[px + a + p[py + b]] % 12, mx, my);
        let n2 = simplex_corner(p[px + 1 + p[py + 1]] % 12, hx, hy);
        70.0 * (n0 + n1 + n2)
    }
    pub fn add(
        &self,
        out: &mut [f64],
        origin: [f64; 2],
        size: [usize; 2],
        scale: [f64; 2],
        amplitude: f64,
    ) {
        assert!(out.len() >= size[0] * size[1]);
        let mut index = 0;
        for y in 0..size[1] {
            let dy = (origin[1] + y as f64) * scale[1] + self.offset[1];
            for x in 0..size[0] {
                let dx = (origin[0] + x as f64) * scale[0] + self.offset[0];
                out[index] += self.sample(dx, dy) * amplitude;
                index += 1;
            }
        }
    }
}

#[derive(Clone)]
pub struct Perlin {
    generators: Vec<Simplex>,
}
impl Perlin {
    pub fn new(random: &mut JavaRandom, count: usize) -> Self {
        Self {
            generators: (0..count).map(|_| Simplex::new(random)).collect(),
        }
    }
    pub fn sample(&self, x: f64, y: f64) -> f64 {
        let mut result = 0.0;
        let mut factor = 1.0;
        for generator in &self.generators {
            result += generator.sample(x * factor, y * factor) / factor;
            factor /= 2.0;
        }
        result
    }
    pub fn generate(
        &self,
        origin: [f64; 2],
        size: [usize; 2],
        scale: [f64; 2],
        lacunarity: f64,
        persistence: f64,
    ) -> Vec<f64> {
        let mut out = vec![0.0; size[0] * size[1]];
        self.fill(&mut out, origin, size, scale, lacunarity, persistence);
        out
    }
    pub fn fill(
        &self,
        out: &mut [f64],
        origin: [f64; 2],
        size: [usize; 2],
        scale: [f64; 2],
        lacunarity: f64,
        persistence: f64,
    ) {
        out.fill(0.0);
        let mut amplitude = 1.0;
        let mut frequency = 1.0;
        for generator in &self.generators {
            generator.add(
                out,
                origin,
                size,
                [
                    scale[0] * frequency * amplitude,
                    scale[1] * frequency * amplitude,
                ],
                0.55 / amplitude,
            );
            frequency *= lacunarity;
            amplitude *= persistence;
        }
    }
}
