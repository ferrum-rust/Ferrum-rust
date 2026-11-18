import com.google.gson.Gson;
import java.io.*;
import java.lang.reflect.Field;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import net.minecraft.client.entity.EntityPlayerSP;
import net.minecraft.client.particle.EntityFX;
import net.minecraft.client.particle.EntityPickupFX;
import net.minecraft.client.renderer.entity.RenderManager;
import net.minecraft.entity.Entity;
import net.minecraft.entity.item.EntityItem;
import net.minecraft.init.Blocks;
import net.minecraft.item.ItemStack;
import net.minecraft.world.*;

/** Capture the exact entity, coordinates, yaw and partial tick passed to the original entity renderer by EntityPickupFX. */
public final class PickupFxOracle {
    static final class Capture extends RenderManager {
        Object frame;
        Capture(VanillaFrameBenchmark.Client mc){super(mc.getTextureManager(),mc.getRenderItem());}
        @Override public boolean renderEntityWithPosYaw(Entity source,double x,double y,double z,float yaw,float partial){
            EntityItem item=(EntityItem)source;
            frame=Arrays.asList(new double[]{x,y,z},yaw,partial,item.getAge(),item.getEntityItem().stackSize);
            return true;
        }
    }
    public static void main(String[] args){
        try{
            VanillaFrameBenchmark.Client mc=MenuOracle.startClient(new File(args[0]),new File(args[1]),"1.8.9 pickup particle oracle");
            mc.gameSettings.renderDistanceChunks=2;
            mc.launchIntegratedServer("pickup-fx","Pickup FX",new WorldSettings(189,WorldSettings.GameType.SURVIVAL,false,false,WorldType.FLAT));
            java.lang.invoke.MethodHandle loop=VanillaFrameBenchmark.privateMethod(mc,"runGameLoop");long start=System.nanoTime();
            while(mc.thePlayer==null||mc.currentScreen!=null){VanillaFrameBenchmark.runFrame(loop);if(System.nanoTime()-start>75_000_000_000L)throw new IOException("No player");}
            EntityPlayerSP player=mc.thePlayer;
            EntityItem item=new EntityItem(mc.theWorld,13.125,81.75,11.375,new ItemStack(Blocks.stone,8));
            MenuOracle.field(EntityItem.class,"age").setInt(item,41);item.rotationYaw=125.5f;item.hoverStart=.7f;
            double[] origin={item.posX,item.posY,item.posZ},camera={8.625,82.25,9.125};
            EntityPickupFX effect=new EntityPickupFX(mc.theWorld,item,player,.5f);
            Capture capture=new Capture(mc);Field rm=MenuOracle.field(EntityPickupFX.class,"field_174842_aB");rm.set(effect,capture);
            EntityFX.interpPosX=camera[0];EntityFX.interpPosY=camera[1];EntityFX.interpPosZ=camera[2];
            List<Object> frames=new ArrayList<>(),dead=new ArrayList<>();dead.add(effect.isDead);
            for(int age=0;age<3;age++){
                double[] previous={8.+age*.25,80+age*.078125,9.-age*.125};
                double[] current={previous[0]+.25,previous[1]+.078125,previous[2]-.125};
                player.lastTickPosX=previous[0];player.lastTickPosY=previous[1];player.lastTickPosZ=previous[2];
                player.setPosition(current[0],current[1],current[2]);
                for(float partial:new float[]{0,.125f,.375f,.8333333f,1}){
                    capture.frame=null;effect.renderParticle(null,player,partial,0,0,0,0,0);
                    if(capture.frame==null)throw new AssertionError("item renderer missing");
                    frames.add(Arrays.asList(age,partial,previous,current,capture.frame));
                }
                effect.onUpdate();dead.add(effect.isDead);
            }
            List<Object> out=Arrays.asList(origin,camera,frames,dead,effect.getFXLayer());
            Files.write(Paths.get(args[2]),new Gson().toJson(out).getBytes(StandardCharsets.UTF_8));
            System.exit(0);
        }catch(Throwable t){t.printStackTrace();System.exit(1);}
    }
}
