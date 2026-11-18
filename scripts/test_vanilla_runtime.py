"""Export actual WorldServer nested block-read/population/lighting traces."""
import json,os,subprocess,sys
from pathlib import Path
from vanilla_world import ROOT,classpath,java_executable
cp=classpath(Path(os.environ['APPDATA'])/'.minecraft/versions/1.8.9/1.8.9.jar')
out=ROOT/'.local/oracles/runtime';out.mkdir(parents=True,exist_ok=True)
subprocess.run(['javac','--release','8','-cp',cp,'-d',str(out),str(ROOT/'tools/java/VanillaWorldBridge.java'),str(ROOT/'tools/java/RuntimeOracle.java')],check=True)
full='--full' in sys.argv
extended='--extended' in sys.argv
if extended:
    data={'cases':[],'fresh_jvm_per_case':True}
    # Coordinates come from the original five-generator discovery oracle.
    origins=[(189,0,0),(189,14,19),(189,-47,47),(189,-6,-47),(189,-73,82),(189,15,19),(0,67,40),(-1,-87,74)]
    for seed,x,z in origins:
        for order in [0,1,2]:
            row=json.loads(subprocess.check_output([java_executable(),'-cp',str(out)+os.pathsep+cp,'RuntimeOracle',str(out/'world'),'full',str(seed),str(x),str(z),str(order),'4'],cwd=ROOT))
            data['cases'].extend(row['cases'])
            print(f'Exported original populated region {seed} ({x},{z}) order{order}',flush=True)
else:
    data=json.loads(subprocess.check_output([java_executable(),'-cp',str(out)+os.pathsep+cp,'RuntimeOracle',str(out/'world')]+(['full'] if full else []),cwd=ROOT))
name='vanilla189-runtime-extended.json' if extended else 'vanilla189-runtime-full.json' if full else 'vanilla189-runtime.json'
(ROOT/'tests/fixtures'/name).write_text(json.dumps(data,separators=(',',':'))+'\n',encoding='utf-8')
print(f'{len(data["cases"])} WorldServer runtime traces / {sum(len(c["chunks"]) for c in data["cases"])} complete chunk and light arrays')
