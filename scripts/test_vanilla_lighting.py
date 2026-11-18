"""Capture original WorldServer lighting; no Java in the game runtime."""
import os
from pathlib import Path
import subprocess
import time
from vanilla_world import ROOT, classpath, java_executable

cp = classpath(Path(os.environ['APPDATA']) / '.minecraft/versions/1.8.9/1.8.9.jar')
classes = ROOT / '.local/lighting-oracle/classes'
classes.mkdir(parents=True, exist_ok=True)
subprocess.run(['javac', '-encoding', 'UTF-8', '-source', '8', '-target', '8', '-cp', cp, '-d', str(classes),
                *[str(ROOT / 'tools/java' / name) for name in ['VanillaWorldBridge.java', 'BlockTickOracle.java', 'LightingOracle.java']]], check=True, cwd=ROOT)
subprocess.run([java_executable(), '-Xmx2G', '-Djava.awt.headless=true', '-cp', str(classes) + os.pathsep + cp,
                'LightingOracle', str(ROOT / 'tests/fixtures/vanilla189-lighting.json'),
                str(ROOT / '.local/lighting-oracle' / str(time.time_ns()))], check=True, cwd=ROOT)
