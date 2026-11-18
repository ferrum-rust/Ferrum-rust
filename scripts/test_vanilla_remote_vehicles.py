"""Capture the original 1.8.9 client vehicle trajectories, seats and horseback jump actions."""
from pathlib import Path
import os
import subprocess
import tempfile

from vanilla_world import ROOT, classpath, java_executable


def main():
    cp = classpath(Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar")
    javac = os.environ.get("MC189_JAVAC", r"C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe")
    with tempfile.TemporaryDirectory(prefix="vehicle-") as work:
        sources = [ROOT / "tools/java" / (name + ".java") for name in
                   ("MovementOracle", "RemoteVehicleOracle")]
        subprocess.run([javac, "-encoding", "UTF-8", "-cp", cp, "-d", work, *sources], cwd=work, check=True)
        subprocess.run([java_executable(), "-cp", work + os.pathsep + cp, "RemoteVehicleOracle",
                        ROOT / "tests/fixtures/vanilla189-remote-vehicles.json", work], cwd=work, check=True)


if __name__ == "__main__":
    main()
