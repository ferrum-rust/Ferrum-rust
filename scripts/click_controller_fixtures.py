"""Record original clickMouse + held damage controller state and protocol output."""
import gzip,json,os,subprocess
from pathlib import Path
from vanilla_world import classpath,java_executable

ROOT=Path(__file__).resolve().parents[1]
version=Path(os.environ["APPDATA"])/".minecraft/versions/1.8.9"
out=ROOT/".local/click-controller-oracle"
out.mkdir(parents=True,exist_ok=True)
cp=classpath(version/"1.8.9.jar")
javac=os.environ.get("MC189_JAVAC",r"C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe")
subprocess.run([javac,"-encoding","UTF-8","-cp",cp,"-d",str(out),*[str(ROOT/"tools/java"/(n+".java")) for n in ["MovementOracle","EntityPickOracle","MiddlePickOracle","ClickControllerOracle","BorderClickOracle"]]],check=True)
result=subprocess.run([java_executable(),"-Xmx512M","-cp",str(out)+os.pathsep+cp,"ClickControllerOracle"],capture_output=True,text=True)
if result.returncode:
    print(result.stderr)
    result.check_returncode()
data=json.loads(result.stdout)
(ROOT/"tests/fixtures/vanilla189-click-controller.json.gz").write_bytes(gzip.compress(json.dumps(data,separators=(",",":")).encode(),mtime=0))
print(len(data),"traces",sum(len(s[2]) for s in data),"ticks")
sounds=subprocess.run([java_executable(),"-Xmx512M","-cp",str(out)+os.pathsep+cp,"ClickControllerOracle","sounds"],capture_output=True,text=True,check=True)
rows=json.loads(sounds.stdout)
assets=Path(os.environ["APPDATA"])/".minecraft/assets"
key=json.loads((assets/"indexes/1.8.json").read_text())["objects"]["minecraft/sounds.json"]["hash"]
sound_registry=json.loads((assets/"objects"/key[:2]/key).read_text())
names=sorted({e for _,e,_,_ in rows})
defaults=[(0,0.25,0.5)]*256
for block,event,volume,pitch in rows:defaults[block]=(names.index(event),volume,pitch)
text="// Original Block.stepSound for the held mining hit.\n"
text+="const SOUNDS:[(usize,f32,f32);256]=[\n"+"".join(f"    ({i},{float(v)!r},{float(p)!r}),\n" for i,v,p in defaults)+"];\n"
text+="const CLIPS:&[(&str,&[&str])]=&[\n"
for name in names:
    clips=sound_registry.get(name,{}).get("sounds",[])
    assert all(isinstance(s,str) for s in clips)
    text+=f'    ({json.dumps(name)},&[{",".join(json.dumps(s) for s in clips)}]),\n'
(ROOT/"src/vanilla/click_sounds_data.rs").write_text(text+"];\n",encoding="utf-8")
border=subprocess.run([java_executable(),"-Xmx512M","-cp",str(out)+os.pathsep+cp,"BorderClickOracle"],capture_output=True,text=True)
if border.returncode:
    print(border.stderr)
    border.check_returncode()
bounds=json.loads(border.stdout)
(ROOT/"tests/fixtures/vanilla189-click-border.json.gz").write_bytes(gzip.compress(json.dumps(bounds,separators=(",",":")).encode(),mtime=0))
table="// Actual Block registry names used by ItemStack.canDestroy.\nconst BLOCK_NAMES:&[(u16,&str)]=&[\n"
table+="".join(f"    ({id},{json.dumps(name)}),\n" for id,name in bounds["names"])+"];\n"
(ROOT/"src/vanilla/click_border_data.rs").write_text(table,encoding="utf-8")
print(len(bounds["border"]),"S44 packets",len(bounds["tools"]),"CanDestroy comparisons")
