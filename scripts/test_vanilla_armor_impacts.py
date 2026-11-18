"""Export original anvil/falling-block hits and their armor, sound, RNG state."""
import argparse
import json
import os
import subprocess
from pathlib import Path
from vanilla_world import ROOT, classpath, java_executable

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--jar',type=Path,default=Path(os.environ['APPDATA'])/'.minecraft/versions/1.8.9/1.8.9.jar')
    args=parser.parse_args()
    cp=classpath(args.jar)
    classes=ROOT/'.local/armor-impact-oracle/classes'
    classes.mkdir(parents=True,exist_ok=True)
    sources=[str(ROOT/'tools/java'/file) for file in ('FoodOracle.java','ArmorOracle.java','ArmorImpactOracle.java')]
    subprocess.run(['javac','-encoding','UTF-8','-source','8','-target','8','-cp',cp,'-d',str(classes),*sources],check=True)
    result=subprocess.run([java_executable(),'-Xmx1G','-cp',str(classes)+os.pathsep+cp,'ArmorImpactOracle'],capture_output=True,text=True,encoding='utf-8')
    if result.returncode:
        print(result.stderr)
        result.check_returncode()
    cases=json.loads(result.stdout)
    (ROOT/'tests/fixtures/vanilla189-armor-impacts.json').write_text(json.dumps(cases,separators=(',',':'))+'\n',encoding='utf-8')
    print(f'Exported {len(cases)} actual falling-object hits')

if __name__=='__main__':
    main()
