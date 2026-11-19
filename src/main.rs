pub(crate) mod ambient_particles;
pub(crate) mod dimension_world;
mod assets;
mod audio;
mod banner;
mod benchmark_scene;
mod benchmark_streaming;
mod benchmark_profile;
mod block_ticks;
mod block_states;
mod celestial;
mod chest_render;
mod model_box;
mod client;
mod local_tick;
mod distant_terrain;
mod server_pack;
mod clipboard;
mod container;
mod first_person;
mod entity_visual;
mod gameplay;
mod horse_model;
mod inventory;
mod lan;
mod keybindings;
mod lighting_view;
mod mesh;
mod models;
mod ms_auth;
mod resource_dump;
mod native_session;
mod populated_world;
mod nbt;
mod network;
mod net_presentation;
mod player;
mod player_save;
mod player_skins;
mod protocol;
mod remote;
mod remote_mobs;
mod renderer;
mod resources;
mod resource_packs;
mod seed;
mod settings;
mod sign_render;
mod skull;
mod ui;
mod visual_light;
#[cfg(test)]
mod validation;
#[cfg(test)]
mod selection_validation;
#[cfg(test)]
mod surface_wait_trace_cli_tests;
#[cfg(test)]
mod pre_acquire_visibility_cli_tests;
#[cfg(test)]
mod packed_frustum_cli_tests;
#[cfg(test)]
mod gpu_stages_cli_tests;
#[cfg(test)]
mod covered_lod_cli_tests;
mod vanilla;
mod villagers;
mod world;
mod world_library;
mod world_types;
mod worldgen;
mod worker_priority;

