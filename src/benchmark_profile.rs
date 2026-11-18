//! Validated, benchmark-only slow-record admission; no timing or rendering work.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SlowFrameMs(f64);
impl Default for SlowFrameMs { fn default() -> Self { Self(8.) } }
impl SlowFrameMs {
    #[inline]
    pub(crate) fn get(self) -> f64 { self.0 }
    pub(crate) fn changes_default(self) -> bool { self != Self::default() }
}
impl std::str::FromStr for SlowFrameMs {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let milliseconds: f64 = value.parse().map_err(|_| "slow-frame threshold must be a number in 0.1..=1000 milliseconds".to_owned())?;
        if !milliseconds.is_finite() || !(0.1..=1000.).contains(&milliseconds) {
            return Err("slow-frame threshold must be finite and in 0.1..=1000 milliseconds".to_owned());
        }
        Ok(Self(milliseconds))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    #[test]
    fn translucent_draw_trial_keeps_direct_default_and_existing_arena_policy() {
        use crate::renderer::translucent_mdi::Mode;
        let ordinary = crate::Options::try_parse_from(["ferrum"]).unwrap();
        assert!(ordinary.benchmark_transparency_draw.is_none());
        assert_eq!(ordinary.benchmark_transparency_draw.unwrap_or_default(), Mode::Direct);
        for prefix in [vec!["ferrum"], vec!["ferrum", "--benchmark"]] {
            let mut args = prefix; args.extend(["--benchmark-transparency-draw", "mdi"]);
            assert!(crate::Options::try_parse_from(args).is_err());
        }
        let base = ["ferrum", "--benchmark", "--benchmark-streaming", "--benchmark-transparency-draw"];
        for (name, mode) in [("direct", Mode::Direct), ("mdi", Mode::Mdi)] {
            let mut args = base.to_vec(); args.push(name);
            let options = crate::Options::try_parse_from(&args).unwrap();
            assert_eq!(options.benchmark_transparency_draw, Some(mode));
            assert!(options.benchmark_near_arena.is_none());
            args.extend(["--benchmark-near-arena", "original"]);
            assert!(crate::Options::try_parse_from(args).is_ok());
        }
        for extra in [vec!["--benchmark-transparency-upload", "queue"],
            vec!["--benchmark-gpu-timestamps"], vec!["--benchmark-capture-uploads"],
            vec!["--benchmark-capture-arena-uploads"], vec!["--benchmark-capture-near-opaque"],
            vec!["--benchmark-near-opaque-runs"], vec!["--benchmark-capture-transparency"]] {
            let mut args = base.to_vec(); args.push("mdi"); args.extend(extra);
            assert!(crate::Options::try_parse_from(args).is_err());
        }
    }
    #[test]
    fn transparent_upload_trial_is_streaming_only_and_keeps_default_queue() {
        use crate::renderer::transparent_staging::Mode;
        let ordinary = crate::Options::try_parse_from(["ferrum"]).unwrap();
        assert!(ordinary.benchmark_transparency_upload.is_none());
        assert_eq!(ordinary.benchmark_transparency_upload.unwrap_or_default(), Mode::Queue);
        assert!(crate::Options::try_parse_from(["ferrum", "--benchmark-transparency-upload", "staged"]).is_err());
        assert!(crate::Options::try_parse_from(["ferrum", "--benchmark", "--benchmark-transparency-upload", "staged"]).is_err());
        let base = ["ferrum", "--benchmark", "--benchmark-streaming", "--benchmark-transparency-upload"];
        for (name, mode) in [("queue", Mode::Queue), ("staged", Mode::Staged)] {
            let mut args = base.to_vec(); args.push(name);
            let options = crate::Options::try_parse_from(&args).unwrap();
            assert_eq!(options.benchmark_transparency_upload, Some(mode));
            assert!(options.benchmark_near_arena.is_none());
            args.extend(["--benchmark-near-arena", "original"]);
            assert!(crate::Options::try_parse_from(args).is_ok());
        }
        for flag in ["--benchmark-gpu-timestamps", "--benchmark-capture-uploads",
            "--benchmark-capture-arena-uploads", "--benchmark-capture-near-opaque",
            "--benchmark-near-opaque-runs", "--benchmark-capture-transparency"] {
            let mut args = base.to_vec(); args.extend(["staged", flag]);
            assert!(crate::Options::try_parse_from(args).is_err(), "{flag}");
        }
    }
    #[test]
    fn gpu_timestamp_cli_is_explicit_streaming_only_without_forcing_arena() {
        assert!(!crate::Options::try_parse_from(["ferrum"]).unwrap().benchmark_gpu_timestamps);
        assert!(crate::Options::try_parse_from(["ferrum", "--benchmark-gpu-timestamps"]).is_err());
        assert!(crate::Options::try_parse_from(["ferrum", "--benchmark", "--benchmark-gpu-timestamps"]).is_err());
        let arguments = ["ferrum", "--benchmark", "--benchmark-streaming", "--benchmark-gpu-timestamps"];
        let automatic = crate::Options::try_parse_from(arguments).unwrap();
        assert!(automatic.benchmark_gpu_timestamps);
        assert!(automatic.benchmark_near_arena.is_none());
        for mode in ["original", "mdi"] {
            let mut args = arguments.to_vec();
            args.extend(["--benchmark-near-arena", mode]);
            assert!(crate::Options::try_parse_from(args).is_ok());
        }
        for incompatible in ["--benchmark-capture-uploads", "--benchmark-capture-arena-uploads",
            "--benchmark-capture-near-opaque", "--benchmark-near-opaque-runs", "--benchmark-capture-transparency"] {
            let mut args = arguments.to_vec();
            args.push(incompatible);
            assert!(crate::Options::try_parse_from(args).is_err(), "{incompatible}");
        }
    }
    #[test]
    fn slow_frame_cli_is_opt_in_benchmark_only_and_finite_bounded() {
        let ordinary = crate::Options::try_parse_from(["ferrum"]).unwrap();
        assert!(ordinary.benchmark_slow_frame_ms.is_none());
        assert_eq!(ordinary.benchmark_slow_frame_ms.unwrap_or_default().get(), 8.);
        assert!(crate::Options::try_parse_from(["ferrum", "--benchmark-slow-frame-ms", "4"]).is_err());
        for (value, expected) in [("0.1", 0.1), ("4", 4.), ("8", 8.), ("1e3", 1000.)] {
            let options = crate::Options::try_parse_from(["ferrum", "--benchmark", "--benchmark-slow-frame-ms", value]).unwrap();
            let threshold = options.benchmark_slow_frame_ms.unwrap();
            assert_eq!(threshold.get(), expected);
            assert_eq!(threshold.changes_default(), expected != 8.);
        }
        for value in ["", "no", "NaN", "inf", "+inf", "-inf", "0", "-1", "0.09999", "1000.0001"] {
            assert!(value.parse::<SlowFrameMs>().is_err(), "invalid scalar {value:?}");
            assert!(crate::Options::try_parse_from(["ferrum", "--benchmark", "--benchmark-slow-frame-ms", value]).is_err(),
                "invalid CLI threshold {value:?}");
        }
        assert!(crate::Options::try_parse_from(["ferrum", "--benchmark", "--benchmark-slow-frame-ms"]).is_err());
    }
}
