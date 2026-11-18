import com.google.gson.Gson;
import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import net.minecraft.client.model.*;
import net.minecraft.entity.player.EntityPlayer;
import net.minecraft.init.Bootstrap;
import net.minecraft.potion.*;
import net.minecraft.profiler.Profiler;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.management.ServerConfigurationManager;
import net.minecraft.stats.StatisticsFile;
import net.minecraft.world.*;
import net.minecraft.world.storage.*;

public final class FirstPersonArmOracle {
    static class Player extends PlayerPoseOracle.Player {
        int haste=-1, fatigue=-1;
        Player(MinecraftServer host,WorldServer world) {super(host,world,1);}
        @Override public boolean isPotionActive(Potion effect) {
            return (effect==Potion.digSpeed && haste>=0)||(effect==Potion.digSlowdown && fatigue>=0);
        }
        @Override public PotionEffect getActivePotionEffect(Potion effect) {
            return new PotionEffect(effect.id,80,effect==Potion.digSpeed?haste:fatigue);
        }
        void animate() {prevSwingProgress=swingProgress; updateArmSwingProgress();}
    }
    static List<Object> geometry(ModelRenderer part) throws Exception {
        Field f=ModelBox.class.getDeclaredField("quadList");
        f.setAccessible(true);List<Object> quads=new ArrayList<>();
        for(TexturedQuad q:(TexturedQuad[])f.get(part.cubeList.get(0))){
            List<Object> vertices=new ArrayList<>();
            for(PositionTextureVertex v:q.vertexPositions) {
                vertices.add(Arrays.asList(v.vector3D.xCoord,v.vector3D.yCoord,v.vector3D.zCoord,
                    v.texturePositionX*64f,v.texturePositionY*64f));
            }
            quads.add(vertices);
        }
        return quads;
    }
    static long bits(float v) {return Integer.toUnsignedLong(Float.floatToIntBits(v));}
    public static void main(String[] args)throws Exception {
        Bootstrap.register(); File dir=new File(args[0]);dir.mkdirs();
        Constructor<?> ctor=Class.forName("VanillaWorldBridge$Host").getDeclaredConstructor(File.class);
        ctor.setAccessible(true);MinecraftServer host=(MinecraftServer)ctor.newInstance(dir);
        WorldSettings settings=new WorldSettings(189,WorldSettings.GameType.SURVIVAL,false,false,WorldType.DEFAULT);
        WorldServer w=new WorldServer(host,new SaveHandlerMP(),new WorldInfo(settings,"arms"),0,new Profiler());
        host.worldServers=new WorldServer[]{w,null,null}; w.init();
        host.setConfigManager(new ServerConfigurationManager(host){
            @Override public StatisticsFile getPlayerStatsFile(EntityPlayer p){return new StatisticsFile(host,new File(dir,"stats.json"));}
        });
        Player p=new Player(host,w);
        ModelPlayer m=new ModelPlayer(0,false);
        m.setRotationAngles(0,0,0,0,0,.0625f,p);
        List<Object> parts=Arrays.asList(geometry(m.bipedRightArm),geometry(m.bipedRightArmwear));
        List<Object> rows=new ArrayList<>();
        for(int haste:new int[]{-1,0,1,3,4,5,6}) for(int fatigue:new int[]{-1,0,2,4}){
            p.haste=haste;p.fatigue=fatigue;p.isSwingInProgress=false;p.swingProgressInt=0;p.swingProgress=0;p.prevSwingProgress=0;
            List<Object> ticks=new ArrayList<>();
            for(int t=0;t<64;t++){
                boolean click=t%11==0 || (t>=16&&t<32);
                if(click)p.swingItem();
                p.animate();
                ticks.add(Arrays.asList(click,p.isSwingInProgress,p.swingProgressInt,bits(p.swingProgress),
                    bits(p.getSwingProgress(.25f)),bits(p.getSwingProgress(.5f)),bits(p.getSwingProgress(1))));
            }
            rows.add(Arrays.asList(haste,fatigue,ticks));
        }
        try(Writer out=new OutputStreamWriter(new FileOutputStream(args[1]),"UTF-8")){
            new Gson().toJson(Arrays.asList(parts,rows),out);
        }
        System.out.println("Original empty arm: 48 vertices, "+rows.size()*64+" swing ticks.");
    }
}
