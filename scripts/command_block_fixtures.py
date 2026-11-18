"""Capture original 1.8.9 command-block edge scheduling and tile defaults."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
from vanilla_world import ROOT, classpath, java_executable

with tempfile.TemporaryDirectory(prefix="ferrum-command-block-") as directory:
    cp = classpath(Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar")
    compiler = os.environ.get("MC189_JAVAC", r"C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe")
    subprocess.run([compiler, "-encoding", "UTF-8", "-cp", cp, "-d", directory,
                    *[str(ROOT / "tools/java" / (name + ".java")) for name in ("FoodOracle", "CommandBlockOracle")]], check=True)
    result = subprocess.check_output([java_executable(), "-cp", directory + os.pathsep + cp,
                                      "CommandBlockOracle"], cwd=directory)
    data = json.loads(result)
    (ROOT / "tests/fixtures/vanilla189-command-block.json").write_text(
        json.dumps(data, separators=(",", ":")) + "\n", encoding="utf8")
    print("Captured 32 original redstone edge sequences, command NBT and permissions")
