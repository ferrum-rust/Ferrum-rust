"""Export original armor/durability callbacks and separate enchantment RNG tails."""
import argparse,json,os,subprocess,hashlib
from pathlib import Path
from vanilla_world import ROOT,classpath,java_executable
def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--jar',type=Path,default=Path(os.environ['APPDATA'])/'.minecraft/versions/1.8.9/1.8.9.jar')
    args=parser.parse_args();cp=classpath(args.jar)
    classes=ROOT/'.local/armor-oracle/classes';classes.mkdir(parents=True,exist_ok=True)
    subprocess.run(['javac','-encoding','UTF-8','-source','8','-target','8','-cp',cp,'-d',str(classes),*[str(ROOT/'tools/java'/n) for n in ('FoodOracle.java','ArmorOracle.java')]],check=True)
    result=subprocess.run([java_executable(),'-Xmx1G','-cp',str(classes)+os.pathsep+cp,'ArmorOracle'],capture_output=True,text=True,encoding='utf-8')
    if result.returncode:print(result.stderr);result.check_returncode()
    data=json.loads(result.stdout);data['original_jar_sha256']=hashlib.sha256(args.jar.read_bytes()).hexdigest()
    (ROOT/'tests/fixtures/vanilla189-armor.json').write_text(json.dumps(data,separators=(',',':'))+'\n',encoding='utf-8')
    rows='// Actual original damageable item registry.\nconst ITEMS:&[(u16,i32,bool,i32)]=&[\n'+'\n'.join(f'({id},{max},{str(armor).lower()},{points}),' for id,max,armor,points in data['registry'])+'\n];\n'
    (ROOT/'src/vanilla/armor_data.rs').write_text(rows,encoding='utf-8')
    print({name:len(data[name]) for name in ('registry','damage','armor','protection')})
if __name__=='__main__':main()
