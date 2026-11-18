import java.io.*;
import java.util.*;
import com.google.gson.Gson;
import net.minecraft.init.Bootstrap;
import net.minecraft.nbt.NBTTagCompound;
import net.minecraft.world.gen.structure.*;

/** Original Nether fortress candidate RNG and complete component graph. */
public final class FortressOracle {
    static final class Generator extends MapGenNetherBridge {
        boolean candidate(StrongholdOracle.TestWorld world,int x,int z){worldObj=world;return canSpawnStructureAtCoords(x,z);}
        StructureStart start(int x,int z){return getStructureStart(x,z);}
        long next(){return rand.nextLong();}
    }
    static Map<String,Object> nbt(StructureComponent p){NBTTagCompound n=p.createStructureBaseNBT();Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("id",n.getString("id"));row.put("BB",n.getIntArray("BB"));row.put("O",n.getInteger("O"));row.put("GD",n.getInteger("GD"));if(n.hasKey("Seed"))row.put("Seed",n.getInteger("Seed"));for(String key:new String[]{"Chest","Mob"})if(n.hasKey(key))row.put(key,n.getBoolean(key));return row;}
    static long hash(long h,long value){for(int n=0;n<8;n++){h^=(value>>>(n*8))&255;h*=1099511628211L;}return h;}
    public static void main(String[] args)throws Exception {
        PrintStream output=System.out;System.setOut(System.err);Bootstrap.register();StructureNetherBridgePieces.registerNetherFortressPieces();List<Object> queries=new ArrayList<Object>(),starts=new ArrayList<Object>();
        for(long seed:new long[]{0,1,-1,123456789012345L,Long.MIN_VALUE,Long.MAX_VALUE}){
            StrongholdOracle.TestWorld world=new StrongholdOracle.TestWorld(seed);Generator gen=new Generator();long h=-3750763034362895579L;
            for(int x=-80;x<=80;x++)for(int z=-80;z<=80;z++){
                boolean can=gen.candidate(world,x,z);h=hash(h,can?1:0);h=hash(h,gen.next());if(!can)continue;
                if(!gen.candidate(world,x,z))throw new AssertionError();StructureStart start=gen.start(x,z);Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("seed",Long.toString(seed));row.put("chunk",new int[]{x,z});row.put("bounds",StrongholdOracle.bbox(start.getBoundingBox()));row.put("next",Long.toString(gen.next()));List<Object> pieces=new ArrayList<Object>();for(StructureComponent p:start.getComponents())pieces.add(nbt(p));row.put("pieces",pieces);starts.add(row);
            }Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("seed",Long.toString(seed));row.put("hash",Long.toUnsignedString(h));queries.add(row);
        }Map<String,Object> result=new LinkedHashMap<String,Object>();result.put("queries",queries);result.put("starts",starts);output.println(new Gson().toJson(result));
    }
}
