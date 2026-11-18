"""Run owned Java bytecode color methods against synthetic palettes."""
import json
import os
from pathlib import Path
import subprocess
from vanilla_world import classpath

ROOT=Path(__file__).resolve().parents[1]
jar=Path(os.environ['APPDATA'])/'.minecraft/versions/1.8.9/1.8.9.jar'
classes=ROOT/'.local/biome-color-oracle'
classes.mkdir(parents=True,exist_ok=True)
cp=classpath(jar)
subprocess.run(['javac','-encoding','UTF-8','-cp',cp,'-d',str(classes),str(ROOT/'tools/java/BiomeColorOracle.java')],check=True)
result=subprocess.run(['java','-Xmx256M','-cp',str(classes)+os.pathsep+cp,'BiomeColorOracle'],check=True,capture_output=True,text=True)
data=json.loads(result.stdout)
target=ROOT/'tests/fixtures/vanilla189-biome-colors.json'
target.write_text(json.dumps(data,separators=(',',':'))+'\n',encoding='utf-8')
print(len(data['cases']),'cases; mutation color delegates:',data['delegates'])
