"""Regenerate farm growth and bone meal results directly from the installed Minecraft 1.8.9 JAR."""
import gzip
import os
from pathlib import Path
import subprocess
import tempfile
from vanilla_world import ROOT, classpath, java_executable


def main():
    cp = classpath(Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar")
    javac = os.environ.get("MC189_JAVAC", r"C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe")
    with tempfile.TemporaryDirectory(prefix="growth-oracle-") as directory:
        work = Path(directory)
        sources = [ROOT / "tools/java" / (name + ".java") for name in ("PlantEventOracle", "CropGrowthOracle")]
        subprocess.run([javac, "-encoding", "UTF-8", "-cp", cp, "-d", work, *sources], check=True)
        data = work / "growth.json"
        subprocess.run([java_executable(), "-Xmx512m", "-cp", directory + os.pathsep + cp,
                        "CropGrowthOracle", data], check=True, cwd=directory)
        (ROOT / "tests/fixtures/vanilla189-crop-growth.json.gz").write_bytes(gzip.compress(data.read_bytes(), mtime=0))


if __name__ == "__main__":
    main()
