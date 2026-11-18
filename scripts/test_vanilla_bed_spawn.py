"""Run the user's 1.8.9 EntityPlayer/BlockBed spawn queries on a controlled grid."""
import os
import subprocess
from pathlib import Path
from vanilla_world import ROOT, classpath, java_executable

cp = classpath(Path(os.environ['APPDATA']) / '.minecraft/versions/1.8.9/1.8.9.jar')
out = ROOT / '.local/oracles/bed-spawn'
out.mkdir(parents=True, exist_ok=True)
subprocess.run(['javac', '-source', '8', '-target', '8', '-cp', cp, '-d', str(out),
                str(ROOT / 'tools/java/BedSpawnOracle.java')], check=True)
subprocess.run([java_executable(), '-cp', str(out) + os.pathsep + cp, 'BedSpawnOracle',
                str(ROOT / 'tests/fixtures/vanilla189-bed-spawn.json')], check=True)
