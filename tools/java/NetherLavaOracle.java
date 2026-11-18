import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import com.google.gson.Gson;
import net.minecraft.init.*;
import net.minecraft.world.*;
import net.minecraft.block.*;
import net.minecraft.util.BlockPos;
import net.minecraft.world.gen.feature.WorldGenHellLava;

/** Immediate Nether spring effects in actual WorldServer, including both RNGs. */
public final class NetherLavaOracle {
    public static void main(String[] args)throws Exception {
        Bootstrap.register();List<Object> rows=new ArrayList<Object>();int index=0;
        for(int pattern=0;pattern<4;pattern++)for(boolean enclosed:new boolean[]{false,true})for(long seed:new long[]{0,-1,189}){
            WorldServer w=BlockTickOracle.world(new File(args[1],"case-"+index++));for(String name:new String[]{"isHellWorld","hasNoSky"}){Field f=WorldProvider.class.getDeclaredField(name);f.setAccessible(true);f.setBoolean(w.provider,true);}
            List<int[]> edits=new ArrayList<int[]>();for(int x=4;x<=12;x++)for(int z=4;z<=12;z++)edits.add(new int[]{x,30,z,87<<4});for(int[] d:new int[][]{{0,1,0},{-1,0,0},{1,0,0},{0,0,-1},{0,0,1}})edits.add(new int[]{8+d[0],31+d[1],8+d[2],87<<4});
            if(pattern==1||pattern==3)edits.add(new int[]{9,31,8,0});if(pattern==2||pattern==3)edits.add(new int[]{8,30,8,0});if(seed==-1)edits.add(new int[]{8,31,8,87<<4});
            for(int[] edit:edits)BlockTickOracle.set(w,edit);w.rand.setSeed(seed^189L);Random random=new Random(seed);BlockFalling.fallInstantly=true;boolean result=new WorldGenHellLava(Blocks.flowing_lava,enclosed).generate(w,random,new BlockPos(8,31,8));BlockFalling.fallInstantly=false;
            Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("seed",Long.toString(seed));row.put("pattern",pattern);row.put("enclosed",enclosed);row.put("initial",edits);row.put("result",result);row.put("population_next",Long.toString(random.nextLong()));row.put("world_next",Long.toString(w.rand.nextLong()));long hash=0xcbf29ce484222325L;for(int x=-8;x<=23;x++)for(int y=1;y<=40;y++)for(int z=-8;z<=23;z++){int state=BlockTickOracle.state(w,x,y,z);hash=DimensionTerrainOracle.byteHash(DimensionTerrainOracle.byteHash(hash,state),state>>8);}row.put("hash",Long.toUnsignedString(hash));
            Field f=WorldServer.class.getDeclaredField("pendingTickListEntriesTreeSet");f.setAccessible(true);List<Object> pending=new ArrayList<Object>();for(Object item:(Set<?>)f.get(w)){NextTickListEntry e=(NextTickListEntry)item;pending.add(Arrays.asList(e.scheduledTime,e.position.getX(),e.position.getY(),e.position.getZ(),Block.getIdFromBlock(e.getBlock())));}row.put("pending",pending);rows.add(row);
        }
        try(Writer writer=new OutputStreamWriter(new FileOutputStream(args[0]),"UTF-8")){new Gson().toJson(rows,writer);}System.out.println(rows.size()+" actual WorldServer Nether spring cases");System.exit(0);
    }
}
