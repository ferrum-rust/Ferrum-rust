"""Export original WorldServer scheduled and falling-entity phases together."""
import os
import subprocess
from pathlib import Path

from vanilla_world import ROOT, classpath, java_executable

jar = Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar"
cp = classpath(jar)
out = ROOT / ".local/oracles/runtime-falling"
out.mkdir(parents=True, exist_ok=True)
sources = [ROOT / "tools/java" / name for name in
           ("VanillaWorldBridge.java", "BlockTickOracle.java", "RuntimeFallingOracle.java")]
subprocess.run(["javac", "--release", "8", "-cp", cp, "-d", str(out), *map(str, sources)], check=True)
subprocess.run([java_executable(), "-Xmx512m", "-cp", str(out) + os.pathsep + cp,
                "RuntimeFallingOracle", str(ROOT / "tests/fixtures/vanilla189-runtime-falling.json.gz"),
                str(out / "world")], cwd=ROOT, check=True)
