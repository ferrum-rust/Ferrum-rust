"""Export callback traces directly from the 1.8.9 EntityAITasks scheduler."""
import os
import subprocess
from pathlib import Path
from vanilla_world import ROOT, classpath, java_executable

cp = classpath(Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar")
out = ROOT / ".local/oracles/goal-selector"
out.mkdir(parents=True, exist_ok=True)
sources = [ROOT / "tools/java" / filename for filename in (
    "GoalSelectorOracle.java", "FallingEntityOracle.java", "ItemEntityOracle.java",
    "VillagerGoalsOracle.java", "RandomGoalOracle.java"
)]
subprocess.run(["javac", "--release", "8", "-cp", cp, "-d", str(out), *map(str, sources)], check=True)
subprocess.run([java_executable(), "-Xmx512m", "-cp", str(out) + os.pathsep + cp, "GoalSelectorOracle",
                str(ROOT / "tests/fixtures/vanilla189-goal-selector.json.gz")], cwd=ROOT, check=True)
subprocess.run([java_executable(), "-Xmx512m", "-cp", str(out) + os.pathsep + cp, "VillagerGoalsOracle",
                str(ROOT / "tests/fixtures/vanilla189-villager-goals.json.gz")], cwd=ROOT, check=True)
subprocess.run([java_executable(), "-Xmx512m", "-cp", str(out) + os.pathsep + cp, "RandomGoalOracle",
                str(ROOT / "tests/fixtures/vanilla189-random-goals.json.gz")], cwd=ROOT, check=True)
