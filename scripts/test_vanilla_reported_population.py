"""Compare the reported bare-terrain seed with fresh original 1.8.9 population.

Only project-local oracle output is written; installed Java worlds are never
opened or changed. Every region runs in a fresh JVM with the original bytecode.
"""
import argparse
from collections import Counter
import hashlib
import json
import os
from pathlib import Path
import struct
import subprocess

from vanilla_world import ROOT, classpath, java_executable


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--seed', type=int, default=-6201231679296862437)
    parser.add_argument('--origin', action='append', help='Chunk x,z; repeat for more regions')
    args = parser.parse_args()
    origins = [tuple(map(int, value.split(','))) for value in args.origin] if args.origin else [(0, 0), (-3, 1), (-2, 6)]
    jar = Path(os.environ['APPDATA']) / '.minecraft/versions/1.8.9/1.8.9.jar'
    cp = classpath(jar)
    classes = ROOT / '.local/oracles/runtime'
    classes.mkdir(parents=True, exist_ok=True)
    sources = [ROOT / 'tools/java/VanillaWorldBridge.java', ROOT / 'tools/java/RuntimeOracle.java']
    if any(not (classes / source.with_suffix('.class').name).is_file()
           or (classes / source.with_suffix('.class').name).stat().st_mtime < source.stat().st_mtime
           for source in sources):
        subprocess.run(['javac', '--release', '8', '-cp', cp, '-d', str(classes), *map(str, sources)], check=True)
    data = {
        'original_jar_sha256': hashlib.sha256(jar.read_bytes()).hexdigest(),
        'fresh_jvm_per_case': True,
        'world_random_seed': '1234',
        'region_width': 4,
        'cases': [],
    }
    for x, z in origins:
        for order in [0, 1]:
            result = subprocess.check_output([
                java_executable(), '-Xmx2G', '-cp', str(classes) + os.pathsep + cp,
                'RuntimeOracle', str(classes / 'world'), 'full', str(args.seed),
                str(x), str(z), str(order), '4',
            ], cwd=ROOT)
            row = json.loads(result)['cases'][0]
            counts = Counter()
            for chunk in row['chunks']:
                raw = classes / 'world/blocks' / f'{row["raw_prefix"]}_{chunk[0]}_{chunk[1]}.bin'
                counts.update(state >> 4 for (state,) in struct.iter_unpack('<H', raw.read_bytes()))
            row['surface_counts'] = {
                'logs': counts[17] + counts[162],
                'leaves': counts[18] + counts[161],
                'tall_grass': counts[31],
                'flowers': counts[37] + counts[38],
                'snow_layers': counts[78],
            }
            data['cases'].append(row)
            print(f'Original {args.seed} ({x},{z}) order {order}: {row["surface_counts"]}', flush=True)
    path = ROOT / 'tests/fixtures/vanilla189-reported-population.json'
    path.write_text(json.dumps(data, separators=(',', ':')) + '\n', encoding='utf-8')
    print(f'Exported {len(data["cases"])} exact populated region traces to {path.name}')


if __name__ == '__main__':
    main()
