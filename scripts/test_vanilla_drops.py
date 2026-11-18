"""Export original 1.8.9 block drops and exact world RNG consumption."""
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
    classes=ROOT/'.local/vanilla-drops-oracle/classes';classes.mkdir(parents=True,exist_ok=True)
    subprocess.run(['javac','-encoding','UTF-8','-source','8','-target','8','-cp',cp,'-d',str(classes),str(ROOT/'tools/java/DropsOracle.java')],check=True)
    output=subprocess.run(['java','-Xmx1G','-cp',str(classes)+os.pathsep+cp,'DropsOracle'],capture_output=True,text=True,encoding='utf-8')
    if output.returncode:
        print(output.stderr)
        output.check_returncode()
    data=json.loads(output.stdout);data['original_jar_sha256']=hashlib.sha256(args.jar.read_bytes()).hexdigest()
    (ROOT/'tests/fixtures/vanilla189-drops.json').write_text(json.dumps(data,separators=(',',':'))+'\n',encoding='utf-8')
    table='// Original canonical state -> deterministic item and damage; RNG item selectors override these values.\nconst STATES:&[(u16,i16,i16)]=&[\n'
    table+='\n'.join(f'({raw},{item},{meta}),' for raw,item,meta in data['states'])+'\n];\n'
    (ROOT/'src/vanilla/drops_data.rs').write_text(table,encoding='utf-8')
    print(f'Exported {len(data["cases"])} actual drops cases; {len(data["states"])} canonical states')

if __name__=='__main__':main()
