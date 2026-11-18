import java.io.*;
import java.lang.reflect.*;
import java.nio.*;
import java.nio.file.*;
import java.util.*;
import com.google.gson.Gson;
import net.minecraft.client.model.ModelBase;
import net.minecraft.client.renderer.GlStateManager;
import net.minecraft.client.renderer.entity.*;
import net.minecraft.entity.*;
import net.minecraft.entity.passive.EntityWolf;
import net.minecraft.world.*;
import org.lwjgl.BufferUtils;
import org.lwjgl.opengl.GL11;
import org.lwjgl.util.glu.GLU;

/** Original client entity dimensions and posed base-model triangles in world and texture coordinates. */
public final class RemoteMobOracle {
    static Method method(Class<?> c,String name,Class<?>... types)throws Exception{for(;c!=null;c=c.getSuperclass())try{Method m=c.getDeclaredMethod(name,types);m.setAccessible(true);return m;}catch(NoSuchMethodException ignored){}throw new NoSuchMethodException(name);}
    static final FloatBuffer capture=BufferUtils.createFloatBuffer(2000000);
    static final FloatBuffer model=BufferUtils.createFloatBuffer(16),projection=BufferUtils.createFloatBuffer(16),point=BufferUtils.createFloatBuffer(3);
    static final IntBuffer viewport=BufferUtils.createIntBuffer(16);
    static List<int[]> frame(EntityLivingBase e,RendererLivingEntity renderer,ModelBase shape,float phase,float speed)throws Exception{
        GlStateManager.matrixMode(GL11.GL_PROJECTION);GlStateManager.pushMatrix();GlStateManager.loadIdentity();GL11.glOrtho(-24,24,-24,24,-24,24);
        GlStateManager.matrixMode(GL11.GL_MODELVIEW);GlStateManager.pushMatrix();GlStateManager.loadIdentity();
        GlStateManager.rotate(21,1,0,0);GlStateManager.rotate(32,0,1,0);
        GL11.glGetFloat(GL11.GL_MODELVIEW_MATRIX,model);GL11.glGetFloat(GL11.GL_PROJECTION_MATRIX,projection);GL11.glGetInteger(GL11.GL_VIEWPORT,viewport);
        // Keep the state cache enabled while feedback captures every face. ModelDragon
        // enables culling internally and mirrors its second wing; a false cache would
        // let that call hide different polygons in each animation pose, corrupting
        // the vertex/UV correspondence required for native pose interpolation.
        GlStateManager.enableCull();GlStateManager.disableLighting();GL11.glDisable(GL11.GL_CULL_FACE);
        capture.clear();GL11.glFeedbackBuffer(GL11.GL_3D_COLOR_TEXTURE,capture);GL11.glRenderMode(GL11.GL_FEEDBACK);
        GlStateManager.rotate(180,0,1,0);GlStateManager.scale(-1,-1,1);
        method(RendererLivingEntity.class,"preRenderCallback",EntityLivingBase.class,float.class).invoke(renderer,e,0f);
        GlStateManager.translate(0,-1.5078125f,0);
        shape.isChild=false;shape.isRiding=false;shape.swingProgress=0;shape.setLivingAnimations(e,phase,speed,0);
        // RenderWolf supplies the health/anger-dependent tail angle in the slot
        // normally occupied by ageInTicks; walking phase would spin the tail.
        float rotation=e instanceof EntityWolf ? ((EntityWolf)e).getTailRotation() : phase;
        shape.setRotationAngles(phase,speed,rotation,0,0,0.0625f,e);
        shape.render(e,phase,speed,rotation,0,0,0.0625f);
        int n=GL11.glRenderMode(GL11.GL_RENDER);GlStateManager.disableCull();GlStateManager.popMatrix();GlStateManager.matrixMode(GL11.GL_PROJECTION);GlStateManager.popMatrix();GlStateManager.matrixMode(GL11.GL_MODELVIEW);
        if(n<=0)throw new IOException("Empty feedback "+n);
        List<int[]> triangles=new ArrayList<>();
        for(int i=0;i<n;){int token=(int)capture.get(i++);if(token!=GL11.GL_POLYGON_TOKEN)throw new IOException("Feedback token "+token);
            int count=(int)capture.get(i++);int[][] polygon=new int[count][5];
            for(int j=0;j<count;j++){GLU.gluUnProject(capture.get(i),capture.get(i+1),capture.get(i+2),model,projection,viewport,point);
                for(int k=0;k<3;k++)polygon[j][k]=Math.round(point.get(k)*2048);
                polygon[j][3]=Math.round(capture.get(i+7)*4096);polygon[j][4]=Math.round(capture.get(i+8)*4096);i+=11;}
            for(int j=2;j<count;j++){int[] t=new int[15];System.arraycopy(polygon[0],0,t,0,5);System.arraycopy(polygon[j-1],0,t,5,5);System.arraycopy(polygon[j],0,t,10,5);triangles.add(t);}
        }
        return triangles;
    }
    public static void main(String[] args){
        try{
            File game=new File(args[0]);VanillaFrameBenchmark.Client mc=MenuOracle.startClient(game,new File(args[1]),"1.8.9 creature model oracle");
            mc.gameSettings.renderDistanceChunks=2;
            mc.launchIntegratedServer("fixture","Fixture",new WorldSettings(189,WorldSettings.GameType.CREATIVE,false,false,WorldType.FLAT));
            java.lang.invoke.MethodHandle loop=VanillaFrameBenchmark.privateMethod(mc,"runGameLoop");long start=System.nanoTime();
            while(mc.thePlayer==null || mc.currentScreen!=null || mc.getRenderManager().livingPlayer!=mc.thePlayer){VanillaFrameBenchmark.runFrame(loop);if(System.nanoTime()-start>75_000_000_000L)throw new IOException("No world");}
            Map<Integer,Object> data=new TreeMap<>();
            for(int id=48;id<=120;id++){
                if(EntityList.getClassFromID(id)==null || Modifier.isAbstract(EntityList.getClassFromID(id).getModifiers()))continue;
                Entity entity=EntityList.createEntityByID(id,mc.theWorld);if(!(entity instanceof EntityLivingBase))continue;
                EntityLivingBase e=(EntityLivingBase)entity;Map<String,Object> spec=new TreeMap<>();data.put(id,spec);
                spec.put("name",EntityList.getEntityString(e));spec.put("size",new float[]{e.width,e.height,e.getCollisionBorderSize()});spec.put("collides",e.canBeCollidedWith());
                Render render=mc.getRenderManager().getEntityRenderObject(e);if(!(render instanceof RendererLivingEntity))continue;
                try{Object texture=method(render.getClass(),"getEntityTexture",Entity.class).invoke(render,e);
                    String path=texture.toString().replace("minecraft:","");spec.put("texture",path);
                    ModelBase shape=((RendererLivingEntity)render).getMainModel();
                    List<Object> frames=new ArrayList<>();for(int j=0;j<9;j++){
                        // ModelIronGolem uses a 13-tick triangle wave for all limbs.
                        float phase=id==99 ? Math.max(j-1,0)*13f/8f : (float)((j-1)*Math.PI/4/0.6662);
                        frames.add(frame(e,(RendererLivingEntity)render,shape,phase,j==0?0:1));}
                    spec.put("frames",frames);System.out.println(id+" "+path+" "+((List<?>)frames.get(0)).size());
                }catch(Throwable unsupported){System.out.println("Model "+id+": "+unsupported);}
            }
            try(Writer w=Files.newBufferedWriter(Paths.get(args[2]))){new Gson().toJson(data,w);}
            System.exit(0);
        }catch(Throwable e){e.printStackTrace();System.exit(1);}
    }
}
