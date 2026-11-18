import java.io.*;
import java.util.*;
import com.google.gson.Gson;
import net.minecraft.init.Bootstrap;
import net.minecraft.nbt.*;
import net.minecraft.profiler.Profiler;
import net.minecraft.world.*;
import net.minecraft.world.biome.*;
import net.minecraft.world.storage.WorldInfo;
import net.minecraft.world.chunk.IChunkProvider;
import net.minecraft.world.gen.structure.*;
import net.minecraft.util.BlockPos;

/** Runs original stronghold candidate and graph construction bytecode. */
public final class StrongholdOracle {
    static final class TestWorld extends World {
        final WorldChunkManager manager;
        TestWorld(long seed){super(null,new WorldInfo(new WorldSettings(seed,WorldSettings.GameType.SURVIVAL,true,false,WorldType.DEFAULT),"oracle"),new WorldProviderSurface(),new Profiler(),false);provider.registerWorld(this);manager=new WorldChunkManager(seed,WorldType.DEFAULT,"");}
        protected IChunkProvider createChunkProvider(){return null;}
        protected int getRenderDistanceChunks(){return 8;}
        public WorldChunkManager getWorldChunkManager(){return manager;}
    }
    static final class Map extends MapGenStronghold {
        List<int[]> positions(TestWorld world){worldObj=world;canSpawnStructureAtCoords(0,0);List<int[]> r=new ArrayList<int[]>();for(BlockPos p:getCoordList())r.add(new int[]{p.getX()>>4,p.getZ()>>4});return r;}
        StructureStart start(TestWorld world,long seed,int x,int z){worldObj=world;rand.setSeed(seed);long a=rand.nextLong(),b=rand.nextLong();rand.setSeed((long)x*a^(long)z*b^seed);rand.nextInt();return getStructureStart(x,z);}
        long next(){return rand.nextLong();}
    }
    static int[] bbox(StructureBoundingBox b){return new int[]{b.minX,b.minY,b.minZ,b.maxX,b.maxY,b.maxZ};}
    static java.util.Map<String,Object> nbt(StructureComponent p){
        NBTTagCompound n=p.createStructureBaseNBT();java.util.Map<String,Object> r=new LinkedHashMap<String,Object>();r.put("id",n.getString("id"));r.put("BB",n.getIntArray("BB"));r.put("O",n.getInteger("O"));r.put("GD",n.getInteger("GD"));r.put("EntryDoor",n.getString("EntryDoor"));
        for(String key:new String[]{"Type","Steps"})if(n.hasKey(key))r.put(key,n.getInteger(key));
        for(String key:new String[]{"Mob","leftLow","leftHigh","rightLow","rightHigh","Tall","Chest","Left","Right","Source"})if(n.hasKey(key))r.put(key,n.getBoolean(key));
        return r;
    }
    public static void main(String[] args)throws Exception {
        PrintStream output=System.out;System.setOut(System.err);Bootstrap.register();StructureStrongholdPieces.registerStrongholdPieces();
        List<Object> locations=new ArrayList<Object>(),starts=new ArrayList<Object>();
        for(long seed:new long[]{0,1,-1,189,7654321,-99999999,123456789012345L,Long.MIN_VALUE,Long.MAX_VALUE,42,817,12963,529683,-8946478}){
            TestWorld world=new TestWorld(seed);Map map=new Map();List<int[]> positions=map.positions(world);java.util.Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("seed",Long.toString(seed));row.put("positions",positions);locations.add(row);
            List<int[]> coords=new ArrayList<int[]>(positions);coords.add(new int[]{0,0});coords.add(new int[]{-1,1});coords.add(new int[]{-32,-58});coords.add(new int[]{12345,-23456});
            for(int[] chunk:coords){StructureStart start=map.start(world,seed,chunk[0],chunk[1]);row=new LinkedHashMap<String,Object>();row.put("seed",Long.toString(seed));row.put("chunk",chunk);row.put("bounds",bbox(start.getBoundingBox()));row.put("next",Long.toString(map.next()));List<Object> pieces=new ArrayList<Object>();for(StructureComponent p:start.getComponents())pieces.add(nbt(p));row.put("pieces",pieces);starts.add(row);}
        }
        java.util.Map<String,Object> result=new LinkedHashMap<String,Object>();result.put("positions",locations);result.put("starts",starts);output.println(new Gson().toJson(result));
    }
}
