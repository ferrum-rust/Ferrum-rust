"""Extract original 1.8.9 collision lists for independent geometry checks."""
import json
import os
from pathlib import Path
import subprocess

from vanilla_world import classpath, java_executable

ROOT = Path(__file__).resolve().parents[1]
classes = ROOT / '.local/collision-shape-oracle'
classes.mkdir(parents=True, exist_ok=True)
cp = classpath(Path(os.environ['APPDATA']) / '.minecraft/versions/1.8.9/1.8.9.jar')
javac = os.environ.get('MC189_JAVAC', r'C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe')
subprocess.run([javac, '-encoding', 'UTF-8', '-cp', cp, '-d', str(classes),
                str(ROOT/'tools/java/CollisionShapeOracle.java')], check=True)
result = subprocess.run([java_executable(), '-Xmx768M', '-cp', str(classes)+os.pathsep+cp,
                         'CollisionShapeOracle', str(ROOT/'tests/fixtures/piston-import/region/r.-1.-1.mca')], capture_output=True, text=True)
if result.returncode:
    print(result.stderr)
    result.check_returncode()
data = json.loads(result.stdout)
destination = ROOT/'tests/fixtures/vanilla189-collision-shapes.json'
destination.write_text(json.dumps(data, separators=(',', ':'))+'\n', encoding='utf-8')
print({name: len(rows) for name, rows in data.items()})

# Store only the isolated registry bounds in the game. Connected shapes are
# calculated from live neighbors in Rust. The context/far/piston rows remain
# independent expected results for those calculations.
used = sorted(({shape for _, direct, prepared in data['canonical'] for shape in (direct,prepared)}
               | {shape for _,_,shape in data['carried'] if shape >= 0}
               | {shape for _,direct,prepared in data['single'] for shape in (direct,prepared)}
               | {shape for row in data['placement'] for shape in row[2:] if shape >= 0}))
assert len(used) < 255
remap = {shape: index for index, shape in enumerate(used)}
static = [255]*4096
for state, direct, prepared in data['canonical']:
    static[state] = remap[prepared]
carried = [[-1]*6 for _ in range(4096)]
for state, face, shape in data['carried']:
    carried[state][face] = remap.get(shape, -1)
variants = sorted(set(map(tuple,carried)))
variant_id = {v: i for i,v in enumerate(variants)}
def array(name, typename, values):
    lines = [f'pub(super) const {name}: [{typename}; {len(values)}] = [']
    for i in range(0,len(values),32):
        lines.append('    '+', '.join(map(str,values[i:i+32]))+',')
    lines.append('];')
    return '\n'.join(lines)
def box_literal(bounds):
    return '['+', '.join(repr(float(value)) for value in bounds)+']'
text = '// Generated numeric bounds from original 1.8.9 Block collision queries.\n'
text += 'pub(super) const SHAPES: &[&[[f64; 6]]] = &[\n'
for shape in used:
    text += '    &['+', '.join(map(box_literal,data['shapes'][shape]))+'],\n'
text += '];\n'+array('STATIC_SHAPE','u8',static)+'\n'
single=[0]*4096
for state,direct,_ in data['single']:
    single[state]=remap[direct]
text+=array('SINGLE_SHAPE','u8',single)+'\n'
text += array('CARRIED_INDEX','u8',[variant_id[tuple(v)] for v in carried])+'\n'
text += 'pub(super) const CARRIED_SHAPES: &[[i16; 6]] = &[\n'
text += ''.join('    ['+', '.join(map(str,v))+'],\n' for v in variants)+'];\n'
text += array('DECODE_META','i16',data['decode'])+'\n'
placements = sorted(data['placement'],key=lambda row:row[0])
columns = list(zip(*(row[2:] for row in placements)))
classes = sorted(set(columns))
class_id = {key:i for i,key in enumerate(classes)}
by_existing = [class_id[columns[0]]]*4096
for (state,_,_), col in zip(data['canonical'],columns):
    by_existing[state]=class_id[col]
assert len(classes) < 256
defaults=[0]*256
table=[[-1]*len(classes) for _ in range(256)]
for i,row in enumerate(placements):
    defaults[row[0]]=row[1]
    table[row[0]]=[remap.get(group[i],-1) for group in classes]
text += array('EXISTING_CLASS','u8',by_existing)+'\n'
text += array('DEFAULT_STATE','u16',defaults)+'\n'
text += f'pub(super) static PLACEMENT_SHAPE: [[i16; {len(classes)}]; 256] = [\n'
text += ''.join('    ['+', '.join(map(str,row))+'],\n' for row in table)+'];\n'
(ROOT/'src/vanilla/collision_shape_data.rs').write_text(text,encoding='utf-8')
