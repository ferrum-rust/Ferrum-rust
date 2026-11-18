"""Capture rod casts, Eye lifecycles and fishing loot from the installed 1.8.9 JAR."""
import os
from pathlib import Path
import subprocess
import tempfile

from vanilla_world import ROOT, classpath, java_executable

jar = Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar"
cp = classpath(jar)
with tempfile.TemporaryDirectory(prefix="mc189-fishing-eye-") as temporary:
    sources = [ROOT / "tools/java" / (name + ".java") for name in ("FoodOracle", "FishingEyeOracle")]
    javac = os.environ.get("MC189_JAVAC", r"C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe")
    subprocess.run([javac, "-encoding", "UTF-8", "-cp", cp, "-d", temporary, *map(str, sources)], check=True)
    subprocess.run([
        java_executable(), "-Xmx256m", "-cp", temporary + os.pathsep + cp,
        "FishingEyeOracle", str(ROOT / "tests/fixtures/vanilla189-fishing-eye.json"),
    ], cwd=temporary, check=True, timeout=60)
