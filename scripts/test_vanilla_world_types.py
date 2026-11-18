"""Regenerate original 1.8.9 world-type state tables and chunk regression hashes."""
import os
import shutil
import subprocess
import tempfile
from pathlib import Path

from vanilla_world import ROOT, classpath, java_executable


def main():
    path = Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar"
    cp = classpath(path)
    javac = os.environ.get("MC189_JAVAC") or shutil.which("javac") or r"C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe"
    with tempfile.TemporaryDirectory(prefix="world-type-oracle-") as d:
        work = Path(d)
        sources = [str(ROOT / "tools/java" / name) for name in ("TerrainOracle.java", "WorldTypesOracle.java")]
        subprocess.run([javac, "-encoding", "UTF-8", "-source", "8", "-target", "8", "-cp", cp, "-d", d, *sources], cwd=work, check=True)
        subprocess.run([java_executable(), "-cp", d + os.pathsep + cp, "WorldTypesOracle", d,
                        str(ROOT / "src/ui/world_presets.json")], cwd=work, check=True)
        for name in ("world_debug_states.bin", "world_flat_blocks.json"):
            shutil.copyfile(work / name, ROOT / "src/vanilla" / name)
        shutil.copyfile(work / "world-terrain-hashes.json", ROOT / "tests/fixtures/vanilla189-world-terrain.json")


if __name__ == "__main__":
    main()
