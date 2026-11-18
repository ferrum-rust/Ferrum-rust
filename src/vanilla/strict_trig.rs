//! Java 8 StrictMath for angles produced by 2π × Random.nextDouble().
//! Range reduction and polynomials adapted from netlib.org/fdlibm
//! (e_rem_pio2.c, k_sin.c, k_cos.c).
// Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
// Developed at SunSoft, a Sun Microsystems, Inc. business.
// Permission to use, copy, modify, and distribute this software is freely
// granted, provided that this notice is preserved.

const HALF_PI_SPLIT: [(f64, f64); 3] = [
    (f64::from_bits(0x3ff921fb54400000), f64::from_bits(0x3dd0b4611a626331)),
    (f64::from_bits(0x3dd0b4611a600000), f64::from_bits(0x3ba3198a2e037073)),
    (f64::from_bits(0x3ba3198a2e000000), f64::from_bits(0x397b839a252049c1)),
];

fn high_word(x: f64) -> u32 {
    (x.abs().to_bits() >> 32) as u32
}

fn reduce(angle: f64) -> (u32, f64, f64) {
    let high = high_word(angle);
    if high <= 0x3fe921fb {
        return (0, angle, 0.);
    }
    let n = (angle * std::f64::consts::FRAC_2_PI + 0.5) as u32;
    let factor = f64::from(n);
    let (head, tail) = HALF_PI_SPLIT[0];
    let mut remainder = angle - factor * head;
    let mut correction = factor * tail;
    let mut x = remainder - correction;
    // Near a quadrant boundary, cancellation exposes the next pieces of π/2.
    // The original n=1 fast path uses the second split once next to π/2.
    for ((head, tail), threshold) in HALF_PI_SPLIT[1..].iter().zip([16, 49]) {
        let refine = if high < 0x4002d97c {
            high == 0x3ff921fb && threshold == 16
        } else {
            (high >> 20) as i32 - (high_word(x) >> 20) as i32 > threshold
        };
        if !refine {
            break;
        }
        let previous = remainder;
        let step = factor * head;
        remainder = previous - step;
        correction = factor * tail - ((previous - remainder) - step);
        x = remainder - correction;
    }
    (n, x, (remainder - x) - correction)
}

fn kernel(x: f64, tail: f64, reduced: bool) -> (f64, f64) {
    let high = high_word(x);
    if high < 0x3e400000 {
        return (1., x);
    }
    let z = x * x;
    let cube = z * x;
    let s1 = f64::from_bits(0xbfc5555555555549);
    let sin_poly = f64::from_bits(0x3f8111111110f8a6)
        + z * (f64::from_bits(0xbf2a01a019c161d5)
        + z * (f64::from_bits(0x3ec71de357b1fe7d)
        + z * (f64::from_bits(0xbe5ae5e68a2b9ceb)
        + z * f64::from_bits(0x3de5d93a5acfd57c))));
    let sin = if reduced {
        x - ((z * (0.5 * tail - cube * sin_poly) - tail) - cube * s1)
    } else {
        x + cube * (s1 + z * sin_poly)
    };
    let cos_poly = z * (f64::from_bits(0x3fa555555555554c)
        + z * (f64::from_bits(0xbf56c16c16c15177)
        + z * (f64::from_bits(0x3efa01a019cb1590)
        + z * (f64::from_bits(0xbe927e4f809c52ad)
        + z * (f64::from_bits(0x3e21ee9ebdb4b1c4)
        + z * f64::from_bits(0xbda8fae9be8838d4))))));
    let quarter = match high {
        0..0x3fd33333 => 0.,
        0x3fd33333..=0x3fe90000 => f64::from_bits(u64::from(high - 0x00200000) << 32),
        _ => 0.28125,
    };
    let cos = (1. - quarter) - ((0.5 * z - quarter) - (z * cos_poly - x * tail));
    (cos, sin)
}

/// Cartesian direction for one nonnegative turn, returned as (cosine, sine).
pub fn unit_circle(angle: f64) -> (f64, f64) {
    debug_assert!((0.0..=std::f64::consts::TAU).contains(&angle));
    let (n, x, tail) = reduce(angle);
    let (cos, sin) = kernel(x, tail, n != 0);
    match n % 4 {
        0 => (cos, sin),
        1 => (-sin, cos),
        2 => (-cos, -sin),
        _ => (sin, -cos),
    }
}
