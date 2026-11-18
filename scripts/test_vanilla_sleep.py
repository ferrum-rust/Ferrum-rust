"""Compare original surface bed activation, safety checks, wake positions and use."""
import os
import subprocess
from pathlib import Path
from vanilla_world import ROOT, classpath, java_executable

cp = classpath(Path(os.environ['APPDATA']) / '.minecraft/versions/1.8.9/1.8.9.jar')
out = ROOT / '.local/oracles/sleep'
out.mkdir(parents=True, exist_ok=True)
sources = [ROOT / 'tools/java' / f'{n}.java' for n in ('VanillaWorldBridge','PlayerPoseOracle','SleepOracle')]
subprocess.run(['javac','--release','8','-cp',cp,'-d',str(out),*map(str,sources)],check=True)
subprocess.run([java_executable(),'-cp',str(out)+os.pathsep+cp,'SleepOracle',str(out),
                str(ROOT/'tests/fixtures/vanilla189-sleep.json')],check=True)
