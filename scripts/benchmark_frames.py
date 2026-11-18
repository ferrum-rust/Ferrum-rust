"""Measure window-present cadence in an offline 1.8.9 world, or compare completed captures."""

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import shutil
import statistics
import subprocess
import tempfile

from vanilla_world import ROOT, classpath, java_executable

SCENE = ROOT / "scripts/benchmark_scene.json"
VERSION = Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9"


def read(path):
    return json.loads(path.read_text(encoding="utf8"))


def write_new(path, data):
    # Benchmark results are evidence of a single run. Preserve previous captures.
    with path.open("x", encoding="utf8") as stream:
        json.dump(data, stream, indent=2, allow_nan=False)
        stream.write("\n")


def world_manifest(world):
    hashes = {
        path.relative_to(world).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in sorted(world.rglob("*"))
        if path.is_file()
    }
    checksum = hashlib.sha256(json.dumps(hashes, sort_keys=True).encode()).hexdigest()
    return {"world_sha256": checksum, "files": hashes}


def prepare(parent):
    from benchmark_anvil import create_world

    parent.mkdir(parents=True, exist_ok=True)
    run = Path(tempfile.mkdtemp(prefix="ferrum-frame-", dir=parent))
    world = run / "canonical"
    create_world(world, read(SCENE))
    manifest = world_manifest(world)
    write_new(run / "world-hashes.json", manifest)
    shutil.copyfile(SCENE, run / "scene.json")
    print(
        json.dumps(
            {"run": str(run), "world": str(world), "world_sha256": manifest["world_sha256"], "chunks": 625}
        )
    )


def verify_world(run):
    actual = world_manifest(run / "canonical")
    if actual != read(run / "world-hashes.json"):
        raise ValueError("Canonical file set, file contents or aggregate hash differs from the manifest")
    return actual["world_sha256"]


def archive_capture(report, image, dest, run, renderer_source=None):
    if dest is None:
        return report
    dest.mkdir(parents=True, exist_ok=False)
    shutil.copyfile(report, dest / report.name)
    shutil.copyfile(image, dest / image.name)
    if renderer_source is not None:
        shutil.copyfile(renderer_source, dest / renderer_source.name)
    shutil.copyfile(run / "world-hashes.json", dest / "world-hashes.json")
    shutil.copyfile(run / "scene.json", dest / "scene.json")
    return dest / report.name


def native(run, adapter, exe, output_dir=None):
    world, scene = run / "canonical", run / "scene.json"
    digest = verify_world(run)
    session = Path(tempfile.mkdtemp(prefix="rust-", dir=run))
    cmd = [
        str(exe),
        "--benchmark",
        "--play",
        "--benchmark-scene",
        str(scene),
        "--world",
        str(world),
        "--benchmark-adapter",
        adapter,
        "--benchmark-output-dir",
        str(session),
    ]
    print("Rust Vulkan render window. Log:", session / "client.log", flush=True)
    with (session / "client.log").open("w", encoding="utf8") as log:
        subprocess.run(cmd, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, timeout=180, check=True)
    captured = session / "benchmark-native-scene.json"
    raw = read(captured)
    if verify_world(run) != digest:
        raise ValueError("Native input world changed during the capture")
    if raw["scene"] != read(scene) or raw["gpu"].casefold() != adapter.casefold():
        raise ValueError("Native camera/settings or adapter differs from request")
    # samefile also resolves Windows extended-length paths (the Rust canonical form).
    actual = raw.get("world_directory")
    if not isinstance(actual, str) or not Path(actual).is_dir() or not Path(actual).samefile(world):
        raise ValueError(f"Native report identifies a different world directory: {actual!r}")
    raw.update(
        world_sha256=digest,
        benchmark_command=cmd,
        renderer_report_sha256=hashlib.sha256(captured.read_bytes()).hexdigest(),
    )
    report = session / "vulkan-frames.json"
    write_new(report, raw)
    image = session / "vulkan-view.png"
    shutil.copyfile(session / "benchmark-native-scene.png", image)
    print("Result:", archive_capture(report, image, output_dir, run, captured), flush=True)


