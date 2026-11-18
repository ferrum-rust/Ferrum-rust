"""Generate actual original client effect packets and SP effect lifecycle."""
import json
import os
from pathlib import Path
import subprocess
from vanilla_world import classpath, java_executable
ROOT = Path(__file__).resolve().parents[1]
classes = ROOT/'.local/movement-oracle'
classes.mkdir(parents=True, exist_ok=True)
cp = classpath(Path(os.environ['APPDATA'])/'.minecraft/versions/1.8.9/1.8.9.jar')
javac = os.environ.get('MC189_JAVAC', r'C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe')
subprocess.run([javac, '-encoding', 'UTF-8', '-cp', cp, '-d', str(classes),
               *[str(ROOT/'tools/java'/f'{name}.java') for name in
                 ['MovementOracle','MovementAttributesOracle','PlayerEffectPacketOracle']]], check=True)
result = subprocess.run([java_executable(), '-Xmx512M', '-Djava.awt.headless=true', '-cp',
                         str(classes)+os.pathsep+cp, 'PlayerEffectPacketOracle'], capture_output=True,text=True)
if result.returncode:
    print(result.stderr)
    result.check_returncode()
data=json.loads(result.stdout)
(ROOT/'tests/fixtures/vanilla189-player-effect-packets.json').write_text(
    json.dumps(data,separators=(',',':'))+'\n',encoding='utf-8')
print(len(data['packets']), 'S1D/S1E pairs;',sum(len(r['steps']) for r in data['sequences']),'original SP effect steps')
