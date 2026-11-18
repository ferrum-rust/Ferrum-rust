"""Read original RenderGlobal celestial vertex streams through an offscreen OpenGL context."""
import os
import subprocess
from pathlib import Path
from vanilla_world import ROOT, classpath, java_executable

version = Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9"
cp = classpath(version / "1.8.9.jar")
out = ROOT / ".local/oracles/celestial"
out.mkdir(parents=True, exist_ok=True)
subprocess.run(["javac", "-source", "8", "-target", "8", "-cp", cp, "-d", str(out),
                str(ROOT / "tools/java/CelestialOracle.java")], check=True)
natives = Path(os.environ.get("MC189_NATIVES", version / "natives-windows-x86_64"))
subprocess.run([java_executable(), f"-Djava.library.path={natives}", "-cp", str(out) + os.pathsep + cp,
                "CelestialOracle", str(ROOT / "tests/fixtures/vanilla189-celestial.json.gz")], check=True, cwd=ROOT)
