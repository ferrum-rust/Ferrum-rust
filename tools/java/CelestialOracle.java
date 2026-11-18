import com.google.gson.Gson;
import java.io.*;
import javax.imageio.ImageIO;
import net.minecraft.client.renderer.texture.TextureUtil;
import org.lwjgl.util.glu.GLU;
import java.lang.reflect.*;
import java.nio.*;
import java.util.*;
import java.util.zip.GZIPOutputStream;
import net.minecraft.client.*;
import net.minecraft.client.entity.EntityPlayerSP;
import net.minecraft.client.multiplayer.WorldClient;
import net.minecraft.client.renderer.*;
import net.minecraft.client.renderer.texture.TextureManager;
import net.minecraft.client.settings.GameSettings;
import net.minecraft.init.Bootstrap;
import net.minecraft.util.*;
import net.minecraft.world.*;
import net.minecraft.world.storage.*;
import org.lwjgl.BufferUtils;
import org.lwjgl.opengl.*;
import sun.misc.Unsafe;

/** Intercept actual Tessellator uploads from the local game's RenderGlobal.renderSky.
 * The uploader records positions, UVs, colors, matrices and GL blend states. */
public final class CelestialOracle {
    static class Space extends WorldClient {
        float rain;
        Space(){super(null,null,0,null,null);}
        @Override public Vec3 getSkyColor(net.minecraft.entity.Entity e,float t){return new Vec3(.5,.6,.8);}
        @Override public float getRainStrength(float t){return rain;}
        @Override public double getHorizon(){return 63.;}
    }
    static class Player extends EntityPlayerSP {
        Player(){super(null,null,null,null);}
        @Override public Vec3 getPositionEyes(float t){return new Vec3(0,80,0);}
    }
    static class Textures extends TextureManager {
        String name="";boolean render;Map<String,Integer> ids=new HashMap<>(); Textures(){super(null);}
        void load()throws Exception {
            for(String s:new String[]{"textures/environment/sun.png","textures/environment/moon_phases.png"}){
                int id=TextureUtil.glGenTextures();
                TextureUtil.uploadTextureImageAllocate(id,ImageIO.read(CelestialOracle.class.getResourceAsStream("/assets/minecraft/"+s)),false,false);
                ids.put(s,id);
            }
        }
        @Override public void bindTexture(ResourceLocation name){this.name=name.getResourcePath();if(render)GlStateManager.bindTexture(ids.get(this.name));}
    }
    static final class Record extends WorldVertexBufferUploader {
        List<Object> calls=new ArrayList<>(); Textures textures;
        @Override public void draw(WorldRenderer b) {
            if(textures.render){super.draw(b);return;}
            ByteBuffer data=b.getByteBuffer().duplicate().order(ByteOrder.nativeOrder());
            int stride=b.getVertexFormat().getNextOffset(),mode=b.getDrawMode();
            List<Object> verts=new ArrayList<>();
            for(int i=0;i<b.getVertexCount();i++){
                int p=i*stride;
                List<Object> row=new ArrayList<>();
                for(int j=0;j<3;j++)row.add(data.getFloat(p+j*4));
                if(mode==6){for(int c=0;c<4;c++)row.add(data.get(p+12+c)&255);}
                else {row.add(data.getFloat(p+12));row.add(data.getFloat(p+16));}
                verts.add(row);
            }
            FloatBuffer f=BufferUtils.createFloatBuffer(16);
            GL11.glGetFloat(GL11.GL_MODELVIEW_MATRIX,f);float[] matrix=new float[16];f.get(matrix);
            ((Buffer)f).clear();GL11.glGetFloat(GL11.GL_CURRENT_COLOR,f);float[] color=new float[4];f.get(color);
            calls.add(Arrays.asList(mode,textures.name,color,matrix,verts,
                GL11.glGetInteger(GL14.GL_BLEND_SRC_RGB),GL11.glGetInteger(GL14.GL_BLEND_DST_RGB),
                GL11.glGetInteger(GL14.GL_BLEND_SRC_ALPHA),GL11.glGetInteger(GL14.GL_BLEND_DST_ALPHA)));
            b.reset();
        }
    }
    static void set(Object o,Class<?> c,String name,Object value)throws Exception{
        Field f=c.getDeclaredField(name);f.setAccessible(true);f.set(o,value);
    }
    public static void main(String[] args)throws Exception{
        Pbuffer ctx=new Pbuffer(64,64,new PixelFormat(),null,null);
        try {
            ctx.makeCurrent();Bootstrap.register();
            Field uf=Unsafe.class.getDeclaredField("theUnsafe");uf.setAccessible(true);Unsafe u=(Unsafe)uf.get(null);
            Minecraft mc=(Minecraft)u.allocateInstance(Minecraft.class);set(null,Minecraft.class,"theMinecraft",mc);
            mc.gameSettings=(GameSettings)u.allocateInstance(GameSettings.class);
            OpenGlHelper.initializeTextures();
            Space w=(Space)u.allocateInstance(Space.class);Player p=(Player)u.allocateInstance(Player.class);
            mc.theWorld=w;mc.thePlayer=p;set(mc,Minecraft.class,"renderViewEntity",p);
            WorldProvider provider=new WorldProviderSurface();set(w,World.class,"provider",provider);
            WorldInfo info=new WorldInfo(new WorldSettings(189,WorldSettings.GameType.SURVIVAL,false,false,WorldType.DEFAULT),"sky");
            set(w,World.class,"worldInfo",info);Textures textures=new Textures();
            RenderGlobal sky=(RenderGlobal)u.allocateInstance(RenderGlobal.class);
            set(sky,RenderGlobal.class,"mc",mc);set(sky,RenderGlobal.class,"theWorld",w);set(sky,RenderGlobal.class,"renderEngine",textures);
            int list=GL11.glGenLists(1);GL11.glNewList(list,GL11.GL_COMPILE);GL11.glEndList();
            for(String key:new String[]{"glSkyList","glSkyList2","starGLCallList"})set(sky,RenderGlobal.class,key,list);
            Tessellator tess=Tessellator.getInstance();Record rec=new Record();rec.textures=textures;set(tess,Tessellator.class,"vboUploader",rec);
            Method stars=RenderGlobal.class.getDeclaredMethod("renderStars",WorldRenderer.class);stars.setAccessible(true);
            WorldRenderer b=new WorldRenderer(65536);stars.invoke(sky,b);b.finishDrawing();
            ByteBuffer packed=b.getByteBuffer().duplicate().order(ByteOrder.nativeOrder());
            long h=0xcbf29ce484222325L;for(int j=0;j<b.getVertexCount()*12;j++){h^=packed.get(j)&255;h*=0x100000001b3L;}
            List<Object> rows=new ArrayList<>();
            int[] hours={0,1000,6000,12000,13500,18000,23500};
            for(int i=0;i<168;i++){
                long time=24000L*(i/21)+hours[i%7];float part=(i%4)*.25f;int eye=(i/7)%3;
                info.setWorldTime(time);w.rain=(i%5)*.25f;rec.calls=new ArrayList<>();
                GL11.glMatrixMode(GL11.GL_MODELVIEW);GL11.glLoadIdentity();sky.renderSky(part,eye);
                float a=w.getCelestialAngle(part);float[] dawn=provider.calcSunriseSunsetColors(a,part);
                rows.add(Arrays.asList(time,part,eye,w.rain,w.getMoonPhase(),w.getStarBrightness(part),a,dawn==null?null:dawn.clone(),rec.calls));
            }
            // Preserve the actual uploaded colors and blend/filter behavior, including black
            // texels in the additive moon texture. Only chosen framebuffer pixels leave the JAR.
            int starsCount=b.getVertexCount();textures.load();textures.render=true;
            int starList=GL11.glGenLists(1);GL11.glNewList(starList,GL11.GL_COMPILE);
            stars.invoke(sky,tess.getWorldRenderer());tess.draw();GL11.glEndList();
            set(sky,RenderGlobal.class,"starGLCallList",starList);
            List<Object> pixels=new ArrayList<>();GL11.glViewport(0,0,64,64);
            for(int i=0;i<13;i++) {
                long t=i<8?24000L*i+18000:(i<10?6000:(i==10?0:12000));
                float partial=0f;w.rain=i==8||i==12?.625f:0;
                info.setWorldTime(t);float angle=w.getCelestialAngle(partial);
                GL11.glMatrixMode(GL11.GL_PROJECTION);GL11.glLoadIdentity();GLU.gluPerspective(60,1,.05f,300);
                GL11.glMatrixMode(GL11.GL_MODELVIEW);GL11.glLoadIdentity();
                if(i<10){GL11.glRotatef(i<8?90:-90,1,0,0);GL11.glRotatef(-angle*360,1,0,0);GL11.glRotatef(90,0,1,0);}
                else GL11.glRotatef(i==10?90:-90,0,1,0);
                FloatBuffer buf=BufferUtils.createFloatBuffer(16);
                float[] proj=new float[16],view=new float[16];GL11.glGetFloat(GL11.GL_PROJECTION_MATRIX,buf);buf.get(proj);
                ((Buffer)buf).clear();GL11.glGetFloat(GL11.GL_MODELVIEW_MATRIX,buf);buf.get(view);
                GlStateManager.clearColor(24/255f,48/255f,96/255f,1f);GL11.glClear(GL11.GL_COLOR_BUFFER_BIT);
                sky.renderSky(partial,2);GL11.glFinish();ByteBuffer image=BufferUtils.createByteBuffer(64*64*4);
                GL11.glReadPixels(0,0,64,64,GL11.GL_RGBA,GL11.GL_UNSIGNED_BYTE,image);
                List<Object> samples=new ArrayList<>();
                for(int x=8;x<60;x+=5)for(int y=9;y<59;y+=5){int o=((63-y)*64+x)*4;
                    samples.add(Arrays.asList(x,y,image.get(o)&255,image.get(o+1)&255,image.get(o+2)&255));}
                float[] rise=provider.calcSunriseSunsetColors(angle,partial);
                pixels.add(Arrays.asList(t,w.rain,proj,view,rise==null?null:rise.clone(),samples));
            }
            try(Writer writer=new OutputStreamWriter(new GZIPOutputStream(new FileOutputStream(args[0])),"UTF-8")){
                new Gson().toJson(Arrays.asList(starsCount,Long.toUnsignedString(h),rows,pixels),writer);
            }
            System.out.println(b.getVertexCount()+" original star vertices, "+rows.size()+" actual RenderGlobal sky frames");
        } finally {ctx.destroy();}
    }
}

