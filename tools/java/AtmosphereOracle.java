import com.google.gson.Gson;
import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import org.lwjgl.BufferUtils;
import org.lwjgl.opengl.*;
import net.minecraft.client.Minecraft;
import net.minecraft.client.entity.EntityPlayerSP;
import net.minecraft.client.multiplayer.WorldClient;
import net.minecraft.client.renderer.*;
import net.minecraft.client.settings.GameSettings;
import net.minecraft.entity.player.PlayerCapabilities;
import net.minecraft.item.ItemStack;
import net.minecraft.init.*;
import net.minecraft.enchantment.Enchantment;
import net.minecraft.potion.*;
import net.minecraft.util.*;
import net.minecraft.block.state.IBlockState;
import net.minecraft.world.*;
import net.minecraft.world.storage.WorldInfo;
import net.minecraft.world.biome.BiomeGenBase;
import sun.misc.Unsafe;

/** Original 1.8.9 private fog setup plus a tiny driver probe, no display or world files. */
public final class AtmosphereOracle {
    static final class Space extends WorldClient {
        BiomeGenBase biome;IBlockState medium;int light;
        Space(){super(null,null,0,null,null);}
        public BiomeGenBase getBiomeGenForCoords(BlockPos pos){return biome;}
        public IBlockState getBlockState(BlockPos p){return medium;}
        public int getLightFromNeighbors(BlockPos p){return light;}
    }
    static final class Eye extends EntityPlayerSP {
        int blind,vision;boolean water; Vec3 look;ItemStack[] armor;
        Eye(){super(null,null,null,null);}
        public Vec3 getLook(float partial){return look;}
        public ItemStack[] getInventory(){return armor;}
        public boolean isPotionActive(Potion p){return p==Potion.blindness?blind>=0:p==Potion.nightVision?vision>=0:p==Potion.waterBreathing&&water;}
        public PotionEffect getActivePotionEffect(Potion p){return new PotionEffect(p.id,p==Potion.blindness?blind:vision);}
        public float getFovModifier(){return 1f;}
    }
    static final class TickComplete extends RuntimeException {}
    static final class ItemBoundary extends ItemRenderer {
        ItemBoundary(){super(null);}
        @Override public void updateEquippedItem(){throw new TickComplete();}
    }
    static void set(Object o,Class<?> cls,String name,Object v)throws Exception{LightmapOracle.set(o,cls,name,v);}
    static Object get(Object o,Class<?> cls,String name)throws Exception{return LightmapOracle.field(cls,name).get(o);}
    static List<Long> bits(float... xs){List<Long>a=new ArrayList<Long>();for(float x:xs)a.add(Integer.toUnsignedLong(Float.floatToIntBits(x)));return a;}
    static List<Long> rgb(Vec3 v){return bits((float)v.xCoord,(float)v.yCoord,(float)v.zCoord);}
    static Map<String,Object> fogProbe(){
        boolean radial=GLContext.getCapabilities().GL_NV_fog_distance;
        if(radial)GL11.glFogi(34138,34139);
        GL11.glViewport(0,0,32,32);GL11.glMatrixMode(GL11.GL_PROJECTION);GL11.glLoadIdentity();GL11.glFrustum(-1,1,-1,1,1,400);
        GL11.glMatrixMode(GL11.GL_MODELVIEW);GL11.glLoadIdentity();GL11.glDisable(GL11.GL_TEXTURE_2D);GL11.glDisable(GL11.GL_DEPTH_TEST);
        GL11.glDisable(GL11.GL_LIGHTING);GL11.glDisable(GL11.GL_DITHER);GL11.glDisable(GL11.GL_BLEND);
        GL11.glEnable(GL11.GL_FOG);GL11.glFogi(GL11.GL_FOG_MODE,GL11.GL_LINEAR);GL11.glFogf(GL11.GL_FOG_START,0);GL11.glFogf(GL11.GL_FOG_END,40);
        java.nio.FloatBuffer black=BufferUtils.createFloatBuffer(4);black.put(new float[]{0,0,0,1}).flip();GL11.glFog(GL11.GL_FOG_COLOR,black);
        GL11.glClearColor(0,0,0,1);GL11.glClear(GL11.GL_COLOR_BUFFER_BIT);GL11.glColor4f(1,1,1,1);
        GL11.glBegin(GL11.GL_QUADS);GL11.glVertex3f(-20,-20,-20);GL11.glVertex3f(20,-20,-20);GL11.glVertex3f(20,20,-20);GL11.glVertex3f(-20,20,-20);GL11.glEnd();
        List<Integer> samples=new ArrayList<Integer>();java.nio.ByteBuffer pixels=BufferUtils.createByteBuffer(4);
        for(int x:new int[]{1,8,16,24,30}){GL11.glReadPixels(x,16,1,1,GL11.GL_RGBA,GL11.GL_UNSIGNED_BYTE,pixels);samples.add(pixels.get(0)&255);}
        Map<String,Object> m=new LinkedHashMap<String,Object>();m.put("renderer",GL11.glGetString(GL11.GL_RENDERER));m.put("nvRadial",radial);m.put("coordinateSource",GL11.glGetInteger(0x8450));m.put("z20_linear_0_40_red",samples);return m;
    }
    public static void main(String[] argv)throws Exception{
        Pbuffer surface=new Pbuffer(32,32,new PixelFormat(),null,null);
        try {surface.makeCurrent();export(argv[0]);}finally{surface.destroy();}
    }
    static void export(String file)throws Exception{
        Bootstrap.register();Unsafe u=(Unsafe)LightmapOracle.field(Unsafe.class,"theUnsafe").get(null);LightmapOracle.unsafe=u;
        Minecraft mc=(Minecraft)u.allocateInstance(Minecraft.class);Space w=(Space)u.allocateInstance(Space.class);Eye eye=(Eye)u.allocateInstance(Eye.class);
        EntityRenderer renderer=(EntityRenderer)u.allocateInstance(EntityRenderer.class);
        mc.theWorld=w;mc.thePlayer=eye;mc.gameSettings=(GameSettings)u.allocateInstance(GameSettings.class);
        set(mc,Minecraft.class,"renderViewEntity",eye);set(renderer,EntityRenderer.class,"mc",mc);
        set(renderer,EntityRenderer.class,"fogColorBuffer",BufferUtils.createFloatBuffer(16));eye.capabilities=new PlayerCapabilities();
        set(w,World.class,"cloudColour",0xFFFFFFL);set(w,World.class,"provider",LightmapOracle.makeProvider(0));
        Method update=EntityRenderer.class.getDeclaredMethod("updateFogColor",float.class);update.setAccessible(true);
        Method fog=EntityRenderer.class.getDeclaredMethod("setupFog",int.class,float.class);fog.setAccessible(true);
        List<Object> rows=new ArrayList<Object>(),lifts=new ArrayList<Object>();
        Random rand=new Random(189);
        List<BiomeGenBase> biomes=new ArrayList<BiomeGenBase>();for(BiomeGenBase b:BiomeGenBase.getBiomeGenArray())if(b!=null && b.biomeID!=9)biomes.add(b);
        for(int test=0;test<1536;test++){
            boolean clear=test<768;boolean flat=test%7==0;
            WorldType type=flat?WorldType.FLAT:WorldType.DEFAULT;
            WorldInfo info=new WorldInfo(new WorldSettings(189,WorldSettings.GameType.SURVIVAL,false,false,type),"sky");set(w,World.class,"worldInfo",info);
            set(w.provider,WorldProvider.class,"terrainType",type);w.biome=biomes.get(test%biomes.size());
            int time=(test*97)%24000;info.setWorldTime(time);
            float partial=(test%5)*0.25f;
            int x=rand.nextInt(10000)-5000,z=rand.nextInt(10000)-5000;double y=new double[]{78,250.5,32,13.75,0.5,-3.25}[test%6];
            eye.prevPosX=eye.posX=x+.25;eye.prevPosY=eye.lastTickPosY=eye.posY=y;eye.prevPosZ=eye.posZ=z+.5;
            float dx=(test%11-5)/5f;eye.look=new Vec3(dx,0,Math.sqrt(1.-(double)dx*dx));
            int radius=new int[]{2,3,4,8,12,16,24,32}[test%8];mc.gameSettings.renderDistanceChunks=radius;
            float rain=clear?0:rand.nextFloat();float thunder=clear?0:rand.nextFloat();
            set(w,World.class,"prevRainingStrength",rain);set(w,World.class,"rainingStrength",rain);
            set(w,World.class,"prevThunderingStrength",thunder);set(w,World.class,"thunderingStrength",thunder);
            int lightning=!clear&&test%9==0?test%3+1:0;set(w,World.class,"lastLightningBolt",lightning);
            int mode=clear?0:test%4;w.medium=(mode==1?Blocks.water: mode==2?Blocks.lava:Blocks.air).getDefaultState();set(renderer,EntityRenderer.class,"cloudFog",mode==3);
            int respiration=test%4;eye.armor=new ItemStack[]{new ItemStack(Items.diamond_helmet)};if(respiration>0)eye.armor[0].addEnchantment(Enchantment.respiration,respiration);
            eye.water=!clear&&test%3==1;eye.blind=!clear&&test%4==0?test%27:-1;eye.vision=!clear&&test%3==0?test%274:-1;
            float brightness=test%3==0?1:rand.nextFloat();set(renderer,EntityRenderer.class,"fogColor1",brightness);set(renderer,EntityRenderer.class,"fogColor2",brightness);
            float boss=!clear&&test%5==1?rand.nextFloat():0;set(renderer,EntityRenderer.class,"bossColorModifier",boss);set(renderer,EntityRenderer.class,"bossColorModifierPrev",boss);
            mc.gameSettings.anaglyph=!clear && test%11==0;
            set(renderer,EntityRenderer.class,"farPlaneDistance",radius*16f);
            update.invoke(renderer,partial);
            List<Long> color=bits((Float)get(renderer,EntityRenderer.class,"fogColorRed"),(Float)get(renderer,EntityRenderer.class,"fogColorGreen"),(Float)get(renderer,EntityRenderer.class,"fogColorBlue"));
            fog.invoke(renderer,-1,partial);float skyStart=GL11.glGetFloat(GL11.GL_FOG_START),skyEnd=GL11.glGetFloat(GL11.GL_FOG_END);
            fog.invoke(renderer,0,partial);
            int fogMode=GL11.glGetInteger(GL11.GL_FOG_MODE);
            float[] light=w.provider.calcSunriseSunsetColors(w.getCelestialAngle(partial),partial);
            float temperature=w.biome.getFloatTemperature(new BlockPos(eye));
            List<Object> input=Arrays.asList(time,partial,x,y,z,w.biome.biomeID,temperature,dx,radius,flat,brightness,mode,respiration,eye.water,eye.blind,eye.vision,rain,w.getThunderStrength(partial),lightning,boss,mc.gameSettings.anaglyph);
            rows.add(Arrays.asList(input,rgb(w.getSkyColor(eye,partial)),color,light==null?Collections.emptyList():bits(light),
                fogMode==GL11.GL_LINEAR?bits(skyStart,skyEnd,GL11.glGetFloat(GL11.GL_FOG_START),GL11.glGetFloat(GL11.GL_FOG_END)):bits(GL11.glGetFloat(GL11.GL_FOG_DENSITY)),w.calculateSkylightSubtracted(1f)));
        }
        // The original updateRenderer advances fogColor1 immediately before
        // calling itemRenderer.updateEquippedItem, the omitted rendering fixture.
        set(renderer,EntityRenderer.class,"fogColor1",0f);set(renderer,EntityRenderer.class,"fogColor2",0f);
        set(renderer,EntityRenderer.class,"mouseFilterXAxis",new MouseFilter());set(renderer,EntityRenderer.class,"mouseFilterYAxis",new MouseFilter());
        set(renderer,EntityRenderer.class,"random",new Random(189));set(renderer,EntityRenderer.class,"itemRenderer",u.allocateInstance(ItemBoundary.class));
        for(int tick=0;tick<256;tick++){
            w.light=rand.nextInt(16);int radius=2+rand.nextInt(31);mc.gameSettings.renderDistanceChunks=radius;
            try{renderer.updateRenderer();throw new AssertionError("Expected omitted ItemRenderer");}
            catch(TickComplete done){}
            lifts.add(Arrays.asList(w.light,radius,bits((Float)get(renderer,EntityRenderer.class,"fogColor2"),(Float)get(renderer,EntityRenderer.class,"fogColor1"))));
        }
        Map<String,Object> result=new LinkedHashMap<String,Object>();result.put("cases",rows);result.put("brightness",lifts);result.put("driver",fogProbe());
        try(Writer writer=new OutputStreamWriter(new java.util.zip.GZIPOutputStream(new FileOutputStream(file)),"UTF-8")){new Gson().toJson(result,writer);}
        System.out.println("Original atmosphere cases "+rows.size()+", brightness ticks "+lifts.size()+", fog probe "+result.get("driver"));
    }
}
