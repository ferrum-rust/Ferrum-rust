import java.io.*;
import java.util.*;
import java.lang.reflect.*;
import com.google.gson.Gson;
import net.minecraft.init.Bootstrap;
import net.minecraft.block.Block;
import net.minecraft.block.state.IBlockState;
import net.minecraft.profiler.Profiler;
import net.minecraft.world.*;
import net.minecraft.world.storage.WorldInfo;
import net.minecraft.world.chunk.*;
import net.minecraft.world.gen.*;
import net.minecraft.util.BlockPos;

/** Executes the owned original providers, not reconstructed density formulas. */
public final class DimensionTerrainOracle {
    static final class TestWorld extends World {
        TestWorld(long seed,boolean hell){super(null,new WorldInfo(new WorldSettings(seed,WorldSettings.GameType.SURVIVAL,false,false,WorldType.DEFAULT),"oracle"),hell?new WorldProviderHell():new WorldProviderEnd(),new Profiler(),false);provider.registerWorld(this);}
        protected IChunkProvider createChunkProvider(){return null;}
        protected int getRenderDistanceChunks(){return 8;}
    }
    static int state(IBlockState s){return Block.getIdFromBlock(s.getBlock())<<4|s.getBlock().getMetaFromState(s);}
    static long byteHash(long h,int b){return(h^(b&255))*0x100000001b3L;}
    static String blocks(ChunkPrimer p){long h=0xcbf29ce484222325L;for(int y=0;y<256;y++)for(int z=0;z<16;z++)for(int x=0;x<16;x++){int s=state(p.getBlockState(x,y,z));h=byteHash(byteHash(h,s),s>>8);}return Long.toUnsignedString(h);}
    static String blocks(Chunk p){long h=0xcbf29ce484222325L;for(int y=0;y<256;y++)for(int z=0;z<16;z++)for(int x=0;x<16;x++){int s=state(p.getBlockState(new BlockPos(x,y,z)));h=byteHash(byteHash(h,s),s>>8);}return Long.toUnsignedString(h);}
    static String biomes(Chunk p){long h=0xcbf29ce484222325L;for(byte b:p.getBiomeArray())h=byteHash(h,b);return Long.toUnsignedString(h);}
    public static void main(String[] args)throws Exception{
        PrintStream out=System.out;System.setOut(System.err);Bootstrap.register();List<Object> rows=new ArrayList<Object>();
        int[][] coords={{0,0},{1,0},{0,1},{-1,0},{0,-1},{-1,-1},{3,4},{-8,7},{8,8},{-16,-12},{31,-33},{-100,81},{12345,-23456},{-1875000,1874999}};
        for(boolean hell:new boolean[]{true,false})for(long seed:new long[]{0,1,-1,189,7654321,-99999999,123456789012345L,Long.MIN_VALUE,Long.MAX_VALUE}){
            TestWorld world=new TestWorld(seed,hell);IChunkProvider provider=hell?new ChunkProviderHell(world,false,seed):new ChunkProviderEnd(world,seed);
            Field f=provider.getClass().getDeclaredField(hell?"hellRNG":"endRNG");f.setAccessible(true);Random random=(Random)f.get(provider);String initial=Long.toString(random.nextLong());
            Method density=provider.getClass().getDeclaredMethod("initializeNoiseField",double[].class,int.class,int.class,int.class,int.class,int.class,int.class);density.setAccessible(true);
            for(int[] c:coords){int x=c[0],z=c[1],grid=hell?4:2;Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("dimension",hell?"nether":"end");row.put("seed",Long.toString(seed));row.put("chunk",c);row.put("initial",initial);
                double[] values=(double[])density.invoke(provider,null,x*grid,0,z*grid,grid+1,hell?17:33,grid+1);List<String> bits=new ArrayList<String>();for(double d:values)bits.add(Long.toHexString(Double.doubleToLongBits(d)));row.put("density",bits);
                ChunkPrimer p=new ChunkPrimer();if(hell)((ChunkProviderHell)provider).func_180515_a(x,z,p);else((ChunkProviderEnd)provider).func_180520_a(x,z,p);row.put("base",blocks(p));
                random.setSeed((long)x*341873128712L+(long)z*132897987541L);if(hell)((ChunkProviderHell)provider).func_180516_b(x,z,p);else((ChunkProviderEnd)provider).func_180519_a(p);row.put("surface",blocks(p));row.put("surface_next",Long.toString(random.nextLong()));
                Chunk chunk=provider.provideChunk(x,z);row.put("blocks",blocks(chunk));row.put("biomes",biomes(chunk));row.put("next",Long.toString(random.nextLong()));rows.add(row);
            }
        }
        out.println(new Gson().toJson(rows));
    }
}
