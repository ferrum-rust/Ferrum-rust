"""Developer-only direct transcription of owned 1.8.9 village template bodies.

The output is ordinary Rust; running the game never needs Java or this script.
No terrain snapshot is baked in: original conditions, loops and RNG calls remain.
"""
from pathlib import Path
import re
ROOT=Path(__file__).resolve().parents[1]
SOURCE=ROOT/'.local/reference/java/net/minecraft/world/gen/structure/StructureVillagePieces.java'
IDS=dict(air=0,grass=2,dirt=3,cobblestone=4,planks=5,flowing_water=8,water=9,flowing_lava=10,gravel=13,log=17,wool=35,double_stone_slab=43,stone_slab=44,torch=50,oak_stairs=53,chest=54,crafting_table=58,farmland=60,furnace=61,oak_door=64,ladder=65,stone_stairs=67,wooden_pressure_plate=72,oak_fence=85,iron_bars=101,glass_pane=102)
DEFAULTS=dict(furnace=2,ladder=2,torch=5,chest=2)
IDS['bookshelf']=47
def endblock(text,start):
    level=0
    for i in range(start,len(text)):
        if text[i]=='{':level+=1
        if text[i]=='}':
            level-=1
            if level==0:return i
    raise ValueError('unclosed')
def args(s):
    depth=0; result=[]; start=0
    for i,c in enumerate(s):
        if c=='(':depth+=1
        if c==')':depth-=1
        if c==',' and depth==0:result.append(s[start:i].strip());start=i+1
    return result+[s[start:].strip()]
def calls(s,pattern,fn):
    while m:=re.search(pattern+r'\(',s):
        level=1;i=m.end()
        while level:
            if s[i]=='(':level+=1
            if s[i]==')':level-=1
            i+=1
        s=s[:m.start()]+fn(m,args(s[m.end():i-1]))+s[i:]
    return s
def expr(s):
    s=s.replace('EnumDyeColor.WHITE.getDyeDamage()','15')
    s=s.replace('MathHelper.getRandomIntegerInRange(random, 2, 7)','(2 + random.next_int(6))')
    for old,new in [('this.coordBaseMode.rotateY()','((self.orientation + 1) & 3)'),('this.coordBaseMode.rotateYCCW()','((self.orientation + 3) & 3)'),('this.coordBaseMode.getOpposite()','((self.orientation + 2) & 3)'),('this.coordBaseMode','self.orientation')]:s=s.replace(old,new)
    s=calls(s,r'Blocks\.torch.getDefaultState\(\).withProperty',lambda m,a:f'torch({expr(a[1])})')
    s=calls(s,r'this.getMetadataWithOffset',lambda m,a:f'(rotate_metadata({IDS[a[0][7:]]}, ({expr(a[1])}) as u8, self.orientation) as i32)')
    s=calls(s,r'EnumFacing.getHorizontal',lambda m,a:f'(({expr(a[0])}) as u8 & 3)')
    s=calls(s,r'Blocks\.(\w+).getStateFromMeta',lambda m,a:f'(({IDS[m[1]]} << 4) | ({expr(a[0])}) as u16)')
    s=calls(s,r'this.cropType([ABCD]).getStateFromMeta',lambda m,a:f'((self.crops[{ord(m[1])-65}] << 4) | ({expr(a[0])}) as u16)')
    s=re.sub(r'Blocks\.(\w+).getDefaultState\(\)',lambda m:str((IDS[m[1]]<<4)|DEFAULTS.get(m[1],0)),s)
    s=calls(s,r'this.getBlockStateFromPos',lambda m,a:'self.get('+', '.join(expr(t) for t in a)+')')
    s=s.replace('.getBlock().getMaterial()',' >> 4').replace('Material.air','0')
    for k,v in dict(isRoofAccessible='roof_accessible',isTallHouse='tall_house',tablePosition='table_position').items():s=s.replace('this.'+k,'self.'+v)
    for k,v in dict(EAST=3,NORTH=2,WEST=1,SOUTH=0).items():s=s.replace('EnumFacing.'+k,str(v))
    s=s.replace('structureBoundingBox','clip')
    s=re.sub(r'bl \? (-?\d+) : (-?\d+)',r'if bl { \1 } else { \2 }',s)
    return s
def method(name,body):
    # Shared exact ground-level implementation runs before every template.
    start=body.index('if (this.field_143015_k < 0)')
    end=endblock(body,body.index('{',start))
    body=body[:start]+body[end+1:]
    if name=='House2':
        start=body.index('if (!this.hasMadeChest');end=endblock(body,body.index('{',start))
        body=body[:start]+'self.blacksmith_chest(world, clip, random);'+body[end+1:]
    lines=[]
    for raw in body.splitlines():
        s=raw.strip()
        if not s or s=='return true;':continue
        if m:=re.match(r'for \((?:int )?(\w+) = (.*); \1 (<=|<|>=|>) (.*); (\+\+|--)\1\) \{',s):
            var,lo,cmp,hi,step=m.groups()
            if step=='++':s=f'for {var} in {expr(lo)}..{"=" if cmp=="<=" else ""}{expr(hi)} {{'
            else:s=f'for {var} in ({expr(hi)}..={expr(lo)}).rev() {{'
        elif m:=re.match(r'int (\w+);',s):s=f'let mut {m[1]}: i32;'
        elif m:=re.match(r'int (\w+) = (.*);',s):s=f'let mut {m[1]} = {expr(m[2])};'
        elif m:=re.match(r'boolean (\w+) = (.*);',s):s=f'let {m[1]} = {expr(m[2])};'
        elif s.startswith('if ('):
            # Final closing ')' belongs to the condition, nested call parens remain.
            close=s.rfind(')');tail=s[close+1:].strip()
            cond=expr(s[4:close])
            s=f'if {cond} '+ ('{ continue; }' if tail=='continue;' else tail)
        elif m:=re.match(r'this\.(\w+)\((.*)\);',s):
            call,a=m[1],args(m[2]);a=[expr(t) for t in a]
            names={'fillWithBlocks':'fill','setBlockState':'place','clearCurrentPositionBlocksUpwards':'clear','replaceAirAndLiquidDownwards':'down','spawnVillagers':'spawn','placeDoorCurrentPosition':'door'}
            if call not in names:raise ValueError(call)
            if call=='placeDoorCurrentPosition':a.pop(2)
            s='self.'+names[call]+'('+', '.join(a)+');'
        else:s=expr(s)
        lines.append(s)
    return f'pub(super) fn template_{name.lower()}(&mut self, world: &mut impl VillageWorld, clip: BoundingBox, random: &mut JavaRandom) {{\n'+'\n'.join(lines)+'\n}\n'
def main():
    text=SOURCE.read_text();out=['//! Direct native transcription of original village block templates.','//! Regenerate with scripts/port_village_templates.py after preparing local references.','#![allow(unused_variables,unused_mut,unused_assignments,unused_parens,clippy::too_many_arguments,clippy::double_parens,clippy::unnecessary_cast)]','use super::*;','impl Piece {']
    for name in ['Torch','Field1','Field2','House2','House3','Hall','WoodHut','House1','Church','House4Garden','Well']:
        start=text.index('public static class '+name+'\n');start=text.index('public boolean addComponentParts',start);start=text.index('{',start);end=endblock(text,start)
        out.append(method(name,text[start+1:end]))
    out.append('}')
    (ROOT/'src/vanilla/village_templates.rs').write_text('\n'.join(out))
if __name__=='__main__':main()
