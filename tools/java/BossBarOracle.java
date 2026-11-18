import com.google.gson.Gson;
import java.io.*;
import java.lang.reflect.Method;
import java.nio.file.*;
import java.util.*;
import net.minecraft.client.gui.*;
import net.minecraft.client.renderer.GlStateManager;
import net.minecraft.client.renderer.culling.ICamera;
import net.minecraft.client.renderer.entity.Render;
import net.minecraft.entity.EntityLivingBase;
import net.minecraft.entity.SharedMonsterAttributes;
import net.minecraft.entity.boss.*;
import net.minecraft.util.AxisAlignedBB;
import net.minecraft.world.*;

public final class BossBarOracle {
    static class Overlay extends GuiIngame{
        List<int[]> quads=new ArrayList<>();
        Overlay(VanillaFrameBenchmark.Client mc){super(mc);}
        @Override public void drawTexturedModalRect(int x,int y,int u,int v,int w,int h){quads.add(new int[]{x,y,u,v,w,h});}
    }
    public static void main(String[] argv){
        try{
            VanillaFrameBenchmark.Client mc=MenuOracle.startClient(new File(argv[0]),new File(argv[1]),"1.8.9 boss HUD oracle");
            mc.gameSettings.renderDistanceChunks=2;
            mc.launchIntegratedServer("fixture","Fixture",new WorldSettings(189,WorldSettings.GameType.CREATIVE,false,false,WorldType.FLAT));
            java.lang.invoke.MethodHandle loop=VanillaFrameBenchmark.privateMethod(mc,"runGameLoop");
            long start=System.nanoTime();while(mc.thePlayer==null || mc.currentScreen!=null || mc.getRenderManager().livingPlayer!=mc.thePlayer){
                VanillaFrameBenchmark.runFrame(loop);if(System.nanoTime()-start>75_000_000_000L)throw new IOException("No camera");}
            Overlay hud=new Overlay(mc);Method draw=GuiIngame.class.getDeclaredMethod("renderBossHealth");draw.setAccessible(true);
            List<Object> frames=new ArrayList<>(), checks=new ArrayList<>();Map<String,Object> output=new LinkedHashMap<>();
            for(int kind:new int[]{64,63}){
                EntityLivingBase boss=kind==64?new EntityWither(mc.theWorld):new EntityDragon(mc.theWorld);
                boss.setPosition(0,64,0);boss.setInvisible(true);boss.setCustomNameTag("§5Plugin §aBoss");
                Render renderer=mc.getRenderManager().getEntityRenderObject(boss);
                for(final boolean inside:new boolean[]{true,false})for(double distance:new double[]{1,110,114,400,853,860}){
                    ICamera c=new ICamera(){public boolean isBoundingBoxInFrustum(AxisAlignedBB box){return inside;}public void setPosition(double x,double y,double z){}};
                    checks.add(Arrays.asList(kind,inside,distance,renderer.shouldRender(boss,c,distance,64.,0.)));
                }
                for(float health:new float[]{0,.125f,12.7f,57.5f,200f}){
                    boss.getEntityAttribute(SharedMonsterAttributes.maxHealth).setBaseValue(200);
                    boss.setHealth(health);renderer.doRender(boss,0,0,0,0,0);
                    String name=BossStatus.bossName;int duration=BossStatus.statusBarTime;int[] drawn=new int[3];
                    List<int[]> sample=null;
                    for(int i=0;i<102;i++){hud.quads.clear();draw.invoke(hud);if(i==0)sample=new ArrayList<>(hud.quads);
                        if(i>=99)drawn[i-99]=hud.quads.size();}
                    frames.add(Arrays.asList(kind,health,name,duration,sample,drawn));
                }
            }
            output.put("frames",frames);output.put("visibility",checks);output.put("width",new ScaledResolution(mc).getScaledWidth());
            try(Writer w=Files.newBufferedWriter(Paths.get(argv[2]))){new Gson().toJson(output,w);}
            System.exit(0);
        }catch(Throwable ex){ex.printStackTrace();System.exit(1);}
    }
}
