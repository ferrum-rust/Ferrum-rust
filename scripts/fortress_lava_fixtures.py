"""Verify the fortress entrance using original WorldServer and block callbacks."""
import os
from pathlib import Path
import subprocess
from vanilla_world import classpath
ROOT=Path(__file__).resolve().parents[1]
jar=Path(os.environ['APPDATA'])/'.minecraft/versions/1.8.9/1.8.9.jar'
classes=ROOT/'.local/fortress-oracle';classes.mkdir(parents=True,exist_ok=True)
worlds=ROOT/'.local/fortress-lava-worlds';worlds.mkdir(parents=True,exist_ok=True)
cp=classpath(jar)
sources=[ROOT/'tools/java'/n for n in ['VanillaWorldBridge.java','BlockTickOracle.java','FortressLavaOracle.java']]
subprocess.run(['javac','-encoding','UTF-8','-cp',cp,'-d',str(classes),*map(str,sources)],check=True)
subprocess.run(['java','-Xmx768M','-Djava.awt.headless=true','-cp',str(classes)+os.pathsep+cp,'FortressLavaOracle',str(ROOT/'tests/fixtures/vanilla189-fortress-lava.json'),str(worlds)],check=True)
