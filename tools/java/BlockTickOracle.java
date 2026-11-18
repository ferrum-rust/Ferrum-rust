// Verification only: schedules the original 1.8.9 block implementations in a headless flat world.
import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import net.minecraft.block.Block;
import net.minecraft.block.state.IBlockState;
import net.minecraft.entity.Entity;
import net.minecraft.entity.item.EntityFallingBlock;
import net.minecraft.init.Bootstrap;
import net.minecraft.profiler.Profiler;
import net.minecraft.server.MinecraftServer;
import net.minecraft.util.BlockPos;
import net.minecraft.world.*;
import net.minecraft.world.chunk.storage.AnvilSaveHandler;
import net.minecraft.world.storage.WorldInfo;
import com.google.gson.GsonBuilder;

public class BlockTickOracle {
    static int state(WorldServer world,int x,int y,int z) {
        IBlockState s=world.getBlockState(new BlockPos(x,y,z));return (Block.getIdFromBlock(s.getBlock())<<4)|s.getBlock().getMetaFromState(s);
    }
    static void set(WorldServer world,int[] edit) {
        Block b=Block.getBlockById(edit[3]>>4);world.setBlockState(new BlockPos(edit[0],edit[1],edit[2]),b.getStateFromMeta(edit[3]&15),3);
    }
    static WorldServer world(File directory) throws Exception {
        directory.mkdirs();
        Constructor<?> constructor=Class.forName("VanillaWorldBridge$Host").getDeclaredConstructor(File.class);constructor.setAccessible(true);
        MinecraftServer host=(MinecraftServer)constructor.newInstance(directory);
        WorldSettings settings=new WorldSettings(189,WorldSettings.GameType.SURVIVAL,false,false,WorldType.FLAT).setWorldName("3;7;1;");
        WorldInfo info=new WorldInfo(settings,"Original block oracle");
        WorldServer w=new WorldServer(host,new AnvilSaveHandler(directory.getParentFile(),directory.getName(),false),info,0,new Profiler());
        host.worldServers=new WorldServer[]{w,null,null};w.init();
        w.getGameRules().setOrCreateGameRule("doMobSpawning","false");w.getGameRules().setOrCreateGameRule("doFireTick","false");
        for(int x=-2;x<=2;x++)for(int z=-2;z<=2;z++)w.theChunkProviderServer.loadChunk(x,z);
        w.rand.setSeed(0);w.setTotalWorldTime(0);return w;
    }
    static Map<String,Object> snapshot(WorldServer w,int tick) {
        long hash=0xcbf29ce484222325L;List<int[]> blocks=new ArrayList<int[]>();
        for(int y=0;y<=10;y++)for(int z=-4;z<=20;z++)for(int x=-4;x<=20;x++) {
            int s=state(w,x,y,z);hash=(hash^(s&255))*0x100000001b3L;hash=(hash^(s>>>8))*0x100000001b3L;
            if(y>0&&s!=0)blocks.add(new int[]{x,y,z,s});
        }
        Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("tick",tick);row.put("hash",Long.toUnsignedString(hash));
        List<Object> falling=new ArrayList<Object>();
        for(Entity e:w.loadedEntityList)if(e instanceof EntityFallingBlock&&!e.isDead)falling.add(Arrays.asList(e.posX,e.posY,e.posZ,e.motionY,((EntityFallingBlock)e).fallTime));
        row.put("falling",falling);row.put("blocks",blocks);return row;
    }
    static Map<String,Object> run(File root,String name,int[][] edits,int removeAt) throws Exception {
        WorldServer w=world(new File(root,name));for(int[] edit:edits)set(w,edit);
        List<int[]> initial=new ArrayList<int[]>();
        for(int y=1;y<=10;y++)for(int z=-4;z<=20;z++)for(int x=-4;x<=20;x++){int s=state(w,x,y,z);if(s!=0)initial.add(new int[]{x,y,z,s});}
        Map<String,Object> result=new LinkedHashMap<String,Object>();result.put("name",name);result.put("initial",initial);result.put("remove_at",removeAt);
        List<Object> snapshots=new ArrayList<Object>();snapshots.add(snapshot(w,0));
        for(int tick=1;tick<=100;tick++) {
            if(tick==removeAt)set(w,new int[]{8,4,8,0});
            w.setTotalWorldTime(tick);w.tickUpdates(false);w.updateEntities();
            if(Arrays.binarySearch(new int[]{1,2,4,5,6,10,20,30,60,100},tick)>=0)snapshots.add(snapshot(w,tick));
        }
        result.put("snapshots",snapshots);return result;
    }
    public static void main(String[] args) throws Exception {
        Bootstrap.register();File out=new File(args[0]),root=new File(args[1]);List<Object> cases=new ArrayList<Object>();
        cases.add(run(root,"water_flat",new int[][]{{8,1,8,8<<4}},-1));
        cases.add(run(root,"water_fall",new int[][]{{8,4,8,8<<4}},-1));
        cases.add(run(root,"water_recedes",new int[][]{{8,4,8,8<<4}},20));
        cases.add(run(root,"sand",new int[][]{{8,5,8,12<<4}},-1));
        cases.add(run(root,"gravel_stack",new int[][]{{8,5,8,13<<4},{8,6,8,13<<4}},-1));
        cases.add(run(root,"water_meets_lava",new int[][]{{8,1,8,8<<4},{11,1,8,10<<4}},-1));
        cases.add(run(root,"two_water_sources",new int[][]{{8,1,8,8<<4},{10,1,8,8<<4}},-1));
        cases.add(run(root,"lava_flat",new int[][]{{8,1,8,10<<4}},-1));
        cases.add(run(root,"lava_recedes",new int[][]{{8,4,8,10<<4}},60));
        cases.add(run(root,"lava_falls_into_water",new int[][]{{8,1,8,8<<4},{8,2,8,10<<4}},-1));
        cases.add(run(root,"water_falls_onto_lava",new int[][]{{8,1,8,10<<4},{8,3,8,8<<4}},-1));
        cases.add(run(root,"red_sand",new int[][]{{8,9,8,(12<<4)|1}},-1));
        cases.add(run(root,"stationary_water_loaded_at_chunk_edge",new int[][]{{16,1,8,9<<4}},-1));
        cases.add(run(root,"stationary_lava_loaded_at_chunk_edge",new int[][]{{16,1,8,11<<4}},-1));
        out.getParentFile().mkdirs();try(Writer w=new OutputStreamWriter(new FileOutputStream(out),"UTF-8")){new GsonBuilder().serializeNulls().setPrettyPrinting().create().toJson(cases,w);}
        System.out.println("Original block tick oracle: "+cases.size()+" cases -> "+out);System.exit(0);
    }
}
