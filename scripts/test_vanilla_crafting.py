"""Export original CraftingManager order, recipe inputs and evaluated outputs."""
import os
import subprocess
from pathlib import Path

from vanilla_world import ROOT, classpath, java_executable

cp = classpath(Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar")
out = ROOT / ".local/oracles/crafting"
out.mkdir(parents=True, exist_ok=True)
subprocess.run(["javac", "--release", "8", "-cp", cp, "-d", str(out),
                str(ROOT / "tools/java/FallingEntityOracle.java"),
                str(ROOT / "tools/java/CraftRegistryOracle.java"),
                str(ROOT / "tools/java/VanillaWorldBridge.java"),
                str(ROOT / "tools/java/RuntimeUseOracle.java"),
                str(ROOT / "tools/java/CraftWindowOracle.java")], check=True)
subprocess.run([java_executable(), "-Xmx512m", "-cp", str(out) + os.pathsep + cp,
                "CraftRegistryOracle", str(ROOT / "src/vanilla/crafting_registry.json"),
                str(ROOT / "tests/fixtures/vanilla189-crafting.json.gz")], cwd=ROOT, check=True)
subprocess.run([java_executable(), "-Xmx512m", "-cp", str(out) + os.pathsep + cp,
                "CraftWindowOracle", str(ROOT / "tests/fixtures/vanilla189-crafting-window.json.gz"),
                str(out / "world")], cwd=ROOT, check=True)
