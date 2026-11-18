import java.io.*;
import java.lang.reflect.*;
import java.nio.ByteBuffer;
import java.util.*;
import java.util.zip.GZIPOutputStream;
import net.minecraft.client.Minecraft;
import net.minecraft.client.renderer.*;
import net.minecraft.client.renderer.texture.TextureManager;
import net.minecraft.client.resources.*;
import net.minecraft.client.settings.GameSettings;
import net.minecraft.client.shader.*;
import net.minecraft.init.Bootstrap;
import net.minecraft.util.ResourceLocation;
import org.lwjgl.BufferUtils;
import org.lwjgl.opengl.*;
import sun.misc.Unsafe;

/** Pixel reference from real ShaderGroup passes on a hidden original OpenGL framebuffer. */
public final class PostEffectsOracle {
    static final int W=48, H=32;
    static void set(Class<?> owner, Object instance, String name, Object value)throws Exception {
        Field field=owner.getDeclaredField(name);
        field.setAccessible(true);
        field.set(instance,value);
    }
    static final class Resources extends SimpleReloadableResourceManager {
        Resources(){super(null);}
        public IResource getResource(ResourceLocation location)throws IOException {
            String id="/assets/"+location.getResourceDomain()+"/"+location.getResourcePath();
            InputStream src=PostEffectsOracle.class.getResourceAsStream(id);
            if(src==null)throw new FileNotFoundException(id);
            return new SimpleResource("local jar",location,src,null,null);
        }
    }
    static void fill(Framebuffer fb, int frame) {
        ByteBuffer b=BufferUtils.createByteBuffer(W*H*4);
        for(int glY=0;glY<H;glY++)for(int x=0;x<W;x++) {
            int y=H-1-glY;
            b.put((byte)(x*4+frame*17)).put((byte)(y*6+frame*13))
             .put((byte)(((x/5+y/4)%2)*95+30+frame*11)).put((byte)255);
        }
        b.flip();
        fb.bindFramebufferTexture();
        GL11.glTexSubImage2D(GL11.GL_TEXTURE_2D,0,0,0,W,H,GL11.GL_RGBA,GL11.GL_UNSIGNED_BYTE,b);
        fb.unbindFramebufferTexture();
    }
    public static void main(String[] args)throws Exception {
        Pbuffer surface=new Pbuffer(W,H,new PixelFormat(),null,null);
        try(DataOutputStream out=new DataOutputStream(new GZIPOutputStream(new FileOutputStream(args[0])))) {
            surface.makeCurrent(); Bootstrap.register();
            Field f=Unsafe.class.getDeclaredField("theUnsafe");f.setAccessible(true);
            Minecraft mc=(Minecraft)((Unsafe)f.get(null)).allocateInstance(Minecraft.class);
            set(Minecraft.class,null,"theMinecraft",mc);
            mc.gameSettings=new GameSettings(); mc.gameSettings.fboEnable=true;
            mc.displayWidth=W; mc.displayHeight=H;
            OpenGlHelper.initializeTextures();
            ShaderLinkHelper.setNewStaticShaderLinkHelper();
            Resources resources=new Resources();
            TextureManager textures=new TextureManager(resources);
            set(Minecraft.class,mc,"mcResourceManager",resources);
            set(Minecraft.class,mc,"renderEngine",textures);
            Field names=EntityRenderer.class.getDeclaredField("shaderResourceLocations"); names.setAccessible(true);
            Framebuffer fb=new Framebuffer(W,H,true);
            fb.setFramebufferColor(0f,0f,0f,0f);
            set(Minecraft.class,mc,"framebufferMc",fb);
            GlStateManager.disableCull();
            GL11.glDisable(GL11.GL_DITHER);
            out.writeByte(W);out.writeByte(H);
            out.writeUTF(GL11.glGetString(GL11.GL_RENDERER));
            for(ResourceLocation location:(ResourceLocation[])names.get(null)){
                String path=location.getResourcePath();
                String name=path.substring(path.lastIndexOf('/')+1,path.lastIndexOf('.'));
                out.writeUTF(name);
                ShaderGroup group=new ShaderGroup(textures,resources,fb,location);
                group.createBindFramebuffers(W,H);
                for(int frame=0;frame<2;frame++){
                    fill(fb,frame);
                    group.loadShaderGroup(frame==0?0.25f:0.75f);
                    fb.bindFramebuffer(true);
                    ByteBuffer b=BufferUtils.createByteBuffer(W*H*4);
                    GL11.glReadPixels(0,0,W,H,GL11.GL_RGBA,GL11.GL_UNSIGNED_BYTE,b);
                    for(int y=0;y<H;y++)for(int x=0;x<W;x++)for(int c=0;c<3;c++)
                        out.writeByte(b.get(((H-y-1)*W+x)*4+c));
                    fb.unbindFramebuffer();
                }
                group.deleteShaderGroup();
            }
            fb.deleteFramebuffer();
        } finally {surface.destroy();}
    }
}
