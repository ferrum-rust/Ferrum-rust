"""Import the complete owned 1.8.9 resource pack and asset index locally.

No game classes are copied into the resource folder. External objects are
verified against Mojang's asset-index SHA-1. Nothing is published.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--jar', type=Path, default=Path(os.environ['APPDATA'])/'.minecraft/versions/1.8.9/1.8.9.jar')
    parser.add_argument('--refresh', action='store_true')
    args = parser.parse_args()
    dest = ROOT/'.local/resources'
    manifest_path = dest/'manifest.json'
    fingerprint = hashlib.sha256(args.jar.read_bytes()).hexdigest()
    if manifest_path.exists() and not args.refresh:
        old = json.loads(manifest_path.read_text(encoding='utf-8'))
        if old.get('jar_sha256') == fingerprint and old.get('format') == 3:
            print(f"Resources ready: {old['jar_files']} JAR files, {old['external_files']} indexed assets.")
            return
    dest.mkdir(parents=True, exist_ok=True)
    reference = ROOT/'.local/reference/java/net/minecraft'
    if not (reference/'block/Block.java').is_file() or not (reference/'item/Item.java').is_file():
        subprocess.run([sys.executable,str(ROOT/'scripts/prepare_reference.py'),'--jar',str(args.jar)],cwd=ROOT,check=True)
    files = set()
    jar_count = 0
    with zipfile.ZipFile(args.jar) as archive:
        for name in archive.namelist():
            if name.endswith('/') or not (name.startswith('assets/') or name == 'pack.png'):
                continue
            parts = PurePosixPath(name).parts
            if '..' in parts or PurePosixPath(name).is_absolute():
                raise ValueError('Unsafe resource path')
            target = dest.joinpath(*parts)
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(archive.read(name))
            files.add(name)
            jar_count += 1
    mc = args.jar.parent.parent.parent
    version = json.loads(args.jar.with_suffix('.json').read_text(encoding='utf-8'))
    index_info = version['assetIndex']
    index_path = mc/'assets/indexes'/(index_info['id']+'.json')
    if index_path.is_file():
        index_bytes = index_path.read_bytes()
    else:
        index_bytes = urllib.request.urlopen(index_info['url'],timeout=60).read()
    if hashlib.sha1(index_bytes).hexdigest() != index_info['sha1']:
        raise ValueError('Asset index checksum mismatch')
    index = json.loads(index_bytes)['objects']
    external_count = 0
    for name, record in index.items():
        digest = record['hash']
        source = mc/'assets/objects'/digest[:2]/digest
        if source.is_file():
            data = source.read_bytes()
        else:
            data = urllib.request.urlopen(f'https://resources.download.minecraft.net/{digest[:2]}/{digest}',timeout=60).read()
        if hashlib.sha1(data).hexdigest() != digest:
            raise ValueError('Resource checksum mismatch: '+name)
        relative = 'assets/'+name if not name.startswith('assets/') else name
        parts = PurePosixPath(relative).parts
        if '..' in parts or PurePosixPath(relative).is_absolute():
            raise ValueError('Unsafe indexed resource path')
        target = dest.joinpath(*parts)
        target.parent.mkdir(parents=True,exist_ok=True)
        target.write_bytes(data)
        files.add(relative)
        external_count += 1
    # Read registry facts from the user's already-prepared local reference.
    # This produces data only, never Java code in the source tree.
    registry = {'blocks':{},'items':{}}
    for category, filename, call in [('blocks',reference/'block/Block.java','registerBlock'),('items',reference/'item/Item.java','registerItem')]:
        if not filename.exists():
            raise SystemExit('Run decompile.bat once before preparing registry data.')
        for line in filename.read_text(encoding='utf-8').splitlines():
            match = re.search(r'\b'+call+r'\((\d+), "([^"]+)"',line)
            if not match:
                continue
            ident, name = match.groups()
            unlocalized = re.search(r'setUnlocalizedName\("([^"]+)"',line)
            hardness = re.search(r'setHardness\((-?[\d.]+)f?\)',line)
            food = re.search(r'new Item(?:Food|AppleGold|SeedFood)\((\d+),\s*([\d.]+)f',line)
            registry[category][ident] = {'name':name,'translation':('tile.' if category=='blocks' else 'item.')+(unlocalized.group(1) if unlocalized else name)+'.name','hardness':float(hardness.group(1)) if hardness else (-1.0 if 'setBlockUnbreakable' in line else 1.0),'food':int(food.group(1)) if food else 0,'saturation':float(food.group(2)) if food else 0.0}
    (dest/'registry.json').write_text(json.dumps(registry,indent=2),encoding='utf-8')
    report = {'format':3,'jar_sha256':fingerprint,'jar_files':jar_count,'external_files':external_count,'languages':sum('/lang/' in n for n in files),'sounds':sum(n.endswith('.ogg') for n in files),'block_registry':len(registry['blocks']),'item_registry':len(registry['items']),'files':sorted(files)}
    manifest_path.write_text(json.dumps(report,indent=2),encoding='utf-8')
    print({key:value for key,value in report.items() if key!='files'})

if __name__ == '__main__':
    main()