use anyhow::{Context, Result};
use clap::{CommandFactory, FromArgMatches, Parser};
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[command(
    version,
    about = "Ferrum 1.8.9 - experimental native Vulkan single-player client"
)]
pub struct Options {
    #[arg(skip)]
    pub world_rules: world_library::Rules,
    #[arg(skip)]
    project_dir: PathBuf,
    /// Start directly in a world instead of the main menu.
    #[arg(long)]
    pub play: bool,
    #[arg(long)]
    pub creative: bool,
    #[arg(long)]
    pub save_id: Option<String>,
    /// Render a specific UI screen for visual verification.
    #[arg(long, default_value = "game")]
    pub screen: String,
    #[arg(long)]
    pub language: Option<String>,
    /// Override entity occlusion culling for reproducible comparisons.
    #[arg(long)]
    pub entity_culling: Option<bool>,
    /// Read textures from your local Minecraft 1.8.9 jar.
    #[arg(long)]
    pub jar: Option<PathBuf>,
    /// Open an existing legacy Anvil world (original files are read-only).
    #[arg(long)]
    pub world: Option<PathBuf>,
    /// Visible chunk radius, 2..=128; beyond 24 uses distant visual terrain LOD.
    #[arg(long, default_value_t = 8, value_parser = clap::value_parser!(u32).range(2..=128))]
    pub render_distance: u32,
    /// Wait for the requested distant terrain before starting the FPS sample.
    #[arg(long, requires = "benchmark")]
    pub benchmark_distant_ready: bool,
    #[arg(long, default_value = "189", value_parser = seed::parse_cli, allow_hyphen_values=true)]
    pub seed: u64,
    /// 4: populated native DEFAULT; 3: terrain preview; 1/2: legacy Ferrum generators.
    #[arg(long, default_value_t = 4, value_parser = clap::value_parser!(u8).range(1..=4))]
    pub generator: u8,
    /// Begin near a legacy Ferrum village (requires --generator 2).
    #[arg(long)]
    pub village: bool,
    /// Render an isolated native structure, outside the playable save pipeline.
    #[arg(long, value_parser = ["village", "mineshaft"], requires = "screenshot", conflicts_with_all = ["world", "connect", "village", "benchmark"])]
    pub structure_preview: Option<String>,
    /// Query a Minecraft server's protocol/status and exit.
    #[arg(long)]
    pub server_status: Option<String>,
    /// Experimental protocol-47 session with a local/offline development server.
    #[arg(long, conflicts_with_all = ["world", "benchmark", "check"])]
    pub connect: Option<String>,
    #[arg(long, default_value = "FerrumPlayer")]
    pub username: String,
    /// Limit presentation to refresh rate. Uncapped by default.
    #[arg(long)]
    pub vsync: bool,
    /// Run a reproducible camera path and write reports/benchmark.json.
    #[arg(long)]
    pub benchmark: bool,
    /// Benchmark-only CPU priority of existing workers; default baseline.
    #[arg(long, requires = "benchmark", value_enum)]
    pub(crate) benchmark_worker_priority: Option<worker_priority::Mode>,
    /// Measure a 12-block/s flight with native chunk generation and streaming enabled.
    #[arg(long, requires = "benchmark", conflicts_with_all = ["benchmark_scene", "world", "connect", "check", "screenshot"])]
    pub benchmark_streaming: bool,
    /// Explicit CPU-overlap trial: prepare camera/section visibility before surface acquisition.
    #[arg(long, requires = "benchmark_streaming")]
    pub benchmark_pre_acquire_visibility: bool,
    /// Explicit exact packed-plane Frustum trial; ordinary mode remains scalar after acquisition.
    #[arg(long, requires = "benchmark_streaming", conflicts_with = "benchmark_pre_acquire_visibility")]
    pub benchmark_packed_frustum: bool,
    /// Experimental omission of provably covered distant tiles; ordinary default stays off.
    #[arg(long, requires = "benchmark_streaming", conflicts_with_all = ["benchmark_packed_frustum", "benchmark_pre_acquire_visibility"])]
    pub benchmark_cull_covered_lod: bool,
    /// Capture bounded late-flight transparency evidence; timings are diagnostic.
    #[arg(long, requires = "benchmark_streaming")]
    pub benchmark_capture_transparency: bool,
    /// Shadow exact opaque draw-run reuse on every measured frame; diagnostic timings only.
    #[arg(long, requires = "benchmark_streaming")]
    pub benchmark_capture_near_opaque: bool,
    /// Capture bounded per-upload CPU attribution; diagnostic timings, not baseline.
    #[arg(long, requires = "benchmark_streaming")]
    pub benchmark_capture_uploads: bool,
    /// Attribute actual arena upload calls without changing automatic backing; diagnostic timing only.
    #[arg(long,requires="benchmark_streaming",conflicts_with_all=["benchmark_capture_uploads","benchmark_capture_near_opaque","benchmark_near_opaque_runs","benchmark_capture_transparency"])]
    pub benchmark_capture_arena_uploads:bool,
    /// Append GPU encoder timestamps during a streaming benchmark; diagnostic, not baseline.
    #[arg(long, requires = "benchmark_streaming", conflicts_with_all = ["benchmark_capture_uploads", "benchmark_capture_arena_uploads", "benchmark_capture_near_opaque", "benchmark_near_opaque_runs", "benchmark_capture_transparency"])]
    pub benchmark_gpu_timestamps: bool,
    /// Add bounded in-pass GPU stages to the explicit timestamp diagnostic; changes no draws.
    #[arg(long, requires = "benchmark_gpu_timestamps")]
    pub benchmark_gpu_stages: bool,
    /// Capture pinned public surface-wait spans; requires a separate surface-wait-trace diagnostic build.
    #[arg(long, requires = "benchmark_streaming", conflicts_with_all = ["benchmark_gpu_timestamps", "benchmark_capture_uploads", "benchmark_capture_arena_uploads", "benchmark_capture_near_opaque", "benchmark_near_opaque_runs", "benchmark_capture_transparency", "benchmark_transparency_upload", "benchmark_transparency_draw"])]
    pub benchmark_surface_waits: bool,
    /// Experimental bounded transparent-index upload path; ordinary default is queue writes.
    #[arg(long, requires = "benchmark_streaming", value_enum, conflicts_with_all = ["benchmark_gpu_timestamps", "benchmark_capture_uploads", "benchmark_capture_arena_uploads", "benchmark_capture_near_opaque", "benchmark_near_opaque_runs", "benchmark_capture_transparency"])]
    pub(crate) benchmark_transparency_upload: Option<renderer::transparent_staging::Mode>,
    /// Experimental ordered translucent MDI; ordinary/default rendering uses direct draws.
    #[arg(long, requires = "benchmark_streaming", value_enum, conflicts_with_all = ["benchmark_transparency_upload", "benchmark_gpu_timestamps", "benchmark_capture_uploads", "benchmark_capture_arena_uploads", "benchmark_capture_near_opaque", "benchmark_near_opaque_runs", "benchmark_capture_transparency"])]
    pub(crate) benchmark_transparency_draw: Option<renderer::translucent_mdi::Mode>,
    /// Experimental exact opaque run bundles; ordinary/default rendering stays unchanged.
    #[arg(long, requires = "benchmark_streaming", conflicts_with = "benchmark_capture_near_opaque")]
    pub benchmark_near_opaque_runs: bool,
    /// Near backing override: original keeps legacy caches; dedicated/direct/mdi are cache-disabled controls. Absent uses guarded automatic admission.
    #[arg(long,requires="benchmark_streaming",value_enum,conflicts_with_all=["benchmark_near_opaque_runs","benchmark_capture_uploads","benchmark_capture_near_opaque","benchmark_capture_transparency"])]
    pub(crate) benchmark_near_arena:Option<renderer::NearArenaMode>,
    /// Benchmark-only transparency scheduling override; ordinary default uses scoped-two with >=16 available logical CPUs, scoped-wide otherwise (unknown included).
    #[arg(long, requires = "benchmark", value_enum)]
    pub(crate) benchmark_transparency_sort: Option<renderer::TransparencySortMode>,
    /// Turn back after this many measured streaming seconds; hold at the origin after returning.
    #[arg(long, requires = "benchmark_streaming", value_parser = benchmark_streaming::positive_seconds)]
    pub benchmark_streaming_return_after: Option<f32>,
    /// Measure the shared fixed camera scene in an imported Anvil fixture.
    #[arg(long, requires_all = ["benchmark", "world"], conflicts_with_all = ["screenshot", "check", "seconds", "vsync", "creative", "render_distance", "entity_culling"], value_parser = benchmark_scene::Scene::load)]
    pub benchmark_scene: Option<benchmark_scene::Scene>,
    /// Use the Vulkan adapter with this name for a same-GPU scene comparison.
    #[arg(long, requires = "benchmark_scene")]
    pub benchmark_adapter: Option<String>,
    /// Directory for this fixed scene's frame intervals and warmed-up framebuffer.
    #[arg(long, requires = "benchmark")]
    pub benchmark_output_dir: Option<PathBuf>,
    /// Retain slow benchmark intervals/renders at this inclusive millisecond threshold (default8; 0.1..=1000).
    #[arg(long, requires = "benchmark", allow_hyphen_values = true, value_name = "MILLISECONDS")]
    pub(crate) benchmark_slow_frame_ms: Option<benchmark_profile::SlowFrameMs>,
    #[arg(long, default_value_t = 15.0)]
    pub seconds: f32,
    /// Render off-screen on Vulkan, save a PNG and exit.
    #[arg(long)]
    pub screenshot: Option<PathBuf>,
    /// Load/mesh the world, report statistics, then exit without a window.
    #[arg(long)]
    pub check: bool,
    #[arg(long)]
    pub list_worlds: bool,
    /// Ignore previous project-local edits for a reproducible run.
    #[arg(long)]
    pub fresh: bool,
}

