"""Run original owned Java bytecode to verify native monument generation."""
import json
import os
from pathlib import Path
import subprocess
from vanilla_world import classpath
ROOT=Path(__file__).resolve().parents[1]
jar=Path(os.environ['APPDATA'])/'.minecraft/versions/1.8.9/1.8.9.jar'
classes=ROOT/'.local/monument-oracle';classes.mkdir(parents=True,exist_ok=True)
cp=classpath(jar)
subprocess.run(['javac','-encoding','UTF-8','-cp',cp,'-d',str(classes),str(ROOT/'tools/java/MonumentOracle.java')],check=True)
result=subprocess.run(['java','-Xmx768M','-cp',str(classes)+os.pathsep+cp,'MonumentOracle'],check=True,capture_output=True,text=True)
data=json.loads(result.stdout)
(ROOT/'tests/fixtures/vanilla189-monument.json').write_text(json.dumps(data,separators=(',',':'))+'\n',encoding='utf-8')
print(len(data['queries'])*193*193,'candidate decisions;',len(data['starts']),'starts;',sum(len(r['pieces']) for r in data['starts']),'pieces')
