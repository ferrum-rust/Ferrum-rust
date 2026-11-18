"""Real WorldServer spring oracle; never called by the Rust game."""
import os
from pathlib import Path
import subprocess
import time
from vanilla_world import ROOT, classpath, java_executable
jar=Path(os.environ['APPDATA'])/'.minecraft/versions/1.8.9/1.8.9.jar'
cp=classpath(jar)
classes=ROOT/'.local/spring-oracle/classes';classes.mkdir(parents=True,exist_ok=True)
subprocess.run(['javac','-encoding','UTF-8','-source','8','-target','8','-cp',cp,'-d',str(classes),*[str(ROOT/'tools/java'/name) for name in ['VanillaWorldBridge.java','BlockTickOracle.java','SpringOracle.java']]],check=True,cwd=ROOT)
subprocess.run([java_executable(),'-Xmx1G','-Xss8m','-Djava.awt.headless=true','-cp',str(classes)+os.pathsep+cp,'SpringOracle',str(ROOT/'tests/fixtures/vanilla189-springs.json'),str(ROOT/'.local/spring-oracle'/str(time.time_ns()))],check=True,cwd=ROOT)
