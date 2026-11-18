"""Transcribe owned 1.8.9 monument templates into native, runtime-independent Rust."""
from pathlib import Path
import json
import re
from port_village_templates import args,calls,endblock
ROOT=Path(__file__).resolve().parents[1]
SOURCE=ROOT/'.local/reference/java/net/minecraft/world/gen/structure/StructureOceanMonumentPieces.java'
registry=json.loads((ROOT/'.local/resources/registry.json').read_text(encoding='utf-8'))
IDS={v['name']:int(k) for k,v in registry['blocks'].items()};IDS['air']=0
DEFAULTS={r[0]:r[2] for r in json.loads((ROOT/'tests/fixtures/vanilla189-village-population.json').read_text())['properties']}
def expr(s):
    s=s.replace('this.field_175830_k','base').replace('this.field_175833_o','self.variant').replace('this.field_175834_o','self.variant')
    for name,val in [('field_175828_a',168<<4),('field_175826_b',168<<4|1),('field_175827_c',168<<4|2),('field_175824_d',168<<4|1),('field_175825_e',169<<4),('field_175822_f',9<<4)]:s=s.replace(name,str(val))
    for name,index in [('DOWN',0),('UP',1),('NORTH',2),('SOUTH',3),('WEST',4),('EAST',5)]:s=s.replace('EnumFacing.'+name+'.getIndex()',str(index))
    s=s.replace('.field_175967_a','.id').replace('.field_175965_b','.neighbors').replace('.field_175966_c','.open').replace('.func_175960_c()', '.open.iter().filter(|&&v|v).count()')
    s=re.sub(r'(\w+\.neighbors\[\d\]) == null',r'\1.is_none()',s)
    s=re.sub(r'(\w+\.neighbors\[\d\]) != null',r'\1.is_some()',s)
    s=calls(s,r'Blocks\.(\w+).getStateFromMeta',lambda m,a:f'(({IDS[m[1]]} << 4) | ({expr(a[0])}) as u16)')
    s=re.sub(r'Blocks\.(\w+).getDefaultState\(\)',lambda m:str(DEFAULTS[IDS[m[1]]]),s)
    s=s.replace('random.nextInt(','random.next_int(').replace('random.nextBoolean()','random.next_boolean()')
    s=s.replace('Math.max(world.getSeaLevel(), 64)','world.sea_level().max(64)').replace('this.boundingBox.minY','self.bounds.min[1]')
    s=s.replace('this.func_175818_a','self.intersects').replace('structureBoundingBox','clip')
    s=re.sub(r'\b0 \+ ', '',s)
    s=re.sub(r'\b(\w+) \+ 0\b', r'\1',s)
    s=s.replace('(random.next_int(4) == 0 ? 0 : 1)','(if random.next_int(4)==0 {0} else {1})')
    if '?' in s:
        m=re.fullmatch(r'bl \? (.*) : (.*)',s)
        if not m:raise ValueError(s)
        s=f'if bl {{ {m[1]} }} else {{ {m[2]} }}'
    return s
def method(name,body,signature=None):
    if name=='MonumentBuilding':
        start=body.index('for (Piece piece');end=endblock(body,body.index('{',start));body=body[:start]+body[end+1:]
    body=body.replace('boolean bl2 = bl =','boolean bl =')
    lines=[]
    if 'this.field_175830_k' in body:lines.append('let base=room(rooms,self.room.unwrap());')
    for raw in body.splitlines():
        s=raw.strip()
        if not s or s=='return true;':continue
        if m:=re.match(r'for \((?:int )?(\w+) = (.*); \1 (<=|<|>=|>) (.*); (.*)\) \{',s):
            var,lo,cmp,hi,step=m.groups();down=cmp in ['>=','>'];amount=1 if step in ['++'+var,'--'+var] else step.removeprefix(var+(' -= ' if down else ' += '))
            seq=f'({expr(hi)}..={expr(lo)}).rev()' if down else f'({expr(lo)}..{"=" if cmp=="<=" else ""}{expr(hi)})'
            if amount!=1:seq+=f'.step_by(({expr(amount)}) as usize)'
            s=f'for {var} in {seq} {{'
        elif m:=re.match(r'(int|boolean|IBlockState) (\w+);',s):s=f'let mut {m[2]}: '+{'int':'i32','boolean':'bool','IBlockState':'u16'}[m[1]]+';'
        elif m:=re.match(r'(int|boolean|IBlockState) (\w+) = (.*);',s):s=f'let mut {m[2]} = {expr(m[3])};'
        elif m:=re.match(r'RoomDefinition (\w+) = (.*);',s):
            value=expr(m[2]);value='room(rooms,'+value+'.unwrap())' if '.neighbors[' in value else value;s=f'let mut {m[1]} = {value};'
        elif m:=re.match(r'if \((.*)\) continue;',s):s=f'if {expr(m[1])} {{continue;}}'
        elif m:=re.match(r'\+\+(\w+);',s):s=m[1]+' += 1;'
        elif m:=re.match(r'(\w+) = (bl \? .*);',s):s=m[1]+' = '+expr(m[2])+';'
        elif m:=re.match(r'this\.(\w+)\((.*)\);',s):
            call,a=m[1],args(m[2]);a=[expr(t) for t in a]
            names={'fillWithBlocks':'fill','setBlockState':'place','replaceAirAndLiquidDownwards':'down','func_181655_a':'water','func_175821_a':'floor','func_175819_a':'water_replace','func_175817_a':'elder'}
            s='self.'+names.get(call,call)+'('+', '.join(a)+');'
        else:s=expr(s)
        lines.append(s)
    if signature is None:signature=f'pub(super) fn template_{name.lower()}(&self,world:&mut impl MonumentWorld,clip:BoundingBox,random:&mut JavaRandom,rooms:&[Option<Room>])'
    output=signature+' {\n'+'\n'.join(lines)+'\n}'
    return output.replace('iBlockState','block_state').replace('roomDefinition','room_definition')
text=SOURCE.read_text();output=['//! Original ocean-monument block template transcription.','#![allow(unused_variables,unused_mut,unused_assignments,unused_parens,clippy::double_parens,clippy::unnecessary_cast,clippy::too_many_arguments)]','use super::*;','use super::population::room;','impl Piece {']
for name in ['Penthouse','WingRoom','MonumentCoreRoom','DoubleYZRoom','DoubleXYRoom','DoubleZRoom','DoubleXRoom','DoubleYRoom','SimpleTopRoom','SimpleRoom','EntryRoom','MonumentBuilding']:
    start=text.index('public static class '+name+'\n');start=text.index('public boolean addComponentParts',start);start=text.index('{',start);end=endblock(text,start);output.append(method(name,text[start+1:end]))
for name in ['func_175840_a','func_175839_b','func_175837_c','func_175841_d','func_175835_e','func_175842_f','func_175838_g','func_175821_a']:
    start=text.index('void '+name+'(');start=text.index('{',start);end=endblock(text,start)
    if name=='func_175821_a':signature='pub(super) fn floor(&self,world:&mut impl MonumentWorld,clip:BoundingBox,n:i32,n2:i32,bl:bool)'
    else:signature='fn '+name+'(&self,'+('bl:bool,n:i32,' if name=='func_175840_a' else '')+'world:&mut impl MonumentWorld,random:&mut JavaRandom,clip:BoundingBox)'
    output.append(method(name,text[start+1:end],signature))
output.append('}')
(ROOT/'src/vanilla/monument_templates.rs').write_text('\n'.join(output),encoding='utf-8')
