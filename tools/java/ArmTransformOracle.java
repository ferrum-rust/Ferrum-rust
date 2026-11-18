import com.google.gson.Gson;
import java.io.*;
import java.lang.reflect.*;
import java.nio.FloatBuffer;
import java.util.*;
import net.minecraft.client.Minecraft;
import net.minecraft.client.entity.*;
import net.minecraft.client.renderer.ItemRenderer;
import net.minecraft.client.renderer.entity.*;
import net.minecraft.client.renderer.texture.TextureManager;
import net.minecraft.entity.Entity;
import net.minecraft.init.Bootstrap;
import net.minecraft.util.ResourceLocation;
import org.lwjgl.BufferUtils;
import org.lwjgl.opengl.*;
import sun.misc.Unsafe;

/** Capture the modelview emitted by the actual private ItemRenderer method.
 * An offscreen OpenGL context is used only by this original-JAR oracle. */
public final class ArmTransformOracle {
    static class NoTextures extends TextureManager {
        NoTextures(){ super(null); }
        @Override public void bindTexture(ResourceLocation skin){}
    }
    static class Player extends EntityPlayerSP {
        Player(){super(null,null,null,null);}
        @Override public ResourceLocation getLocationSkin(){return null;}
    }
    static class Capture extends RenderPlayer {
        Capture(){super(null);}
        float[] matrix;
        @Override public void renderRightArm(AbstractClientPlayer p){
            // The original right arm has pivot (-5,2,0), a 0.1 radian Z
            // rotation at rest, and renders its display-list box at 1/16 scale.
            GL11.glTranslatef(-5f/16f,2f/16f,0);
            GL11.glRotatef(0.1f*57.295776f,0,0,1);
            GL11.glScalef(1f/16f,1f/16f,1f/16f);
            FloatBuffer buf=BufferUtils.createFloatBuffer(16);
            GL11.glGetFloat(GL11.GL_MODELVIEW_MATRIX,buf);
            matrix=new float[16];buf.get(matrix);
        }
    }
    static class Manager extends RenderManager {
        Capture capture;
        Manager(){super(null,null);}
        @Override @SuppressWarnings("unchecked") public <T extends Entity> Render<T> getEntityRenderObject(Entity entity){return (Render<T>)capture;}
    }
    static void set(Object o,String key,Object value)throws Exception{
        Field f=o.getClass().getDeclaredField(key);f.setAccessible(true);f.set(o,value);
    }
    public static void main(String[] args)throws Exception{
        Pbuffer context=new Pbuffer(32,32,new PixelFormat(),null,null);
        try {
            context.makeCurrent();Bootstrap.register();
            Field f=Unsafe.class.getDeclaredField("theUnsafe");f.setAccessible(true);Unsafe alloc=(Unsafe)f.get(null);
            Minecraft mc=(Minecraft)alloc.allocateInstance(Minecraft.class);
            mc.gameSettings=(net.minecraft.client.settings.GameSettings)alloc.allocateInstance(net.minecraft.client.settings.GameSettings.class);
            set(mc,"theMinecraft",mc);
            Manager manager=(Manager)alloc.allocateInstance(Manager.class);
            manager.capture=(Capture)alloc.allocateInstance(Capture.class);
            mc.thePlayer=(Player)alloc.allocateInstance(Player.class);
            set(mc,"renderEngine",new NoTextures());set(mc,"renderManager",manager);
            ItemRenderer item=new ItemRenderer(mc);
            Method draw=ItemRenderer.class.getDeclaredMethod("renderPlayerArm",AbstractClientPlayer.class,float.class,float.class);
            draw.setAccessible(true);
            List<Object> cases=new ArrayList<>();
            for(float equip:new float[]{0,.25f,.625f,1}) for(float swing:new float[]{0,.0625f,.125f,.375f,.5f,.875f,1}){
                GL11.glMatrixMode(GL11.GL_MODELVIEW);GL11.glLoadIdentity();
                draw.invoke(item,mc.thePlayer,equip,swing);
                cases.add(Arrays.asList(equip,swing,manager.capture.matrix));
            }
            try(Writer out=new OutputStreamWriter(new FileOutputStream(args[0]),"UTF-8")){new Gson().toJson(cases,out);}
            System.out.println("Actual ItemRenderer modelview: "+cases.size()+" hand poses.");
        } finally { context.destroy(); }
    }
}
