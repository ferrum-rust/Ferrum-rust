"""Export original Block/Item/EntityPlayer mining facts and oracle cases."""
import os
import subprocess
from pathlib import Path

from vanilla_world import ROOT, classpath, java_executable

cp = classpath(Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar")
out = ROOT / ".local/oracles/mining"
out.mkdir(parents=True, exist_ok=True)
subprocess.run(["javac", "--release", "8", "-cp", cp, "-d", str(out),
                *map(str,[ROOT / "tools/java" / f for f in ("FallingEntityOracle.java","MiningRatesOracle.java","RuntimeDigOracle.java","RuntimeUseOracle.java","RuntimeFallingOracle.java","VanillaWorldBridge.java","BlockTickOracle.java")])], check=True)
subprocess.run([java_executable(), "-Xmx512m", "-cp", str(out) + os.pathsep + cp, "MiningRatesOracle",
                str(ROOT / "src/vanilla/mining_registry.json"),str(ROOT / "tests/fixtures/vanilla189-mining-rates.json.gz")],
               cwd=ROOT, check=True)
subprocess.run([java_executable(), "-Xmx512m", "-cp", str(out) + os.pathsep + cp, "RuntimeDigOracle",
                str(ROOT / "tests/fixtures/vanilla189-runtime-dig.json.gz"),str(out / "world")],cwd=ROOT,check=True)
