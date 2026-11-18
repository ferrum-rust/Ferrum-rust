"""Extract banner placement, original ModelBox UVs, and composited pixels from the local 1.8.9 client."""
import base64
import gzip
import json
import os
from pathlib import Path
import subprocess
import tempfile

from vanilla_world import ROOT, classpath, java_executable


def main():
    mc = Path(os.environ["APPDATA"]) / ".minecraft"
    version = mc / "versions/1.8.9"
    cp = classpath(version / "1.8.9.jar")
    javac = os.environ.get("MC189_JAVAC", r"C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe")
    with tempfile.TemporaryDirectory(prefix="banner189-") as folder:
        work = Path(folder)
        game = work / "game"
        game.mkdir()
        names = ("VanillaFrameBenchmark", "MenuOracle", "VanillaWorldBridge", "RuntimeUseOracle", "BannerOracle")
        subprocess.run([javac, "-encoding", "UTF-8", "-cp", cp, "-d", work, *[
            ROOT / "tools/java" / (name + ".java") for name in names]], cwd=work, check=True)
        native = version / "natives-windows-x86_64"
        output = work / "banner.json"
        subprocess.run([java_executable(), "-Xmx640M", f"-Djava.library.path={native}",
                        f"-Dorg.lwjgl.librarypath={native}", f"-Duser.home={work}",
                        "-cp", folder + os.pathsep + cp, "BannerOracle", game, mc / "assets", output],
                       cwd=work, check=True, timeout=90)
        data = json.loads(output.read_bytes())
        data["rgba"] = list(base64.b64decode(data["rgba"]))
        target = ROOT / "tests/fixtures/vanilla189-banner.json.gz"
        target.write_bytes(gzip.compress(json.dumps(data, separators=(",", ":")).encode(), mtime=0))


if __name__ == "__main__":
    main()
