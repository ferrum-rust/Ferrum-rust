"""Export actual client lightmap calculations without creating an OpenGL window."""
import os
import sys
from pathlib import Path
import subprocess
from vanilla_world import ROOT, classpath, java_executable

cp = classpath(Path(os.environ['APPDATA']) / '.minecraft/versions/1.8.9/1.8.9.jar')
classes = ROOT / '.local/lightmap-oracle/classes'
classes.mkdir(parents=True, exist_ok=True)
subprocess.run([os.environ.get('MC189_JAVAC', 'javac'), '-encoding', 'UTF-8', '-source', '8', '-target', '8', '-cp', cp,
                '-d', str(classes), str(ROOT / 'tools/java/LightmapOracle.java')], check=True, cwd=ROOT)
if '--flicker-only' not in sys.argv:
    subprocess.run([java_executable(), '-Xmx1G', '-cp', str(classes) + os.pathsep + cp, 'LightmapOracle',
                    str(ROOT / 'tests/fixtures/vanilla189-lightmap.json')], check=True, cwd=ROOT)
subprocess.run([java_executable(), '-cp', str(classes) + os.pathsep + cp, 'LightmapOracle', '--flicker',
                str(ROOT / 'tests/fixtures/vanilla189-torch-flicker.json')], check=True, cwd=ROOT)
