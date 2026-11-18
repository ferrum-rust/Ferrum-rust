"""Export actual structure discovery and NBT restart iteration ordering."""
import json,os,subprocess
from pathlib import Path
from vanilla_world import ROOT,classpath,java_executable
cp=classpath(Path(os.environ['APPDATA'])/'.minecraft/versions/1.8.9/1.8.9.jar');out=ROOT/'.local/oracles/structure-order';out.mkdir(parents=True,exist_ok=True)
subprocess.run(['javac','--release','8','-cp',cp,'-d',str(out),str(ROOT/'tools/java/VanillaWorldBridge.java'),str(ROOT/'tools/java/StructureOrderOracle.java')],check=True)
data=json.loads(subprocess.check_output([java_executable(),'-cp',str(out)+os.pathsep+cp,'StructureOrderOracle',str(out/'world')],cwd=ROOT))
(ROOT/'tests/fixtures/vanilla189-structure-order.json').write_text(json.dumps(data,separators=(',',':'))+'\n',encoding='utf-8')
print(f'{len(data["cases"])} seeds, {sum(len(c["steps"]) for c in data["cases"])} discovery/restart steps; original live/feature maps + shared-world RNG tails')
