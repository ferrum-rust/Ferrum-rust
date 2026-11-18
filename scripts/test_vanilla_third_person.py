"""Regenerate original 1.8.9 F5 fixtures, or compare actual OpenGL and Vulkan player renders."""
import argparse
import gzip
import json
import os
import shutil
import subprocess
import tempfile
from pathlib import Path
from test_vanilla_menus import capture
from vanilla_world import ROOT


def compare_pose_images(report):
    from PIL import Image, ImageDraw
    import numpy as np
    comparisons = []
    for tick, label in ((4, "Idle head turn (front)"), (13, "Walking and swinging (rear)")):
        a = np.array(Image.open(report / f"java-{tick}.png").convert("RGB"))
        b = np.array(Image.open(report / f"rust-{tick}.png").convert("RGB"))
        original = np.any(a != [255, 0, 255], axis=2)
        # At render distance two the empty Vulkan fixture clears to a solid sky/fog color.
        # Each row's unobstructed pixel gives the sky color at the exact elevation.
        native = np.any(b != b[:, :1], axis=2)
        yy, xx = np.nonzero(original | native)
        box = (slice(max(0, yy.min()-8), yy.max()+9), slice(max(0, xx.min()-8), xx.max()+9))
        panels = [np.where(mask[box][:, :, None], pixels[box], 47).astype(np.uint8)
                  for mask, pixels in ((original, a), (native, b))]
        out = Image.new("RGB", (panels[0].shape[1]*2, panels[0].shape[0]+44), (47,47,47))
        draw = ImageDraw.Draw(out)
        draw.text((8, 5), label, fill="white")
        for i, panel in enumerate(panels):
            x = i*panel.shape[1]
            draw.text((x+8, 25), ("Minecraft 1.8.9", "Rust / Vulkan")[i], fill="white")
            out.paste(Image.fromarray(panel), (x,44))
        out.save(report / f"compare-{tick}.png")
        comparisons.append(dict(tick=tick, scene=label, original_pixels=int(original.sum()),
                                native_pixels=int(native.sum()),
                                silhouette_iou=float((native & original).sum() / (native | original).sum())))
    return comparisons


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--test-exe", type=Path, help="Render through the compiled Rust test executable and write comparison PNGs")
    parser.add_argument("--reference", type=Path, help="Existing original ThirdPersonOracle work directory")
    parser.add_argument("--report", type=Path, default=ROOT / "reports/f5-189")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="third-person-") as tmp:
        if args.reference:
            source = args.reference
            path = source / "layouts.json"
        else:
            source, path = capture("ThirdPersonOracle", Path(tmp))
        result = json.loads(path.read_text(encoding="utf8"))
        if args.test_exe:
            report = args.report.resolve()
            report.mkdir(parents=True, exist_ok=True)
            for tick in (4,13):
                shutil.copy2(source / f"game/screenshots/java-{tick}.png", report / f"java-{tick}.png")
            verified = []
            for name in ["original_f5_tick_turns_and_132_rendered_model_matrices_and_limb_poses",
                         "original_front_and_rear_camera_clips_at_eight_ray_corners",
                         "f5_world_mesh_matches_original_renderplayer_gl_vertices_and_skin_uvs"]:
                run = subprocess.run([str(args.test_exe.resolve()), name, "--include-ignored", "--nocapture"],
                                     cwd=ROOT, env=dict(os.environ, FERRUM_F5_CAPTURE=str(report)), check=True,
                                     stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
                print(run.stdout)
                if "1 passed" not in run.stdout:
                    raise RuntimeError(f"Test executable did not include {name}")
                verified.append(name)
            checks = dict(resolution=[1600,900], original_game="1.8.9", original_ticks=len(result["frames"]),
                          intermediate_and_current_poses=3*len(result["frames"]), body_parts_per_pose=6,
                          original_feedback_vertices=sum(len(row[1]) for row in result["mesh"]),
                          camera_transforms=len(result["cameras"]), passing_tests=verified, images=compare_pose_images(report))
            (report / "comparison.json").write_text(json.dumps(checks,indent=2)+"\n",encoding="utf8")
        else:
            files = [("geometry","src/vanilla/player_models189.json.gz"),("frames","tests/fixtures/vanilla189-third-person.json.gz"),
                     ("mesh","tests/fixtures/vanilla189-f5-vertices.json.gz"),("cameras","tests/fixtures/vanilla189-f5-camera.json.gz")]
            for key, target in files:
                (ROOT / target).write_bytes(gzip.compress(json.dumps(result[key],separators=(",",":")).encode(),mtime=0))
            print("Captured",len(result["frames"]),"ticks and",3*len(result["frames"]),"original rendered poses")


if __name__ == "__main__":
    main()
