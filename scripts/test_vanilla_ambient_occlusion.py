"""Export actual BlockModelRenderer face light and shade interpolation."""
import json
import os
from pathlib import Path
import subprocess
from vanilla_world import ROOT, classpath, java_executable

cp = classpath(Path(os.environ['APPDATA']) / '.minecraft/versions/1.8.9/1.8.9.jar')
classes = ROOT / '.local/ambient-occlusion-oracle/classes'
classes.mkdir(parents=True, exist_ok=True)
subprocess.run([os.environ.get('MC189_JAVAC', 'javac'), '-encoding', 'UTF-8', '-source', '8', '-target', '8', '-cp', cp,
               '-d', str(classes), str(ROOT / 'tools/java/AmbientOcclusionOracle.java')], check=True, cwd=ROOT)
target = ROOT / 'tests/fixtures/vanilla189-ambient-occlusion.json'
subprocess.run([java_executable(), '-Xmx1G', '-cp', str(classes) + os.pathsep + cp, 'AmbientOcclusionOracle', str(target)], check=True, cwd=ROOT)
data = json.loads(target.read_text())
def rust(value):
    if isinstance(value, bool): return str(value).lower()
    if isinstance(value, list): return '[' + ','.join(map(rust, value)) + ']'
    return str(value)
lines = ['// Numeric original renderer properties exported by test_vanilla_ambient_occlusion.py.']
for name, shape, value in [
    ('FACTS', '[[u32; 4]; 256]', data['facts']),
    ('SIDES', '[[usize; 4]; 6]', [r[0] for r in data['neighbors']]),
    ('PARTIAL', '[bool; 6]', [r[1] for r in data['neighbors']]),
    ('WEIGHTS', '[[[usize; 8]; 4]; 6]', [[w if w else [0]*8 for w in r[2]] for r in data['neighbors']]),
    ('TRANSLATIONS', '[[usize; 4]; 6]', data['translations']),
    ('VERTICES', '[[[usize; 3]; 4]; 6]', data['vertices']),
]: lines.append(f'pub const {name}: {shape} = {rust(value)};')
(ROOT / 'src/vanilla/ambient_occlusion_data.rs').write_text('\n'.join(lines) + '\n')
