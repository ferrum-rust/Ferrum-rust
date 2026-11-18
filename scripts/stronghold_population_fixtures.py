"""Actual original bytecode stronghold template fixtures, developer-only."""
import json
import os
from pathlib import Path
import subprocess
from vanilla_world import classpath
ROOT=Path(__file__).resolve().parents[1]
jar=Path(os.environ['APPDATA'])/'.minecraft/versions/1.8.9/1.8.9.jar'
classes=ROOT/'.local/stronghold-oracle';classes.mkdir(parents=True,exist_ok=True)
cp=classpath(jar)
sources=[ROOT/'tools/java'/n for n in ['StrongholdOracle.java','ScatteredOracle.java','StrongholdPopulationOracle.java']]
subprocess.run(['javac','-encoding','UTF-8','-cp',cp,'-d',str(classes),*map(str,sources)],check=True)
result=subprocess.run(['java','-Xmx768M','-cp',str(classes)+os.pathsep+cp,'StrongholdPopulationOracle'],check=True,capture_output=True,text=True)
data=json.loads(result.stdout)
(ROOT/'tests/fixtures/vanilla189-stronghold-population.json').write_text(json.dumps(data,separators=(',',':'))+'\n',encoding='utf-8')
print(len(data),'cases;',sum(len(r['blocks']) for r in data),'written states')
