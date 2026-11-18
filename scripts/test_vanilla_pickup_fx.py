"""Sample the original EntityPickupFX renderer as its collector moves over all three client ticks."""
from pathlib import Path
import tempfile
from test_vanilla_menus import capture
from vanilla_world import ROOT

if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="pickup-fx-") as work:
        _, result = capture("PickupFxOracle", Path(work))
        (ROOT / "tests/fixtures/vanilla189-pickup-fx.json").write_bytes(result.read_bytes())
