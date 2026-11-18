"""Run original Nether/End bytecode for density, full chunk and RNG fixtures."""
import json
import os
from pathlib import Path
import subprocess
from vanilla_world import classpath
JAVA = os.environ.get('MC189_JAVA', r'C:\Program Files\Java\jre1.8.0_501\bin\java.exe')
JAVAC = os.environ.get('MC189_JAVAC', r'C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe')
ROOT = Path(__file__).resolve().parents[1]
jar = Path(os.environ['APPDATA']) / '.minecraft/versions/1.8.9/1.8.9.jar'
classes = ROOT / '.local/dimension-terrain-oracle'
classes.mkdir(parents=True, exist_ok=True)
cp = classpath(jar)
subprocess.run([JAVAC, '-encoding', 'UTF-8', '-cp', cp, '-d', str(classes), str(ROOT / 'tools/java/DimensionTerrainOracle.java')], check=True)
result = subprocess.run([JAVA, '-Xmx768M', '-cp', str(classes) + os.pathsep + cp, 'DimensionTerrainOracle'], check=True, capture_output=True, text=True)
rows = json.loads(result.stdout)
(ROOT / 'tests/fixtures/vanilla189-dimension-terrain.json').write_text(json.dumps(rows, separators=(',', ':')) + '\n', encoding='utf-8')
print(len(rows), 'original provider cases;', sum(len(r['density']) for r in rows), 'density values')
