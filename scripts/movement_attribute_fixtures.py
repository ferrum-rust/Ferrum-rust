"""Run original S20 packet, attribute instance, and SP sprint bytecode."""
import json
import os
from pathlib import Path
import subprocess
from vanilla_world import classpath, java_executable

ROOT = Path(__file__).resolve().parents[1]
classes = ROOT / '.local/movement-oracle'
classes.mkdir(parents=True, exist_ok=True)
cp = classpath(Path(os.environ['APPDATA']) / '.minecraft/versions/1.8.9/1.8.9.jar')
javac = os.environ.get('MC189_JAVAC', r'C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe')
subprocess.run([javac, '-encoding', 'UTF-8', '-cp', cp, '-d', str(classes),
               str(ROOT/'tools/java/MovementOracle.java'),
               str(ROOT/'tools/java/MovementAttributesOracle.java')], check=True)
result = subprocess.run([java_executable(), '-Xmx512M', '-Djava.awt.headless=true', '-cp',
                         str(classes)+os.pathsep+cp, 'MovementAttributesOracle'],
                        capture_output=True, text=True)
if result.returncode:
    print(result.stderr)
    result.check_returncode()
data = json.loads(result.stdout)
(ROOT/'tests/fixtures/vanilla189-movement-attributes.json').write_text(
    json.dumps(data, separators=(',', ':'))+'\n', encoding='utf-8')
print(len(data), 'attribute sequences;', sum(map(len, data)), 'original packet snapshots')
