"""Run actual original block placement requirements and ItemBlock registry."""
import base64
import json
import os
from pathlib import Path
import subprocess
from vanilla_world import classpath, java_executable
ROOT=Path(__file__).resolve().parents[1]
out=ROOT/'.local/collision-shape-oracle'
out.mkdir(parents=True,exist_ok=True)
cp=classpath(Path(os.environ['APPDATA'])/'.minecraft/versions/1.8.9/1.8.9.jar')
javac=os.environ.get('MC189_JAVAC',r'C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe')
subprocess.run([javac,'-encoding','UTF-8','-cp',cp,'-d',str(out),
                *[str(ROOT/'tools/java'/f'{name}.java') for name in ['MovementOracle','MovementAttributesOracle','CollisionShapeOracle','HandPlacementOracle']]],check=True)
result=subprocess.run([java_executable(),'-Xmx768M','-cp',str(out)+os.pathsep+cp,'HandPlacementOracle'],capture_output=True,text=True)
if result.returncode:
    print(result.stderr)
    result.check_returncode()
data=json.loads(result.stdout)
placed=data.pop('placed')
for row in data['bits']:
    row[3]=base64.b64decode(row[3]).hex()
(ROOT/'tests/fixtures/vanilla189-hand-placement.json').write_text(json.dumps(data,separators=(',',':'))+'\n',encoding='utf-8')
fire=[0]*256
generic=[False]*256
for block,item,normal,encouragement,*values in data['registry']:
    fire[block]=int(encouragement>0)
    if item==block and normal:
        generic[block]=True
table='// Original generic ItemBlock registry, metadata and fire properties.\n'
table+='const GENERIC: [bool;256] = ['+', '.join(str(v).lower() for v in generic)+'];\n'
table+='const CATCH_FIRE: [u8;256] = ['+', '.join(map(str,fire))+'];\n'
replace=[0]*256
for state,answer in data['replaceable']:
    if answer:replace[state>>4]|=1<<(state&15)
table+='const TARGET_REPLACEABLE: [u16;256] = ['+', '.join(map(str,replace))+'];\n'
import itertools
profiles=[]
profile_ids={}
index=[[0]*16 for _ in range(256)]
for row in placed:
    for meta in range(16):
        values=row[1+144*meta:1+144*(meta+1)]
        runs=tuple((len(list(group)),value) for value,group in itertools.groupby(values))
        if runs not in profile_ids:profile_ids[runs]=len(profiles);profiles.append(runs)
        index[row[0]][meta]=profile_ids[runs]
table+='const PROFILE: [[u16;16];256] = [\n'+''.join('    ['+', '.join(map(str,row))+'],\n' for row in index)+'];\n'
table+='const STATE_RUNS: &[&[(u8,i32)]] = &[\n'+''.join('    &['+', '.join(f'({n},{v})'for n,v in row)+'],\n'for row in profiles)+'];\n'
assets=Path(os.environ['APPDATA'])/'.minecraft/assets'
asset_hash=json.loads((assets/'indexes/1.8.json').read_text(encoding='utf-8'))['objects']['minecraft/sounds.json']['hash']
sound_registry=json.loads((assets/'objects'/asset_hash[:2]/asset_hash).read_text(encoding='utf-8'))
events=sorted({event for _,event,_,_ in data['sounds']})
clips=[sound_registry[event]['sounds'] for event in events]
assert all(isinstance(path,str) for files in clips for path in files)
properties=[(0,1.0,0.8)]*256
for block,event,volume,pitch in data['sounds']:properties[block]=(events.index(event),volume,pitch)
table+='const PLACE_SOUNDS: [(usize,f32,f32);256] = [\n'+''.join(f'    ({event},{float(volume)!r},{float(pitch)!r}),\n' for event,volume,pitch in properties)+'];\n'
table+='const PLACE_CLIPS: &[&[&str]] = &[\n'+''.join('    &['+', '.join(json.dumps(path) for path in files)+'],\n' for files in clips)+'];\n'
(ROOT/'src/vanilla/hand_placement_data.rs').write_text(table,encoding='utf-8')
print('fixed',len(data['bits'])*len(data['states']),'random',len(data['random']),'generic item blocks',sum(generic),'clicks',len(data['clicks']),'state profiles',len(profiles))
