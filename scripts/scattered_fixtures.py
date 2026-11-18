"""Developer-only original-bytecode scattered structure differential corpus."""
import json
import os
from pathlib import Path
import subprocess
from vanilla_world import classpath
ROOT=Path(__file__).resolve().parents[1]
jar=Path(os.environ['APPDATA'])/'.minecraft/versions/1.8.9/1.8.9.jar'
classes=ROOT/'.local/scattered-oracle';classes.mkdir(parents=True,exist_ok=True)
cp=classpath(jar)
subprocess.run(['javac','-encoding','UTF-8','-cp',cp,'-d',str(classes),str(ROOT/'tools/java/ScatteredOracle.java')],check=True)
result=subprocess.run(['java','-Xmx768M','-cp',str(classes)+os.pathsep+cp,'ScatteredOracle'],check=True,capture_output=True,text=True)
data=json.loads(result.stdout)
(ROOT/'tests/fixtures/vanilla189-scattered.json').write_text(json.dumps(data,separators=(',',':'))+'\n',encoding='utf-8')
print(len(data['candidates']),'candidates;',len(data['cases']),'population cases;',sum(len(r['blocks']) for r in data['cases']),'written states')
