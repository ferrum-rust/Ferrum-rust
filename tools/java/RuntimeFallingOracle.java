import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import java.util.concurrent.atomic.AtomicLong;
import java.util.zip.GZIPOutputStream;
import com.google.gson.Gson;
import net.minecraft.block.Block;
import net.minecraft.block.state.IBlockState;
import net.minecraft.entity.Entity;
import net.minecraft.entity.item.*;
import net.minecraft.init.Bootstrap;
import net.minecraft.item.Item;
import net.minecraft.profiler.Profiler;
import net.minecraft.server.MinecraftServer;
import net.minecraft.util.BlockPos;
import net.minecraft.world.*;
import net.minecraft.world.storage.*;

/** Original WorldServer scheduled update and loaded-entity phases together. */
public final class RuntimeFallingOracle {
    static Field field(Class<?> c,String name) {try {Field f=c.getDeclaredField(name);f.setAccessible(true);return f;}catch(Exception e){throw new RuntimeException(e);}}
    static final Field SEED=field(Random.class,"seed"),ENTITY_RANDOM=field(Entity.class,"rand"),PICKUP=field(EntityItem.class,"delayBeforeCanPickup");
    static String randomState(Random r){try {return Long.toString(((AtomicLong)SEED.get(r)).get());}catch(Exception e){throw new RuntimeException(e);}}
    static List<String> bits(double... v){List<String> out=new ArrayList<>();for(double d:v)out.add(Long.toHexString(Double.doubleToRawLongBits(d)));return out;}
    static int state(IBlockState s){return Block.getIdFromBlock(s.getBlock())<<4|s.getBlock().getMetaFromState(s);}
    static class Fixture extends WorldServer {
        final IdentityHashMap<Entity,Integer> ids=new IdentityHashMap<>();
        final List<Object> drops=new ArrayList<>(),effects=new ArrayList<>();
        Fixture(MinecraftServer host){super(host,new SaveHandlerMP(),new WorldInfo(new WorldSettings(189,WorldSettings.GameType.SURVIVAL,false,false,WorldType.FLAT).setWorldName("3;7;1;"),"native falling oracle"),0,new Profiler());}
        public boolean spawnEntityInWorld(Entity e) {
            boolean ok=super.spawnEntityInWorld(e);if(!ok)return false;
            try {
                if(e instanceof EntityFallingBlock){int id=ids.size();ids.put(e,id);((Random)ENTITY_RANDOM.get(e)).setSeed(189L+17L*id);}
                if(e instanceof EntityItem){EntityItem item=(EntityItem)e;drops.add(Arrays.asList(bits(e.posX,e.posY,e.posZ),Item.getIdFromItem(item.getEntityItem().getItem()),item.getEntityItem().getMetadata(),item.getEntityItem().stackSize,PICKUP.getInt(item)));}
            }catch(Exception ex){throw new RuntimeException(ex);}return true;
        }
        public void playAuxSFX(int id,BlockPos p,int data){effects.add(Arrays.asList(id,p.getX(),p.getY(),p.getZ(),data));super.playAuxSFX(id,p,data);}
    }
    static List<Object> snapshot(Fixture w,int time)throws Exception {
        List<Object> blocks=new ArrayList<>(),falling=new ArrayList<>();
        for(int y=1;y<=12;y++)for(int z=6;z<=10;z++)for(int x=6;x<=10;x++){int s=state(w.getBlockState(new BlockPos(x,y,z)));if(s!=0)blocks.add(new int[]{x,y,z,s});}
        for(Entity e:w.loadedEntityList)if(e instanceof EntityFallingBlock&&!e.isDead){EntityFallingBlock f=(EntityFallingBlock)e;falling.add(Arrays.asList(w.ids.get(e),state(f.getBlock()),f.fallTime,bits(e.posX,e.posY,e.posZ,e.motionX,e.motionY,e.motionZ),e.onGround,randomState((Random)ENTITY_RANDOM.get(e))));}
        return Arrays.asList(time,blocks,falling,new ArrayList<>(w.drops),new ArrayList<>(w.effects),randomState(w.rand));
    }
    static Map<String,Object> run(MinecraftServer host,int s,int scenario)throws Exception {
        Fixture w=new Fixture(host);host.worldServers=new WorldServer[]{w,null,null};w.init();w.getGameRules().setOrCreateGameRule("doMobSpawning","false");w.getGameRules().setOrCreateGameRule("doFireTick","false");
        for(int x=-2;x<=2;x++)for(int z=-2;z<=2;z++)w.theChunkProviderServer.loadChunk(x,z);
        w.setTotalWorldTime(0);w.rand.setSeed(0);
        List<int[]> edits=new ArrayList<>();
        if(scenario==1){edits.add(new int[]{8,7,8,s});edits.add(new int[]{8,8,8,s});}
        if(scenario==2)edits.add(new int[]{8,5,8,16});
        if(scenario==3)edits.add(new int[]{8,2,8,30<<4});
        if(scenario==4)edits.add(new int[]{8,1,8,50<<4|5});
        if(scenario==5)edits.add(new int[]{8,1,8,44<<4});
        if(scenario==6)edits.add(new int[]{8,1,8,165<<4});
        edits.add(new int[]{8,6,8,s});
        for(int[] edit:edits)BlockTickOracle.set(w,edit);
        List<Object> frames=new ArrayList<>();frames.add(snapshot(w,0));
        int steps=scenario==3?100:25;
        for(int i=1;i<=steps;i++){
            if(i==5&&scenario==2)BlockTickOracle.set(w,new int[]{8,5,8,0});
            w.setTotalWorldTime(i);w.tickUpdates(false);w.updateEntities();frames.add(snapshot(w,i));
        }
        Map<String,Object> row=new LinkedHashMap<>();row.put("state",s);row.put("scenario",scenario);row.put("edits",edits);row.put("frames",frames);return row;
    }
    public static void main(String[] args)throws Exception {
        Bootstrap.register();File dir=new File(args[1]);dir.mkdirs();Constructor<?> c=Class.forName("VanillaWorldBridge$Host").getDeclaredConstructor(File.class);c.setAccessible(true);MinecraftServer host=(MinecraftServer)c.newInstance(dir);
        List<Object> cases=new ArrayList<>();for(int state:new int[]{12<<4,12<<4|1,13<<4,145<<4|5,145<<4|10})for(int scenario=0;scenario<7;scenario++)cases.add(run(host,state,scenario));
        try(Writer out=new OutputStreamWriter(new GZIPOutputStream(new FileOutputStream(args[0])),"UTF-8")){new Gson().toJson(cases,out);}
        System.out.println("Original WorldServer falling runtime: "+cases.size()+" scenarios");
    }
}
