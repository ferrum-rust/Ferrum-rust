"""Capture 1.8.9 damage/death camera, hand transforms and death FOV in a disposable game."""
import gzip
from pathlib import Path
import tempfile

from test_vanilla_menus import capture
from vanilla_world import ROOT


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="hurt-camera-") as work:
        _, output = capture("HurtCameraOracle", Path(work))
        (ROOT / "tests/fixtures/vanilla189-hurt-camera.json.gz").write_bytes(
            gzip.compress(output.read_bytes(), mtime=0)
        )
