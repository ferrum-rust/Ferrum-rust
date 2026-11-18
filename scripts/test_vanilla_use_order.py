"""Verify runTick order in original bytecode and export accepted-use tick traces."""
import argparse
import json
import os
import re
import subprocess
from pathlib import Path
from vanilla_world import ROOT,classpath,java_executable

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--jar',type=Path,default=Path(os.environ['APPDATA'])/'.minecraft/versions/1.8.9/1.8.9.jar')
    args=parser.parse_args();cp=classpath(args.jar)
    bytecode=subprocess.check_output(['javap','-classpath',cp,'-c','-p','net.minecraft.client.Minecraft'],text=True,encoding='utf-8')
    tick=bytecode.split('public void runTick()',1)[1].split('\n  public ',1)[0]
    clicks=[int(v) for v in re.findall(r'^\s*(\d+):[^\n]*// Method rightClickMouse:\(\)V',tick,re.M)]
    updates=[int(v) for v in re.findall(r'^\s*(\d+):[^\n]*// Method [^\n]*\.updateEntities:\(\)V',tick,re.M)]
    assert len(clicks)==2 and len(updates)==1 and max(clicks)<updates[0],(clicks,updates)
    classes=ROOT/'.local/use-order-oracle/classes';classes.mkdir(parents=True,exist_ok=True)
    sources=[str(ROOT/'tools/java'/n) for n in ('FoodOracle.java','ItemFoodOracle.java','UseOrderOracle.java')]
    subprocess.run(['javac','-encoding','UTF-8','-source','8','-target','8','-cp',cp,'-d',str(classes),*sources],check=True)
    result=subprocess.run([java_executable(),'-Xmx1G','-cp',str(classes)+os.pathsep+cp,'UseOrderOracle'],capture_output=True,text=True,encoding='utf-8')
    if result.returncode:
        print(result.stderr);result.check_returncode()
    rows=json.loads(result.stdout)
    data={'right_click_bytecode_offsets':clicks,'update_entities_bytecode_offset':updates[0],'cases':rows}
    (ROOT/'tests/fixtures/vanilla189-use-order.json').write_text(json.dumps(data,separators=(',',':'))+'\n',encoding='utf-8')
    print(f'Original right-click offsets {clicks} precede updateEntities {updates[0]}; exported {len(rows)} traces')

if __name__=='__main__':main()
