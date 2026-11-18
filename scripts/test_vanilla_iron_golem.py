"""Export original iron golem combat, drops, pose math and model geometry."""
import gzip
import json
import os
import subprocess
from pathlib import Path
from vanilla_world import ROOT, classpath, java_executable

cp = classpath(Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar")
out = ROOT / ".local/oracles/iron-golem"
out.mkdir(parents=True, exist_ok=True)
sources = [ROOT / "tools/java" / name for name in ("FallingEntityOracle.java", "ItemEntityOracle.java", "IronGolemOracle.java",
                                                  "VillagerGoalsOracle.java", "GolemTargetOracle.java", "VillageReputationOracle.java", "GolemActionOracle.java")]
subprocess.run(["javac", "--release", "8", "-cp", cp, "-d", str(out), *map(str, sources)], check=True)
fixture = ROOT / "tests/fixtures/vanilla189-iron-golem.json.gz"
subprocess.run([java_executable(), "-Xmx512m", "-cp", str(out) + os.pathsep + cp, "IronGolemOracle",
                str(fixture)], cwd=ROOT, check=True)
mesh = json.loads(gzip.decompress(fixture.read_bytes()))["mesh"]
lines = ["// Geometry exported by scripts/test_vanilla_iron_golem.py from ModelIronGolem.",
         "const PARTS: &[ModelPart] = &["]
for pivot, quads in mesh:
    lines.append("    (" + repr(pivot) + ", &[")
    for quad in quads:
        lines.append("        " + repr(quad) + ",")
    lines.append("    ]),")
lines.append("];")
(ROOT / "src/golem_mesh_data.rs").write_text("\n".join(lines) + "\n", encoding="utf-8")
subprocess.run([java_executable(), "-Xmx512m", "-cp", str(out) + os.pathsep + cp, "GolemTargetOracle",
                str(ROOT / "tests/fixtures/vanilla189-golem-targets.json.gz")], cwd=ROOT, check=True)
subprocess.run([java_executable(), "-Xmx512m", "-cp", str(out) + os.pathsep + cp, "VillageReputationOracle",
                str(ROOT / "tests/fixtures/vanilla189-village-reputation.json.gz")], cwd=ROOT, check=True)
subprocess.run([java_executable(), "-Xmx512m", "-cp", str(out) + os.pathsep + cp, "GolemActionOracle",
                str(ROOT / "tests/fixtures/vanilla189-golem-actions.json.gz")], cwd=ROOT, check=True)
