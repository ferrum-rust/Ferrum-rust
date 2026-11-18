"""Compare the original player save position, velocity, rotation and abilities."""
import os
from pathlib import Path
import subprocess
from vanilla_world import ROOT, classpath, java_executable

cp = classpath(Path(os.environ['APPDATA']) / '.minecraft/versions/1.8.9/1.8.9.jar')
out = ROOT / '.local/oracles/player-pose'
out.mkdir(parents=True, exist_ok=True)
subprocess.run(['javac','--release','8','-cp',cp,'-d',str(out),
                str(ROOT/'tools/java/VanillaWorldBridge.java'),str(ROOT/'tools/java/PlayerPoseOracle.java')],check=True,cwd=ROOT)
subprocess.run([java_executable(),'-cp',str(out)+os.pathsep+cp,'PlayerPoseOracle',str(out),
                str(ROOT/'tests/fixtures/vanilla189-player-pose.json')],check=True,cwd=ROOT)
