"""Export original block/material properties; Java is used only by this oracle."""
import hashlib,json,os,subprocess
from pathlib import Path
from vanilla_world import ROOT,classpath,java_executable
jar=Path(os.environ['APPDATA'])/'.minecraft/versions/1.8.9/1.8.9.jar'
cp=classpath(jar);classes=ROOT/'.local/oracles/material';classes.mkdir(parents=True,exist_ok=True)
subprocess.run(['javac','--release','8','-encoding','UTF-8','-cp',cp,'-d',str(classes),str(ROOT/'tools/java/MaterialOracle.java')],check=True)
raw=subprocess.check_output([java_executable(),'-cp',str(classes)+os.pathsep+cp,'MaterialOracle'],cwd=ROOT)
data=json.loads(raw);data['original_jar_sha256']=hashlib.sha256(jar.read_bytes()).hexdigest()
dest=ROOT/'tests/fixtures/vanilla189-materials.json';dest.write_text(json.dumps(data,separators=(',',':'))+'\n',encoding='utf-8')
flags=[row[2] for row in data['rows']]
light_tables=''.join('pub const '+name+': [u16; 256] = '+str([row[column] for row in data['rows']])+';\n' for column,name in [(4,'LIGHT_OPACITY'),(5,'LIGHT_VALUE'),(6,'NEIGHBOR_BRIGHTNESS')])
(ROOT/'src/vanilla/material_data.rs').write_text('//! Exported by scripts/test_vanilla_materials.py from the owned original JAR.\npub const FLAGS: [u16; 256] = '+str(flags)+';\n'+light_tables,encoding='utf-8')
print(f'Exported {len(flags)} original block/material property rows; unknown IDs resolve to {data["unknown_resolved_ids"]}')
