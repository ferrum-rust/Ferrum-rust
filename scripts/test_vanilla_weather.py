"""Regenerate the overworld weather tick fixture from the local 1.8.9 JAR."""
import os
import subprocess
import tempfile
from pathlib import Path

from vanilla_world import ROOT, classpath, java_executable


def main():
    jar = Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar"
    dependencies = classpath(jar)
    javac = os.environ.get("MC189_JAVAC", r"C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe")
    with tempfile.TemporaryDirectory(prefix="weather-oracle-") as tmp:
        subprocess.run([javac, "-encoding", "UTF-8", "-cp", dependencies, "-d", tmp,
                        str(ROOT / "tools/java/WeatherOracle.java")], cwd=tmp, check=True)
        fixture = ROOT / "tests/fixtures/vanilla189-weather.json"
        subprocess.run([java_executable(), "-Xmx256M", "-cp", tmp + os.pathsep + dependencies,
                        "WeatherOracle", str(fixture)], cwd=tmp, check=True)


if __name__ == "__main__":
    main()
