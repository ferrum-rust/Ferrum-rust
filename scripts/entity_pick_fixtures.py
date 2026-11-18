"""Execute the original 1.8.9 client target selector without creating an OpenGL window."""
import json
import os
from pathlib import Path
import subprocess
from vanilla_world import classpath, java_executable

ROOT = Path(__file__).resolve().parents[1]
out = ROOT / ".local/entity-pick-oracle"
out.mkdir(parents=True, exist_ok=True)
cp = classpath(Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar")
compiler = os.environ.get("MC189_JAVAC", r"C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe")
subprocess.run([compiler, "-encoding", "UTF-8", "-cp", cp, "-d", str(out),
                str(ROOT / "tools/java/MovementOracle.java"),
                str(ROOT / "tools/java/EntityPickOracle.java"),
                str(ROOT / "tools/java/EntityPickPacketsOracle.java")], check=True)
def run(name):
    result = subprocess.run([java_executable(), "-Djava.awt.headless=true", "-Xmx512M",
                             "-cp", str(out)+os.pathsep+cp, name],
                            capture_output=True, text=True)
    if result.returncode:
        print(result.stderr)
        result.check_returncode()
    return json.loads(result.stdout)

data = run("EntityPickOracle")
packets = run("EntityPickPacketsOracle")
for name, content in [("entity-pick", data), ("entity-pick-packets", packets)]:
    (ROOT / f"tests/fixtures/vanilla189-{name}.json").write_text(
        json.dumps(content, separators=(",", ":"))+"\n", encoding="utf-8")
from collections import Counter
print(len(data), Counter(row["kind"] for row in data))
