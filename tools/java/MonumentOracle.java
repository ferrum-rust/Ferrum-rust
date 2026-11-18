import java.io.*;
import java.util.*;
import java.lang.reflect.*;
import com.google.gson.Gson;
import net.minecraft.init.Bootstrap;
import net.minecraft.profiler.Profiler;
import net.minecraft.world.*;
import net.minecraft.world.biome.*;
import net.minecraft.world.storage.WorldInfo;
import net.minecraft.world.chunk.IChunkProvider;
import net.minecraft.world.gen.structure.*;

/** Executable 1.8.9 monument candidate/hidden room graph oracle. */
public final class MonumentOracle {
    static final class TestWorld extends World {
        final WorldChunkManager manager;
        TestWorld(long seed,boolean real){super(null,new WorldInfo(new WorldSettings(seed,WorldSettings.GameType.SURVIVAL,true,false,WorldType.DEFAULT),"oracle"),new WorldProviderSurface(),new Profiler(),false);provider.registerWorld(this);manager=real?new WorldChunkManager(seed,WorldType.DEFAULT,""):new WorldChunkManagerHell(BiomeGenBase.deepOcean,0.5f);}
        protected IChunkProvider createChunkProvider(){return null;}
        protected int getRenderDistanceChunks(){return 8;}
        public WorldChunkManager getWorldChunkManager(){return manager;}
    }
    static final class Generator extends StructureOceanMonument {
        boolean candidate(TestWorld world,int x,int z){worldObj=world;return canSpawnStructureAtCoords(x,z);}
        StructureStart start(TestWorld world,int x,int z){worldObj=world;return getStructureStart(x,z);}
        long next(){return rand.nextLong();}
    }
    static Object field(Object object,String name)throws Exception{Class<?> c=object.getClass();while(c!=null){try{Field f=c.getDeclaredField(name);f.setAccessible(true);return f.get(object);}catch(NoSuchFieldException e){c=c.getSuperclass();}}throw new AssertionError(name);}
    static int[] bbox(StructureBoundingBox b){return new int[]{b.minX,b.minY,b.minZ,b.maxX,b.maxY,b.maxZ};}
    static Map<String,Object> graph(StructureStart start)throws Exception {
        Object building=start.getComponents().getFirst();Map<String,Object> result=new LinkedHashMap<String,Object>();result.put("bounds",bbox(start.getBoundingBox()));result.put("orientation",((net.minecraft.util.EnumFacing)field(building,"coordBaseMode")).getHorizontalIndex());result.put("core",field(field(building,"field_175844_p"),"field_175967_a"));
        Map<Integer,Object> rooms=new TreeMap<Integer,Object>();List<Object> pending=new ArrayList<Object>();pending.add(field(building,"field_175845_o"));
        while(!pending.isEmpty()){Object room=pending.removeLast();int id=(Integer)field(room,"field_175967_a");if(rooms.containsKey(id))continue;rooms.put(id,room);for(Object n:(Object[])field(room,"field_175965_b"))if(n!=null)pending.add(n);}
        List<Object> rows=new ArrayList<Object>();for(Map.Entry<Integer,Object> entry:rooms.entrySet()){Object room=entry.getValue();Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("id",entry.getKey());List<Integer> neighbors=new ArrayList<Integer>();for(Object n:(Object[])field(room,"field_175965_b"))neighbors.add(n==null?null:(Integer)field(n,"field_175967_a"));row.put("neighbors",neighbors);row.put("open",field(room,"field_175966_c"));row.put("claimed",field(room,"field_175963_d"));row.put("source",field(room,"field_175964_e"));row.put("scan",field(room,"field_175962_f"));rows.add(row);}result.put("rooms",rows);
        rows=new ArrayList<Object>();for(Object piece:(List<?>)field(building,"field_175843_q")){Map<String,Object> row=new LinkedHashMap<String,Object>();String kind=piece.getClass().getSimpleName();row.put("kind",kind);row.put("bounds",bbox(((StructureComponent)piece).getBoundingBox()));Object room=field(piece,"field_175830_k");row.put("room",room==null?null:field(room,"field_175967_a"));row.put("variant",kind.equals("SimpleRoom")?field(piece,"field_175833_o"):kind.equals("WingRoom")?field(piece,"field_175834_o"):0);rows.add(row);}result.put("pieces",rows);return result;
    }
    static long hash(long h,long value){for(int n=0;n<8;n++){h^=(value>>>(n*8))&255;h*=1099511628211L;}return h;}
    public static void main(String[] args)throws Exception {
        PrintStream output=System.out;System.setOut(System.err);Bootstrap.register();StructureOceanMonumentPieces.registerOceanMonumentPieces();
        List<Object> queries=new ArrayList<Object>(),starts=new ArrayList<Object>();
        for(long seed:new long[]{0,1,-1,189,123456789012345L,Long.MIN_VALUE,Long.MAX_VALUE}){
            for(boolean real:new boolean[]{false,true}){TestWorld world=new TestWorld(seed,real);Generator gen=new Generator();long h=-3750763034362895579L;List<int[]> positives=new ArrayList<int[]>();for(int x=-96;x<=96;x++)for(int z=-96;z<=96;z++){boolean can=gen.candidate(world,x,z);h=hash(h,can?1:0);h=hash(h,world.rand.nextLong());if(can)positives.add(new int[]{x,z});}Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("seed",Long.toString(seed));row.put("real",real);row.put("hash",Long.toUnsignedString(h));row.put("candidates",positives);queries.add(row);}
            TestWorld world=new TestWorld(seed,false);Generator gen=new Generator();for(int[] chunk:new int[][]{{0,0},{-1,1},{-32,-58},{12345,-23456},{28,33},{-17,-39}}){StructureStart start=gen.start(world,chunk[0],chunk[1]);Map<String,Object> row=graph(start);row.put("seed",Long.toString(seed));row.put("chunk",chunk);row.put("next",Long.toString(gen.next()));starts.add(row);}
        }
        Map<String,Object> result=new LinkedHashMap<String,Object>();result.put("queries",queries);result.put("starts",starts);output.println(new com.google.gson.GsonBuilder().serializeNulls().create().toJson(result));
    }
}
