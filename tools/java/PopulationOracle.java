// Test-only invocation of original 1.8.9 WorldGen feature bytecode.
import com.google.gson.Gson;
import java.lang.reflect.*;
import java.nio.file.*;
import java.util.*;
import sun.misc.Unsafe;
import net.minecraft.init.*;
import net.minecraft.block.*;
import net.minecraft.block.state.IBlockState;
import net.minecraft.item.Item;
import net.minecraft.item.ItemStack;
import net.minecraft.nbt.*;
import net.minecraft.tileentity.*;
import net.minecraft.entity.Entity;
import net.minecraft.entity.boss.EntityDragon;
import net.minecraft.util.*;
import net.minecraft.world.*;
import net.minecraft.world.biome.*;
import net.minecraft.world.gen.feature.*;
import net.minecraft.world.gen.ChunkProviderGenerate;
import net.minecraft.world.gen.GeneratorBushFeature;
import net.minecraft.world.storage.WorldInfo;

public final class PopulationOracle {
    static final Comparator<BlockPos> ORDER=new Comparator<BlockPos>(){public int compare(BlockPos a,BlockPos b){int c=Integer.compare(a.getX(),b.getX());if(c==0)c=Integer.compare(a.getY(),b.getY());return c==0?Integer.compare(a.getZ(),b.getZ()):c;}};
    static IBlockState state(int raw){return Block.getBlockById(raw>>4).getStateFromMeta(raw&15);}
    static int raw(IBlockState s){return (Block.getIdFromBlock(s.getBlock())<<4)|s.getBlock().getMetaFromState(s);}
    static int initial(BlockPos p,int scenario){
        int x=p.getX(),y=p.getY(),z=p.getZ();if(y<0||y>=256)return 0;
        if(scenario==0)return 16;
        if(scenario==1)return Math.floorMod(x+y+z,7)==0?16|3:Math.floorMod(x+y+z,11)==0?48:16;
        if(scenario>=2&&scenario<=5){if(y>64)return scenario==3&&y<=68?144:0;return y>=60?48:16;}
        if(scenario==6||scenario==7){if(y>=40&&y<=41&&z==16)return 0;return scenario==7&&x==17&&y==39?0:16;}
        if(scenario>=9&&scenario<=14){
            if(scenario==14&&x==16&&z==16&&y==66)return 16;
            if(y>64)return 0;if(y<64)return 16;
            if(scenario==10)return 12<<4;if(scenario==11&&Math.floorMod(x,4)==0)return 9<<4;
            if(scenario==12)return 3<<4|2;if(scenario==13)return 60<<4;return 2<<4;
        }
        if(scenario==15){if(y>64)return 0;return y==64?80<<4:y>=60?3<<4:16;}
        if(scenario==18){if(y>64)return 0;return y==64?110<<4:16;}
        if(scenario==19){if(y>64)return 0;return y==64?9<<4:y==63?2<<4:16;}
        if(scenario==20){if(x==17&&y>=65&&y<128)return 16;if(y>64)return 0;return y==64?2<<4:16;}
        if(scenario==21||scenario==22){int top=scenario==21?64:244;if(y>top)return 0;return y==top?121<<4:16;}
        return 0;
    }
    static final class OracleWorld extends WorldServer {
        TreeMap<BlockPos,IBlockState> changed;TreeMap<BlockPos,TileEntity> tiles;List<Object> entities;int scenario;BiomeGenBase stopBiome;
        OracleWorld(){super(null,null,null,0,null);}
        @Override public IBlockState getBlockState(BlockPos p){IBlockState s=changed.get(p);return s==null?state(initial(p,scenario)):s;}
        @Override public boolean isAirBlock(BlockPos p){return getBlockState(p).getBlock()==Blocks.air;}
        @Override public boolean setBlockState(BlockPos p,IBlockState s,int flags){
            if(p.getY()<0||p.getY()>=256)return false;
            BlockPos key=new BlockPos(p);if(raw(s)==initial(p,scenario))changed.remove(key);else changed.put(key,s);
            if(s.getBlock()==Blocks.chest){TileEntityChest chest=new TileEntityChest();chest.setPos(key);tiles.put(key,chest);}
            else if(s.getBlock()==Blocks.mob_spawner){TileEntityMobSpawner spawner=new TileEntityMobSpawner();spawner.setPos(key);tiles.put(key,spawner);}
            else tiles.remove(key);
            return true;
        }
        @Override public boolean setBlockToAir(BlockPos p){return setBlockState(p,Blocks.air.getDefaultState(),3);}
        @Override public TileEntity getTileEntity(BlockPos p){return tiles.get(p);}
        @Override public boolean spawnEntityInWorld(Entity entity){
            Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("kind",entity instanceof EntityDragon?"EnderDragon":"EnderCrystal");
            row.put("position",new double[]{entity.posX,entity.posY,entity.posZ});row.put("yaw_bits",Integer.toUnsignedLong(Float.floatToRawIntBits(entity.rotationYaw)));entities.add(row);return true;
        }
        @Override public int getLightFor(EnumSkyBlock kind,BlockPos p){return kind==EnumSkyBlock.SKY?(scenario==4?0:15):0;}
        @Override public int getLight(BlockPos p){return getLightFor(EnumSkyBlock.SKY,p);}
        @Override public BlockPos getHeight(BlockPos p){for(int y=255;y>=0;y--)if(getBlockState(new BlockPos(p.getX(),y,p.getZ())).getBlock().getLightOpacity()>0)return new BlockPos(p.getX(),y+1,p.getZ());return new BlockPos(p.getX(),0,p.getZ());}
        @Override public BlockPos getTopSolidOrLiquidBlock(BlockPos p){for(int y=255;y>=0;y--){net.minecraft.block.material.Material m=getBlockState(new BlockPos(p.getX(),y,p.getZ())).getBlock().getMaterial();if(m.blocksMovement()&&m!=net.minecraft.block.material.Material.leaves)return new BlockPos(p.getX(),y+1,p.getZ());}return new BlockPos(p.getX(),-1,p.getZ());}
        @Override public BiomeGenBase getBiomeGenForCoords(BlockPos p){return stopBiome!=null?stopBiome:BiomeGenBase.getBiome(scenario==4?14:scenario==5?12:1);}
    }
    static final class Stop extends RuntimeException {final Random random;Stop(Random random){this.random=random;}}
    static final class StopBiome extends BiomeGenBase {
        StopBiome(){super(254);}
        @Override public void decorate(World world,Random random,BlockPos pos){throw new Stop(random);}
    }
    static final class OreDecorator extends BiomeDecorator {
        @Override protected void genDecorations(BiomeGenBase biome){generateOres();}
    }
    static OracleWorld world(int scenario)throws Exception{
        Field u=Unsafe.class.getDeclaredField("theUnsafe");u.setAccessible(true);
        OracleWorld w=(OracleWorld)((Unsafe)u.get(null)).allocateInstance(OracleWorld.class);
        w.scenario=scenario;w.changed=new TreeMap<BlockPos,IBlockState>(ORDER);w.tiles=new TreeMap<BlockPos,TileEntity>(ORDER);w.entities=new ArrayList<Object>();
        Field info=World.class.getDeclaredField("worldInfo");info.setAccessible(true);info.set(w,new WorldInfo(new WorldSettings(189L,WorldSettings.GameType.SURVIVAL,false,false,WorldType.DEFAULT),"Oracle"));
        Field provider=World.class.getDeclaredField("provider");provider.setAccessible(true);provider.set(w,new WorldProviderSurface());
        return w;
    }
    static int[] coords(BlockPos p){return new int[]{p.getX(),p.getY(),p.getZ()};}
    static List<Object> chest(TileEntityChest chest){
        List<Object> slots=new ArrayList<Object>();
        for(int i=0;i<27;i++){
            ItemStack stack=chest.getStackInSlot(i);if(stack==null){slots.add(null);continue;}
            Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("id",Item.getIdFromItem(stack.getItem()));row.put("meta",stack.getMetadata());row.put("count",stack.stackSize);
            List<Object> enchants=new ArrayList<Object>();if(stack.hasTagCompound()){
                NBTTagList list=stack.getTagCompound().getTagList("StoredEnchantments",10);
                for(int j=0;j<list.tagCount();j++){NBTTagCompound e=list.getCompoundTagAt(j);enchants.add(new int[]{e.getShort("id"),e.getShort("lvl")});}
            }
            row.put("enchants",enchants);slots.add(row);
        }
        return slots;
    }
    static Map<String,Object> run(String kind,long seed,int scenario,int size,int raw,int[] position)throws Exception{
        OracleWorld world=world(scenario);Random random=new Random(seed);BlockPos p=new BlockPos(position[0],position[1],position[2]);boolean success=true;
        if(kind.equals("prelude")){
            Field info=World.class.getDeclaredField("worldInfo");info.setAccessible(true);info.set(world,new WorldInfo(new WorldSettings(seed,WorldSettings.GameType.SURVIVAL,false,false,WorldType.DEFAULT),"Oracle"));world.stopBiome=new StopBiome();
            Field manager=WorldProvider.class.getDeclaredField("worldChunkMgr");manager.setAccessible(true);manager.set(world.provider,new WorldChunkManagerHell(BiomeGenBase.plains,0.5f));
            ChunkProviderGenerate provider=new ChunkProviderGenerate(world,seed,false,"");
            try{provider.populate(null,p.getX(),p.getZ());throw new AssertionError("Did not reach biome decorator");}catch(Stop stop){random=stop.random;}
        }
        if(kind.equals("decorator")){
            for(BiomeGenBase b:BiomeGenBase.getBiomeGenArray())if(b!=null){
                b.theBiomeDecorator.generateLakes=false;
                Field tree=BiomeGenBase.class.getDeclaredField("worldGeneratorBigTree");tree.setAccessible(true);Object generator=tree.get(b);
                Field height=WorldGenBigTree.class.getDeclaredField("heightLimit");height.setAccessible(true);height.setInt(generator,0);
                Field mushroom=BiomeDecorator.class.getDeclaredField("bigMushroomGen");mushroom.setAccessible(true);Object giant=mushroom.get(b.theBiomeDecorator);Field type=WorldGenBigMushroom.class.getDeclaredField("mushroomType");type.setAccessible(true);type.set(giant,null);
            }
            world.stopBiome=BiomeGenBase.getBiome(size);world.stopBiome.decorate(world,random,p);
        }
        if(kind.equals("ore"))success=new WorldGenMinable(state(raw),size).generate(world,random,p);
        if(kind.equals("ores"))new OreDecorator().decorate(world,random,BiomeGenBase.plains,p);
        if(kind.equals("lake"))success=new WorldGenLakes(Block.getBlockById(raw>>4)).generate(world,random,p);
        if(kind.equals("dungeon"))success=new WorldGenDungeons().generate(world,random,p);
        if(kind.equals("tree"))success=new WorldGenTrees(false).generate(world,random,p);
        if(kind.equals("birch"))success=new WorldGenForest(false,false).generate(world,random,p);
        if(kind.equals("tall_birch"))success=new WorldGenForest(false,true).generate(world,random,p);
        if(kind.equals("spruce"))success=new WorldGenTaiga2(false).generate(world,random,p);
        if(kind.equals("pine"))success=new WorldGenTaiga1().generate(world,random,p);
        if(kind.equals("acacia"))success=new WorldGenSavannaTree(false).generate(world,random,p);
        if(kind.equals("well"))success=new WorldGenDesertWells().generate(world,random,p);
        if(kind.equals("swamp_tree"))success=new WorldGenSwamp().generate(world,random,p);
        if(kind.equals("end_spike"))success=new WorldGenSpikes(Blocks.end_stone).generate(world,random,p);
        if(kind.startsWith("mega_pine")||kind.startsWith("mega_spruce")){
            WorldGenMegaPineTree tree=new WorldGenMegaPineTree(false,kind.startsWith("mega_spruce"));success=tree.generate(world,random,p);
            if(success&&kind.endsWith("decorated"))tree.func_180711_a(world,random,p);
        }
        if(kind.equals("mega_jungle"))success=new WorldGenMegaJungle(false,10,20,state(17<<4|3),state(18<<4|3)).generate(world,random,p);
        if(kind.equals("blob"))success=new WorldGenBlockBlob(Blocks.mossy_cobblestone,0).generate(world,random,p);
        if(kind.equals("shrub"))success=new WorldGenShrub(state(17<<4|3),state(18<<4)).generate(world,random,p);
        if(kind.equals("dark_oak"))success=new WorldGenCanopyTree(false).generate(world,random,p);
        if(kind.equals("melon"))success=new WorldGenMelon().generate(world,random,p);
        if(kind.equals("vines"))success=new WorldGenVines().generate(world,random,p);
        if(kind.equals("ice_path"))success=new WorldGenIcePath(4).generate(world,random,p);
        if(kind.equals("ice_spike"))success=new WorldGenIceSpike().generate(world,random,p);
        if(kind.equals("big_mushroom")||kind.equals("big_mushroom_sequence")){
            WorldGenBigMushroom mushroom=new WorldGenBigMushroom();success=mushroom.generate(world,random,p);
            if(kind.equals("big_mushroom_sequence")){mushroom.generate(world,random,p.add(32,0,0));mushroom.generate(world,random,p.add(64,0,0));}
        }
        if(kind.equals("big_oak")||kind.equals("big_oak_raw")||kind.equals("big_oak_sequence")){
            WorldGenBigTree tree=new WorldGenBigTree(false);if(!kind.equals("big_oak_raw"))tree.func_175904_e();
            success=tree.generate(world,random,p);
            if(kind.equals("big_oak_sequence")){tree.generate(world,random,p.add(32,0,0));tree.generate(world,random,p.add(64,0,0));}
        }
        if(kind.equals("jungle"))success=new WorldGenTrees(false,5,state(17<<4|3),state(18<<4|3),true).generate(world,random,p);
        if(kind.equals("grass"))success=new WorldGenTallGrass(BlockTallGrass.EnumType.GRASS).generate(world,random,p);
        if(kind.equals("flower"))success=new WorldGenFlowers(Blocks.yellow_flower,BlockFlower.EnumFlowerType.DANDELION).generate(world,random,p);
        if(kind.equals("mushroom"))success=new GeneratorBushFeature(Blocks.brown_mushroom).generate(world,random,p);
        if(kind.equals("dead_bush"))success=new WorldGenDeadBush().generate(world,random,p);
        if(kind.equals("reeds"))success=new WorldGenReed().generate(world,random,p);
        if(kind.equals("cactus"))success=new WorldGenCactus().generate(world,random,p);
        if(kind.equals("waterlily"))success=new WorldGenWaterlily().generate(world,random,p);
        if(kind.equals("pumpkin"))success=new WorldGenPumpkin().generate(world,random,p);
        if(kind.equals("sand"))success=new WorldGenSand(Blocks.sand,7).generate(world,random,p);
        if(kind.equals("clay"))success=new WorldGenClay(4).generate(world,random,p);
        Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("kind",kind);row.put("seed",Long.toString(seed));row.put("scenario",scenario);row.put("size",size);row.put("state",raw);row.put("position",position);row.put("success",success);
        long hash=0xcbf29ce484222325L;List<Object> changed=new ArrayList<Object>();
        for(Map.Entry<BlockPos,IBlockState> e:world.changed.entrySet()){
            int[] values=new int[]{e.getKey().getX(),e.getKey().getY(),e.getKey().getZ(),raw(e.getValue())};changed.add(values);
            for(int value:values)for(int shift=0;shift<32;shift+=8)hash=(hash^((value>>>shift)&255))*0x100000001b3L;
        }
        row.put("changed",world.changed.size());row.put("hash",Long.toUnsignedString(hash,16));
        List<Object> chests=new ArrayList<Object>(),spawners=new ArrayList<Object>();
        for(Map.Entry<BlockPos,TileEntity> e:world.tiles.entrySet()){
            if(e.getValue() instanceof TileEntityChest)chests.add(Arrays.asList(coords(e.getKey()),chest((TileEntityChest)e.getValue())));
            else if(e.getValue() instanceof TileEntityMobSpawner){NBTTagCompound nbt=new NBTTagCompound();e.getValue().writeToNBT(nbt);spawners.add(Arrays.asList(coords(e.getKey()),nbt.getString("EntityId")));}
        }
        row.put("chests",chests);row.put("spawners",spawners);row.put("entities",world.entities);row.put("next_long",Long.toString(random.nextLong()));return row;
    }
    public static void main(String[] args)throws Exception{
        Bootstrap.register();List<Object> cases=new ArrayList<Object>();
        for(long seed:new long[]{0L,1L,189L,-1L,Long.MIN_VALUE}){
            for(int scenario:new int[]{0,1})for(int size:new int[]{1,7,17,33})cases.add(run("ore",seed,scenario,size,56<<4,new int[]{-3,16,11}));
            for(int scenario:new int[]{0,1,2})cases.add(run("ores",seed,scenario,0,0,new int[]{-16,0,0}));
            for(int scenario:new int[]{0,2,3,4,5})for(int raw:new int[]{9<<4,11<<4})cases.add(run("lake",seed,scenario,0,raw,new int[]{16,67,16}));
            for(int scenario:new int[]{0,6,7,8})cases.add(run("dungeon",seed,scenario,0,0,new int[]{16,40,16}));
            for(int scenario:new int[]{9,10,12,13,14})for(String kind:new String[]{"tree","jungle","grass","flower","dead_bush","cactus","pumpkin"})cases.add(run(kind,seed,scenario,0,0,new int[]{16,65,16}));
            for(int scenario:new int[]{9,10,12,13,14})for(String kind:new String[]{"birch","tall_birch","spruce","big_oak","big_oak_raw","big_oak_sequence","mushroom"})cases.add(run(kind,seed,scenario,0,0,new int[]{16,65,16}));
            for(int scenario:new int[]{9,10,12,13,14})for(String kind:new String[]{"pine","acacia","well"})cases.add(run(kind,seed,scenario,0,0,new int[]{16,65,16}));
            for(int scenario:new int[]{3,9,10,13,15,18,19})for(String kind:new String[]{"swamp_tree","ice_path","ice_spike","big_mushroom","big_mushroom_sequence"})cases.add(run(kind,seed,scenario,0,0,new int[]{16,65,16}));
            for(int scenario:new int[]{9,10,12,13,14,20})for(String kind:new String[]{"mega_pine","mega_spruce","mega_pine_decorated","mega_spruce_decorated","mega_jungle","blob","shrub","dark_oak","melon","vines"})cases.add(run(kind,seed,scenario,0,0,new int[]{16,65,16}));
            for(int scenario:new int[]{9,21,22})cases.add(run("end_spike",seed,scenario,0,0,new int[]{16,scenario==22?245:65,16}));
            for(int scenario:new int[]{9,21})cases.add(run("decorator",seed,scenario,9,0,new int[]{0,0,0}));
            for(int scenario:new int[]{3,9,11})for(String kind:new String[]{"reeds","waterlily","sand","clay"})cases.add(run(kind,seed,scenario,0,0,new int[]{16,65,16}));
            for(int scenario:new int[]{0,6,9})for(int[] p:new int[][]{{0,0,0},{-1,0,2}})cases.add(run("prelude",seed,scenario,0,0,p));
            for(int scenario:new int[]{3,9})for(int biome:new int[]{0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37,38,39,129,130,131,132,133,134,140,149,151,155,156,157,158,160,161,162,163,164,165,166,167})cases.add(run("decorator",seed,scenario,biome,0,new int[]{-16,0,0}));
            cases.add(run("decorator",seed,15,140,0,new int[]{-16,0,0}));
        }
        for(long seed=2;seed<32;seed++)cases.add(run("dungeon",seed,6,0,0,new int[]{16,40,16}));
        List<Object> reseeds=new ArrayList<Object>();for(long seed:new long[]{0,1,189,-1,Long.MIN_VALUE})for(int[] p:new int[][]{{0,0},{-31,17},{Integer.MIN_VALUE,Integer.MAX_VALUE}}){Random r=new Random(seed);long a=r.nextLong()/2L*2L+1L,b=r.nextLong()/2L*2L+1L;r.setSeed((long)p[0]*a+(long)p[1]*b^seed);reseeds.add(Arrays.asList(Long.toString(seed),p,Long.toString(r.nextLong())));}
        List<Boolean> full=new ArrayList<Boolean>(),solid=new ArrayList<Boolean>(),movement=new ArrayList<Boolean>();List<Integer> opacity=new ArrayList<Integer>();
        for(int id=0;id<256;id++){Block b=Block.getBlockById(id);full.add(b.isFullBlock());solid.add(b.getMaterial().isSolid());movement.add(b.getMaterial().blocksMovement());opacity.add(b.getLightOpacity());}
        Map<String,Object> out=new LinkedHashMap<String,Object>();out.put("cases",cases);out.put("reseeds",reseeds);out.put("full_blocks",full);out.put("solid_material",solid);out.put("blocks_movement",movement);out.put("light_opacity",opacity);System.out.println(new Gson().toJson(out));
    }
}