impl Options {
    pub(crate) fn validate_surface_wait_build(&self) -> Result<()> {
        anyhow::ensure!(!self.benchmark_surface_waits || cfg!(feature = "surface-wait-trace"),
            "--benchmark-surface-waits requires a separate diagnostic build with --features surface-wait-trace; the ordinary installed build intentionally has this feature disabled");
        Ok(())
    }
    /// Interactive entry waits for the original spawn and nearby presentation,
    /// while the remaining view is streamed. Diagnostics retain a complete view.
    pub(crate) fn progressive_world_entry(&self) -> bool {
        !self.check && self.screenshot.is_none() && self.benchmark_scene.is_none()
            && (!self.benchmark || self.benchmark_streaming)
    }
}

fn minecraft_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default()).join(".minecraft")
}

fn default_jar_path(root: &std::path::Path) -> PathBuf {
    let candidates = [
        PathBuf::from(".local/resources/1.8.9.jar"),
        root.join(".local/resources/1.8.9.jar"),
        PathBuf::from("1.8.9.jar"),
        root.join("1.8.9.jar"),
    ];
    for candidate in candidates {
        if candidate.is_file() {
            return candidate;
        }
    }
    minecraft_dir().join("versions/1.8.9/1.8.9.jar")
}

fn absolutize_cli_path(cwd: &std::path::Path, slot: &mut Option<PathBuf>) {
    if let Some(path) = slot.as_mut() {
        if path.is_relative() {
            *path = cwd.join(&*path);
        }
    }
}

