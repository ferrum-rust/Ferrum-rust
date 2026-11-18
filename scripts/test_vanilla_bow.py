"""Export installed 1.8.9 ItemBow right-click/release and inventory outcomes."""
import gzip
import json
import os
from pathlib import Path
import subprocess

from vanilla_world import ROOT, classpath, java_executable


def main():
    jar = Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar"
    cp = classpath(jar)
    classes = ROOT / ".local/vanilla-bow-oracle/classes"
    classes.mkdir(parents=True, exist_ok=True)
    sources = [ROOT / "tools/java" / name for name in ("FoodOracle.java", "BowUseOracle.java")]
    subprocess.run(
        ["javac", "-encoding", "UTF-8", "-source", "8", "-target", "8",
         "-cp", cp, "-d", str(classes), *map(str, sources)],
        check=True,
    )
    result = subprocess.run(
        [java_executable(), "-Xmx512M", "-cp", str(classes) + os.pathsep + cp, "BowUseOracle"],
        capture_output=True, text=True, encoding="utf-8",
    )
    if result.returncode:
        print(result.stderr)
        result.check_returncode()
    cases = json.loads(result.stdout)
    target = ROOT / "tests/fixtures/vanilla189-bow-use.json.gz"
    target.write_bytes(gzip.compress(json.dumps(cases, separators=(",", ":")).encode(), mtime=0))
    print(f"Exported {len(cases)} original bow actions")


if __name__ == "__main__":
    main()
