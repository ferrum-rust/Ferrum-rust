"""Invoke original 1.8.9 feature bytecode to generate native population fixtures."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
from vanilla_world import ROOT, classpath

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--jar',type=Path,default=Path(os.environ.get('APPDATA',str(Path.home())))/'.minecraft/versions/1.8.9/1.8.9.jar')
    args=parser.parse_args();cp=classpath(args.jar)
    classes=ROOT/'.local/vanilla-population-oracle/classes';classes.mkdir(parents=True,exist_ok=True)
    subprocess.run(['javac','-encoding','UTF-8','-source','8','-target','8','-cp',cp,'-d',str(classes),str(ROOT/'tools/java/PopulationOracle.java')],check=True)
    output=subprocess.run(['java','-cp',str(classes)+os.pathsep+cp,'PopulationOracle'],check=True,capture_output=True,text=True,encoding='utf-8')
    data=json.loads(output.stdout);data['original_jar_sha256']=hashlib.sha256(args.jar.read_bytes()).hexdigest()
    (ROOT/'tests/fixtures/vanilla189-population.json').write_text(json.dumps(data,separators=(',',':'))+'\n',encoding='utf-8')
    tables='// Generated from original 1.8.9 Block/material registry.\n'
    for key,name,kind in [('light_opacity','LIGHT_OPACITY','u16')]:
        tables+=f'const {name}: [{kind};256] = '+json.dumps(data[key])+';\n'
    (ROOT/'src/vanilla/population_blocks.rs').write_text(tables,encoding='utf-8')
    print(f'Exported {len(data["cases"])} feature cases and {len(data["reseeds"])} population RNG reseeds')

if __name__=='__main__':main()
