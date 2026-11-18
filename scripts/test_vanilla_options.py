"""Capture the original 1.8.9 widget layouts without starting a visible client."""
import os
from pathlib import Path
import subprocess
import tempfile

from vanilla_world import ROOT, classpath, java_executable


def main():
    jar = Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar"
    cp = classpath(jar)
    javac = os.environ.get("MC189_JAVAC", r"C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe")
    with tempfile.TemporaryDirectory(prefix="options-oracle-") as tmp:
        subprocess.run([javac, "-encoding", "UTF-8", "-cp", cp, "-d", tmp,
                        str(ROOT / "tools/java/OptionsLayoutOracle.java")], cwd=tmp, check=True)
        subprocess.run([java_executable(), "-Xmx256M", "-cp", tmp + os.pathsep + cp, "OptionsLayoutOracle",
                        str(ROOT / "tests/fixtures/vanilla189-options-layout.json")], cwd=tmp, check=True)


if __name__ == "__main__":
    main()
