"""Capture original Korean singleplayer world menus at 1600×900 with GUI scale 2."""
import argparse
import json
import tempfile
from pathlib import Path

from test_vanilla_menus import capture
from vanilla_world import ROOT


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--parent", type=Path, default=Path(tempfile.gettempdir()))
    args = parser.parse_args()
    work, output = capture("WorldMenuOracle", args.parent, (
        "-Duser.language=ko", "-Duser.country=KR", "-Duser.timezone=Asia/Seoul",
    ))
    layouts = json.loads(output.read_text(encoding="utf8"))
    if any(screen["size"] != [800, 450] for screen in layouts.values()):
        raise RuntimeError("Unexpected scaled resolution")
    path = ROOT / "tests/fixtures/vanilla189-world-menus.json"
    path.write_text(json.dumps(layouts, ensure_ascii=False, separators=(",", ":")) + "\n", encoding="utf8")
    presets = json.loads((work / "presets.json").read_text(encoding="utf8"))
    (ROOT / "src/ui/world_presets.json").write_text(
        json.dumps(presets, ensure_ascii=False, separators=(",", ":")) + "\n", encoding="utf8",
    )
    print(f"{len(layouts)} layouts written to {path}")
    print("Original screenshots and disposable saves:", work / "game")


if __name__ == "__main__":
    main()
