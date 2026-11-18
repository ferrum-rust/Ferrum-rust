"""Collect original server S08 bytes and movement admission decisions."""
import os
import subprocess
from pathlib import Path
from vanilla_world import ROOT, classpath, java_executable

cp = classpath(Path(os.environ['APPDATA']) / '.minecraft/versions/1.8.9/1.8.9.jar')
out = ROOT / '.local/oracles/server-teleport'
out.mkdir(parents=True, exist_ok=True)
subprocess.run(['javac', '--release', '8', '-cp', cp, '-d', str(out),
                str(ROOT / 'tools/java/VanillaWorldBridge.java'),
                str(ROOT / 'tools/java/ServerTeleportOracle.java')], check=True)
subprocess.run([java_executable(), '-cp', str(out) + os.pathsep + cp, 'ServerTeleportOracle',
                str(out), str(ROOT / 'tests/fixtures/vanilla189-server-teleport.json')], check=True)
