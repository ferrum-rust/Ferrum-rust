"""Original WorldServer scheduled queue, using an inert dispatch probe."""
import os
from pathlib import Path
import subprocess
from vanilla_world import ROOT, classpath, java_executable

cp = classpath(Path(os.environ['APPDATA']) / '.minecraft/versions/1.8.9/1.8.9.jar')
classes = ROOT / '.local/scheduled-tick-oracle/classes'
classes.mkdir(parents=True, exist_ok=True)
subprocess.run([os.environ.get('MC189_JAVAC', 'javac'), '-encoding', 'UTF-8', '-source', '8', '-target', '8', '-cp', cp,
               '-d', str(classes), str(ROOT / 'tools/java/ScheduledTickOracle.java')], check=True, cwd=ROOT)
subprocess.run([java_executable(), '-cp', str(classes) + os.pathsep + cp, 'ScheduledTickOracle',
                str(ROOT / 'tests/fixtures/vanilla189-scheduled-ticks.json')], check=True, cwd=ROOT)
