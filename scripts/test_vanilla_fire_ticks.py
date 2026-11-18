"""Export original WorldServer scheduled fire observations and numeric registry facts."""
import gzip
import json
import os
import subprocess
from pathlib import Path

from vanilla_world import ROOT, classpath, java_executable

cp = classpath(Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar")
out = ROOT / ".local/oracles/fire-tick"
out.mkdir(parents=True, exist_ok=True)
sources = [ROOT / "tools/java" / name for name in
           ("VanillaWorldBridge.java", "RuntimeUseOracle.java", "FirePortalOracle.java", "FireTickOracle.java")]
fixture = ROOT / "tests/fixtures/vanilla189-fire-ticks.json.gz"
subprocess.run(["javac", "--release", "8", "-cp", cp, "-d", str(out), *map(str, sources)], check=True)
subprocess.run([java_executable(), "-Xmx512m", "-cp", str(out) + os.pathsep + cp, "FireTickOracle",
                str(fixture), str(out/"world")], cwd=ROOT, check=True)
data = json.load(gzip.open(fixture))
with (ROOT/"src/vanilla/runtime_fire_data.rs").open("w",encoding="utf8",newline="\n") as rust:
    rust.write("// Numeric observations from the original 1.8.9 block and biome registries.\n")
    rust.write("const FUEL: [(u8, u8); 256] = [\n")
    for i in range(0,256,8):
        rust.write("    " + ", ".join(f"({e}, {f})" for e,f in data["fuel"][i:i+8]) + ",\n")
    rust.write("];\nconst CAN_RAIN: [bool; 256] = [\n")
    for i in range(0,256,16):
        rust.write("    " + ", ".join(str(b).lower() for b in data["rain"][i:i+16]) + ",\n")
    rust.write("];\n")
cases = data["cases"]
print("Original fire scenarios", len(cases), "checkpoints", sum(len(c["frames"]) for c in cases),
      "with primed TNT",sum(bool(c["frames"][-1][1][-1]) for c in cases))
