"""Run original block selection and voxel traversal over canonical states."""
import json
import os
from pathlib import Path
import subprocess
from vanilla_world import classpath, java_executable
ROOT=Path(__file__).resolve().parents[1]
out=ROOT/'.local/collision-shape-oracle'
out.mkdir(parents=True,exist_ok=True)
cp=classpath(Path(os.environ['APPDATA'])/'.minecraft/versions/1.8.9/1.8.9.jar')
javac=os.environ.get('MC189_JAVAC',r'C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe')
subprocess.run([javac,'-encoding','UTF-8','-cp',cp,'-d',str(out),str(ROOT/'tools/java/CollisionShapeOracle.java'),str(ROOT/'tools/java/RayTraceOracle.java')],check=True)
result=subprocess.run([java_executable(),'-Xmx1G','-cp',str(out)+os.pathsep+cp,'RayTraceOracle'],capture_output=True,text=True)
if result.returncode:
    print(result.stderr)
    result.check_returncode()
data=json.loads(result.stdout)
(ROOT/'tests/fixtures/vanilla189-ray-trace.json').write_text(json.dumps(data,separators=(',',':'))+'\n',encoding='utf-8')
used=sorted({shape for state,shape,flags in data['registry']})
remap={shape:i for i,shape in enumerate(used)}
registry=[[0,0] for _ in range(4096)]
for state,shape,flags in data['registry']:registry[state]=[remap[shape],flags]
text='// Original position-prepared local selection bounds and collision query flags.\n'
text+='const BOXES: &[[f64;6]]=&[\n'+''.join('    ['+', '.join(repr(float(v)) for v in data['boxes'][i])+'],\n' for i in used)+'];\n'
for field,name in [(0,'BOX'),(1,'FLAGS')]:
    text+=f'const {name}: [u8;4096]=[\n'+''.join('    '+', '.join(str(r[field]) for r in registry[i:i+32])+',\n' for i in range(0,4096,32))+'];\n'
(ROOT/'src/vanilla/ray_trace_data.rs').write_text(text,encoding='utf-8')
print('canonical original rays',len(data['registry'])*len(data['inputs'])*9,'neighbor selection boxes',len(data['contexts']),'multi-block DDA',sum(len(s['rays']) for s in data['scenes'])*8)
