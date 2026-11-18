"""Export original item-NBT payloads including modified UTF and nonfinite numbers."""
import argparse,json,os,subprocess
from pathlib import Path
from vanilla_world import ROOT,classpath,java_executable
def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--jar',type=Path,default=Path(os.environ['APPDATA'])/'.minecraft/versions/1.8.9/1.8.9.jar')
    args=parser.parse_args();cp=classpath(args.jar);classes=ROOT/'.local/item-nbt-oracle/classes';classes.mkdir(parents=True,exist_ok=True)
    subprocess.run(['javac','-encoding','UTF-8','-source','8','-target','8','-cp',cp,'-d',str(classes),str(ROOT/'tools/java/ItemNbtOracle.java')],check=True)
    result=subprocess.run([java_executable(),'-cp',str(classes)+os.pathsep+cp,'ItemNbtOracle'],capture_output=True,text=True,encoding='utf-8')
    if result.returncode:print(result.stderr);result.check_returncode()
    data=json.loads(result.stdout);(ROOT/'tests/fixtures/vanilla189-item-nbt.json').write_text(json.dumps(data)+'\n',encoding='utf-8');print('Exported',len(data),'actual item NBT payloads')
    subprocess.run(['javac','-encoding','UTF-8','-source','8','-target','8','-cp',cp,'-d',str(classes),str(ROOT/'tools/java/ItemNbtEqualityOracle.java')],check=True)
    equality=subprocess.run([java_executable(),'-cp',str(classes)+os.pathsep+cp,'ItemNbtEqualityOracle',str(ROOT/'tests/fixtures/vanilla189-item-nbt.json')],capture_output=True,text=True,encoding='utf-8')
    if equality.returncode:print(equality.stderr);equality.check_returncode()
    data=json.loads(equality.stdout);(ROOT/'tests/fixtures/vanilla189-item-nbt-equality.json').write_text(json.dumps(data,separators=(',',':'))+'\n',encoding='utf-8');print('Exported',len(data),'actual NBT equality comparisons')
if __name__=='__main__':main()
