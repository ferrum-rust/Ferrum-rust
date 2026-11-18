"""Rebuild the native boat/cart meshes and the real JAR's position, rail and damage transform fixtures."""
import json
import tempfile
from pathlib import Path

from test_vanilla_menus import capture
from vanilla_world import ROOT


def wrap_quads(quads):
    # ModelMinecart's interior repeats its 64px-wide skin. Split its crossing faces before
    # packing into the Vulkan atlas so sampling outside the rectangle uses the original repeat.
    def clip(poly, cut, sign):
        out = []
        for a, b in zip(poly[-1:] + poly[:-1], poly):
            inside_a, inside_b = sign * (a[3] - cut) >= 0, sign * (b[3] - cut) >= 0
            if inside_a != inside_b:
                t = (cut - a[3]) / (b[3] - a[3])
                out.append([round(x + (y - x) * t) for x, y in zip(a, b)])
            if inside_b:
                out.append(b)
        return out

    result = []
    for face in quads:
        low, high = min(v[3] for v in face), max(v[3] for v in face)
        for tile in range(low // 64, max(low // 64, (high - 1) // 64) + 1):
            part = clip(clip(face, tile * 64, 1), (tile + 1) * 64, -1)
            assert len(part) == 4
            result.append([[x, y, z, u - 64 * tile, v] for x, y, z, u, v in part])
    return result


def main():
    with tempfile.TemporaryDirectory(prefix="vehicle-render-") as work:
        _, output = capture("VehicleModelOracle", Path(work))
        data = json.loads(output.read_text(encoding="utf8"))
        (ROOT / "src/vanilla/vehicle_models189.json").write_text(
            json.dumps([wrap_quads(data[name]) for name in ("boat", "cart")], separators=(",", ":")), encoding="utf8")
        (ROOT / "tests/fixtures/vanilla189-vehicle-render.json").write_text(
            json.dumps(data["frames"], separators=(",", ":")), encoding="utf8")
        print(len(data["boat"]), "boat quads,", len(data["cart"]), "cart quads,", len(data["frames"]), "matrix captures")


if __name__ == "__main__":
    main()
