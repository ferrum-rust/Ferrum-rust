"""Original Nether spring execution in WorldServer, not a mocked callback."""
import os
from pathlib import Path
import subprocess
from vanilla_world import classpath
JAVA = os.environ.get('MC189_JAVA', r'C:\Program Files\Java\jre1.8.0_501\bin\java.exe')
JAVAC = os.environ.get('MC189_JAVAC', r'C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe')
ROOT=Path(__file__).resolve().parents[1]
classes=ROOT/'.local/nether-lava-oracle';classes.mkdir(parents=True,exist_ok=True)
worlds=ROOT/'.local/nether-lava-worlds';worlds.mkdir(parents=True,exist_ok=True)
cp=classpath(Path(os.environ['APPDATA'])/'.minecraft/versions/1.8.9/1.8.9.jar')
sources=[ROOT/'tools/java'/n for n in ['VanillaWorldBridge.java','BlockTickOracle.java','DimensionTerrainOracle.java','NetherLavaOracle.java']]
subprocess.run([JAVAC,'-encoding','UTF-8','-cp',cp,'-d',str(classes),*map(str,sources)],check=True)
subprocess.run([JAVA,'-Xmx768M','-Djava.awt.headless=true','-cp',str(classes)+os.pathsep+cp,'NetherLavaOracle',str(ROOT/'tests/fixtures/vanilla189-nether-lava.json'),str(worlds)],check=True)
