import com.google.gson.Gson;
import java.io.*;
import java.lang.reflect.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import net.minecraft.entity.item.EntityItem;
import net.minecraft.init.*;
import net.minecraft.item.ItemStack;
import net.minecraft.network.play.client.C07PacketPlayerDigging;
import net.minecraft.network.play.client.C03PacketPlayer.C04PacketPlayerPosition;
import net.minecraft.server.MinecraftServer;
import net.minecraft.util.*;
import net.minecraft.world.*;

/** C07 DROP_ITEM/DROP_ALL_ITEMS and the original player's collision callback, ahead of the world's item tick. */
public final class QPickupOracle {
    static Field delay;
    static class Scene {
        final RuntimeUseOracle.Fixture world;
        final RuntimeUseOracle.Player player;
        final RuntimeUseOracle.Handler connection;
        Scene(MinecraftServer host) throws Exception {
            world=new RuntimeUseOracle.Fixture(host);host.worldServers=new WorldServer[]{world,null,null};
            world.init();world.theChunkProviderServer.loadChunk(0,0);
            for(int x=0;x<16;x++)for(int z=0;z<16;z++)
                world.getChunkFromChunkCoords(0,0).setBlockState(new BlockPos(x,63,z),Blocks.stone.getDefaultState());
            player=new RuntimeUseOracle.Player(host,world);connection=new RuntimeUseOracle.Handler(host,player);
            player.theItemInWorldManager.setGameType(WorldSettings.GameType.SURVIVAL);
            stand(8,8);
        }
        void stand(double x,double z) {
            player.setPosition(x,64,z);player.onGround=true;
            player.motionX=player.motionY=player.motionZ=0;
        }
        int inventory() {
            int count=0;
            for(ItemStack s:player.inventory.mainInventory)if(s!=null&&s.getItem()==net.minecraft.item.Item.getItemFromBlock(Blocks.stone))count+=s.stackSize;
            return count;
        }
        Object sample(int tick,EntityItem e) throws Exception {
            return Arrays.asList(tick,e.getAge(),delay.getInt(e),new double[]{e.posX,e.posY,e.posZ},
                new double[]{e.motionX,e.motionY,e.motionZ},new double[]{player.posX,player.posY,player.posZ},
                inventory(),e.getEntityItem().stackSize,e.isDead);
        }
        Map<String,Object> trace(String name,boolean all,float pitch,String path) throws Exception {
            boolean packet=path.startsWith("packet-");
            if(packet){
                path=path.substring(7);
                connection.setPlayerLocation(8,64,8,0,pitch);
                // First accepted post-teleport C04 before pressing Q.
                player.ticksExisted=1;
                connection.processPlayer(new C04PacketPlayerPosition(8,64,8,true));
            }
            player.rotationYaw=0;player.rotationPitch=pitch;player.getRNG().setSeed(189);
            player.inventory.mainInventory[0]=new ItemStack(Blocks.stone,12);
            connection.processPlayerDigging(new C07PacketPlayerDigging(all?C07PacketPlayerDigging.Action.DROP_ALL_ITEMS:C07PacketPlayerDigging.Action.DROP_ITEM,
                                                                     BlockPos.ORIGIN,EnumFacing.DOWN));
            EntityItem e=null;
            for(Object obj:world.loadedEntityList)if(obj instanceof EntityItem)e=(EntityItem)obj;
            if(e==null)throw new AssertionError("No C07 ground entity");
            Map<String,Object> out=new LinkedHashMap<>();out.put("name",name);out.put("all",all);out.put("pitch",pitch);out.put("seed",189);out.put("thrower",e.getThrower());
            List<Object> frames=new ArrayList<>();frames.add(sample(0,e));out.put("frames",frames);
            for(int tick=1;tick<=70&&!e.isDead;tick++){
                double x=path.equals("follow")?e.posX:8;
                double z=path.equals("follow")?e.posZ:path.equals("approach")&&tick>49?Math.min(e.posZ,8+(tick-49)*0.215):8;
                if(packet)connection.processPlayer(new C04PacketPlayerPosition(x,64,z,true));
                else {stand(x,z);player.onLivingUpdate();}
                if(!e.isDead){++e.ticksExisted;e.onUpdate();}
                frames.add(sample(tick,e));
            }
            return out;
        }
        Object boundary(double x,double y,double z) throws Exception {
            stand(8,8);
            EntityItem e=new EntityItem(world,8+x,64+y,8+z,new ItemStack(Blocks.stone,1));
            e.setNoPickupDelay();world.spawnEntityInWorld(e);
            int old=inventory();player.onLivingUpdate();boolean picked=inventory()==old+1;
            e.setDead();world.removeEntity(e);
            return Arrays.asList(new double[]{x,y,z},picked);
        }
        Object capacity(int room,boolean creative) throws Exception {
            player.capabilities.isCreativeMode=creative;
            for(int i=0;i<36;i++)player.inventory.mainInventory[i]=new ItemStack(i==0?Blocks.stone:Blocks.dirt,i==0?64-room:64);
            EntityItem e=new EntityItem(world,8,64,8,new ItemStack(Blocks.stone,8));e.setNoPickupDelay();world.spawnEntityInWorld(e);
            stand(8,8);player.onLivingUpdate();
            Object result=Arrays.asList(room,creative,inventory(),e.getEntityItem().stackSize,e.isDead);
            e.setDead();world.removeEntity(e);return result;
        }
    }
    public static void main(String[] args)throws Exception{
        Bootstrap.register();Constructor<?> c=Class.forName("VanillaWorldBridge$Host").getDeclaredConstructor(File.class);c.setAccessible(true);
        MinecraftServer host=(MinecraftServer)c.newInstance(new File(args[1]));
        RuntimeUseOracle.field(MinecraftServer.class,"serverThread").set(host,Thread.currentThread());
        delay=RuntimeUseOracle.field(EntityItem.class,"delayBeforeCanPickup");
        List<Object> traces=new ArrayList<>();
        traces.add(new Scene(host).trace("Q stationary",false,0,"stay"));
        traces.add(new Scene(host).trace("Ctrl+Q stationary",true,0,"stay"));
        traces.add(new Scene(host).trace("Q looking down",false,90,"stay"));
        traces.add(new Scene(host).trace("Ctrl+Q following",true,0,"follow"));
        traces.add(new Scene(host).trace("Q approaching after tick 49",false,0,"approach"));
        List<Object> packetTraces=new ArrayList<>();
        packetTraces.add(new Scene(host).trace("C04 Q looking down",false,90,"packet-stay"));
        packetTraces.add(new Scene(host).trace("C04 Q approaching after tick 49",false,0,"packet-approach"));
        Scene s=new Scene(host);double edge=1.125+(double)(s.player.width/2f);
        double above=.5+(double)s.player.height;
        List<Object> bounds=new ArrayList<>();
        for(double[] d:new double[][]{{edge-1e-7,0,edge-1e-7},{edge,0,0},{edge+1e-7,0,0},{-edge,0,0},
                                      {0,above-1e-7,0},{0,above,0},{0,-.75,0},{0,-.75+1e-7,0},
                                      {0,.5,0},{1.4,1.9,1.4}})
            bounds.add(s.boundary(d[0],d[1],d[2]));
        List<Object> capacity=new ArrayList<>();for(boolean creative:new boolean[]{false,true})
            for(int room:new int[]{0,3,8})capacity.add(s.capacity(room,creative));
        Map<String,Object> output=new LinkedHashMap<>();output.put("traces",traces);output.put("bounds",bounds);
        output.put("capacity",capacity);output.put("serverMovementPackets",packetTraces);
        Files.write(Paths.get(args[0]),new Gson().toJson(output).getBytes(StandardCharsets.UTF_8));
        System.exit(0);
    }
}
