"""Native source transcription of owned 1.8.9 scattered-feature templates."""
from pathlib import Path
import json
import re
from port_village_templates import args,calls,endblock
ROOT=Path(__file__).resolve().parents[1]
SOURCE=ROOT/'.local/reference/java/net/minecraft/world/gen/structure/ComponentScatteredFeaturePieces.java'
registry=json.loads((ROOT/'.local/resources/registry.json').read_text(encoding='utf-8'))
IDS={v['name']:int(k) for k,v in registry['blocks'].items()}
IDS['air']=0
IDS['oak_fence']=85
props=json.loads((ROOT/'tests/fixtures/vanilla189-village-population.json').read_text())['properties']
DEFAULTS={row[0]:row[2] for row in props}

def expr(s):
    s=s.replace('this.scatteredFeatureSizeX','self.size[0]').replace('this.scatteredFeatureSizeY','self.size[1]').replace('this.scatteredFeatureSizeZ','self.size[2]')
    for name,val in {'BlockPlanks.EnumType.SPRUCE.getMetadata()':1,'BlockSandStone.EnumType.SMOOTH.getMetadata()':2,'BlockSandStone.EnumType.CHISELED.getMetadata()':1,'BlockStoneSlab.EnumType.SAND.getMetadata()':1,'BlockStoneBrick.CHISELED_META':3,'~EnumDyeColor.ORANGE.getDyeDamage() & 0xF':1,'~EnumDyeColor.BLUE.getDyeDamage() & 0xF':11}.items():s=s.replace(name,str(val))
    for name,hor,index in [('DOWN',0,0),('UP',0,1),('NORTH',2,2),('SOUTH',0,3),('WEST',1,4),('EAST',3,5)]:
        s=s.replace(f'EnumFacing.{name}.getHorizontalIndex()',str(hor)).replace(f'EnumFacing.{name}.getIndex()',str(index))
    s=s.replace('.withProperty(BlockFlowerPot.CONTENTS, BlockFlowerPot.EnumFlowerType.MUSHROOM_RED)','')
    s=s.replace('.withProperty(BlockTripWireHook.ATTACHED, true)',' | 4').replace('.withProperty(BlockTripWire.ATTACHED, true)',' | 4')
    s=calls(s,r'BlockLever.getMetadataForFacing',lambda m,a:f'lever(({expr(a[0])}) as u8)')
    s=calls(s,r'EnumFacing.getFront',lambda m,a:expr(a[0]))
    s=calls(s,r'this.getMetadataWithOffset',lambda m,a:f'i32::from(rotate_metadata({IDS[a[0][7:]]}, ({expr(a[1])}) as u8, self.orientation))')
    s=calls(s,r'Blocks\.(\w+).getStateFromMeta',lambda m,a:f'(({IDS[m[1]]} << 4) | ({expr(a[0])}) as u16)')
    s=re.sub(r'Blocks\.(\w+).getDefaultState\(\)',lambda m:str(DEFAULTS[IDS[m[1]]]),s)
    s=s.replace('structureBoundingBox','clip')
    return s

def replace_block(body,needle,text):
    start=body.index(needle);end=endblock(body,body.index('{',start))
    return body[:start]+text+body[end+1:]

def method(name,body):
    if name!='DesertPyramid':body=replace_block(body,'if (!this.func_74935_a','')
    if name=='SwampHut':body=replace_block(body,'if (!this.hasWitch','self.spawn_witch(world, clip);')
    if name=='JunglePyramid':
        for flag,index,pos,extra in [('placedMainChest',0,[8,-3,3],None),('placedHiddenChest',1,[9,-3,10],None),('placedTrap1',2,[3,-2,1],2),('placedTrap2',3,[9,-2,3],4)]:
            call='temple_chest' if extra is None else 'trap'
            tail='' if extra is None else ', '+str(extra)
            body=replace_block(body,'if (!this.'+flag,f'self.{call}(world, clip, random, {index}, {pos}{tail});')
    if name=='DesertPyramid':body=replace_block(body,'for (EnumFacing enumFacing','for (index, dx, dz) in [(2, 0, -2), (3, 2, 0), (0, 0, 2), (1, -2, 0)] {\nself.temple_chest(world, clip, random, index, [10 + dx, -11, 10 + dz]);\n}')
    lines=[]
    for raw in body.splitlines():
        s=raw.strip()
        if not s or s=='return true;':continue
        if m:=re.match(r'for \((?:int )?(\w+) = (.*); \1 (<=|<) (.*); (.*)\) \{',s):
            var,lo,cmp,hi,step=m.groups();step=1 if step=='++'+var else step.removeprefix(var+' += ')
            seq=f'({expr(lo)}..{"=" if cmp=="<=" else ""}{expr(hi)})'
            if step!=1:seq+=f'.step_by(({expr(step)}) as usize)'
            s=f'for {var} in {seq} {{'
        elif m:=re.match(r'int (\w+);',s):s=f'let mut {m[1]}: i32;'
        elif m:=re.match(r'int (\w+) = (.*);',s):s=f'let mut {m[1]} = {expr(m[2])};'
        elif m:=re.match(r'this\.(\w+)\((.*)\);',s):
            call,a=m[1],args(m[2]);a=[expr(t) for t in a]
            name_map={'fillWithBlocks':'fill','setBlockState':'place','replaceAirAndLiquidDownwards':'down','fillWithAir':'air','fillWithRandomizedBlocks':'stones'}
            if call=='fillWithRandomizedBlocks':a.pop()
            s='self.'+name_map[call]+'('+', '.join(a)+');'
        else:s=expr(s)
        lines.append(s)
    return f'pub(super) fn template_{name.lower()}(&mut self, world: &mut impl ScatteredWorld, clip: BoundingBox, random: &mut JavaRandom) {{\n'+'\n'.join(lines)+'\n}'

text=SOURCE.read_text();output=['//! Original scattered-feature block template transcription.','#![allow(unused_variables,unused_mut,unused_assignments,unused_parens,clippy::double_parens,clippy::unnecessary_cast)]','use super::*;','impl Piece {']
for name in ['DesertPyramid','JunglePyramid','SwampHut']:
    start=text.index('public static class '+name+'\n');start=text.index('public boolean addComponentParts',start);start=text.index('{',start);end=endblock(text,start)
    output.append(method(name,text[start+1:end]))
output.append('}')
(ROOT/'src/vanilla/scattered_templates.rs').write_text('\n'.join(output),encoding='utf-8')
