"""Export original villager purchase, close/reopen, restock and reputation events."""
import argparse
import json
import os
import subprocess
from pathlib import Path
from vanilla_world import ROOT,classpath,java_executable

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--jar',type=Path,default=Path(os.environ['APPDATA'])/'.minecraft/versions/1.8.9/1.8.9.jar')
    args=parser.parse_args();cp=classpath(args.jar)
    classes=ROOT/'.local/villager-restock-oracle/classes';classes.mkdir(parents=True,exist_ok=True)
    sources=[str(ROOT/'tools/java'/n) for n in ('FoodOracle.java','MerchantOracle.java','VillagerCareerOracle.java','VillagerRestockOracle.java')]
    subprocess.run(['javac','-encoding','UTF-8','-source','8','-target','8','-cp',cp,'-d',str(classes),*sources],check=True)
    result=subprocess.run([java_executable(),'-Xmx1G','-cp',str(classes)+os.pathsep+cp,'VillagerRestockOracle'],capture_output=True,text=True,encoding='utf-8')
    if result.returncode:
        print(result.stderr);result.check_returncode()
    data=json.loads(result.stdout)
    (ROOT/'tests/fixtures/vanilla189-villager-restock.json').write_text(json.dumps(data,separators=(',',':'))+'\n',encoding='utf-8')
    print(f'Exported {len(data)} actual villager restock traces')

if __name__=='__main__':main()
