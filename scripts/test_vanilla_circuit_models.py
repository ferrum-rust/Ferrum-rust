"""Regenerate original block-state mappings and circuit model witnesses using the user's 1.8.9 jar."""
import gzip
import os
import subprocess
import tempfile
from pathlib import Path
from vanilla_world import ROOT, classpath, java_executable


def main():
    jar = Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar"
    cp = classpath(jar)
    with tempfile.TemporaryDirectory() as directory:
        work = Path(directory)
        subprocess.run([os.environ.get("MC189_JAVAC", "javac"), "-encoding", "UTF-8", "-source", "8", "-target", "8", "-cp", cp, "-d", work,
                        ROOT / "tools/java/TorchModelOracle.java", ROOT / "tools/java/CircuitModelOracle.java"], cwd=work, check=True)
        mapping, quads = work / "states.json", work / "circuits.json"
        subprocess.run([java_executable(), "-cp", directory + os.pathsep + cp, "CircuitModelOracle",
                        ROOT / ".local/resources/assets/minecraft", mapping, quads], cwd=work, check=True)
        for source, target in [(mapping, "src/vanilla/block-model-states189.json.gz"), (quads, "tests/fixtures/vanilla189-circuit-models.json.gz")]:
            (ROOT / target).write_bytes(gzip.compress(source.read_bytes(), mtime=0))


if __name__ == "__main__":
    main()
