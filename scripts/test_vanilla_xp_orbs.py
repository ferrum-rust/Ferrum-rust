"""Export EntityXPOrb.onUpdate, pickup and EntityPlayer.addExperience from 1.8.9."""
import os
import subprocess
from pathlib import Path
from vanilla_world import ROOT, classpath, java_executable

cp = classpath(Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar")
out = ROOT / ".local/oracles/xp-orbs"
out.mkdir(parents=True, exist_ok=True)
sources = [ROOT / "tools/java" / name for name in (
    "FallingEntityOracle.java", "ItemEntityOracle.java", "ItemMotionOracle.java", "XpOrbOracle.java"
)]
subprocess.run(["javac", "--release", "8", "-cp", cp, "-d", str(out), *map(str, sources)], check=True)
subprocess.run([java_executable(), "-Xmx512m", "-cp", str(out) + os.pathsep + cp, "XpOrbOracle",
                str(ROOT / "tests/fixtures/vanilla189-xp-orbs.json.gz")], cwd=ROOT, check=True)
