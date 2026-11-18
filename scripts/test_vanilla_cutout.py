"""Export the original non-mipmapped block layer from the installed 1.8.9 JAR."""
import os
import subprocess
from pathlib import Path
from vanilla_world import ROOT, classpath, java_executable

cp = classpath(Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar")
out = ROOT / ".local/oracles/cutout-layer"
out.mkdir(parents=True, exist_ok=True)
subprocess.run(["javac", "--release", "8", "-cp", cp, "-d", str(out),
                str(ROOT / "tools/java/CutoutLayerOracle.java")], check=True)
subprocess.run([java_executable(), "-cp", str(out) + os.pathsep + cp,
                "CutoutLayerOracle", str(ROOT / "tests/fixtures/vanilla189-cutout-blocks.json")],
               check=True)
