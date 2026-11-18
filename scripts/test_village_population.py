"""Generate original 1.8.9 village structure-write differential fixtures."""
from pathlib import Path
import json,subprocess,os
from vanilla_world import ROOT,classpath
out=ROOT/'.local/oracles/village';out.mkdir(parents=True,exist_ok=True)
cp=classpath(Path(os.environ['APPDATA'])/'.minecraft/versions/1.8.9/1.8.9.jar')
subprocess.run(['javac','-cp',cp,'-d',str(out),str(ROOT/'tools/java/VillageOracle.java'),str(ROOT/'tools/java/VillagePopulationOracle.java')],check=True)
raw=subprocess.check_output(['java','-cp',str(out)+';'+cp,'VillagePopulationOracle'],cwd=ROOT)
data=json.loads(raw);dest=ROOT/'tests/fixtures/vanilla189-village-population.json';dest.write_text(json.dumps(data,separators=(',',':')))
flags=[0]*256
for id,properties,state in data['properties']:flags[id]=properties
(ROOT/'src/vanilla/block_properties.rs').write_text('//! Original Block registry material/cube flags, exported by VillagePopulationOracle.\npub const FLAGS:[u8;256]='+str(flags).replace(' ','')+';\n')
print(f"{len(data['cases'])} original village population cases; {sum(len(c['blocks']) for c in data['cases'])} written block states -> {dest}")
