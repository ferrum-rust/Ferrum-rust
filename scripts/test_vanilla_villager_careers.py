"""Export original 1.8.9 villager careers, prices, enchanting rules, and RNG."""
import argparse
import json
import os
import subprocess
from pathlib import Path
from vanilla_world import ROOT,classpath,java_executable

def rule(entry):
    kind=entry[0]
    if kind=='book':
        return 'Rule::Book'
    item,price=entry[1:3]
    args=[*item,*price]
    if kind=='exchange':
        args.extend([*entry[3],*entry[4]])
    return f'Rule::{kind.title()}('+','.join(str(value) for value in args)+')'

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--jar',type=Path,default=Path(os.environ['APPDATA'])/'.minecraft/versions/1.8.9/1.8.9.jar')
    args=parser.parse_args()
    cp=classpath(args.jar)
    classes=ROOT/'.local/villager-career-oracle/classes'
    classes.mkdir(parents=True,exist_ok=True)
    sources=[str(ROOT/'tools/java'/n) for n in ('FoodOracle.java','MerchantOracle.java','VillagerCareerOracle.java')]
    subprocess.run(['javac','-encoding','UTF-8','-source','8','-target','8','-cp',cp,'-d',str(classes),*sources],check=True)
    result=subprocess.run([java_executable(),'-Xmx1G','-cp',str(classes)+os.pathsep+cp,'VillagerCareerOracle'],capture_output=True,text=True,encoding='utf-8')
    if result.returncode:
        print(result.stderr);result.check_returncode()
    data=json.loads(result.stdout)
    (ROOT/'tests/fixtures/vanilla189-villager-careers.json').write_text(json.dumps(data,separators=(',',':'))+'\n',encoding='utf-8')
    lines=['// Original 1.8.9 trade registry, grouped by profession, career and level.','const CAREERS:&[&[&[&[Rule]]]]=&[']
    for profession in data['registry']:
        lines.append('    &[')
        for career in profession:
            lines.append('        &[')
            for level in career:
                lines.append('            &['+', '.join(rule(entry) for entry in level)+'],')
            lines.append('        ],')
        lines.append('    ],')
    lines.append('];')
    lines.append('const BOOK_POOL:&[(u16,i32,i32)]=&['+','.join(f'({id},{minimum},{maximum})' for id,minimum,maximum in data['book_pool'])+'];')
    (ROOT/'src/vanilla/villager_career_data.rs').write_text('\n'.join(lines)+'\n',encoding='utf-8')
    items=['// Actual item enchantability and Enchantment.type.canEnchantItem registry.','const ITEM_ENCHANT:&[(u16,i32,&[u16])]=&[']
    for id,power,applicable in data['items']:
        items.append(f'    ({id},{power},&['+','.join(map(str,applicable))+']),')
    items.append('];')
    (ROOT/'src/vanilla/item_enchant_data.rs').write_text('\n'.join(items)+'\n',encoding='utf-8')
    levels=sum(len(row[2]) for row in data['careers'])
    print(f'Exported {len(data["careers"])} actual careers and {levels} progression checkpoints')

if __name__=='__main__':main()