def vanilla(run, output_dir=None):
    digest = verify_world(run)
    session = Path(tempfile.mkdtemp(prefix="vanilla-", dir=run))
    game = session / "game"
    world = game / "saves" / "benchmark"
    shutil.copytree(run / "canonical", world)
    if world_manifest(world)["world_sha256"] != digest:
        raise ValueError("Original client's private copy differs from the canonical snapshot")
    classes, scratch = session / "classes", session / "tmp"
    classes.mkdir()
    scratch.mkdir()
    cp = classpath(VERSION / "1.8.9.jar")
    compiler = os.environ.get("MC189_JAVAC", r"C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe")
    subprocess.run(
        [
            compiler,
            "-encoding",
            "UTF-8",
            "-cp",
            cp,
            "-d",
            str(classes),
            str(ROOT / "tools/java/VanillaFrameBenchmark.java"),
        ],
        check=True,
        cwd=session,
    )
    natives = VERSION / "natives-windows-x86_64"
    report = session / "opengl-frames.json"
    cmd = [
        java_executable(),
        "-Xms256M",
        "-Xmx768M",
        f"-Djava.library.path={natives}",
        f"-Dorg.lwjgl.librarypath={natives}",
        f"-Djava.io.tmpdir={scratch}",
        f"-Duser.home={session}",
        "-Djava.net.preferIPv4Stack=true",
        "-cp",
        str(classes) + os.pathsep + cp,
        "VanillaFrameBenchmark",
        str(game),
        str(VERSION.parents[1] / "assets"),
        str(run / "scene.json"),
        str(report),
        digest,
    ]
    print("Original 1.8.9 render window. Log:", session / "client.log", flush=True)
    with (session / "client.log").open("w", encoding="utf8") as log:
        subprocess.run(cmd, cwd=session, stdout=log, stderr=subprocess.STDOUT, timeout=240, check=True)
    verify_world(run)
    print(
        "Result:",
        archive_capture(report, game / "screenshots" / "vanilla-view.png", output_dir, run),
        flush=True,
    )


def validate_capture(path, role, run, digest, scene, gpu):
    payload = path.read_bytes()
    report = json.loads(payload)

    def require(ok, reason):
        if not ok:
            raise ValueError(f"{path}: {reason}")

    require(
        report["world_sha256"] == digest and report["scene"] == scene,
        "different world snapshot or camera/settings",
    )
    require(report["resolution_pixels"] == scene["resolution"], "different physical viewport")
    require(gpu is None or report["gpu"].casefold() == gpu.casefold(), "different GPU")
    expected = (2 * scene["render_distance"] + 1) ** 2
    if role == "original":
        require(
            report["kind"] == "vanilla_window_frame_intervals" and report["backend"] == "OpenGL",
            "not original OpenGL",
        )
        require(
            report.get("original_install_read_only") and report.get("external_network_disabled"),
            "not an isolated run",
        )
        require(
            report["camera_feet"] == scene["player_feet"]
            and report["yaw_degrees"] == scene["yaw_degrees"]
            and report["pitch_degrees"] == scene["pitch_degrees"],
            "original camera differed from requested pose",
        )
        require(report["entities_including_player"] == 1, "unexpected active entities")
        require(report["warmup_seconds"] >= scene["warmup_seconds"], "original warm-up was too short")
        clients, servers = report["client_loaded_chunks"], report["server_loaded_chunks"]
        require(
            clients == [expected, expected] and len(servers) == 2 and servers[0] == servers[1],
            "unstable chunk residency",
        )
    else:
        require(
            report["kind"] == "native_window_frame_intervals" and report["backend"] == "Vulkan",
            "not native Vulkan",
        )
        directory = report.get("world_directory")
        require(
            isinstance(directory, str)
            and Path(directory).is_dir()
            and Path(directory).samefile(run / "canonical"),
            "native world directory missing or mismatched",
        )
        require(report["loaded_chunks"] == expected, "native view is incomplete")
        require(report["entities"] == 0, "unexpected active entities")
        settings = report["settings"]
        for key in ("render_distance", "fancy", "smooth_lighting", "gamma", "clouds", "vsync"):
            require(settings[key] == scene[key], f"native setting {key} differed from the scene")
        require(settings["fov"] == scene["fov_degrees"], "native FOV differed from the scene")
        require(settings["mipmaps"] == scene["mipmap_levels"], "native mipmaps differed from the scene")
        require(settings["max_fps"] == 0, "native frame rate was limited")
    frames = report["frame_seconds"]
    require(
        bool(frames) and len(frames) == report["frames"] and all(math.isfinite(v) and v > 0 for v in frames),
        "invalid frame intervals",
    )
    seconds = math.fsum(frames)
    tail = sorted(frames, reverse=True)[: math.ceil(len(frames) / 100)]
    fps, low = len(frames) / seconds, len(tail) / math.fsum(tail)
    for field, actual in [("measured_seconds", seconds), ("average_fps", fps), ("one_percent_low_fps", low)]:
        require(
            math.isclose(actual, report[field], rel_tol=1e-6), f"{field} contradicts recorded frame intervals"
        )
    require(seconds + 0.001 >= scene["measure_seconds"], "measurement shorter than requested")
    return {
        **{key: value for key, value in report.items() if key not in ("frame_seconds", "scene")},
        "raw_report": str(path.resolve()),
        "raw_report_sha256": hashlib.sha256(payload).hexdigest(),
    }


