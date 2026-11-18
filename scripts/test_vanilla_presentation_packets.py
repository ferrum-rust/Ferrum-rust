"""Export the original 1.8.9 Play presentation packet payloads for Rust regression tests."""
import json
import os
from pathlib import Path
import subprocess
from vanilla_world import classpath, java_executable

root = Path(__file__).resolve().parents[1]
classes = root / ".local/presentation-packet-oracle"
classes.mkdir(parents=True, exist_ok=True)
cp = classpath(Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar")
javac = os.getenv("MC189_JAVAC", r"C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe")
subprocess.run([javac, "-encoding", "UTF-8", "-cp", cp, "-d", str(classes),
                str(root / "tools/java/PresentationPacketOracle.java")], check=True)
result = subprocess.run([java_executable(), "-Xmx512M", "-Djava.awt.headless=true", "-cp",
                         str(classes) + os.pathsep + cp, "PresentationPacketOracle"],
                        cwd=classes, check=True, capture_output=True, text=True)
packets = json.loads(result.stdout.splitlines()[-1])
(root / "tests/fixtures/vanilla189-presentation-packets.json").write_text(
    json.dumps(packets, separators=(",", ":")) + "\n", encoding="utf8")
print(f"{len(packets)} original packets")
