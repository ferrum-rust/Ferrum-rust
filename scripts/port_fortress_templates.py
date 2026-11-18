"""Native transcription of original owned Nether fortress block templates."""
from pathlib import Path
import json
import re
from port_village_templates import args,calls,endblock
ROOT=Path(__file__).resolve().parents[1]
SOURCE=ROOT/'.local/reference/java/net/minecraft/world/gen/structure/StructureNetherBridgePieces.java'
registry=json.loads((ROOT/'.local/resources/registry.json').read_text(encoding='utf-8'))
IDS={v['name']:int(k) for k,v in registry['blocks'].items()};IDS['air']=0
DEFAULTS={r[0]:r[2] for r in json.loads((ROOT/'tests/fixtures/vanilla189-village-population.json').read_text())['properties']}
def expr(s):
    s=calls(s,r'Math.(max|min)',lambda m,a:'std::cmp::'+m[1]+'('+', '.join(expr(t)for t in a)+')')
    s=calls(s,r'this.getMetadataWithOffset',lambda m,a:f'i32::from(rotate_metadata({IDS[a[0][7:]]}, ({expr(a[1])}) as u8, self.orientation))')
    s=calls(s,r'Blocks\.(\w+).getStateFromMeta',lambda m,a:f'(({IDS[m[1]]} << 4) | ({expr(a[0])}) as u16)')
    s=re.sub(r'Blocks\.(\w+).getDefaultState\(\)',lambda m:str(DEFAULTS[IDS[m[1]]]),s)
    return s.replace('structureBoundingBox','clip').replace('random.nextInt(','random.next_int(').replace('random2.nextInt(','random2.next_int(').replace('this.fillSeed','self.fill_seed')
def replace_block(body,needle,replacement):
    start=body.index(needle);end=endblock(body,body.index('{',start));return body[:start]+replacement+body[end+1:]
def method(name,body):
    if name in ['Corridor','Corridor2']:
        pos='[3,2,3]' if name=='Corridor' else '[1,2,3]';body=replace_block(body,'if (this.field_11102',f'self.chest(world,clip,random,{pos});')
    if name=='Throne':body=replace_block(body,'if (!this.hasSpawner','self.spawner(world,clip);').replace('BlockPos blockPos;','')
    if name=='Entrance':
        start=body.index('BlockPos blockPos =');end=endblock(body,body.index('{',body.index('if (structureBoundingBox.isVecInside',start)));body=body[:start]+'self.force_lava(world,clip,random);'+body[end+1:]
    body=body.replace('Random random2 = new Random(this.fillSeed);','let mut random2=JavaRandom::new(self.fill_seed as i64);')
    lines=[]
    for raw in body.splitlines():
        s=raw.strip()
        if not s or s=='return true;':continue
        if m:=re.match(r'for \((?:int )?(\w+) = (.*); \1 (<=|<) (.*); (.*)\) \{',s):
            var,lo,cmp,hi,step=m.groups();amount=1 if step=='++'+var else step.removeprefix(var+' += ');seq=f'({expr(lo)}..{"=" if cmp=="<=" else ""}{expr(hi)})'
            if amount!=1:seq+=f'.step_by(({expr(amount)}) as usize)'
            s=f'for {var} in {seq} {{'
        elif m:=re.match(r'int (\w+);',s):s=f'let mut {m[1]}: i32;'
        elif m:=re.match(r'int (\w+) = (.*);',s):s=f'let mut {m[1]} = {expr(m[2])};'
        elif m:=re.match(r'if \((.*)\) continue;',s):s=f'if {expr(m[1])} {{continue;}}'
        elif m:=re.match(r'this\.(\w+)\((.*)\);',s):
            call,a=m[1],args(m[2]);a=[expr(t) for t in a];names={'fillWithBlocks':'fill','setBlockState':'place','replaceAirAndLiquidDownwards':'down'};s='self.'+names[call]+'('+', '.join(a)+');'
        else:s=expr(s)
        lines.append(s)
    return f'pub(super) fn template_{name.lower()}(&mut self,world:&mut impl FortressWorld,clip:BoundingBox,random:&mut JavaRandom) {{\n'+'\n'.join(lines)+'\n}'
text=SOURCE.read_text();output=['//! Original Nether fortress block template transcription.','#![allow(unused_variables,unused_mut,unused_assignments,unused_parens,clippy::double_parens,clippy::unnecessary_cast)]','use super::*;','impl Piece {']
for name in ['Straight','Crossing3','Crossing','Stairs','Throne','Entrance','Corridor5','Crossing2','Corridor2','Corridor','Corridor3','Corridor4','NetherStalkRoom','End']:
    start=text.index('public static class '+name+'\n');start=text.index('public boolean addComponentParts',start);start=text.index('{',start);end=endblock(text,start);output.append(method(name,text[start+1:end]))
output.append('}')
(ROOT/'src/vanilla/fortress_templates.rs').write_text('\n'.join(output),encoding='utf-8')
