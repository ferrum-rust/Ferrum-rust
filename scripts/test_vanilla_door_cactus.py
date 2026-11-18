"""Bake the installed 1.8.9 door and cactus models with the original FaceBakery."""
import json
import os
from pathlib import Path
import struct
import subprocess

from vanilla_world import ROOT, classpath, java_executable

cp = classpath(Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar")
classes = ROOT / ".local/oracles/door-cactus"
classes.mkdir(parents=True, exist_ok=True)
registry = classes / "states.json"
geometry = classes / "faces.json"
blocks = ["wooden_door", "iron_door", "spruce_door", "birch_door", "jungle_door",
          "acacia_door", "dark_oak_door", "cactus"]
subprocess.run(["javac", "--release", "8", "-cp", cp, "-d", str(classes),
                "tools/java/TorchModelOracle.java", "tools/java/CircuitModelOracle.java"],
               cwd=ROOT, check=True)
subprocess.run([java_executable(), "-cp", str(classes) + os.pathsep + cp, "CircuitModelOracle",
                str(ROOT / ".local/resources/assets/minecraft"), str(registry), str(geometry), *blocks],
               cwd=ROOT, check=True)

faces = {(name, key): quads for name, key, quads in json.loads(geometry.read_text())}
states = {(state >> 4, state & 15): (name, key)
          for state, name, key in json.loads(registry.read_text()) if name in blocks}
rows = []
for (block, meta), (name, state_key) in states.items():
    if name != "cactus" and meta >= 8:
        continue
    for half in ([0] if name == "cactus" else [0, 1, 8, 9]):
        # The mapper supplies facing/open in the lower state and hinge in the upper.
        key = state_key
        if name != "cactus":
            lower = dict(field.split("=") for field in state_key.split(","))
            upper = dict(field.split("=") for field in states[block, 8 | (half & 1)][1].split(","))
            lower["hinge"] = upper["hinge"]
            if half & 8:
                lower["half"] = upper["half"]
            key = ",".join(f"{k}={v}" for k, v in lower.items())
        digest = 0xcbf29ce484222325
        for texture, facing, cull, shade, tint, vertices in faces[name, key]:
            words = [*texture.encode("ascii"), facing, cull, shade]
            for vertex in vertices:
                words.extend(struct.unpack("<5I", struct.pack("<5f", *vertex)))
            for word in words:
                digest = ((digest ^ word) * 0x100000001b3) & 0xffffffffffffffff
        rows.append([block, meta, half, digest])
(ROOT / "tests/fixtures/vanilla189-door-cactus-model.json").write_text(
    json.dumps(rows, separators=(",", ":")), encoding="utf-8")
print(f"{len(rows)} door halves and cactus ages")
