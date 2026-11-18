import com.google.gson.Gson;
import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import java.nio.*;
import java.nio.file.*;
import net.minecraft.client.entity.EntityPlayerSP;
import net.minecraft.client.model.*;
import net.minecraft.client.renderer.GlStateManager;
import net.minecraft.client.renderer.entity.*;
import net.minecraft.client.settings.KeyBinding;
import net.minecraft.entity.Entity;
import net.minecraft.init.*;
import net.minecraft.item.ItemStack;
import net.minecraft.util.BlockPos;
import net.minecraft.util.ResourceLocation;
import net.minecraft.util.ScreenShotHelper;
import net.minecraft.world.*;
import org.lwjgl.BufferUtils;
import org.lwjgl.opengl.GL11;
import org.lwjgl.util.glu.GLU;

/** Sample real EntityPlayerSP ticks, RenderPlayer's root matrix, and ModelPlayer's six animated body parts. */
public final class ThirdPersonOracle {
    static float[] matrix(){FloatBuffer b=BufferUtils.createFloatBuffer(16);GL11.glGetFloat(GL11.GL_MODELVIEW_MATRIX,b);float[] a=new float[16];b.get(a);return a;}
    static ModelRenderer[] base(ModelBiped m){return new ModelRenderer[]{m.bipedHead,m.bipedBody,m.bipedRightArm,m.bipedLeftArm,m.bipedRightLeg,m.bipedLeftLeg};}
    static ModelRenderer[] wear(ModelPlayer m){return new ModelRenderer[]{m.bipedHeadwear,m.bipedBodyWear,m.bipedRightArmwear,m.bipedLeftArmwear,m.bipedRightLegwear,m.bipedLeftLegwear};}
    static Object skin(ModelRenderer[] parts)throws Exception{
        Field f=ModelBox.class.getDeclaredField("quadList");f.setAccessible(true);List<Object> all=new ArrayList<>();
        for(ModelRenderer part:parts){List<Object> quads=new ArrayList<>();
            for(Object box:part.cubeList)for(TexturedQuad q:(TexturedQuad[])f.get(box)){
                List<Object> quad=new ArrayList<>();for(PositionTextureVertex v:q.vertexPositions){
                    quad.add(new float[]{(float)v.vector3D.xCoord,(float)v.vector3D.yCoord,(float)v.vector3D.zCoord,
                        v.texturePositionX,v.texturePositionY});}quads.add(quad);
            }all.add(quads);
        }return all;
    }
    static class Model extends ModelPlayer {
        Object frame;Model(boolean slim){super(0,slim);}
        @Override public void render(Entity e,float phase,float amplitude,float age,float head,float pitch,float scale){
            float[] root=matrix();super.render(e,phase,amplitude,age,head,pitch,scale);List<Object> bones=new ArrayList<>();
            for(ModelRenderer p:base(this)){bones.add(new float[]{p.rotationPointX,p.rotationPointY,p.rotationPointZ,p.rotateAngleX,p.rotateAngleY,p.rotateAngleZ});}
            frame=Arrays.asList(new float[]{phase,amplitude,age,head,pitch,swingProgress},root,bones);
        }
    }
    static class Renderer extends RenderPlayer{
        final Model capture; final boolean slim;
        Renderer(RenderManager rm,boolean slim){super(rm,slim);this.slim=slim;capture=new Model(slim);mainModel=capture;layerRenderers.clear();}
        @Override protected ResourceLocation getEntityTexture(net.minecraft.client.entity.AbstractClientPlayer p){
            return new ResourceLocation(slim?"textures/entity/alex.png":"textures/entity/steve.png");
        }
    }
    static void screenshot(VanillaFrameBenchmark.Client mc,File game,Renderer r,EntityPlayerSP p,int tick)throws Exception {
        mc.getFramebuffer().bindFramebuffer(true);GlStateManager.viewport(0,0,1600,900);
        GlStateManager.clearColor(1,0,1,1);GlStateManager.depthMask(true);GlStateManager.clearDepth(1);
        GlStateManager.clear(GL11.GL_COLOR_BUFFER_BIT|GL11.GL_DEPTH_BUFFER_BIT);
        GlStateManager.matrixMode(GL11.GL_PROJECTION);GlStateManager.pushMatrix();GlStateManager.loadIdentity();
        GLU.gluPerspective(70,1600f/900f,.05f,128f);
        GlStateManager.matrixMode(GL11.GL_MODELVIEW);GlStateManager.pushMatrix();GlStateManager.loadIdentity();
        // The comparison isolates the player against an empty scene, without ground clipping the rear camera.
        // Ground, wall, partial block and fluid camera matrices are verified independently below.
        double y=p.posY, prevY=p.prevPosY;p.posY+=80;p.prevPosY+=80;
        Method camera=mc.entityRenderer.getClass().getDeclaredMethod("orientCamera",float.class);camera.setAccessible(true);camera.invoke(mc.entityRenderer,1f);
        p.posY=y;p.prevPosY=prevY;
        GlStateManager.disableFog();GlStateManager.enableDepth();GlStateManager.depthFunc(GL11.GL_LEQUAL);
        GlStateManager.enableAlpha();GlStateManager.enableCull();GlStateManager.cullFace(GL11.GL_BACK);GlStateManager.enableTexture2D();
        GlStateManager.color(1,1,1,1);mc.entityRenderer.disableLightmap();
        net.minecraft.client.renderer.RenderHelper.enableStandardItemLighting();
        r.doRender(p,0,0,0,0,.375f);
        net.minecraft.client.renderer.RenderHelper.disableStandardItemLighting();
        GlStateManager.popMatrix();GlStateManager.matrixMode(GL11.GL_PROJECTION);GlStateManager.popMatrix();GlStateManager.matrixMode(GL11.GL_MODELVIEW);
        ScreenShotHelper.saveScreenshot(game,"java-"+tick+".png",mc.displayWidth,mc.displayHeight,mc.getFramebuffer());
    }
    static final FloatBuffer feedback=BufferUtils.createFloatBuffer(100000);
    static Object rendered(Renderer r,EntityPlayerSP p,float alpha)throws Exception{
        FloatBuffer model=BufferUtils.createFloatBuffer(16),projection=BufferUtils.createFloatBuffer(16),point=BufferUtils.createFloatBuffer(3);
        IntBuffer view=BufferUtils.createIntBuffer(16);
        GlStateManager.matrixMode(GL11.GL_PROJECTION);GlStateManager.pushMatrix();GlStateManager.loadIdentity();GL11.glOrtho(-3,3,-3,3,-3,3);
        GlStateManager.matrixMode(GL11.GL_MODELVIEW);GlStateManager.pushMatrix();GlStateManager.loadIdentity();
        GlStateManager.rotate(18,1,0,0);GlStateManager.rotate(27,0,1,0);
        GL11.glGetFloat(GL11.GL_MODELVIEW_MATRIX,model);GL11.glGetFloat(GL11.GL_PROJECTION_MATRIX,projection);GL11.glGetInteger(GL11.GL_VIEWPORT,view);
        feedback.clear();GL11.glFeedbackBuffer(GL11.GL_3D_COLOR_TEXTURE,feedback);GL11.glRenderMode(GL11.GL_FEEDBACK);
        r.doRender(p,0,0,0,0,alpha);int count=GL11.glRenderMode(GL11.GL_RENDER);
        GlStateManager.popMatrix();GlStateManager.matrixMode(GL11.GL_PROJECTION);GlStateManager.popMatrix();GlStateManager.matrixMode(GL11.GL_MODELVIEW);
        if(count<=0)throw new AssertionError("No GL geometry");
        List<int[]> vertices=new ArrayList<>();Set<String> seen=new HashSet<>();
        for(int i=0;i<count;){int token=(int)feedback.get(i++);if(token!=GL11.GL_POLYGON_TOKEN)throw new AssertionError("feedback "+token);
            int n=(int)feedback.get(i++);for(int j=0;j<n;j++){
                GLU.gluUnProject(feedback.get(i),feedback.get(i+1),feedback.get(i+2),model,projection,view,point);
                int[] v={Math.round(point.get(0)*8192),Math.round(point.get(1)*8192),Math.round(point.get(2)*8192),
                    Math.round(feedback.get(i+7)*64),Math.round(feedback.get(i+8)*64)};
                if(seen.add(Arrays.toString(v)))vertices.add(v);i+=11;
            }
        }return vertices;
    }
    static void key(KeyBinding key,boolean down){KeyBinding.setKeyBindState(key.getKeyCode(),down);}
    public static void main(String[] args){
        try{
            VanillaFrameBenchmark.Client mc=MenuOracle.startClient(new File(args[0]),new File(args[1]),"1.8.9 F5 pose oracle");
            mc.gameSettings.renderDistanceChunks=2;
            mc.launchIntegratedServer("fixture","Fixture",new WorldSettings(189,WorldSettings.GameType.CREATIVE,false,false,WorldType.FLAT));
            java.lang.invoke.MethodHandle loop=VanillaFrameBenchmark.privateMethod(mc,"runGameLoop");long start=System.nanoTime();
            while(mc.thePlayer==null||mc.currentScreen!=null ||mc.getRenderManager().livingPlayer!=mc.thePlayer){
                VanillaFrameBenchmark.runFrame(loop);if(System.nanoTime()-start>75_000_000_000L)throw new IOException("No player");}
            Map<String,Object> out=new LinkedHashMap<>();List<Object> geometry=new ArrayList<>();
            for(boolean slim:new boolean[]{false,true}){ModelPlayer m=new ModelPlayer(0,slim);geometry.add(skin(base(m)));geometry.add(skin(wear(m)));}
            geometry.add(skin(base(new ModelBiped(1))));geometry.add(skin(base(new ModelBiped(.5f))));out.put("geometry",geometry);
            mc.theWorld.doPreChunk(0,0,true);for(int x=0;x<16;x++)for(int z=0;z<16;z++)mc.theWorld.getChunkFromChunkCoords(0,0).setBlockState(new BlockPos(x,79,z),Blocks.stone.getDefaultState());
            EntityPlayerSP p=mc.thePlayer;p.setPositionAndRotation(8.5,80,8.5,0,0);p.renderYawOffset=p.prevRenderYawOffset=p.rotationYawHead=p.prevRotationYawHead=0;
            p.limbSwing=p.limbSwingAmount=p.prevLimbSwingAmount=0;p.motionX=p.motionY=p.motionZ=0;p.ticksExisted=0;
            List<Object> frames=new ArrayList<>(),mesh=new ArrayList<>();out.put("mesh",mesh);Renderer[] render={new Renderer(mc.getRenderManager(),false),new Renderer(mc.getRenderManager(),true)};
            for(int t=0;t<44;t++){
                float yaw=t<3?0:t<8?37:t<14?132:t<19?179:t<26?-165:t<33?-72:t<39?0:77;
                float pitch=t<7?0:t<11?37:t<22?-28:t<27?76:t<38?5:0;
                p.rotationYaw=yaw;p.rotationPitch=pitch;
                key(mc.gameSettings.keyBindForward,t>=7&&t<25);key(mc.gameSettings.keyBindLeft,t>=15&&t<30);
                key(mc.gameSettings.keyBindSneak,t>=26&&t<32);key(mc.gameSettings.keyBindBack,t>=32&&t<36);
                key(mc.gameSettings.keyBindJump,t==20);if(t==10||t==34)p.swingItem();
                boolean use=t>=27&&t<32||t>=39;
                p.inventory.currentItem=0;p.inventory.mainInventory[0]=t<13?null:t<26?new ItemStack(Blocks.dirt):t<33?new ItemStack(Items.iron_sword):new ItemStack(Items.bow);
                if(use)p.setItemInUse(p.getHeldItem(),16);else p.clearItemInUse();
                double x=p.posX,z=p.posZ;++p.ticksExisted;p.onUpdate(); // World.updateEntityWithOptionalForce increments before onUpdate.
                p.getDataWatcher().updateObject(10,Byte.valueOf((byte)127));
                boolean slim=(t&1)!=0;mc.gameSettings.thirdPersonView=t%2+1;
                List<Object> samples=new ArrayList<>();
                for(float a:new float[]{0,.375f,1}){
                    GlStateManager.matrixMode(GL11.GL_MODELVIEW);GlStateManager.pushMatrix();GlStateManager.loadIdentity();
                    Renderer r=render[slim?1:0];r.capture.frame=null;r.doRender(p,0,0,0,0,a);GlStateManager.popMatrix();
                    if(r.capture.frame==null)throw new AssertionError("F5 body not rendered");samples.add(r.capture.frame);
                }
                if(t==3||t==8||t==12||t==27||t==40){mesh.add(Arrays.asList(t+1,rendered(render[slim?1:0],p,.375f)));}
                if(t==3||t==12)screenshot(mc,new File(args[0]),render[slim?1:0],p,t+1);
                frames.add(Arrays.asList(t+1,slim,new double[]{p.posX-x,p.posZ-z},new float[]{p.rotationYaw,p.rotationPitch,p.swingProgress},
                    new float[]{p.renderYawOffset,p.rotationYawHead,p.limbSwing,p.limbSwingAmount},p.isSneaking(),render[slim?1:0].capture.heldItemRight,
                    render[slim?1:0].capture.aimedBow,samples));
            }
            List<Object> cameras=new ArrayList<>();Method camera=mc.entityRenderer.getClass().getDeclaredMethod("orientCamera",float.class);camera.setAccessible(true);
            key(mc.gameSettings.keyBindSneak,false);p.movementInput.sneak=false;p.setSneaking(false);p.setPositionAndRotation(8.46,80,8.62,0,0);
            p.lastTickPosX=p.prevPosX=p.posX;p.lastTickPosY=p.prevPosY=p.posY;p.lastTickPosZ=p.prevPosZ=p.posZ;
            for(int layout=0;layout<3;layout++){
                List<int[]> blocks=new ArrayList<>();
                if(layout==1)for(int x=6;x<=10;x++)for(int y=80;y<=83;y++)for(int z:new int[]{6,10})blocks.add(new int[]{x,y,z,16});
                if(layout==2){blocks.add(new int[]{7,81,7,44*16});blocks.add(new int[]{9,81,9,85*16});blocks.add(new int[]{8,81,10,9*16});}
                for(int[] b:blocks)mc.theWorld.getChunkFromChunkCoords(0,0).setBlockState(new BlockPos(b[0],b[1],b[2]),net.minecraft.block.Block.getBlockById(b[3]>>4).getStateFromMeta(b[3]&15));
                for(int perspective:new int[]{0,1,2})for(float[] angles:new float[][]{
                    {0,0},{25,20},{128,-21},{-149,37},{138.72368f,90},{138.72368f,89.99f},
                    {-42,-90},{-42,-89.99f},{0,90},{90,90},{180,-90}}){
                    mc.gameSettings.thirdPersonView=perspective;p.rotationYaw=p.prevRotationYaw=angles[0];p.rotationPitch=p.prevRotationPitch=angles[1];
                    GlStateManager.pushMatrix();GlStateManager.loadIdentity();camera.invoke(mc.entityRenderer,1f);float[] m=matrix();GlStateManager.popMatrix();
                    cameras.add(Arrays.asList(blocks,perspective,angles,m));
                }
                for(int[] b:blocks)mc.theWorld.getChunkFromChunkCoords(0,0).setBlockState(new BlockPos(b[0],b[1],b[2]),Blocks.air.getDefaultState());
            }
            out.put("cameras",cameras);
            out.put("frames",frames);try(Writer w=Files.newBufferedWriter(Paths.get(args[2]))){new Gson().toJson(out,w);}
            System.exit(0);
        }catch(Throwable t){t.printStackTrace();System.exit(1);}
    }
}
