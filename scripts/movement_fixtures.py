"""Run actual original EntityPlayerSP movement using the pinned Java 8 helper."""
import json
import os
from pathlib import Path
import subprocess
from vanilla_world import classpath,java_executable
ROOT=Path(__file__).resolve().parents[1]
classes=ROOT/'.local/movement-oracle';classes.mkdir(parents=True,exist_ok=True)
cp=classpath(Path(os.environ['APPDATA'])/'.minecraft/versions/1.8.9/1.8.9.jar')
javac=os.environ.get('MC189_JAVAC',r'C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe')
subprocess.run([javac,'-encoding','UTF-8','-cp',cp,'-d',str(classes),str(ROOT/'tools/java/MovementOracle.java')],check=True)
for args,suffix in [([],''),(['extended'],'-extended'),(['potions'],'-potions')]:
    result=subprocess.run([java_executable(),'-Xmx768M','-Djava.awt.headless=true','-cp',str(classes)+os.pathsep+cp,'MovementOracle',*args],capture_output=True,text=True)
    if result.returncode:print(result.stderr);result.check_returncode()
    data=json.loads(result.stdout)
    (ROOT/f'tests/fixtures/vanilla189-movement{suffix}.json').write_text(json.dumps(data,separators=(',',':'))+'\n',encoding='utf-8')
    print(len(data),'movement scenarios;',sum(len(r['ticks']) for r in data),'original player ticks')
