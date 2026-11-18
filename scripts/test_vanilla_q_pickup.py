"""Regenerate Q/Control-Q entity birth, throw, pickup collision and lifetime traces with the local 1.8.9 classes."""
import gzip
import os
import subprocess
import tempfile
from pathlib import Path

from vanilla_world import ROOT, classpath, java_executable


def main():
    cp = classpath(Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar")
    javac = os.environ.get("MC189_JAVAC", r"C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe")
    with tempfile.TemporaryDirectory(prefix="q-pickup-") as name:
        work = Path(name)
        sources = [ROOT / "tools/java" / (n + ".java") for n in
                   ("VanillaWorldBridge", "RuntimeUseOracle", "QPickupOracle")]
        subprocess.run([javac, "-encoding", "UTF-8", "-cp", cp, "-d", work, *sources], cwd=work, check=True)
        output = work / "q-pickup.json"
        subprocess.run([java_executable(), "-Xmx400M", "-cp", name + os.pathsep + cp, "QPickupOracle",
                        output, work], cwd=work, check=True)
        (ROOT / "tests/fixtures/vanilla189-q-pickup.json.gz").write_bytes(gzip.compress(output.read_bytes(), mtime=0))


if __name__ == "__main__":
    main()
