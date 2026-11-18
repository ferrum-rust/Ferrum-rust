"""Export actual BlockFire/BlockPortal state, RNG and scheduler observations."""
import os
import subprocess
from pathlib import Path

from vanilla_world import ROOT, classpath, java_executable

cp = classpath(Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar")
out = ROOT / ".local/oracles/fire-portal"
out.mkdir(parents=True, exist_ok=True)
sources = [ROOT / "tools/java" / name for name in
           ("VanillaWorldBridge.java","RuntimeUseOracle.java","FirePortalOracle.java")]
subprocess.run(["javac","--release","8","-cp",cp,"-d",str(out),*map(str,sources)],check=True)
subprocess.run([java_executable(),"-Xmx512m","-cp",str(out)+os.pathsep+cp,"FirePortalOracle",
                str(ROOT/"tests/fixtures/vanilla189-fire-portal.json.gz"),str(out/"world")],cwd=ROOT,check=True)
