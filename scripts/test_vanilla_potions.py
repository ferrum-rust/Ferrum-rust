"""Run original potion-map callbacks, effect combination and resistance arithmetic."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
from vanilla_world import ROOT, classpath, java_executable

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--jar',type=Path,default=Path(os.environ.get('APPDATA',str(Path.home())))/'.minecraft/versions/1.8.9/1.8.9.jar')
    args=parser.parse_args();cp=classpath(args.jar)
    classes=ROOT/'.local/vanilla-potions-oracle/classes';classes.mkdir(parents=True,exist_ok=True)
    sources=[str(ROOT/'tools/java'/name) for name in ('FoodOracle.java','PotionsOracle.java')]
    subprocess.run(['javac','-encoding','UTF-8','-source','8','-target','8','-cp',cp,'-d',str(classes),*sources],check=True)
    output=subprocess.run([java_executable(),'-Xmx1G','-cp',str(classes)+os.pathsep+cp,'PotionsOracle'],capture_output=True,text=True,encoding='utf-8')
    if output.returncode:
        print(output.stderr);output.check_returncode()
    data=json.loads(output.stdout);data['original_jar_sha256']=hashlib.sha256(args.jar.read_bytes()).hexdigest()
    (ROOT/'tests/fixtures/vanilla189-potions.json').write_text(json.dumps(data,separators=(',',':'))+'\n',encoding='utf-8')
    print(f'Exported {len(data["sequences"])} actual potion-map sequences and {len(data["resistance"])} resistance cases')

if __name__=='__main__':main()
