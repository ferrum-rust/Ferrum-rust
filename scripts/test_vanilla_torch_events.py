"""Original ordinary torch placement/neighbor support callbacks."""
import os
from pathlib import Path
import subprocess
from vanilla_world import ROOT, classpath, java_executable

cp = classpath(Path(os.environ['APPDATA']) / '.minecraft/versions/1.8.9/1.8.9.jar')
classes = ROOT / '.local/torch-oracle/classes'
classes.mkdir(parents=True, exist_ok=True)
subprocess.run(['javac', '-encoding', 'UTF-8', '-source', '8', '-target', '8', '-cp', cp,
                '-d', str(classes), *[str(ROOT / 'tools/java' / name) for name in ['PlantEventOracle.java','TorchEventOracle.java']]], check=True, cwd=ROOT)
subprocess.run([java_executable(), '-Xmx1G', '-cp', str(classes) + os.pathsep + cp, 'TorchEventOracle',
                str(ROOT / 'tests/fixtures/vanilla189-torch-events.json')], check=True, cwd=ROOT)
