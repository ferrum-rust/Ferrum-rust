import com.google.gson.*;
import java.awt.image.BufferedImage;
import java.io.*;
import java.lang.reflect.Field;
import java.nio.ByteBuffer;
import java.util.*;
import javax.imageio.ImageIO;
import net.minecraft.client.Minecraft;
import net.minecraft.client.renderer.texture.*;
import net.minecraft.client.resources.data.*;
import net.minecraft.client.settings.GameSettings;
import net.minecraft.block.Block;
import net.minecraft.init.Bootstrap;
import net.minecraft.util.EnumWorldBlockLayer;
import org.lwjgl.BufferUtils;
import org.lwjgl.opengl.*;
import sun.misc.Unsafe;

/** Capture what the original atlas actually uploaded, including interpolated
 * pixels and independently generated mips. No texture bytes leave the JAR. */
public final class BlockAnimationOracle {
    static class Sprite extends TextureAtlasSprite {
        Sprite(String name){super(name);}
    }
    static String spriteHash(int width,int level) {
        int size=width>>level;
        ByteBuffer buf=BufferUtils.createByteBuffer(size*size*4);
        GL11.glGetTexImage(GL11.GL_TEXTURE_2D,level,GL11.GL_RGBA,GL11.GL_UNSIGNED_BYTE,buf);
        long h=0xcbf29ce484222325L;
        for(int i=0;i<buf.capacity();i++){h^=buf.get(i)&255;h*=0x100000001b3L;}
        return Long.toUnsignedString(h);
    }
    public static void main(String[] args)throws Exception{
        Pbuffer ctx=new Pbuffer(32,32,new PixelFormat(),null,null);
        try {
            ctx.makeCurrent();
            Field u=Unsafe.class.getDeclaredField("theUnsafe");u.setAccessible(true);Unsafe alloc=(Unsafe)u.get(null);
            Minecraft mc=(Minecraft)alloc.allocateInstance(Minecraft.class);
            mc.gameSettings=(GameSettings)alloc.allocateInstance(GameSettings.class);
            Field singleton=Minecraft.class.getDeclaredField("theMinecraft");singleton.setAccessible(true);singleton.set(null,mc);
            Bootstrap.register();
            List<Integer> translucent=new ArrayList<>();
            for(Block b:Block.blockRegistry)if(b.getBlockLayer()==EnumWorldBlockLayer.TRANSLUCENT)translucent.add(Block.getIdFromBlock(b));
            Collections.sort(translucent);
            try(Writer out=new OutputStreamWriter(new FileOutputStream(args[1]),"UTF-8")){new Gson().toJson(translucent,out);}
            Map<String,Object> results=new TreeMap<>();int count=0;
            for(String name:new String[]{"fire_layer_0","fire_layer_1","lava_flow","lava_still","portal","prismarine_rough","sea_lantern","water_flow","water_still"}){
                String path="/assets/minecraft/textures/blocks/"+name+".png";
                BufferedImage[] imgs=new BufferedImage[5]; imgs[0]=ImageIO.read(BlockAnimationOracle.class.getResourceAsStream(path));
                AnimationMetadataSection meta;
                try(Reader in=new InputStreamReader(BlockAnimationOracle.class.getResourceAsStream(path+".mcmeta"),"UTF-8")){
                    meta=new AnimationMetadataSectionSerializer().deserialize(new JsonParser().parse(in).getAsJsonObject().get("animation"),null,null);
                }
                int w=imgs[0].getWidth();
                Sprite sprite=new Sprite(name);sprite.loadSprite(imgs,meta);sprite.generateMipmaps(4);sprite.initSprite(w,w,0,0,false);
                int id=GL11.glGenTextures();TextureUtil.allocateTextureImpl(id,4,w,w);
                TextureUtil.uploadTextureMipmap(sprite.getFrameTextureData(0),w,w,0,0,false,false);
                List<Object> samples=new ArrayList<>();
                int last=name.equals("prismarine_rough")?6600:160;
                for(int tick=0;tick<=last;tick++) {
                    if(tick>0)sprite.updateAnimation();
                    if(tick>160 && tick%47!=0 && tick%300!=0 && tick%300!=299)continue;
                    List<String> levels=new ArrayList<>();
                    for(int level=0;level<5;level++) levels.add(spriteHash(w,level));
                    samples.add(Arrays.asList(tick,levels));count++;
                }
                TextureUtil.deleteTexture(id);results.put(name,samples);
            }
            try(Writer out=new OutputStreamWriter(new FileOutputStream(args[0]),"UTF-8")){new Gson().toJson(results,out);}
            System.out.println(results.size()+" real sprite sheets, "+count+" uploaded snapshots.");
        } finally {ctx.destroy();}
    }
}
