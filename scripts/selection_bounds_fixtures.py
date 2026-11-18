"""Regenerate crosshair outline and walk-through-block ray witnesses using the local 1.8.9 JAR."""
import gzip
import json
import os
from pathlib import Path
import subprocess
import tempfile

from vanilla_world import ROOT, classpath, java_executable


def main():
    cp = classpath(Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar")
    javac = os.environ.get("MC189_JAVAC", r"C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe")
    with tempfile.TemporaryDirectory(prefix="selection-bounds-") as work:
        sources = [ROOT / "tools/java" / (name + ".java") for name in ("CollisionShapeOracle", "SelectionBoundsOracle")]
        subprocess.run([javac, "-encoding", "UTF-8", "-cp", cp, "-d", work, *sources], check=True, cwd=work)
        data = subprocess.check_output([java_executable(), "-Xmx512M", "-cp", work + os.pathsep + cp,
                                        "SelectionBoundsOracle"], cwd=work)
    outlines, narrow = json.loads(data)
    path = ROOT / "tests/fixtures/vanilla189-selected-bounds.json.gz"
    path.write_bytes(gzip.compress(data.strip(), mtime=0))
    print(f"{len(outlines)} selected boxes, {len(narrow)} collision lists, {sum(len(r[4]) for r in narrow)} picks")


if __name__ == "__main__":
    main()
