"""Transcribe the owned 1.8.9 stronghold block templates into native Rust."""
from pathlib import Path
import json
import re
from port_village_templates import args,calls,endblock
ROOT=Path(__file__).resolve().parents[1]
SOURCE=ROOT/'.local/reference/java/net/minecraft/world/gen/structure/StructureStrongholdPieces.java'
registry=json.loads((ROOT/'.local/resources/registry.json').read_text(encoding='utf-8'))
IDS={v['name']:int(k) for k,v in registry['blocks'].items()};IDS.update(air=0,oak_fence=85,oak_door=64)
DEFAULTS={r[0]:r[2] for r in json.loads((ROOT/'tests/fixtures/vanilla189-village-population.json').read_text())['properties']}

def expr(s):
    for name,value in {'field_143013_d':'door','field_74996_b':'exits[0]','field_74997_c':'exits[1]','field_74995_d':'exits[2]','field_74999_h':'exits[3]','isLargeRoom':'large','roomType':'room_type','expandsX':'exits[0]','expandsZ':'exits[1]','field_74993_a':'steps','coordBaseMode':'orientation','hasMadeChest':'chest_made'}.items():s=s.replace('this.'+name,'self.'+value)
    for name,value in [('OPENING','Opening'),('WOOD_DOOR','Wood'),('GRATES','Grates'),('IRON_DOOR','Iron')]:s=s.replace('Stronghold.Door.'+name,'Door::'+value)
    for name,hor,index in [('NORTH',2,2),('SOUTH',0,3),('WEST',1,4),('EAST',3,5)]:s=s.replace(f'EnumFacing.{name}.getHorizontalIndex()',str(hor)).replace(f'EnumFacing.{name}.getIndex()',str(index)).replace('EnumFacing.'+name,str(hor))
    for name,value in [('STONE',0),('SMOOTHBRICK',5)]:s=s.replace('BlockStoneSlab.EnumType.'+name+'.getMetadata()',str(value))
    s=calls(s,r'this.getMetadataWithOffset',lambda m,a:f'i32::from(rotate_metadata({IDS[a[0][7:]]}, ({expr(a[1])}) as u8, self.orientation))')
    s=calls(s,r'Blocks\.(\w+).getStateFromMeta',lambda m,a:f'(({IDS[m[1]]} << 4) | ({expr(a[0])}) as u16)')
    s=re.sub(r'Blocks\.(\w+).getDefaultState\(\)',lambda m:str(DEFAULTS[IDS[m[1]]]),s)
    s=s.replace('.withProperty(BlockEndPortalFrame.EYE, random.nextFloat() > 0.9f)',' | if random.next_float() > 0.9 { 4 } else { 0 }')
    return s.replace('structureBoundingBox','clip').replace('random.nextFloat()','random.next_float()').replace('random.nextInt(','random.next_int(').replace('0.07f','0.07').replace('0.1f','0.1')

def replace_block(body,needle,replacement):
    start=body.index(needle);end=endblock(body,body.index('{',start));return body[:start]+replacement+body[end+1:]

