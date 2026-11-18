"""Sample invisible Wither/Dragon visibility, 100-frame lifetimes and exact GuiIngame quads."""
import json
import tempfile
from pathlib import Path
from test_vanilla_menus import capture
from vanilla_world import ROOT

if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="boss-bar-") as work:
        _, output = capture("BossBarOracle", Path(work))
        data = json.loads(output.read_text(encoding="utf8"))
        (ROOT / "tests/fixtures/vanilla189-boss-bar.json").write_text(
            json.dumps(data, separators=(",", ":"), ensure_ascii=False), encoding="utf8")
        print(len(data["frames"]), "health/lifetime cases,", len(data["visibility"]), "visibility checks")
