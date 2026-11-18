"""Export original item-food finish, potion requests and use countdown traces."""
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
    classes=ROOT/'.local/vanilla-item-food-oracle/classes';classes.mkdir(parents=True,exist_ok=True)
    sources=[str(ROOT/'tools/java'/name) for name in ('FoodOracle.java','ItemFoodOracle.java')]
    subprocess.run(['javac','-encoding','UTF-8','-source','8','-target','8','-cp',cp,'-d',str(classes),*sources],check=True)
    output=subprocess.run([java_executable(),'-Xmx1G','-cp',str(classes)+os.pathsep+cp,'ItemFoodOracle'],capture_output=True,text=True,encoding='utf-8')
    if output.returncode:
        print(output.stderr)
        output.check_returncode()
    data=json.loads(output.stdout);data['original_jar_sha256']=hashlib.sha256(args.jar.read_bytes()).hexdigest()
    (ROOT/'tests/fixtures/vanilla189-item-food.json').write_text(json.dumps(data,separators=(',',':'))+'\n',encoding='utf-8')
    table='// Original ItemFood potion configuration; duration is stored in seconds.\nconst EFFECTS:&[(u16,u8,i32,i32,u32)]=&[\n'
    table+='\n'.join(f'({item},{effect},{duration},{amp},{chance}),' for item,effect,duration,amp,chance in data['properties'])+'\n];\n'
    (ROOT/'src/vanilla/item_food_data.rs').write_text(table,encoding='utf-8')
    print(f'Exported {len(data["finish"])} actual food finishes and {len(data["use"])} use-countdown traces')

if __name__=='__main__':main()
