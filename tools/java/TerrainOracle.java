// Test-only harness: invokes the user's original ChunkProviderGenerate bytecode.
import com.google.gson.Gson;
import java.lang.reflect.*;
import java.security.MessageDigest;
import java.util.*;
import java.nio.file.*;
import sun.misc.Unsafe;
import net.minecraft.init.Bootstrap;
import net.minecraft.block.Block;
import net.minecraft.block.state.IBlockState;
import net.minecraft.world.*;
import net.minecraft.world.biome.*;
import net.minecraft.world.chunk.*;
import net.minecraft.world.gen.*;
import net.minecraft.world.storage.WorldInfo;
import net.minecraft.util.BlockPos;

public final class TerrainOracle {
    static final class OracleWorld extends WorldServer {
        OracleWorld(){super(null,null,null,0,null);}
        @Override public BiomeGenBase getBiomeGenForCoords(BlockPos pos){return getWorldChunkManager().getBiomeGenerator(pos);}
    }
    static Object get(Object object,Class<?> cls,String name)throws Exception {Field f=cls.getDeclaredField(name);f.setAccessible(true);return f.get(object);}
    static void set(Object object,Class<?> cls,String name,Object value)throws Exception {Field f=cls.getDeclaredField(name);f.setAccessible(true);f.set(object,value);}
    static World world(long seed,WorldChunkManager manager)throws Exception {
        Field u=Unsafe.class.getDeclaredField("theUnsafe");u.setAccessible(true);
        World w=(World)((Unsafe)u.get(null)).allocateInstance(OracleWorld.class);
        set(w,World.class,"worldInfo",new WorldInfo(new WorldSettings(seed,WorldSettings.GameType.SURVIVAL,false,false,WorldType.DEFAULT),"Oracle"));
        WorldProvider provider=new WorldProviderSurface();set(provider,WorldProvider.class,"worldChunkMgr",manager);
        set(w,World.class,"provider",provider);w.setSeaLevel(63);return w;
    }
    static byte[] bytes(ChunkPrimer primer) {
        byte[] data=new byte[65536*2];int i=0;
        for(int y=0;y<256;y++)for(int z=0;z<16;z++)for(int x=0;x<16;x++){
            IBlockState state=primer.getBlockState(x,y,z);int value=(Block.getIdFromBlock(state.getBlock())<<4)|state.getBlock().getMetaFromState(state);
            data[i++]=(byte)value;data[i++]=(byte)(value>>8);
        }
        return data;
    }
    static String sha(byte[] data)throws Exception {StringBuilder s=new StringBuilder();for(byte b:MessageDigest.getInstance("SHA-256").digest(data))s.append(String.format("%02x",b&255));return s.toString();}
    static String fnv(byte[] data){long hash=0xcbf29ce484222325L;for(byte b:data)hash=(hash^(b&255))*0x100000001b3L;return Long.toUnsignedString(hash,16);}
    static List<Integer> ids(BiomeGenBase[] biomes){List<Integer> result=new ArrayList<Integer>();for(BiomeGenBase b:biomes)result.add(b.biomeID);return result;}
    static Map<String,Object> run(long seed,int x,int z,int fixed,Path directory)throws Exception {
        WorldChunkManager manager=fixed<0?new WorldChunkManager(seed,WorldType.DEFAULT,""):new WorldChunkManagerHell(BiomeGenBase.getBiome(fixed),0.5f);
        World world=world(seed,manager);ChunkProviderGenerate gen=new ChunkProviderGenerate(world,seed,false,"");
        long initial=((Random)get(gen,ChunkProviderGenerate.class,"rand")).nextLong();
        ((Random)get(gen,ChunkProviderGenerate.class,"rand")).setSeed((long)x*341873128712L+(long)z*132897987541L);
        ChunkPrimer primer=new ChunkPrimer();gen.setBlocksInChunk(x,z,primer);
        String name=Long.toString(seed)+"_"+x+"_"+z+"_"+fixed;
        Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("name",name);row.put("seed",Long.toString(seed));row.put("x",x);row.put("z",z);row.put("fixed",fixed);
        row.put("initial_next_long",Long.toString(initial));
        row.put("generation",ids(manager.getBiomesForGeneration(null,x*4-2,z*4-2,10,10)));
        double[] density=(double[])get(gen,ChunkProviderGenerate.class,"field_147434_q");List<String> bits=new ArrayList<String>();for(double d:density)bits.add(String.format("%016x",Double.doubleToRawLongBits(d)));row.put("density",bits);
        byte[] base=bytes(primer);row.put("base_sha256",sha(base));row.put("base_fnv",fnv(base));Files.write(directory.resolve(name+".base.bin"),base);
        BiomeGenBase[] biomes=manager.loadBlockGeneratorData(null,x*16,z*16,16,16);row.put("biomes",ids(biomes));
        gen.replaceBlocksForBiome(x,z,primer,biomes);
        byte[] surface=bytes(primer);row.put("surface_sha256",sha(surface));row.put("surface_fnv",fnv(surface));Files.write(directory.resolve(name+".surface.bin"),surface);
        List<Integer> tops=new ArrayList<Integer>();for(BiomeGenBase b:biomes)tops.add((Block.getIdFromBlock(b.topBlock.getBlock())<<4)|b.topBlock.getBlock().getMetaFromState(b.topBlock));row.put("carver_tops",tops);
        new MapGenCaves().generate(gen,world,x,z,primer);new MapGenRavine().generate(gen,world,x,z,primer);
        byte[] carved=bytes(primer);row.put("carved_sha256",sha(carved));row.put("carved_fnv",fnv(carved));Files.write(directory.resolve(name+".carved.bin"),carved);
        row.put("next_long",Long.toString(((Random)get(gen,ChunkProviderGenerate.class,"rand")).nextLong()));
        return row;
    }
    public static void main(String[] args)throws Exception {
        Bootstrap.register();Path directory=Paths.get(args[0]);Files.createDirectories(directory);
        List<Object> cases=new ArrayList<Object>();
        for(long seed:new long[]{0L,1L,189L,-1L,Long.MIN_VALUE})for(int[] p:new int[][]{{0,0},{-1,2},{31,-17}})cases.add(run(seed,p[0],p[1],-1,directory));
        for(BiomeGenBase b:BiomeGenBase.getBiomeGenArray())if(b!=null)cases.add(run(189L,-3,5,b.biomeID,directory));
        Map<String,Object> output=new LinkedHashMap<String,Object>();output.put("cases",cases);
        System.out.println(new Gson().toJson(output));
    }
}
