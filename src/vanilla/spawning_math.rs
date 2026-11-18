//! StrictMath.log compatibility used by java.util.Random.nextGaussian.
// Adapted from https://netlib.org/fdlibm/e_log.c (same algorithm as JDK 8).
// Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
// Developed at SunSoft, a Sun Microsystems, Inc. business.
// Permission to use, copy, modify, and distribute this software is freely
// granted, provided that this notice is preserved.
pub fn log(mut x: f64) -> f64 {
    const HI: f64 = f64::from_bits(0x3fe62e42fee00000);
    const LO: f64 = f64::from_bits(0x3dea39ef35793c76);
    const L: [f64; 7] = [
        f64::from_bits(0x3fe5555555555593),
        f64::from_bits(0x3fd999999997fa04),
        f64::from_bits(0x3fd2492494229359),
        f64::from_bits(0x3fcc71c51d8e78af),
        f64::from_bits(0x3fc7466496cb03de),
        f64::from_bits(0x3fc39a09d078c69f),
        f64::from_bits(0x3fc2f112df3e5244),
    ];
    let mut h = (x.to_bits() >> 32) as i32;
    let low = x.to_bits() as u32;
    let mut exponent = 0;
    if h < 0x00100000 {
        if ((h as u32 & 0x7fffffff) | low) == 0 {
            return f64::NEG_INFINITY;
        }
        if h < 0 {
            return f64::NAN;
        }
        exponent -= 54;
        x *= f64::from_bits(0x4350000000000000);
        h = (x.to_bits() >> 32) as i32;
    }
    if h >= 0x7ff00000 {
        return x + x;
    }
    exponent += (h >> 20) - 1023;
    h &= 0x000fffff;
    let adjust = (h + 0x95f64) & 0x100000;
    x = f64::from_bits(((h | (adjust ^ 0x3ff00000)) as u64) << 32 | (x.to_bits() & 0xffffffff));
    exponent += adjust >> 20;
    let f = x - 1.;
    let k = exponent as f64;
    if (0x000fffff & (2 + h)) < 3 {
        if f == 0. {
            return if exponent == 0 { 0. } else { k * HI + k * LO };
        }
        let r = f * f * (0.5 - 0.3333333333333333 * f);
        return if exponent == 0 {
            f - r
        } else {
            k * HI - ((r - k * LO) - f)
        };
    }
    let s = f / (2. + f);
    let z = s * s;
    let w = z * z;
    let t1 = w * (L[1] + w * (L[3] + w * L[5]));
    let t2 = z * (L[0] + w * (L[2] + w * (L[4] + w * L[6])));
    let r = t2 + t1;
    if ((h - 0x6147a) | (0x6b851 - h)) > 0 {
        let half_square = 0.5 * f * f;
        if exponent == 0 {
            f - (half_square - s * (half_square + r))
        } else {
            k * HI - ((half_square - (s * (half_square + r) + k * LO)) - f)
        }
    } else if exponent == 0 {
        f - s * (f - r)
    } else {
        k * HI - ((s * (f - r) - k * LO) - f)
    }
}
