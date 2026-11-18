"""Original spawn choice, biome/ground probes and startup chunk request order."""
import json
import os
from pathlib import Path
import subprocess
import sys
from vanilla_world import ROOT, classpath, java_executable

cp = classpath(Path(os.environ['APPDATA']) / '.minecraft/versions/1.8.9/1.8.9.jar')
out = ROOT / '.local/oracles/world-spawn'
out.mkdir(parents=True, exist_ok=True)
sources = ['VanillaWorldBridge.java', 'RuntimeOracle.java', 'WorldSpawnOracle.java']
subprocess.run(['javac', '--release', '8', '-cp', cp, '-d', str(out),
                *[str(ROOT / 'tools/java' / source) for source in sources]], check=True, cwd=ROOT)
rows = []
if '--players' in sys.argv:
    trace = out / 'spawn-players-189.json'
    subprocess.run([java_executable(), '-Xmx1G', '-cp', str(out) + os.pathsep + cp,
                    'WorldSpawnOracle', str(out), '189', '0', str(trace), 'players'],
                   check=True, cwd=ROOT, stdout=subprocess.DEVNULL)
    row = json.loads(trace.read_text(encoding='utf-8'))
    if len(row['players']) != 576:
        raise RuntimeError('Incomplete EntityPlayerMP spawn oracle')
    (ROOT / 'tests/fixtures/vanilla189-player-spawn.json').write_text(
        json.dumps({'spawn': row['spawn'], 'players': row['players']}, separators=(',', ':')) + '\n', encoding='utf-8')
    print('Original EntityPlayerMP spawn: 576 offsets, RNG states and collision-lift traces')
    sys.exit(0)
if '--full-region' in sys.argv:
    trace = out / 'full-region-189.json'
    subprocess.run([java_executable(), '-Xmx1G', '-cp', str(out) + os.pathsep + cp,
                    'WorldSpawnOracle', str(out), '189', '0', str(trace), 'full-region'],
                   check=True, cwd=ROOT, stdout=subprocess.DEVNULL)
    row = json.loads(trace.read_text(encoding='utf-8'))
    if len(row['warmup']) != 625 or len(row['region']) < 625:
        raise RuntimeError('Incomplete actual initial spawn region')
    (ROOT / 'tests/fixtures/vanilla189-world-start-region.json').write_text(
        json.dumps(row, separators=(',', ':')) + '\n', encoding='utf-8')
    print(f"Original seed 189 initial region: {len(row['region'])} whole chunks, 625 startup requests")
    sys.exit(0)
cases = [(seed, 0) for seed in [0, 1, -1, 189, 8675309, 1234567890123, -(2**63), 2**63-1]]
for i, (seed, mode) in enumerate(cases + [(189, 1), (-1, 2)]):
    trace = out / f'trace-{i}.json'
    subprocess.run([java_executable(), '-Xmx1G', '-cp', str(out) + os.pathsep + cp,
                    'WorldSpawnOracle', str(out), str(seed), str(mode), str(trace)],
                   check=True, cwd=ROOT, stdout=subprocess.DEVNULL)
    row = json.loads(trace.read_text(encoding='utf-8'))
    if len(row['warmup']) != 625:
        raise RuntimeError(f"Incomplete initial spawn load for seed {seed}")
    rows.append(row)
    print(f"seed {seed} mode {mode}: spawn {row['spawn']}, {len(row['probes'])} probes, {len(row['chunks'])} chunks", flush=True)
(ROOT / 'tests/fixtures/vanilla189-world-spawn.json').write_text(
    json.dumps(rows, separators=(',', ':')) + '\n', encoding='utf-8')
