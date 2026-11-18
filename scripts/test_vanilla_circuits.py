"""Regenerate piston mobility and structure plans from the installed 1.8.9 classes."""
import gzip
import json
import os
from pathlib import Path
import subprocess
import tempfile
from vanilla_world import ROOT, classpath, java_executable

cp = classpath(Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar")
with tempfile.TemporaryDirectory() as work:
    javac = os.environ.get("MC189_JAVAC", r"C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe")
    classes = ["MovementOracle", "MovementAttributesOracle", "CollisionShapeOracle", "HandPlacementOracle", "RedstoneCircuitOracle"]
    subprocess.run([javac, "-cp", cp, "-d", work, *(ROOT / "tools/java" / (c+".java") for c in classes)], check=True)
    output = subprocess.check_output([java_executable(), "-Xmx256m", "-cp", work+os.pathsep+cp, "RedstoneCircuitOracle"], cwd=work)
    mobility, plans, devices, placement = json.loads(output.splitlines()[-1])
    (ROOT / "src/vanilla/piston_mobility.rs").write_text("// 1.8.9 block registry: movable, destroyed, immovable.\n["+",\n".join("    "+", ".join(map(str,mobility[i:i+16])) for i in range(0,256,16))+"\n]\n",encoding="utf8")
    (ROOT / "tests/fixtures/vanilla189-piston-plans.json.gz").write_bytes(gzip.compress(json.dumps(plans,separators=(",",":")).encode(),mtime=0))
    (ROOT / "tests/fixtures/vanilla189-circuit-uses.json.gz").write_bytes(gzip.compress(json.dumps([devices, placement],separators=(",",":")).encode(),mtime=0))
