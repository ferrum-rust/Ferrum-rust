// Actual ChunkProviderGenerate.populate tail, World freeze/snow predicates,
// and Chunk.getPrecipitationHeight. Java is a test oracle only.
import com.google.gson.Gson;
import java.lang.reflect.*;
import java.util.*;
import sun.misc.Unsafe;
import net.minecraft.init.*;
import net.minecraft.block.*;
import net.minecraft.block.state.IBlockState;
import net.minecraft.entity.EnumCreatureType;
import net.minecraft.util.BlockPos;
import net.minecraft.world.*;
import net.minecraft.world.biome.*;
import net.minecraft.world.chunk.*;
import net.minecraft.world.gen.ChunkProviderGenerate;
import net.minecraft.world.storage.WorldInfo;

public final class SnowOracle {
    static final int[] TOP={2<<4,9<<4,9<<4|1,8<<4,79<<4,174<<4,18<<4,161<<4,78<<4|6,78<<4|7,44<<4|8,20<<4,85<<4,31<<4|1,0,10<<4};
    static IBlockState state(int raw){return Block.getBlockById(raw>>4).getStateFromMeta(raw&15);}
    static int raw(IBlockState s){return Block.getIdFromBlock(s.getBlock())<<4|s.getBlock().getMetaFromState(s);}
    static final class EmptyBiome extends BiomeGenBase {
        EmptyBiome(){super(254);spawnableCreatureList.clear();}
        @Override public void decorate(World world,Random random,BlockPos p){}
    }
    static final class OracleWorld extends WorldServer {
        int height,light,biome,topOverride=-1;boolean firstBiome;BiomeGenBase empty;
        Map<BlockPos,IBlockState> changed;Map<Long,Chunk> chunks;List<int[]> writes;
        OracleWorld(){super(null,null,null,0,null);}
        int initial(BlockPos p){if(p.getY()<0||p.getY()>=256)return 0;if(p.getY()<height)return 1<<4;if(p.getY()>height)return 0;return topOverride>=0?topOverride:TOP[Math.floorMod(p.getX()+p.getZ()*3,TOP.length)];}
        @Override public IBlockState getBlockState(BlockPos p){IBlockState s=changed.get(p);return s==null?state(initial(p)):s;}
        @Override public int getLightFor(EnumSkyBlock kind,BlockPos p){return kind==EnumSkyBlock.BLOCK?light:15;}
        @Override public BiomeGenBase getBiomeGenForCoords(BlockPos p){if(firstBiome){firstBiome=false;return empty;}return BiomeGenBase.getBiome(biome);}
        @Override public boolean setBlockState(BlockPos p,IBlockState s,int flags){if(p.getY()<0||p.getY()>=256)return false;changed.put(new BlockPos(p),s);writes.add(new int[]{p.getX(),p.getY(),p.getZ(),raw(s)});return true;}
        @Override public BlockPos getPrecipitationHeight(BlockPos p){
            int cx=p.getX()>>4,cz=p.getZ()>>4;long key=(long)cx&0xffffffffL|((long)cz&0xffffffffL)<<32;Chunk chunk=chunks.get(key);
            if(chunk==null){ChunkPrimer primer=new ChunkPrimer();for(int x=0;x<16;x++)for(int z=0;z<16;z++)for(int y=0;y<256;y++)primer.setBlockState(x,y,z,state(initial(new BlockPos(cx*16+x,y,cz*16+z))));chunk=new Chunk(this,primer,cx,cz);chunks.put(key,chunk);}
            return chunk.getPrecipitationHeight(p);
        }
    }
    static void set(Object o,Class<?> cls,String name,Object value)throws Exception{Field f=cls.getDeclaredField(name);f.setAccessible(true);f.set(o,value);}
    static OracleWorld world(int biome,int height,int light)throws Exception{
        Field f=Unsafe.class.getDeclaredField("theUnsafe");f.setAccessible(true);OracleWorld w=(OracleWorld)((Unsafe)f.get(null)).allocateInstance(OracleWorld.class);
        w.height=height;w.light=light;w.biome=biome;w.topOverride=-1;w.changed=new HashMap<BlockPos,IBlockState>();w.chunks=new HashMap<Long,Chunk>();w.writes=new ArrayList<int[]>();
        WorldProviderSurface provider=new WorldProviderSurface();set(provider,WorldProvider.class,"worldChunkMgr",new WorldChunkManagerHell(BiomeGenBase.plains,0.5f));set(w,World.class,"provider",provider);
        set(w,World.class,"worldInfo",new WorldInfo(new WorldSettings(189L,WorldSettings.GameType.SURVIVAL,false,false,WorldType.DEFAULT),"SnowOracle"));set(w,World.class,"rand",new Random(189));return w;
    }
    static long hash(List<int[]> rows){long h=0xcbf29ce484222325L;for(int[] row:rows)for(int v:row)for(int shift=0;shift<32;shift+=8)h=(h^((v>>>shift)&255))*0x100000001b3L;return h;}
    static Map<String,Object> run(int biome,int height,int light,int cx,int cz)throws Exception{
        OracleWorld w=world(biome,height,light);w.empty=new EmptyBiome();w.firstBiome=true;
        new ChunkProviderGenerate(w,189L,false,"{\"useWaterLakes\":false,\"useLavaLakes\":false,\"useDungeons\":false}").populate(null,cx,cz);
        Map<String,Object> out=new LinkedHashMap<String,Object>();out.put("biome",biome);out.put("height",height);out.put("light",light);out.put("chunk",new int[]{cx,cz});out.put("count",w.writes.size());out.put("write_hash",String.format("%016x",hash(w.writes)));
        List<int[]> precipitation=new ArrayList<int[]>();for(int x=0;x<16;x++)for(int z=0;z<16;z++)precipitation.add(new int[]{w.getPrecipitationHeight(new BlockPos(cx*16+8+x,0,cz*16+8+z)).getY()});out.put("precipitation_hash",String.format("%016x",hash(precipitation)));return out;
    }
    public static void main(String[] args)throws Exception{
        Bootstrap.register();List<Integer> ids=new ArrayList<Integer>();for(BiomeGenBase biome:BiomeGenBase.getBiomeGenArray())if(biome!=null)ids.add(biome.biomeID);
        List<Object> cases=new ArrayList<Object>();int index=0;for(int biome:ids)for(int height:new int[]{0,64,128,255})for(int light:new int[]{0,9,10}){int cx=index%3==0?-3:index%3==1?0:45,cz=index%3==0?5:index%3==1?0:-72;cases.add(run(biome,height,light,cx,cz));index++;}
        List<Object> supports=new ArrayList<Object>();Set<Integer> seen=new TreeSet<Integer>();for(Object o:Block.blockRegistry){Block block=(Block)o;for(IBlockState s:block.getBlockState().getValidStates()){int raw=raw(s);if(!seen.add(raw))continue;OracleWorld w=world(12,64,0);w.topOverride=raw;supports.add(Arrays.asList(raw,Blocks.snow_layer.canPlaceBlockAt(w,new BlockPos(0,65,0)),w.canSnowAt(new BlockPos(0,65,0),true),w.getPrecipitationHeight(new BlockPos(0,0,0)).getY()));}}
        Map<String,Object> out=new LinkedHashMap<String,Object>();out.put("cases",cases);out.put("supports",supports);System.out.println(new Gson().toJson(out));
    }
}
