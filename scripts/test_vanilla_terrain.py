"""Export original 1.8.9 density bits and pre-carving chunk hashes (Java test oracle only)."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
from vanilla_world import ROOT,classpath,java_executable

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--jar',type=Path,default=Path(os.environ.get('APPDATA',str(Path.home())))/'.minecraft/versions/1.8.9/1.8.9.jar')
    args=parser.parse_args();cp=classpath(args.jar)
    classes=ROOT/'.local/vanilla-terrain-oracle/classes';classes.mkdir(parents=True,exist_ok=True)
    subprocess.run(['javac','-encoding','UTF-8','-source','8','-target','8','-cp',cp,'-d',str(classes),str(ROOT/'tools/java/TerrainOracle.java')],check=True)
    output=subprocess.run([java_executable(),'-cp',str(classes)+os.pathsep+cp,'TerrainOracle',str(ROOT/'.local/reference/terrain')],check=True,capture_output=True,text=True,encoding='utf-8')
    data=json.loads(output.stdout);data['original_jar_sha256']=hashlib.sha256(args.jar.read_bytes()).hexdigest()
    target=ROOT/'tests/fixtures/vanilla189-terrain.json';target.parent.mkdir(parents=True,exist_ok=True)
    target.write_text(json.dumps(data,separators=(',',':'))+'\n',encoding='utf-8')
    print(f'Exported {len(data["cases"])} original terrain cases, {sum(len(r["density"]) for r in data["cases"])} density values')

if __name__=='__main__':main()
