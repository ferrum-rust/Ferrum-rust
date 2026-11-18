import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import com.google.gson.Gson;
import net.minecraft.init.Bootstrap;
import net.minecraft.profiler.Profiler;
import net.minecraft.server.MinecraftServer;
import net.minecraft.util.*;
import net.minecraft.world.*;
import net.minecraft.world.biome.BiomeGenBase;
import net.minecraft.world.chunk.*;
import net.minecraft.world.chunk.storage.IChunkLoader;
import net.minecraft.world.gen.ChunkProviderServer;
import net.minecraft.world.storage.*;
import net.minecraft.entity.EnumCreatureType;

/** Original ChunkProviderServer + inherited Chunk.populateChunk controller.
 * Synthetic chunk generation and light initialization isolate event ordering. */
public final class ProviderOracle {
    static final List<Object> events=new ArrayList<Object>();
    static void event(String name,int x,int z){events.add(Arrays.asList(name,x,z));}
    static class TraceChunk extends Chunk {
        boolean dirty;
        TraceChunk(World w,int x,int z,boolean populated){super(w,x,z);setTerrainPopulated(populated);}
        public void onChunkLoad(){event("on_load",xPosition,zPosition);}
        public void onChunkUnload(){event("on_unload",xPosition,zPosition);}
        public void func_150809_p(){event("light",xPosition,zPosition);setTerrainPopulated(true);setLightPopulated(false);}
        public void setChunkModified(){super.setChunkModified();dirty=true;event("modified",xPosition,zPosition);}
    }
    static class Loader implements IChunkLoader {
        final Map<String,Boolean> saved=new HashMap<String,Boolean>();
        public Chunk loadChunk(World w,int x,int z){event("load_file",x,z);if(x==90&&z==90)throw new IllegalStateException("oracle load failure");Boolean p=saved.get(x+","+z);return p==null?null:new TraceChunk(w,x,z,p);}
        public void saveChunk(World w,Chunk c){saved.put(c.xPosition+","+c.zPosition,c.isTerrainPopulated());event("save",c.xPosition,c.zPosition);}
        public void saveExtraChunkData(World w,Chunk c){event("save_extra",c.xPosition,c.zPosition);}
        public void chunkTick(){}
        public void saveExtraData(){}
    }
    static class Generator implements IChunkProvider {
        WorldServer world; final List<int[]> nested; final boolean repopulate;
        Generator(WorldServer w,List<int[]> nested,boolean repopulate){world=w;this.nested=nested;this.repopulate=repopulate;}
        public boolean chunkExists(int x,int z){return true;}
        public Chunk provideChunk(int x,int z){event("generate",x,z);return new TraceChunk(world,x,z,false);}
        public Chunk provideChunk(BlockPos p){return provideChunk(p.getX()>>4,p.getZ()>>4);}
        public void populate(IChunkProvider provider,int x,int z){event("populate",x,z);for(int[] row:nested)if(row[0]==x&&row[1]==z)provider.provideChunk(row[2],row[3]);}
        public boolean populateChunk(IChunkProvider provider,Chunk c,int x,int z){event("repopulate",x,z);return repopulate;}
        public void recreateStructures(Chunk c,int x,int z){event("recreate",x,z);if(x==91&&z==90)throw new IllegalStateException("oracle recreate failure");}
        public boolean saveChunks(boolean all,IProgressUpdate progress){return true;}
        public boolean unloadQueuedChunks(){return false;}
        public boolean canSave(){return true;}
        public String makeString(){return "Provider ordering oracle";}
        public List<BiomeGenBase.SpawnListEntry> getPossibleCreatures(EnumCreatureType t,BlockPos p){return Collections.emptyList();}
        public BlockPos getStrongholdGen(World w,String name,BlockPos p){return null;}
        public int getLoadedChunkCount(){return 0;}
        public void saveExtraData(){}
    }
    static Map<String,Object> run(MinecraftServer host,String name,List<int[]> operations,int[][] saved,List<int[]> nested,boolean repopulate)throws Exception {
        WorldServer world=new WorldServer(host,new SaveHandlerMP(),new WorldInfo(new WorldSettings(189,WorldSettings.GameType.SURVIVAL,false,false,WorldType.DEFAULT),"provider-oracle"),0,new Profiler());
        host.worldServers=new WorldServer[]{world,null,null};
        Loader loader=new Loader();for(int[] p:saved)loader.saved.put(p[0]+","+p[1],p[2]!=0);
        Generator generator=new Generator(world,nested,repopulate);ChunkProviderServer provider=new ChunkProviderServer(world,loader,generator);
        world.theChunkProviderServer=provider;Field f=World.class.getDeclaredField("chunkProvider");f.setAccessible(true);f.set(world,provider);
        Field finding=World.class.getDeclaredField("findingSpawnPoint");finding.setAccessible(true);
        events.clear();List<Object> results=new ArrayList<Object>();
        for(int[] op:operations){
            if(op[0]==0){provider.loadChunk(op[1],op[2]);results.add(true);}
            else if(op[0]==1){Chunk c=provider.provideChunk(op[1],op[2]);results.add(!(c instanceof EmptyChunk));}
            else if(op[0]==2){provider.populate(provider,op[1],op[2]);results.add(true);}
            else if(op[0]==3){provider.chunkLoadOverride=op[1]!=0;results.add(null);}
            else if(op[0]==4){finding.setBoolean(world,op[1]!=0);results.add(null);}
        }
        List<Object> loaded=new ArrayList<Object>();for(Chunk c:provider.func_152380_a())loaded.add(Arrays.asList(c.xPosition,c.zPosition,c.isTerrainPopulated(),c.isLightPopulated(),((TraceChunk)c).dirty));
        Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("name",name);row.put("operations",operations);row.put("saved",saved);row.put("nested",new ArrayList<int[]>(nested));row.put("repopulate_result",repopulate);row.put("results",results);row.put("events",new ArrayList<Object>(events));row.put("loaded",loaded);return row;
    }
    static void permutations(List<int[]> out,int[] a,int n){if(n==a.length){out.add(a.clone());return;}for(int i=n;i<a.length;i++){int t=a[n];a[n]=a[i];a[i]=t;permutations(out,a,n+1);t=a[n];a[n]=a[i];a[i]=t;}}
    public static void main(String[] args)throws Exception {
        PrintStream output=System.out;System.setOut(System.err);Bootstrap.register();File root=new File(args[0]);root.mkdirs();Constructor<?> c=Class.forName("VanillaWorldBridge$Host").getDeclaredConstructor(File.class);c.setAccessible(true);MinecraftServer host=(MinecraftServer)c.newInstance(root);
        List<Object> cases=new ArrayList<Object>();List<int[]> orders=new ArrayList<int[]>();permutations(orders,new int[]{0,1,2,3},0);
        for(int off:new int[]{0,-7})for(int i=0;i<orders.size();i++){List<int[]> ops=new ArrayList<int[]>();for(int p:orders.get(i))ops.add(new int[]{0,off+(p&1),off+(p>>1)});ops.add(new int[]{0,off,off});cases.add(run(host,"square-"+off+"-"+i,ops,new int[0][],Collections.<int[]>emptyList(),false));}
        for(int trial=0;trial<12;trial++){List<int[]> ops=new ArrayList<int[]>();for(int x=-1;x<=2;x++)for(int z=-1;z<=2;z++)ops.add(new int[]{0,x,z});Collections.shuffle(ops,new Random(trial));ops.add(new int[]{2,0,0});List<int[]> nested=trial%2==0?Arrays.asList(new int[]{0,0,3,0},new int[]{0,0,3,1},new int[]{2,0,4,0},new int[]{2,0,4,1}):Collections.<int[]>emptyList();cases.add(run(host,"reentrant-"+trial,ops,new int[][]{{-1,-1,1},{0,-1,0},{1,1,1}},nested,trial%3==0));}
        cases.add(run(host,"override-and-explicit-populate",Arrays.asList(new int[]{3,0},new int[]{1,0,0},new int[]{4,1},new int[]{1,0,0},new int[]{4,0},new int[]{1,1,1},new int[]{0,1,1},new int[]{3,1},new int[]{2,1,1},new int[]{2,1,1},new int[]{0,0,1},new int[]{0,1,0}),new int[0][],Collections.<int[]>emptyList(),true));
        cases.add(run(host,"disk-and-recreate-errors-regenerate",Arrays.asList(new int[]{0,90,90},new int[]{0,91,90},new int[]{0,90,91},new int[]{0,91,91}),new int[][]{{90,90,1},{91,90,1}},Collections.<int[]>emptyList(),false));
        Map<String,Object> result=new LinkedHashMap<String,Object>();result.put("cases",cases);output.println(new Gson().toJson(result));
    }
}
