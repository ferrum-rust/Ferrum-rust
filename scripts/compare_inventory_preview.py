"""Compare the player in five survival inventory poses with the installed game."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile

import numpy as np
from PIL import Image
from test_vanilla_menus import capture
from vanilla_world import ROOT


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--test-binary", type=Path, required=True)
    parser.add_argument("--reference", type=Path, help="Existing InventoryPreviewOracle screenshots")
    parser.add_argument("--report", type=Path, default=ROOT / "reports/survival-inventory-preview")
    args = parser.parse_args()
    report = args.report.resolve()
    report.mkdir(parents=True, exist_ok=True)
    reference = args.reference
    if not reference:
        work, _ = capture("InventoryPreviewOracle", Path(tempfile.gettempdir()))
        reference = work / "game/screenshots"
    subprocess.run([args.test_binary.resolve(), "--ignored", "--exact",
                    "client::ui_tests::survival_inventory_turns_the_player_toward_the_previous_frame_cursor"],
                   cwd=ROOT, check=True, env={**os.environ, "FERRUM_INVENTORY_PREVIEW_CAPTURE": str(report)})
    results, pairs = [], []
    for pose in ("center", "left", "right", "up", "down"):
        def pixels(path):
            with Image.open(path / (pose + ".png")) as im:
                return np.array(im.convert("RGB").crop((676, 300, 780, 436)), dtype=np.int16)
        java, rust = pixels(reference), pixels(report)
        a, b = java.max(axis=2) > 8, rust.max(axis=2) > 8
        union = a | b
        results.append({"pose": pose, "silhouette_iou": round((a & b).sum() / union.sum(), 5),
                        "mean_channel_error_on_player": round(float(abs(java-rust)[union].mean()), 4)})
        pairs.append(np.concatenate((java, rust), axis=0))
    Image.fromarray(np.concatenate(pairs, axis=1).astype(np.uint8)).save(report / "poses.png")
    (report / "comparison.json").write_text(json.dumps(results, indent=2) + "\n", encoding="utf8")
    print(results)


if __name__ == "__main__":
    main()
