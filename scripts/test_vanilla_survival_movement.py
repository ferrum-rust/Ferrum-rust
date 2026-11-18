"""Export original movement, fall damage and hunger during integrated survival ticks."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

from vanilla_world import ROOT, classpath, java_executable


def main():
    jar = Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar"
    cp = classpath(jar)
    javac = os.environ.get("MC189_JAVAC", r"C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe")
    with tempfile.TemporaryDirectory(prefix="survival-motion-") as folder:
        sources = [str(ROOT / "tools/java" / name) for name in (
            "MovementOracle.java", "FoodOracle.java", "SurvivalMovementOracle.java")]
        subprocess.run([javac, "-encoding", "UTF-8", "-cp", cp, "-d", folder, *sources],
                       cwd=folder, check=True)
        command = [java_executable(), "-Djava.awt.headless=true", "-cp",
                   folder + os.pathsep + cp, "SurvivalMovementOracle"]
        output = subprocess.run(command,
                                cwd=folder, check=True, stdout=subprocess.PIPE)
        rows = json.loads(output.stdout)
        melee = subprocess.run([*command,"melee"], cwd=folder, check=True, stdout=subprocess.PIPE)
        (ROOT / "tests/fixtures/vanilla189-survival-melee.json").write_text(
            json.dumps(json.loads(melee.stdout), separators=(",",":"))+"\n",encoding="utf-8")
    path = ROOT / "tests/fixtures/vanilla189-survival-movement.json"
    path.write_text(json.dumps(rows, separators=(",", ":"))+"\n", encoding="utf-8")
    print(f"{len(rows)} movement cases and {sum(row[6] for row in rows)} ticks")


if __name__ == "__main__":
    main()