def compare_runs(run, originals, natives):
    digest, scene = verify_world(run), read(run / "scene.json")
    if not originals or len(originals) != len(natives):
        raise ValueError("Provide one original and one native report for every repetition")
    if len({p.resolve() for p in originals + natives}) != 2 * len(originals):
        raise ValueError("Each report may only participate once")
    gpu = None
    groups = {"original": [], "native": []}
    used = set()
    for role, paths in [("original", originals), ("native", natives)]:
        for path in paths:
            row = validate_capture(path, role, run, digest, scene, gpu)
            if row["raw_report_sha256"] in used:
                raise ValueError("Copies of the same capture cannot be used as additional repetitions")
            used.add(row["raw_report_sha256"])
            gpu = row["gpu"]
            groups[role].append(row)
    metrics = {}
    for field in ["average_fps", "one_percent_low_fps"]:
        original = statistics.median(row[field] for row in groups["original"])
        native_fps = statistics.median(row[field] for row in groups["native"])
        ratio = native_fps / original
        direction = "equal"
        if native_fps != original:
            direction = "higher" if native_fps > original else "lower"
        metrics[field] = {
            "original": original,
            "native": native_fps,
            "native_over_original": ratio,
            "native_percent_change": (ratio - 1) * 100,
            "native_direction": direction,
        }
    return {
        "scene": scene,
        "world_sha256": digest,
        "gpu": gpu,
        "paired_repetitions": len(originals),
        "aggregation": "Median of each client's per-run statistics, one static window scene.",
        "metrics": metrics,
        "runs": groups,
    }


def bundle_results(dest, run, comparison):
    # Validate the provenance and locate all input screens before reserving the output.
    sources = []
    for role, rows in comparison["runs"].items():
        for number, row in enumerate(rows, 1):
            report = Path(row["raw_report"])
            names = (
                ["vanilla-view.png", "game/screenshots/vanilla-view.png"]
                if role == "original"
                else ["vulkan-view.png", "benchmark-native-scene.png"]
            )
            names.append(report.with_suffix(".png").name)
            image = next((report.parent / name for name in names if (report.parent / name).is_file()), None)
            if not image:
                raise ValueError(f"Missing capture screenshot next to {report}")
            if hashlib.sha256(report.read_bytes()).hexdigest() != row["raw_report_sha256"]:
                raise ValueError(f"Capture changed after validation: {report}")
            renderer_source = None
            if "renderer_report_sha256" in row:
                renderer_source = report.parent / "benchmark-native-scene.json"
                if not renderer_source.exists():
                    renderer_source = report.with_name(report.stem + "-renderer.json")
                if hashlib.sha256(renderer_source.read_bytes()).hexdigest() != row["renderer_report_sha256"]:
                    raise ValueError(f"Original renderer report hash differs: {renderer_source}")
            sources.append((row, report, image, renderer_source, f"{role}-{number}"))
    dest.mkdir(parents=True, exist_ok=False)
    for row, report, image, renderer_source, stem in sources:
        shutil.copyfile(report, dest / f"{stem}.json")
        shutil.copyfile(image, dest / f"{stem}.png")
        row.update(raw_report=f"{stem}.json", image=f"{stem}.png")
        if renderer_source is not None:
            row["renderer_report"] = f"{stem}-renderer.json"
            shutil.copyfile(renderer_source, dest / row["renderer_report"])
    shutil.copyfile(run / "world-hashes.json", dest / "world-hashes.json")
    shutil.copyfile(run / "scene.json", dest / "scene.json")
    write_new(dest / "comparison.json", comparison)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    actions = parser.add_subparsers(dest="mode", required=True)
    create = actions.add_parser("prepare", help="Generate one deterministic Anvil world in scratch")
    default_parent = Path(os.environ.get("FERRUM_BENCH_TMP") or tempfile.gettempdir())
    create.add_argument("--parent", type=Path, default=default_parent)
    reference = actions.add_parser("vanilla", help="Run installed Java 1.8.9")
    raster = actions.add_parser("native", help="Run project release on an explicit Vulkan adapter")
    for action in [reference, raster]:
        action.add_argument("run", type=Path)
        action.add_argument(
            "--output-dir", type=Path, help="Copy measurements and image into a new directory"
        )
    raster.add_argument("--adapter", required=True)
    raster.add_argument("--exe", type=Path, default=ROOT / "target/release/ferrum189.exe")
    comp = actions.add_parser("compare", help="Recompute FPS statistics and verify input identity")
    comp.add_argument("run", type=Path)
    comp.add_argument("--original", action="append", required=True, type=Path)
    comp.add_argument("--native", action="append", required=True, type=Path)
    dest = comp.add_mutually_exclusive_group()
    dest.add_argument("--output", type=Path, help="Write a new summary JSON only")
    dest.add_argument(
        "--output-dir", type=Path, help="Bundle summary, all raw captures, images and world manifest"
    )
    args = parser.parse_args()
    try:
        if args.mode == "prepare":
            prepare(args.parent.resolve())
            return
        run = args.run.resolve()
        if args.output_dir and args.output_dir.exists():
            raise ValueError(f"Output directory already exists: {args.output_dir}")
        if args.mode == "native":
            native(run, args.adapter, args.exe.resolve(), args.output_dir)
        elif args.mode == "vanilla":
            vanilla(run, args.output_dir)
        else:
            result = compare_runs(run, args.original, args.native)
            if args.output_dir:
                bundle_results(args.output_dir, run, result)
            elif args.output:
                write_new(args.output, result)
            else:
                print(json.dumps(result, indent=2))
    except (ValueError, KeyError, OSError) as failure:
        parser.exit(1, f"Benchmark failed: {failure}\n")


if __name__ == "__main__":
    main()
