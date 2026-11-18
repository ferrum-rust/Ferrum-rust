"""Record original village structure component NBT and RNG continuation."""
import json,os,subprocess,hashlib
from pathlib import Path
from vanilla_world import ROOT,classpath

jar=Path(os.environ['APPDATA'])/'.minecraft/versions/1.8.9/1.8.9.jar'
cp=classpath(jar);classes=ROOT/'.local/vanilla-village-oracle/classes';classes.mkdir(parents=True,exist_ok=True)
subprocess.run(['javac','-encoding','UTF-8','--release','8','-cp',cp,'-d',str(classes),str(ROOT/'tools/java/VillageOracle.java')],check=True)
result=subprocess.run(['java','-cp',str(classes)+os.pathsep+cp,'VillageOracle'],check=True,capture_output=True,text=True,encoding='utf-8')
data=json.loads(result.stdout);data['original_jar_sha256']=hashlib.sha256(jar.read_bytes()).hexdigest()
target=ROOT/'tests/fixtures/vanilla189-villages.json';target.write_text(json.dumps(data,separators=(',',':'))+'\n',encoding='utf-8')
print(f'Exported {len(data["cases"])} original village layouts, {sum(len(c["nbt"]["Children"]) for c in data["cases"])} exact component records')
