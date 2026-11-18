"""Regenerate native cave/ravine tests from the locally owned original JAR.
This is an optional development oracle; the Rust client never launches Java.
"""
import json
import os
from pathlib import Path
import subprocess
from vanilla_world import classpath

ROOT = Path(__file__).resolve().parents[1]
jar = Path(os.environ['APPDATA']) / '.minecraft/versions/1.8.9/1.8.9.jar'
classes = ROOT / '.local/carver-oracle'
classes.mkdir(parents=True, exist_ok=True)
cp = classpath(jar)
subprocess.run(['javac', '-encoding', 'UTF-8', '-cp', cp, '-d', str(classes),
                str(ROOT / 'tools/java/CarverOracle.java')], check=True)
result = subprocess.run(['java', '-Xmx512M', '-cp', str(classes)+os.pathsep+cp,
                         'CarverOracle'], check=True, capture_output=True, text=True)
data = json.loads(result.stdout)
target = ROOT / 'tests/fixtures/vanilla189-carvers.json'
target.parent.mkdir(parents=True, exist_ok=True)
target.write_text(json.dumps(data, separators=(',', ':'))+'\n', encoding='utf-8')
print(f'{len(data["cases"])} original carver cases written to {target}')
print('Changed cases:', sum(c['changed'] > 0 for c in data['cases']))
