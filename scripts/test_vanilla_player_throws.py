"""Capture actual EntityPlayer.dropOneItem and Container.slotClick results from the installed 1.8.9."""
import os
from pathlib import Path
import subprocess
import tempfile
from vanilla_world import ROOT, classpath, java_executable

jar=Path(os.environ["APPDATA"])/".minecraft/versions/1.8.9/1.8.9.jar"
cp=classpath(jar)
with tempfile.TemporaryDirectory(prefix="mc189-player-throw-") as tmp:
    sources=[ROOT/"tools/java"/(name+".java") for name in ("VanillaWorldBridge","BlockTickOracle","RuntimeItemOracle","PlayerThrowOracle")]
    javac=os.environ.get("MC189_JAVAC",r"C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe")
    subprocess.run([javac,"-encoding","UTF-8","-cp",cp,"-d",tmp,*map(str,sources)],check=True)
    subprocess.run([java_executable(),"-Xmx256m","-cp",tmp+os.pathsep+cp,"PlayerThrowOracle",
                    str(ROOT/"tests/fixtures/vanilla189-player-throws.json.gz"),str(Path(tmp)/"world")],cwd=tmp,check=True,timeout=80)
