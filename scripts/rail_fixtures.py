"""Original Java 8 rail graph and block-callback differential fixtures."""
import json
import os
from pathlib import Path
import subprocess
from vanilla_world import classpath
ROOT=Path(__file__).resolve().parents[1]
JAVA=os.environ.get('MC189_JAVA',r'C:\Program Files\Java\jre1.8.0_501\bin\java.exe')
JAVAC=os.environ.get('MC189_JAVAC',r'C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe')
classes=ROOT/'.local/rail-oracle';classes.mkdir(parents=True,exist_ok=True)
cp=classpath(Path(os.environ['APPDATA'])/'.minecraft/versions/1.8.9/1.8.9.jar')
subprocess.run([JAVAC,'-encoding','UTF-8','-cp',cp,'-d',str(classes),str(ROOT/'tools/java/RailOracle.java')],check=True)
result=subprocess.run([JAVA,'-Xmx768M','-cp',str(classes)+os.pathsep+cp,'RailOracle'],check=True,capture_output=True,text=True)
data=json.loads(result.stdout)
(ROOT/'tests/fixtures/vanilla189-rail-events.json').write_text(json.dumps(data,separators=(',',':'))+'\n',encoding='utf-8')
print(len(data),'rail cases;',sum(r['reads'] for r in data),'ordered reads;',sum(len(r['writes']) for r in data),'ordered writes')
