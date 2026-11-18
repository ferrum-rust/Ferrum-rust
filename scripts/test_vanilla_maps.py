"""Export original empty-map allocation and explored map pixel/marker fixtures."""
import json, os, subprocess
from pathlib import Path
from vanilla_world import ROOT, classpath, java_executable

jar=Path(os.environ['APPDATA'])/'.minecraft/versions/1.8.9/1.8.9.jar'
cp=classpath(jar)
classes=ROOT/'.local/vanilla-map-oracle/classes';classes.mkdir(parents=True,exist_ok=True)
sources=[ROOT/'tools/java'/name for name in ['FoodOracle.java','MapUseOracle.java']]
subprocess.run(['javac','-source','8','-target','8','-cp',cp,'-d',str(classes),*map(str,sources)],check=True)
run=subprocess.run([java_executable(),'-Xmx512M','-cp',str(classes)+os.pathsep+cp,'MapUseOracle'],capture_output=True,text=True)
if run.returncode:
    print(run.stderr);run.check_returncode()
data=json.loads(run.stdout)
(ROOT/'src/map_colors.rs').write_text('// Original 1.8.9 Block.getMapColor(state).colorIndex.\nconst MAP_COLORS: [u8;4096] = '+json.dumps(data.pop('map_colors'))+';\n',encoding='utf-8')
(ROOT/'tests/fixtures/vanilla189-maps.json').write_text(json.dumps(data,separators=(',',':'))+'\n',encoding='utf-8')
print('Exported',len(data['cases']),'original map traces and 4096 block-state map colors')
