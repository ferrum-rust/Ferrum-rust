"""Compare the Vulkan creative inventory to the local Korean 1.8.9 client in disposable profiles."""
import argparse
import gzip
import json
import os
import subprocess
import tempfile
from pathlib import Path
import numpy as np
from PIL import Image
from test_vanilla_menus import capture
from vanilla_world import ROOT


def isolated_profile(parent, settings):
    work = Path(tempfile.mkdtemp(prefix="rust-visual-review-", dir=parent))
    resources = ROOT / ".local/resources"
    (work / ".local").mkdir()
    (work / "profile").mkdir()
    if os.name == "nt":
        subprocess.run(["powershell.exe", "-NoProfile", "-NonInteractive", "-Command",
                        "New-Item -ItemType Junction -Path $env:MC_REVIEW_DEST -Target $env:MC_REVIEW_RESOURCES | Out-Null"],
                       env=dict(os.environ, MC_REVIEW_DEST=str(work / ".local/resources"), MC_REVIEW_RESOURCES=str(resources)),
                       creationflags=subprocess.CREATE_NO_WINDOW, check=True)
    else:
        (work / ".local/resources").symlink_to(resources, target_is_directory=True)
    (work / "options.json").write_text(json.dumps(settings), encoding="utf8")
    jar = Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar"
    env = dict(os.environ, APPDATA=str(work / "profile"))
    return work,jar,env

def review(binary, reference, parent, report, utility=False):
    work,jar,env=isolated_profile(parent,{"gui_scale":2,"language":"ko_kr","volume":0,"render_distance":2})
    report.mkdir(parents=True, exist_ok=True)
    rows = []
    names = (("dispenser","dropper","brewing","hopper","horse-inventory","donkey-inventory") if utility else
             ("creative", "creative-search", "creative-inventory", "creative-decorations"))
    for name in names:
        path = report / (name + ".png")
        subprocess.run([str(binary), "--jar", str(jar), "--fresh", "--creative", "--generator", "1",
                        "--language", "ko_kr", "--username", "Menu189", "--screen", name,
                        "--screenshot", str(path)], cwd=work, env=env, check=True, timeout=120)
        actual = np.array(Image.open(path).convert("RGB"), dtype=np.int16)
        original = np.array(Image.open(reference / (name + ".png")).convert("RGB"), dtype=np.int16)
        assert actual.shape == original.shape == (900, 1600, 3)
        # Exclude the underlying scene. The Java reference draws its UI over a clear framebuffer.
        # Include the full opaque panel interior plus every tab's icon and selected/unselected backing.
        mask = np.zeros(actual.shape[:2], dtype=bool)
        if utility:
            height = 133 if name=="hopper" else 166
            x, y = 624, (450-height)//2*2
            mask[y+6:y+height*2-6, x+6:x+346] = True
            crop = (slice(y-4,y+height*2+4),slice(x-4,x+356))
        else:
            x, y = 604, 314
            mask[y+6:y+266, x+6:x+384] = True
            for i in range(12):
                col = i % 6
                left = x + (167 if col == 5 else col*29)*2
                top = y + (-28 if i < 6 else 132)*2
                mask[top+8:top+52, left+8:left+48] = True
            crop = (slice(252, 648), slice(596, 1002))
        Image.fromarray(np.concatenate((original[crop], actual[crop]), axis=1).astype(np.uint8)).save(report/("compare-"+name+".png"))
        delta = abs(actual-original)[mask]
        rows.append(dict(screen=name, compared_pixels=int(delta.shape[0]), exact_pixels=int((delta.max(1)==0).sum()),
                         pixels_over_8=int((delta.max(1)>8).sum()), mean_channel_error=round(float(delta.mean()), 4)))
    (report / "comparison.json").write_text(json.dumps({"locale":"ko_KR","resolution":[1600,900],"gui_scale":2,
        "side_by_side":"Original 1.8.9 at left, Rust/Vulkan at right", "comparison":"Opaque inner panel" + ("" if utility else " and 12 tab interiors") + "; world background excluded.", "screens":rows},indent=2)+"\n",encoding="utf8")
    print(rows)


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("--binary",type=Path,required=True)
    p.add_argument("--parent",type=Path,default=Path(tempfile.gettempdir()))
    p.add_argument("--reference",type=Path,help="Existing original CreativeInventoryOracle screenshot directory")
    p.add_argument("--report",type=Path)
    p.add_argument("--utility",action="store_true",help="Compare brewing, hopper, dispenser, dropper, horse and donkey inventories")
    p.add_argument("--refresh-utility-models",action="store_true",
                   help="Regenerate slot geometry and horse meshes from the installed game during a fresh utility capture")
    args=p.parse_args()
    oracle = "UtilityWindowOracle" if args.utility else "CreativeInventoryOracle"
    if args.refresh_utility_models and (not args.utility or args.reference):
        p.error("--refresh-utility-models needs --utility and a fresh original capture")
    if args.reference:
        reference=args.reference
    else:
        work,layout=capture(oracle,args.parent)
        reference=work/"game/screenshots"
        if args.refresh_utility_models:
            (ROOT/"tests/fixtures/vanilla189-utility-slots.json").write_bytes(layout.read_bytes())
            (ROOT/"src/ui/horse_inventory_models.json.gz").write_bytes(
                gzip.compress((work/"horse-models.json").read_bytes(),compresslevel=9,mtime=0))
    report=args.report or ROOT/"reports"/("utility-windows-189" if args.utility else "creative-inventory-189")
    review(args.binary.resolve(),reference,args.parent,report,args.utility)


if __name__ == "__main__":
    main()
