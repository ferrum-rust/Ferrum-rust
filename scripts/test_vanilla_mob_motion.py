"""Export original EntityLiving server movement for four entity shapes."""
import os
import subprocess
from pathlib import Path
from vanilla_world import ROOT, classpath, java_executable

cp = classpath(Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar")
out = ROOT / ".local/oracles/mob-motion"; out.mkdir(parents=True, exist_ok=True)
sources = [ROOT / "tools/java" / n for n in ("VanillaWorldBridge.java", "FallingEntityOracle.java", "GroundPathOracle.java",
           "GroundNavigationOracle.java", "MobMoveOracle.java", "ItemEntityOracle.java", "ItemMotionOracle.java", "MobMotionOracle.java")]
subprocess.run(["javac", "--release", "8", "-cp", cp, "-d", str(out), *map(str, sources)], check=True)
subprocess.run([java_executable(), "-Xmx512m", "-cp", str(out) + os.pathsep + cp, "MobMotionOracle",
                str(ROOT / "tests/fixtures/vanilla189-mob-motion.json.gz"), str(out)], cwd=out, check=True)
