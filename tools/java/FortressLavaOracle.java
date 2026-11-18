import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import com.google.gson.Gson;
import net.minecraft.init.Bootstrap;
import net.minecraft.nbt.NBTTagCompound;
import net.minecraft.world.*;
import net.minecraft.world.gen.structure.*;
import net.minecraft.block.Block;

/** Actual WorldServer execution of fortress Entrance and forced Nether lava. */
public final class FortressLavaOracle {
    public static void main(String[] args)throws Exception {
        Bootstrap.register();StructureNetherBridgePieces.registerNetherFortressPieces();List<Object> rows=new ArrayList<Object>();int index=0;
        for(int orientation=0;orientation<4;orientation++)for(long seed:new long[]{0,-1}){
            WorldServer w=BlockTickOracle.world(new File(args[1],"case-"+index++));for(String name:new String[]{"isHellWorld","hasNoSky"}){Field f=WorldProvider.class.getDeclaredField(name);f.setAccessible(true);f.setBoolean(w.provider,true);}
            NBTTagCompound tag=new NBTTagCompound();tag.setString("id","NeCE");tag.setIntArray("BB",new int[]{-3,40,-5,9,53,7});tag.setInteger("O",orientation);tag.setInteger("GD",5);StructureComponent piece=MapGenStructureIO.getStructureComponent(tag,w);
            Random random=new Random(seed);w.rand.setSeed(seed^189L);piece.addComponentParts(w,random,new StructureBoundingBox(-8,0,-8,23,255,23));
            Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("seed",Long.toString(seed));row.put("orientation",orientation);row.put("population_next",Long.toString(random.nextLong()));row.put("world_next",Long.toString(w.rand.nextLong()));List<int[]> blocks=new ArrayList<int[]>();for(int x=-8;x<=23;x++)for(int y=1;y<=64;y++)for(int z=-8;z<=23;z++){int state=BlockTickOracle.state(w,x,y,z);if(state!=0)blocks.add(new int[]{x,y,z,state});}row.put("blocks",blocks);
            Field f=WorldServer.class.getDeclaredField("pendingTickListEntriesTreeSet");f.setAccessible(true);List<Object> pending=new ArrayList<Object>();for(Object item:(Set<?>)f.get(w)){NextTickListEntry e=(NextTickListEntry)item;pending.add(Arrays.asList(e.scheduledTime,e.position.getX(),e.position.getY(),e.position.getZ(),Block.getIdFromBlock(e.getBlock())));}row.put("pending",pending);rows.add(row);
        }try(Writer writer=new OutputStreamWriter(new FileOutputStream(args[0]),"UTF-8")){new Gson().toJson(rows,writer);}System.out.println(rows.size()+" original WorldServer fortress lava cases");System.exit(0);
    }
}
