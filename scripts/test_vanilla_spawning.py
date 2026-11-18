"""Export actual original SpawnerAnimals.performWorldGenSpawning results."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
from vanilla_world import ROOT,classpath

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--jar',type=Path,default=Path(os.environ.get('APPDATA',str(Path.home())))/'.minecraft/versions/1.8.9/1.8.9.jar')
    args=parser.parse_args();cp=classpath(args.jar)
    classes=ROOT/'.local/vanilla-spawning-oracle/classes';classes.mkdir(parents=True,exist_ok=True)
    subprocess.run(['javac','-encoding','UTF-8','-source','8','-target','8','-cp',cp,'-d',str(classes),str(ROOT/'tools/java/SpawningOracle.java')],check=True)
    output=subprocess.run(['java','-cp',str(classes)+os.pathsep+cp,'SpawningOracle'],check=True,capture_output=True,text=True,encoding='utf-8')
    data=json.loads(output.stdout);data['original_jar_sha256']=hashlib.sha256(args.jar.read_bytes()).hexdigest()
    (ROOT/'tests/fixtures/vanilla189-spawning.json').write_text(json.dumps(data,separators=(',',':'))+'\n',encoding='utf-8')
    names={'Sheep':'Sheep','Rabbit':'Rabbit','Pig':'Pig','Chicken':'Chicken','Cow':'Cow','Wolf':'Wolf','EntityHorse':'Horse','MushroomCow':'Mooshroom'}
    lines=['// Generated from original 1.8.9 CREATURE spawn lists; preserves duplicate entries.','const SPAWN_LISTS: &[(u8,f32,&[SpawnEntry])] = &[']
    for row in data['spawn_lists']:
        entries=', '.join(f'SpawnEntry{{kind:AnimalKind::{names[kind]},weight:{weight},min:{minimum},max:{maximum}}}' for kind,weight,minimum,maximum in row['entries'])
        lines.append(f'({row["id"]},f32::from_bits({row["chance_bits"]}),&[{entries}]),')
    lines.append('];')
    (ROOT/'src/vanilla/spawning_data.rs').write_text('\n'.join(lines)+'\n',encoding='utf-8')
    print(f'Exported {len(data["cases"])} actual world-generation spawning cases, {sum(len(r["events"]) for r in data["cases"])} spawn events')

if __name__=='__main__':main()
