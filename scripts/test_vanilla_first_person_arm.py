"""Compare the right arm and sleeve, swing progress and partial-tick samples."""
import os
import subprocess
from pathlib import Path
from vanilla_world import ROOT, classpath, java_executable

cp = classpath(Path(os.environ['APPDATA']) / '.minecraft/versions/1.8.9/1.8.9.jar')
out = ROOT / '.local/oracles/first-person-arm'
out.mkdir(parents=True, exist_ok=True)
sources = [ROOT / 'tools/java' / f'{name}.java' for name in
           ('VanillaWorldBridge', 'PlayerPoseOracle', 'FirstPersonArmOracle')]
subprocess.run(['javac', '--release', '8', '-cp', cp, '-d', str(out), *map(str, sources)], check=True)
subprocess.run([java_executable(), '-cp', str(out) + os.pathsep + cp, 'FirstPersonArmOracle',
                str(out), str(ROOT / 'tests/fixtures/vanilla189-first-person-arm.json')], check=True)
subprocess.run(['javac','-source','8','-target','8','-cp',cp,'-d',str(out),
                str(ROOT/'tools/java/ArmTransformOracle.java')],check=True)
natives = Path(os.environ.get('MC189_NATIVES', Path(os.environ['APPDATA']) /
               '.minecraft/versions/1.8.9/natives-windows-x86_64'))
subprocess.run([java_executable(),f'-Djava.library.path={natives}', '-cp',str(out)+os.pathsep+cp,
                'ArmTransformOracle',str(ROOT/'tests/fixtures/vanilla189-arm-transform.json')],check=True)
