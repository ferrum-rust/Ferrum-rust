"""Export original 1.8.9 biome IDs/metadata and exact GenLayer test grids."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
from vanilla_world import ROOT,classpath


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--jar',type=Path,default=Path(os.environ.get('APPDATA',str(Path.home())))/'.minecraft/versions/1.8.9/1.8.9.jar')
    args=parser.parse_args();cp=classpath(args.jar)
    classes=ROOT/'.local/vanilla-biome-oracle/classes';classes.mkdir(parents=True,exist_ok=True)
    subprocess.run(['javac','-encoding','UTF-8','--release','8','-cp',cp,'-d',str(classes),str(ROOT/'tools/java/BiomeOracle.java')],check=True)
    result=subprocess.run(['java','-cp',str(classes)+os.pathsep+cp,'BiomeOracle'],check=True,capture_output=True,text=True,encoding='utf-8')
    data=json.loads(result.stdout);data['original_jar_sha256']=hashlib.sha256(args.jar.read_bytes()).hexdigest()
    target=ROOT/'tests/fixtures/vanilla189-biomes.json';target.parent.mkdir(parents=True,exist_ok=True);target.write_text(json.dumps(data,separators=(',',':'))+'\n',encoding='utf-8')
    rows=['// Numerical registry data exported by scripts/test_vanilla_biomes.py from the owned 1.8.9 bytecode.','use super::biome::Biome;','pub static BIOMES: &[Biome] = &[']
    for b in data['registry']:
        f=lambda name:f'f32::from_bits(0x{b[name]&0xffffffff:08x})'
        rows.append(f'    Biome {{ id: {b["id"]}, name: {json.dumps(b["name"])}, class: {json.dumps(b["class"])}, base_height: {f("height")}, height_variation: {f("variation")}, temperature: {f("temperature")}, rainfall: {f("rainfall")}, snowy: {str(b["snowy"]).lower()}, mesa: {str(b["mesa"]).lower()}, temp_category: {b["temp_category"]}, top: {b["top"]}, filler: {b["filler"]} }},')
    rows.append('];');(ROOT/'src/vanilla/biome_data.rs').write_text('\n'.join(rows)+'\n',encoding='utf-8')
    print(f'Exported {len(data["registry"])} biome registry entries; {len(data["cases"])} oracle grids / {sum(len(r["values"]) for r in data["cases"])} cells')


if __name__=='__main__':main()
