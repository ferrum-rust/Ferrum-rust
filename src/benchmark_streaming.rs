//! Opt-in streaming benchmark path; ordinary gameplay never calls this module.

pub fn positive_seconds(value: &str) -> Result<f32, String> {
    let seconds = value.parse::<f32>().map_err(|_| "expected seconds".to_owned())?;
    if seconds.is_finite() && seconds > 0.0 {
        Ok(seconds)
    } else {
        Err("seconds must be positive and finite".to_owned())
    }
}

/// Begin at the first measured present's actual player pose. Continue at 12
/// blocks/s along +X, turn 180 degrees at `turn_after`, return along -X, then
/// hold the measured origin. The turnaround frame remains in the usual sample.
/// Warmup and the default forward route keep their existing camera math.
pub fn out_and_back(origin: [f64; 3], elapsed: f64, turn_after: f64) -> ([f64; 3], f32) {
    debug_assert!(elapsed.is_finite() && elapsed >= 0.0);
    debug_assert!(turn_after.is_finite() && turn_after > 0.0);
    let outbound = elapsed < turn_after;
    let distance_seconds = if outbound {
        elapsed
    } else {
        turn_after - (elapsed - turn_after).min(turn_after)
    };
    ([origin[0] + distance_seconds * 12.0, origin[1], origin[2]],
        if outbound { -90.0 } else { 90.0 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn route_is_continuous_constant_speed_and_returns_to_its_actual_anchor() {
        let origin = [-25.25, 124.5, 7.75];
        let turn = 60.0;
        for (time, x, yaw) in [
            (0.0, -25.25, -90.0), (0.25, -22.25, -90.0),
            (30.0, 334.75, -90.0), (59.75, 691.75, -90.0),
            (60.0, 694.75, 90.0), (60.25, 691.75, 90.0),
            (90.0, 334.75, 90.0), (119.75, -22.25, 90.0),
            (120.0, -25.25, 90.0), (180.0, -25.25, 90.0),
        ] {
            assert_eq!(out_and_back(origin, time, turn), ([x, origin[1], origin[2]], yaw));
        }
        // Equal distances on both sides of the turn, with no jump in position.
        for offset in [0.001, 0.25, 1.0, 16.0, 60.0] {
            let before = out_and_back(origin, turn - offset, turn).0;
            let after = out_and_back(origin, turn + offset, turn).0;
            assert!((before[0] - after[0]).abs() < 1e-10);
        }
    }

    #[test]
    fn return_option_is_positive_finite_and_requires_streaming_benchmark() {
        for value in ["0", "-1", "NaN", "inf", "-inf", "1e40", "bad"] {
            assert!(positive_seconds(value).is_err(), "{value}");
        }
        assert_eq!(positive_seconds("60"), Ok(60.0));
        assert!(crate::Options::try_parse_from([
            "ferrum", "--benchmark-streaming-return-after", "60",
        ]).is_err());
        assert!(crate::Options::try_parse_from([
            "ferrum", "--benchmark", "--benchmark-streaming-return-after", "60",
        ]).is_err());
        let options = crate::Options::try_parse_from([
            "ferrum", "--benchmark", "--benchmark-streaming",
            "--benchmark-streaming-return-after", "60", "--seconds", "120",
        ]).unwrap();
        assert_eq!(options.benchmark_streaming_return_after, Some(60.0));
        let default = crate::Options::try_parse_from([
            "ferrum", "--benchmark", "--benchmark-streaming",
        ]).unwrap();
        assert_eq!(default.benchmark_streaming_return_after, None);
    }
}
