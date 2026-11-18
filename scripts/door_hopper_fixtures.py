"""Trace door placement and a power-cycled hopper in the locally installed original game."""
import os
import subprocess
import tempfile
from pathlib import Path
from vanilla_world import ROOT, classpath, java_executable

cp = classpath(Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar")
javac = os.environ.get("MC189_JAVAC", r"C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe")
sources = ["MovementOracle", "MovementAttributesOracle", "CollisionShapeOracle", "HandPlacementOracle", "DoorHopperOracle"]
with tempfile.TemporaryDirectory() as tmp:
    subprocess.run([javac, "-encoding", "UTF-8", "-cp", cp, "-d", tmp,
                    *(ROOT / "tools/java" / (name + ".java") for name in sources)], check=True)
    result = subprocess.check_output([java_executable(), "-cp", tmp + os.pathsep + cp, "DoorHopperOracle"], cwd=tmp)
(ROOT / "tests/fixtures/vanilla189-doors-hopper.json").write_bytes(result.strip() + b"\n")