fn main() -> Result<()> {
    let args = Options::command().get_matches();
    let explicit_render_distance =
        args.value_source("render_distance") == Some(clap::parser::ValueSource::CommandLine);
    let explicit_username =
        args.value_source("username") == Some(clap::parser::ValueSource::CommandLine);
    let mut options = Options::from_arg_matches(&args)?;
    let root = ms_auth::project_root();
    if !std::path::Path::new(".local/resources/manifest.json").is_file()
        && root.join(".local/resources/manifest.json").is_file()
    {
        if let Ok(cwd) = std::env::current_dir() {
            absolutize_cli_path(&cwd, &mut options.jar);
            absolutize_cli_path(&cwd, &mut options.world);
            absolutize_cli_path(&cwd, &mut options.benchmark_output_dir);
            absolutize_cli_path(&cwd, &mut options.screenshot);
        }
        let _ = std::env::set_current_dir(&root);
    }
    options.validate_surface_wait_build()?;
    if options.benchmark {
        worker_priority::configure(options.benchmark_worker_priority.unwrap_or_default())
            .map_err(anyhow::Error::msg)?;
    }
    anyhow::ensure!(!options.village || options.generator == 2,
        "--village uses the legacy Ferrum generator: specify --generator 2. To inspect native structures use --structure-preview village --screenshot reports/native-village.png.");
    if options.structure_preview.is_some() {
        options.fresh = true;
        options.creative = true;
        options.screen = "terrain".into();
    }
    if let Some(address) = &options.server_status {
        let status = network::status(address, std::time::Duration::from_secs(5))?;
        println!(
            "{}\nLatency: {:.2} ms",
            serde_json::to_string_pretty(&status.json)?,
            status.latency.as_secs_f64() * 1000.
        );
        return Ok(());
    }
    if options.connect.is_some() {
        options.play = true;
    }
    if let Some(id) = &options.save_id {
        anyhow::ensure!(
            !id.is_empty()
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "Invalid save ID"
        );
    }
    anyhow::ensure!(
        options.seconds.is_finite() && options.seconds > 0.0,
        "--seconds must be positive and finite"
    );
    if let Some(turn_after) = options.benchmark_streaming_return_after {
        anyhow::ensure!(turn_after < options.seconds,
            "--benchmark-streaming-return-after must be less than --seconds");
    }
    if options.list_worlds {
        let path = minecraft_dir().join("saves");
        for entry in
            std::fs::read_dir(&path).with_context(|| format!("Reading {}", path.display()))?
        {
            let entry = entry?;
            if entry.path().join("level.dat").is_file() {
                match world::world_metadata(&entry.path()) {
                    Ok(meta) => println!(
                        "{} | legacy={} | spawn {:?}",
                        entry.path().display(),
                        meta.legacy,
                        meta.spawn
                    ),
                    Err(e) => println!("{} | {}", entry.path().display(), e),
                }
            }
        }
        return Ok(());
    }
    if !std::path::Path::new(".local/resources/manifest.json").is_file() {
        let dest = root.join(".local").join("resources");
        if let Some((mc_dir, jar_src)) = resource_dump::find_appdata_189_jar() {
            let dumped = resource_dump::ensure_dumped_resources(&mc_dir, &jar_src, &dest, |_, _, _| {})?;
            if options.jar.is_none() {
                options.jar = Some(dumped);
            }
            if let Ok(cwd) = std::env::current_dir() {
                absolutize_cli_path(&cwd, &mut options.jar);
                absolutize_cli_path(&cwd, &mut options.world);
                absolutize_cli_path(&cwd, &mut options.benchmark_output_dir);
                absolutize_cli_path(&cwd, &mut options.screenshot);
            }
            let _ = std::env::set_current_dir(&root);
        } else if options.server_status.is_none() && !options.list_worlds {
            anyhow::bail!("{}", resource_dump::MISSING_JAR_NOTICE);
        }
    }
    if options.jar.is_none() {
        options.jar = Some(default_jar_path(&root));
    }
    if !options.benchmark
        && !options.check
        && (options.screenshot.is_none() || options.connect.is_some())
        && options.server_status.is_none()
        && std::env::var_os("FERRUM189_MC_ACCESS_TOKEN").is_none()
    {
        let store_path = ms_auth::default_store_path();
        if let Ok(mut store) = ms_auth::AccountStore::load_from(&store_path) {
            if store.selected().is_some() {
                if let Ok(Some(acc)) = store.ensure_fresh_selected(&store_path) {
                    std::env::set_var("FERRUM189_MC_ACCESS_TOKEN", &acc.mc_access_token);
                    std::env::set_var("FERRUM189_MC_PROFILE_ID", &acc.uuid);
                    std::env::set_var("FERRUM189_MC_USERNAME", &acc.username);
                    if !explicit_username {
                        options.username = acc.username;
                    }
                }
            }
        }
    }
    client::run(options, explicit_render_distance)
}
