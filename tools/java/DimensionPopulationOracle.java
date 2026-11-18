import java.io.*;
import java.util.*;
import java.lang.reflect.*;
import com.google.gson.Gson;
import net.minecraft.init.*;
import net.minecraft.block.*;
import net.minecraft.block.material.Material;
import net.minecraft.block.state.*;
import net.minecraft.block.state.pattern.BlockHelper;
import net.minecraft.entity.Entity;
import net.minecraft.entity.boss.EntityDragon;
import net.minecraft.profiler.Profiler;
import net.minecraft.world.*;
import net.minecraft.world.storage.WorldInfo;
import net.minecraft.world.biome.*;
import net.minecraft.world.chunk.*;
import net.minecraft.world.gen.*;
import net.minecraft.world.gen.feature.*;
import net.minecraft.world.gen.structure.*;
import net.minecraft.util.*;

/** Original feature/provider invocation. Lava and block-added callbacks are
 * recorded here; their world effects are tested by separate WorldServer oracles. */
public final class DimensionPopulationOracle {
    static final Comparator<BlockPos> ORDER=(a,b)->{int c=Integer.compare(a.getX(),b.getX());if(c==0)c=Integer.compare(a.getY(),b.getY());return c==0?Integer.compare(a.getZ(),b.getZ()):c;};
    static int initial(BlockPos p,int pattern){int x=p.getX(),y=p.getY(),z=p.getZ();if(y<0||y>=256)return 0;
        switch(pattern){
        case 0:return y<128?87<<4:0;
        case 1:return y<=31||y>=96&&y<128?87<<4:0;
        case 2:return y%16<8&&Math.floorMod(x+z,7)!=0?87<<4:0;
        case 3:return y==255?0:y==254?3<<4|2:y==253?110<<4:0;
        case 4:return y<=64?121<<4:0;
        case 5:return y<=64&&x*x+z*z<10000?121<<4:0;
        default:return 0;}
    }
    static IBlockState state(int raw){return Block.getBlockById(raw>>4).getStateFromMeta(raw&15);}
    static int raw(IBlockState s){return Block.getIdFromBlock(s.getBlock())<<4|s.getBlock().getMetaFromState(s);}
    static long intHash(long h,int value){for(int b=0;b<4;b++)h=DimensionTerrainOracle.byteHash(h,value>>>(b*8));return h;}
    static final class TestWorld extends World {
        final TreeMap<BlockPos,IBlockState> changed=new TreeMap<BlockPos,IBlockState>(ORDER);
        final List<int[]> forced=new ArrayList<int[]>();final List<Object> entities=new ArrayList<Object>();
        final int pattern;int writes;long writeHash=0xcbf29ce484222325L;int fortressCalls;int reads;long readHash=0xcbf29ce484222325L;
        TestWorld(long seed,int pattern,boolean hell){super(null,new WorldInfo(new WorldSettings(seed,WorldSettings.GameType.SURVIVAL,false,false,WorldType.DEFAULT),"oracle"),hell?new WorldProviderHell():new WorldProviderEnd(),new Profiler(),false);this.pattern=pattern;provider.registerWorld(this);rand.setSeed(seed);}
        protected IChunkProvider createChunkProvider(){return null;}
        protected int getRenderDistanceChunks(){return 8;}
        public IBlockState getBlockState(BlockPos p){reads++;readHash=intHash(intHash(intHash(readHash,p.getX()),p.getY()),p.getZ());IBlockState s=changed.get(p);return s==null?state(initial(p,pattern)):s;}
        public boolean isAirBlock(BlockPos p){return getBlockState(p).getBlock().getMaterial()==Material.air;}
        public boolean setBlockState(BlockPos p,IBlockState s,int flags){writes++;writeHash=intHash(intHash(intHash(intHash(intHash(writeHash,p.getX()),p.getY()),p.getZ()),raw(s)),flags);if(p.getY()<0||p.getY()>=256)return false;BlockPos key=new BlockPos(p);if(raw(s)==initial(p,pattern))changed.remove(key);else changed.put(key,s);return true;}
        public void forceBlockUpdateTick(Block b,BlockPos p,Random random){forced.add(new int[]{p.getX(),p.getY(),p.getZ(),Block.getIdFromBlock(b)});}
        public BiomeGenBase getBiomeGenForCoords(BlockPos p){return pattern>=4?BiomeGenBase.sky:BiomeGenBase.hell;}
        public int getLight(BlockPos p){return Math.floorMod(p.getX()+p.getZ(),11)==0?14:0;}
        public int getLightFor(EnumSkyBlock kind,BlockPos p){return kind==EnumSkyBlock.BLOCK?getLight(p):0;}
        public BlockPos getHeight(BlockPos p){for(int y=255;y>=0;y--)if(getBlockState(new BlockPos(p.getX(),y,p.getZ())).getBlock().getLightOpacity()>0)return new BlockPos(p.getX(),y+1,p.getZ());return new BlockPos(p.getX(),0,p.getZ());}
        public BlockPos getTopSolidOrLiquidBlock(BlockPos p){for(int y=255;y>=0;y--){Material m=getBlockState(new BlockPos(p.getX(),y,p.getZ())).getBlock().getMaterial();if(m.blocksMovement()&&m!=Material.leaves)return new BlockPos(p.getX(),y+1,p.getZ());}return new BlockPos(p.getX(),-1,p.getZ());}
        public boolean spawnEntityInWorld(Entity e){Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("kind",e instanceof EntityDragon?"EnderDragon":"EnderCrystal");row.put("position",new double[]{e.posX,e.posY,e.posZ});row.put("yaw",Integer.toUnsignedLong(Float.floatToRawIntBits(e.rotationYaw)));entities.add(row);return true;}
    }
    static final class Fortress extends MapGenNetherBridge {
        public boolean generateStructure(World world,Random random,ChunkCoordIntPair pair){((TestWorld)world).fortressCalls++;if(((TestWorld)world).writes!=0)throw new AssertionError("fortress not first");return false;}
    }
    static Map<String,Object> run(String kind,long seed,int pattern,int[] origin)throws Exception{
        TestWorld world=new TestWorld(seed,pattern,!kind.equals("end"));Random random=new Random(seed);BlockPos p=new BlockPos(origin[0],origin[1],origin[2]);boolean result=true;
        if(kind.equals("nether")){ChunkProviderHell provider=new ChunkProviderHell(world,false,seed);Field f=ChunkProviderHell.class.getDeclaredField("hellRNG");f.setAccessible(true);random=(Random)f.get(provider);random.setSeed(seed);f=ChunkProviderHell.class.getDeclaredField("genNetherBridge");f.setAccessible(true);f.set(provider,new Fortress());provider.populate(null,origin[0],origin[2]);}
        else if(kind.equals("end")){new ChunkProviderEnd(world,seed).populate(null,origin[0],origin[2]);random=world.rand;}
        else {WorldGenerator generator=kind.equals("fire")?new WorldGenFire():kind.equals("glow1")?new WorldGenGlowStone1():kind.equals("glow2")?new WorldGenGlowStone2():kind.equals("brown")?new GeneratorBushFeature(Blocks.brown_mushroom):kind.equals("red")?new GeneratorBushFeature(Blocks.red_mushroom):kind.equals("quartz")?new WorldGenMinable(Blocks.quartz_ore.getDefaultState(),14,BlockHelper.forBlock(Blocks.netherrack)):new WorldGenHellLava(Blocks.flowing_lava,kind.equals("enclosed"));result=generator.generate(world,random,p);}
        Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("kind",kind);row.put("seed",Long.toString(seed));row.put("pattern",pattern);row.put("origin",origin);row.put("result",result);row.put("reads",world.reads);row.put("read_hash",Long.toUnsignedString(world.readHash));row.put("writes",world.writes);row.put("write_hash",Long.toUnsignedString(world.writeHash));row.put("forced",world.forced);row.put("entities",world.entities);row.put("fortress",world.fortressCalls);row.put("next",Long.toString(random.nextLong()));
        long hash=0xcbf29ce484222325L;for(Map.Entry<BlockPos,IBlockState> e:world.changed.entrySet()){BlockPos q=e.getKey();hash=intHash(intHash(intHash(intHash(hash,q.getX()),q.getY()),q.getZ()),raw(e.getValue()));}row.put("changed",world.changed.size());row.put("hash",Long.toUnsignedString(hash));return row;
    }
    public static void main(String[] args)throws Exception{
        PrintStream out=System.out;System.setOut(System.err);Bootstrap.register();List<Object> rows=new ArrayList<Object>();
        for(long seed:new long[]{0,1,-1,189,7654321,Long.MIN_VALUE,Long.MAX_VALUE}){
            for(int pattern=0;pattern<4;pattern++)for(String kind:new String[]{"fire","glow1","glow2","brown","red","quartz","lava","enclosed"})for(int y:new int[]{31,32,63,95,255})rows.add(run(kind,seed,pattern,new int[]{16,y,-16}));
            for(int pattern=0;pattern<3;pattern++)for(int[] pos:new int[][]{{0,0,0},{-1,0,2},{7,0,-5}})rows.add(run("nether",seed,pattern,pos));
            for(int pattern=4;pattern<6;pattern++)for(int[] pos:new int[][]{{0,0,0},{-1,0,2},{7,0,-5}})rows.add(run("end",seed,pattern,pos));
        }
        out.println(new Gson().toJson(rows));
    }
}
