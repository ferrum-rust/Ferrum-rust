"""Export C08 server interactions using the installed original client bytecode."""
import gzip
import json
import os
import subprocess
from pathlib import Path

from vanilla_world import ROOT, classpath, java_executable

cp = classpath(Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar")
out = ROOT / ".local/oracles/runtime-use"
out.mkdir(parents=True, exist_ok=True)
sources = [ROOT / "tools/java" / name for name in ("VanillaWorldBridge.java", "RuntimeUseOracle.java")]
subprocess.run(["javac", "--release", "8", "-cp", cp, "-d", str(out), *map(str, sources)], check=True)
fixture = ROOT / "tests/fixtures/vanilla189-runtime-use.json.gz"
subprocess.run([java_executable(), "-Xmx512m", "-cp", str(out) + os.pathsep + cp,
                "RuntimeUseOracle", str(fixture), str(out / "world")], cwd=ROOT, check=True)
registry = json.load(gzip.open(fixture, "rt"))
ids = registry["overrides"]
sounds = [f"({json.dumps(name)}, {volume!r}, {pitch!r})" for name, volume, pitch in registry["sounds"]]
(ROOT / "src/vanilla/runtime_activation_data.rs").write_text(
    "// Original registry classes overriding Block.onBlockActivated.\n"
    + "const BLOCK_ACTIVATION: &[u16] = &" + str(ids) + ";\n"
    + "const PLACE_SOUND: [(&str,f32,f32);256] = [\n" + ",\n".join(sounds) + "\n];\n"
    + "const USE_STATS: [&str;256] = [\n" + ",\n".join(map(json.dumps,registry["stats"])) + "\n];\n",
    encoding="utf-8")
