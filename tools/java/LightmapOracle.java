// Verification only. Execute EntityRenderer's actual color calculation while
// replacing only its final GPU upload with an inert DynamicTexture subclass.
import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import sun.misc.Unsafe;
import com.google.gson.Gson;
import net.minecraft.client.Minecraft;
import net.minecraft.client.entity.EntityPlayerSP;
import net.minecraft.client.multiplayer.WorldClient;
import net.minecraft.client.renderer.EntityRenderer;
import net.minecraft.client.renderer.texture.DynamicTexture;
import net.minecraft.client.settings.GameSettings;
import net.minecraft.init.Bootstrap;
import net.minecraft.potion.*;
import net.minecraft.profiler.Profiler;
import net.minecraft.world.*;
import net.minecraft.world.storage.WorldInfo;

public class LightmapOracle {
    static Unsafe unsafe;
    static Field field(Class<?> c,String n)throws Exception {Field f=c.getDeclaredField(n);f.setAccessible(true);return f;}
    static void set(Object o,Class<?> c,String n,Object v)throws Exception {field(c,n).set(o,v);}
    static class Texture extends DynamicTexture {
        Texture(){super(16,16);}
        @Override public void updateDynamicTexture(){}
    }
    static class Player extends EntityPlayerSP {
        int night;
        Player(){super(null,null,null,null);}
        @Override public boolean isPotionActive(Potion p){return p==Potion.nightVision&&night>=0;}
        @Override public PotionEffect getActivePotionEffect(Potion p){return new PotionEffect(p.id,night);}
    }
    static class ClientWorld extends WorldClient {
        Float fixedSun;long time;
        ClientWorld(){super(null,null,0,null,null);}
        @Override public float getSunBrightness(float partial){return fixedSun==null?super.getSunBrightness(partial):fixedSun;}
        @Override public long getWorldTime(){return time;}
    }
    static long bits(float f){return Integer.toUnsignedLong(Float.floatToRawIntBits(f));}
    static WorldProvider makeProvider(int dim)throws Exception {
        WorldProvider p=dim==-1?new WorldProviderHell():dim==1?new WorldProviderEnd():new WorldProviderSurface();
        set(p,WorldProvider.class,"dimensionId",dim);
        Method m=(dim==-1?WorldProviderHell.class:WorldProvider.class).getDeclaredMethod("generateLightBrightnessTable");m.setAccessible(true);m.invoke(p);return p;
    }
    static String colors(int[] a){StringBuilder b=new StringBuilder();for(int v:a)b.append(String.format(Locale.ROOT,"%06x",v&0xffffff));return b.toString();}
    static void exportFlicker(String file)throws Exception {
        EntityRenderer renderer=(EntityRenderer)unsafe.allocateInstance(EntityRenderer.class);
        Class<?> holder=Class.forName("java.lang.Math$RandomNumberGeneratorHolder");
        Random global=(Random)field(holder,"randomNumberGenerator").get(null);
        Method tick=EntityRenderer.class.getDeclaredMethod("updateTorchFlicker"); tick.setAccessible(true);
        Field x=field(EntityRenderer.class,"torchFlickerX"), dx=field(EntityRenderer.class,"torchFlickerDX");
        List<Object> result=new ArrayList<>();
        for(long seed:new long[]{0,1,189,-1,Long.MIN_VALUE,Long.MAX_VALUE}) {
            global.setSeed(seed);x.setFloat(renderer,0);dx.setFloat(renderer,0);
            List<Object> frames=new ArrayList<>();
            for(int i=0;i<1000;i++){tick.invoke(renderer);frames.add(Arrays.asList(bits(x.getFloat(renderer)),bits(dx.getFloat(renderer))));}
            result.add(Arrays.asList(Long.toString(seed),frames,Long.toString(global.nextLong())));
        }
        try(Writer out=new OutputStreamWriter(new FileOutputStream(file),"UTF-8")){new Gson().toJson(result,out);}
        System.out.println("Original EntityRenderer torch flicker: 6,000 ticks");
    }
    public static void main(String[] args)throws Exception {
        unsafe=(Unsafe)field(Unsafe.class,"theUnsafe").get(null);Bootstrap.register();
        if(args[0].equals("--flicker")){exportFlicker(args[1]);return;}
        Minecraft mc=(Minecraft)unsafe.allocateInstance(Minecraft.class);
        ClientWorld w=(ClientWorld)unsafe.allocateInstance(ClientWorld.class);
        WorldInfo info=new WorldInfo(new WorldSettings(189L,WorldSettings.GameType.SURVIVAL,false,false,WorldType.DEFAULT),"LightmapOracle");set(w,World.class,"worldInfo",info);
        Player player=(Player)unsafe.allocateInstance(Player.class);
        GameSettings settings=(GameSettings)unsafe.allocateInstance(GameSettings.class);
        set(mc,Minecraft.class,"mcProfiler",new Profiler());mc.theWorld=w;mc.thePlayer=player;mc.gameSettings=settings;
        EntityRenderer renderer=(EntityRenderer)unsafe.allocateInstance(EntityRenderer.class);
        set(renderer,EntityRenderer.class,"mc",mc);int[] out=new int[256];set(renderer,EntityRenderer.class,"lightmapColors",out);
        set(renderer,EntityRenderer.class,"lightmapTexture",unsafe.allocateInstance(Texture.class));
        Method update=EntityRenderer.class.getDeclaredMethod("updateLightmap",float.class);update.setAccessible(true);
        List<Object> rows=new ArrayList<Object>(),tables=new ArrayList<Object>(),sun=new ArrayList<Object>();
        Random random=new Random(189);int count=0;
        for(int dim:new int[]{-1,0,1}) {
            set(w,World.class,"provider",makeProvider(dim));List<Long> table=new ArrayList<Long>();for(float f:w.provider.getLightBrightnessTable())table.add(bits(f));tables.add(Arrays.asList(dim,table));
            for(int i=0;i<1200;i++) {
                float s=i<750?new float[]{0,.2f,.5f,.8f,1}[i%5]:random.nextFloat();
                float flicker=i<750?new float[]{-2,0,2}[(i/5)%3]:random.nextFloat()*8-4;
                float gamma=i<750?new float[]{0,.25f,.5f,.75f,1}[(i/15)%5]:random.nextFloat();
                int bolt=(i/75)%2;float boss=i<150?0:random.nextFloat(),previous=i<150?0:random.nextFloat(),partial=new float[]{0,.125f,.5f,.875f,1}[(i/150)%5];
                player.night=i<150?-1:new int[]{-1,0,1,19,20,99,199,200,201,1200}[i%10];
                w.fixedSun=s;set(w,World.class,"lastLightningBolt",bolt);settings.gammaSetting=gamma;
                set(renderer,EntityRenderer.class,"torchFlickerX",flicker);set(renderer,EntityRenderer.class,"bossColorModifier",boss);set(renderer,EntityRenderer.class,"bossColorModifierPrev",previous);set(renderer,EntityRenderer.class,"lightmapUpdateNeeded",true);
                update.invoke(renderer,partial);
                rows.add(Arrays.asList(dim,bits(s),bits(flicker),bits(gamma),bits(boss),bits(previous),bits(partial),bolt,player.night,colors(out)));count++;
            }
            w.fixedSun=null;
            for(long time:new long[]{Long.MIN_VALUE,-24001,-24000,-1,0,1,6000,12000,12500,18000,23000,23999,24000,Long.MAX_VALUE})for(float partial:new float[]{0,.5f,1})for(float rain:new float[]{0,.4f,1})for(float thunder:new float[]{0,1}) {
                w.time=time;info.setWorldTime(time);set(w,World.class,"prevRainingStrength",rain);set(w,World.class,"rainingStrength",rain);set(w,World.class,"prevThunderingStrength",thunder);set(w,World.class,"thunderingStrength",thunder);
                sun.add(Arrays.asList(dim,Long.toString(time),bits(partial),bits(rain),bits(thunder),bits(w.getCelestialAngle(partial)),bits(w.getSunBrightness(partial))));
            }
        }
        Map<String,Object> data=new LinkedHashMap<String,Object>();data.put("tables",tables);data.put("colors",rows);data.put("sun",sun);
        try(Writer writer=new OutputStreamWriter(new FileOutputStream(args[0]),"UTF-8")){new Gson().toJson(data,writer);}
        System.out.println("Original EntityRenderer lightmaps: "+count+" x 256; celestial/sun cases: "+sun.size());
    }
}