def method(name,body,door=False):
    if not door and 'if (this.isLiquid' in body:body=replace_block(body,'if (this.isLiquid','')
    if name=='PortalRoom':
        body=replace_block(body,'if (!this.hasSpawner','self.spawn_silverfish(world, clip);')
        start=body.index('n = EnumFacing.NORTH.getHorizontalIndex();');finish=endblock(body,body.index('{',body.index('if (this.coordBaseMode != null)',start)))
        body=body[:start]+'let (n,n4,n5,n6) = match self.orientation { 0 => (0,2,3,1), 3 => (3,1,0,2), 1 => (1,3,0,2), _ => (2,0,3,1) };'+body[finish+1:]
    if name=='ChestCorridor':body=replace_block(body,'if (!this.hasMadeChest','if !self.chest_made && clip.contains(self.point(3,2,3)) {\nself.chest_made=true;\nself.chest(world,clip,random, [3,2,3], 0);\n}')
    if name=='RoomCrossing':
        body=replace_block(body,'default:','')
        end=endblock(body,body.index('{',body.index('switch (this.roomType)')))
        body=body[:end]+'default: {\n}\n'+body[end:]
    body=calls(body,r'this.generateChestContents',lambda m,a:f'self.chest(world, clip, random, [{a[3]},{a[4]},{a[5]}], '+('1' if 'Library' in a[6] else '2')+')')
    lines=[]
    for raw in body.splitlines():
        s=raw.strip()
        if not s or s in ['return true;','break;']:continue
        if m:=re.match(r'for \((?:int )?(\w+) = (.*); \1 (<=|<) (.*); (.*)\) \{',s):
            var,lo,cmp,hi,step=m.groups();step=1 if step=='++'+var else step.removeprefix(var+' += ');seq=f'({expr(lo)}..{"=" if cmp=="<=" else ""}{expr(hi)})'
            if step!=1:seq+=f'.step_by(({expr(step)}) as usize)'
            s=f'for {var} in {seq} {{'
        elif m:=re.match(r'int (\w+);',s):s=f'let mut {m[1]}: i32;'
        elif m:=re.match(r'(int|boolean) (\w+) = (.*);',s):s=f'let mut {m[2]} = {expr(m[3])};'
        elif m:=re.match(r'switch \((.*)\) \{',s):s=f'match {expr(m[1])} {{'
        elif s=='default: {':s=('Door::Opening' if door else '_')+' => {'
        elif m:=re.match(r'case (\w+): \{',s):
            val=m[1]
            if door:val='Door::'+{'OPENING':'Opening','WOOD_DOOR':'Wood','GRATES':'Grates','IRON_DOOR':'Iron'}[val]
            s=f'{val} => {{'
        elif m:=re.match(r'if \((.*)\) continue;',s):s=f'if {expr(m[1])} {{continue;}}'
        elif m:=re.match(r'this\.(\w+)\((.*)\);',s):
            call,a=m[1],args(m[2]);a=[expr(t) for t in a]
            names={'fillWithBlocks':'fill','setBlockState':'place','fillWithRandomizedBlocks':'stones','placeDoor':'door','randomlyPlaceBlock':'random_place','func_175805_a':'random_fill'}
            if call=='fillWithRandomizedBlocks':a.pop()
            s='self.'+names[call]+'('+', '.join(a)+');'
        else:s=expr(s)
        lines.append(s)
    signature='pub(super) fn template_'+name.lower()+'(&mut self, world: &mut impl StrongholdWorld, clip: BoundingBox, random: &mut JavaRandom)'
    if door:signature='fn door(&self, world: &mut impl StrongholdWorld, random: &mut JavaRandom, clip: BoundingBox, door: Door, n:i32,n2:i32,n3:i32)'
    return signature+' {\n'+'\n'.join(lines)+'\n}'

text=SOURCE.read_text();output=['//! Original stronghold block template transcription.','#![allow(unused_variables,unused_mut,unused_assignments,unused_parens,clippy::double_parens,clippy::unnecessary_cast,clippy::too_many_arguments)]','use super::*;','impl Piece {']
for name in ['PortalRoom','Crossing','Library','Prison','RoomCrossing','RightTurn','LeftTurn','StairsStraight','ChestCorridor','Straight','Stairs','Corridor']:
    start=text.index('public static class '+name+'\n');start=text.index('public boolean addComponentParts',start);start=text.index('{',start);end=endblock(text,start);output.append(method(name,text[start+1:end]))
start=text.index('protected void placeDoor');start=text.index('{',start);end=endblock(text,start);output.append(method('Door',text[start+1:end],True));output.append('}')
(ROOT/'src/vanilla/stronghold_templates.rs').write_text('\n'.join(output),encoding='utf-8')
