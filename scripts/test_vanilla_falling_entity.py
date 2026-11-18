"""Regenerate original EntityFallingBlock state/callback traces using Java 8."""
import os
from pathlib import Path
import subprocess
from vanilla_world import ROOT, classpath, java_executable

cp = classpath(Path(os.environ['APPDATA']) / '.minecraft/versions/1.8.9/1.8.9.jar')
classes = ROOT / '.local/falling-entity-oracle/classes'
classes.mkdir(parents=True, exist_ok=True)
subprocess.run([os.environ.get('MC189_JAVAC', 'javac'), '-encoding', 'UTF-8', '-source', '8', '-target', '8', '-cp', cp,
               '-d', str(classes), str(ROOT / 'tools/java/FallingEntityOracle.java'),
               str(ROOT / 'tools/java/FallingBlockOracle.java')], check=True, cwd=ROOT)
subprocess.run([java_executable(), '-cp', str(classes) + os.pathsep + cp, 'FallingEntityOracle',
                str(ROOT / 'tests/fixtures/vanilla189-falling-entity.json.gz')], check=True, cwd=ROOT)
subprocess.run([java_executable(), '-cp', str(classes) + os.pathsep + cp, 'FallingBlockOracle',
                str(ROOT / 'tests/fixtures/vanilla189-falling-block.json'),
                str(ROOT / 'tests/fixtures/vanilla189-falling-placement.json')], check=True, cwd=ROOT)
