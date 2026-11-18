"""Regenerate keyboard names, original bindings and duplicate-key behavior from the local JAR."""
import os
import subprocess
import tempfile
from pathlib import Path

from vanilla_world import ROOT, classpath, java_executable


def main():
    version = Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9"
    dependencies = classpath(version / "1.8.9.jar")
    javac = os.environ.get("MC189_JAVAC", r"C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe")
    with tempfile.TemporaryDirectory(prefix="controls-oracle-") as tmp:
        subprocess.run([javac, "-encoding", "UTF-8", "-cp", dependencies, "-d", tmp,
                        str(ROOT / "tools/java/ControlsOracle.java")], cwd=tmp, check=True)
        subprocess.run([java_executable(), "-Xmx256M", f"-Djava.library.path={version/'natives-windows-x86_64'}",
                        "-cp", tmp + os.pathsep + dependencies, "ControlsOracle",
                        str(ROOT / "tests/fixtures/vanilla189-controls.json")], cwd=tmp, check=True)


if __name__ == "__main__":
    main()
