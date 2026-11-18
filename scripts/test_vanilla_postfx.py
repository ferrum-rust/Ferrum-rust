"""Export two small original framebuffer captures for every Super Secret Settings graph."""
import os
import subprocess
import tempfile
from pathlib import Path

from vanilla_world import ROOT, classpath, java_executable


def main():
    version = Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9"
    cp = classpath(version / "1.8.9.jar")
    javac = os.environ.get("MC189_JAVAC", r"C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe")
    with tempfile.TemporaryDirectory(prefix="post-oracle-") as tmp:
        subprocess.run([javac, "-cp", cp, "-d", tmp, str(ROOT/"tools/java/PostEffectsOracle.java")],
                       cwd=tmp, check=True)
        output = Path(tmp)/"post-effects.rgb.gz"
        subprocess.run([java_executable(), "-Xmx256M", f"-Djava.library.path={version/'natives-windows-x86_64'}",
                        "-cp", tmp+os.pathsep+cp, "PostEffectsOracle", str(output)], cwd=tmp, check=True)
        (ROOT/"tests/fixtures/vanilla189-post-effects.rgb.gz").write_bytes(output.read_bytes())


if __name__ == "__main__":
    main()
