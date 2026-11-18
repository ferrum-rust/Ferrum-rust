"""Export original EntityItem.onUpdate motion and all observed callbacks."""
import os
import subprocess
from pathlib import Path
from vanilla_world import ROOT,classpath,java_executable

cp=classpath(Path(os.environ["APPDATA"])/".minecraft/versions/1.8.9/1.8.9.jar")
out=ROOT/".local/oracles/item-motion";out.mkdir(parents=True,exist_ok=True)
sources=[ROOT/"tools/java"/s for s in ("FallingEntityOracle.java","ItemEntityOracle.java","ItemMotionOracle.java")]
subprocess.run(["javac","--release","8","-cp",cp,"-d",str(out),*map(str,sources)],check=True)
subprocess.run([java_executable(),"-Xmx512m","-cp",str(out)+os.pathsep+cp,"ItemMotionOracle",str(ROOT/"tests/fixtures/vanilla189-item-motion.json.gz")],cwd=ROOT,check=True)
