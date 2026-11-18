"""Original container onAdded orientation, including ordered world accesses."""
import json,os,subprocess
from pathlib import Path
from vanilla_world import ROOT,classpath,java_executable
cp=classpath(Path(os.environ['APPDATA'])/'.minecraft/versions/1.8.9/1.8.9.jar');out=ROOT/'.local/oracles/facing';out.mkdir(parents=True,exist_ok=True)
subprocess.run(['javac','--release','8','-cp',cp,'-d',str(out),str(ROOT/'tools/java/FacingOracle.java')],check=True)
data=json.loads(subprocess.check_output([java_executable(),'-cp',str(out)+os.pathsep+cp,'FacingOracle'],cwd=ROOT))
(ROOT/'tests/fixtures/vanilla189-facing.json').write_text(json.dumps(data,separators=(',',':'))+'\n',encoding='utf-8')
print(f'{len(data["cases"])} original furnace/dispenser/dropper/chest facing cases')
