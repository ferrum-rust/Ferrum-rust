"""Original baked floor/wall torch vertices; uses the owner's resource pack."""
import os
from pathlib import Path
import subprocess
from vanilla_world import ROOT, classpath, java_executable

cp = classpath(Path(os.environ['APPDATA']) / '.minecraft/versions/1.8.9/1.8.9.jar')
classes = ROOT / '.local/oracles/torch-model'
classes.mkdir(parents=True, exist_ok=True)
subprocess.run(['javac', '--release', '8', '-cp', cp, '-d', str(classes),
                str(ROOT / 'tools/java/TorchModelOracle.java'),
                str(ROOT / 'tools/java/BedModelOracle.java'),
                str(ROOT / 'tools/java/PaneModelOracle.java'),
                str(ROOT / 'tools/java/FireModelOracle.java'),
                str(ROOT / 'tools/java/GrassModelOracle.java')], check=True, cwd=ROOT)
subprocess.run([java_executable(), '-cp', str(classes) + os.pathsep + cp, 'TorchModelOracle',
                str(ROOT / '.local/resources/assets/minecraft'),
                str(ROOT / 'tests/fixtures/vanilla189-torch-model.json')], check=True, cwd=ROOT)
subprocess.run([java_executable(), '-cp', str(classes) + os.pathsep + cp, 'BedModelOracle',
                str(ROOT / '.local/resources/assets/minecraft'),
                str(ROOT / 'tests/fixtures/vanilla189-bed-model.json')], check=True, cwd=ROOT)
subprocess.run([java_executable(), '-cp', str(classes) + os.pathsep + cp, 'PaneModelOracle',
                str(ROOT / '.local/resources/assets/minecraft'),
                str(ROOT / 'tests/fixtures/vanilla189-pane-model.json')], check=True, cwd=ROOT)
subprocess.run([java_executable(), '-cp', str(classes) + os.pathsep + cp, 'GrassModelOracle',
                str(ROOT / '.local/resources/assets/minecraft'),
                str(ROOT / 'tests/fixtures/vanilla189-grass-model.json')], check=True, cwd=ROOT)
subprocess.run([java_executable(), '-cp', str(classes) + os.pathsep + cp, 'FireModelOracle',
                str(ROOT / '.local/resources/assets/minecraft'),
                str(ROOT / 'tests/fixtures/vanilla189-fire-models.json.gz')], check=True, cwd=ROOT)
