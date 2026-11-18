"""Compare special block left-click callbacks and hit particles to 1.8.9 bytecode."""
import gzip,json,os,subprocess
from pathlib import Path
from vanilla_world import classpath,java_executable
ROOT=Path(__file__).resolve().parents[1]
out=ROOT/".local/special-click-oracle";out.mkdir(parents=True,exist_ok=True)
cp=classpath(Path(os.environ["APPDATA"])/".minecraft/versions/1.8.9/1.8.9.jar")
compiler=os.environ.get("MC189_JAVAC",r"C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe")
subprocess.run([compiler,"-encoding","UTF-8","-cp",cp,"-d",str(out),*[str(ROOT/"tools/java"/(n+".java")) for n in ["MovementOracle","EntityPickOracle","SpecialClickOracle","NoteOracle"]]],check=True)
p=subprocess.run([java_executable(),"-Xmx512M","-cp",str(out)+os.pathsep+cp,"SpecialClickOracle"],capture_output=True,text=True)
if p.returncode:
    print(p.stderr);p.check_returncode()
data=json.loads(p.stdout)
(ROOT/"tests/fixtures/vanilla189-special-click.json.gz").write_bytes(gzip.compress(json.dumps(data,separators=(",",":")).encode(),mtime=0))
instrument=[0]*256
for block,category in data["instruments"]:instrument[block]=category
render=[False]*4096
for state,points in data["hits"]:render[state]=any(p is not None for p in points)
text="// Original note-block instrument materials and particle render predicates.\n"
text+="const INSTRUMENTS:[u8;256]=[\n"+"".join("    "+",".join(map(str,instrument[i:i+32]))+",\n" for i in range(0,256,32))+"];\n"
text+="const HAS_HIT:[u8;512]=[\n"+"".join("    "+",".join(str(sum(int(render[(i+b)*8+k])<<k for k in range(8))) for b in range(16))+",\n" for i in range(0,512,16))+"];\n"
(ROOT/"src/vanilla/special_click_data.rs").write_text(text,encoding="utf-8")
print(len(data["clicks"]),"special clicks",len(data["hits"])*6,"original hit particles")

note=subprocess.run([java_executable(),"-Xmx256M","-cp",str(out)+os.pathsep+cp,"NoteOracle"],capture_output=True,text=True)
if note.returncode:
    print(note.stderr);note.check_returncode()
data=json.loads(note.stdout)
(ROOT/"tests/fixtures/vanilla189-note-blocks.json.gz").write_bytes(gzip.compress(json.dumps(data,separators=(",",":")).encode(),mtime=0))
keys=[tuple(row) for row in data["masks"]]
text="// Original positive weak/strong directions in isolation (bit = EnumFacing ordinal).\n"
text+="const SOURCES: &[(u16,u8,u8)] = &[\n"+"".join(f"    ({state},{weak},{strong}),\n" for state,weak,strong in keys)+"];\n"
(ROOT/"src/vanilla/note_power_data.rs").write_text(text,encoding="utf-8")
print(len(data["turns"]),"tuning cases",len(data["power"]),"power + rising/held/falling cases")
