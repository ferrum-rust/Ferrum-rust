"""Export original village door discovery, home lookup and reputation traces."""
import os
import subprocess
from pathlib import Path
from vanilla_world import ROOT, classpath, java_executable

cp = classpath(Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar")
out = ROOT / ".local/oracles/village-homes"
out.mkdir(parents=True, exist_ok=True)
sources = [ROOT / "tools/java" / name for name in (
    "FallingEntityOracle.java", "ItemEntityOracle.java", "VillageHomeOracle.java", "VillageGolemOracle.java"
)]
subprocess.run(["javac", "--release", "8", "-cp", cp, "-d", str(out), *map(str, sources)], check=True)
subprocess.run([java_executable(), "-Xmx512m", "-cp", str(out) + os.pathsep + cp, "VillageHomeOracle",
                str(ROOT / "tests/fixtures/vanilla189-village-homes.json.gz")], cwd=ROOT, check=True)
subprocess.run([java_executable(), "-Xmx512m", "-cp", str(out) + os.pathsep + cp, "VillageGolemOracle",
                str(ROOT / "tests/fixtures/vanilla189-village-golem.json.gz")], cwd=ROOT, check=True)
