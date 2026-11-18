"""Export original World and private EntityRenderer fog colors/modes on a hidden LWJGL PBuffer."""

from pathlib import Path
import os
import subprocess
import tempfile
from vanilla_world import ROOT, classpath, java_executable

version = Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9"
cp = classpath(version / "1.8.9.jar")
with tempfile.TemporaryDirectory(prefix="atmosphere-oracle-") as tmp:
    compiler = os.environ.get("MC189_JAVAC", r"C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe")
    subprocess.run(
        [
            compiler,
            "-encoding",
            "UTF-8",
            "-cp",
            cp,
            "-d",
            tmp,
            str(ROOT / "tools/java/LightmapOracle.java"),
            str(ROOT / "tools/java/AtmosphereOracle.java"),
        ],
        check=True,
        cwd=tmp,
    )
    jar = version / "natives-windows-x86_64"
    subprocess.run(
        [
            java_executable(),
            "-Xmx256M",
            f"-Djava.library.path={jar}",
            "-cp",
            tmp + os.pathsep + cp,
            "AtmosphereOracle",
            str(ROOT / "tests/fixtures/vanilla189-atmosphere.json.gz"),
        ],
        check=True,
        cwd=tmp,
    )
