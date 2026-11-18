"""Generate world-message and sign vectors with the installed protocol-47 packet classes."""
import os
from pathlib import Path
import subprocess
import tempfile

from vanilla_world import ROOT, classpath, java_executable


def main():
    cp = classpath(Path(os.environ["APPDATA"])/".minecraft/versions/1.8.9/1.8.9.jar")
    javac = os.environ.get("MC189_JAVAC", r"C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe")
    with tempfile.TemporaryDirectory(prefix="play-events-") as work:
        subprocess.run([javac, "-encoding", "UTF-8", "-cp", cp, "-d", work,
                        ROOT/"tools/java/PlayEventsOracle.java"], cwd=work, check=True)
        subprocess.run([java_executable(), "-cp", work+os.pathsep+cp, "PlayEventsOracle",
                        ROOT/"tests/fixtures/vanilla189-play-events.json"], cwd=work, check=True)


if __name__ == "__main__":
    main()
