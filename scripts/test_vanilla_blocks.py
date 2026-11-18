"""Run original Java scheduled blocks as a test oracle; native gameplay never starts Java."""
import os
from pathlib import Path
import subprocess
import time
from vanilla_world import ROOT, classpath

jar=Path(os.environ['APPDATA'])/'.minecraft/versions/1.8.9/1.8.9.jar'
cp=classpath(jar)
classes=ROOT/'.local/block-oracle/classes';classes.mkdir(parents=True,exist_ok=True)
fixture=ROOT/'tests/fixtures/vanilla189-blocks.json'
worlds=ROOT/'.local/block-oracle'/str(time.time_ns())
subprocess.run(['javac','-encoding','UTF-8','-source','8','-target','8','-cp',cp,'-d',str(classes),str(ROOT/'tools/java/VanillaWorldBridge.java'),str(ROOT/'tools/java/BlockTickOracle.java')],check=True,cwd=ROOT)
subprocess.run(['java','-Xmx1G','-Djava.awt.headless=true','-cp',str(classes)+os.pathsep+cp,'BlockTickOracle',str(fixture),str(worlds)],check=True,cwd=ROOT)
