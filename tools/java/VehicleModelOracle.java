import com.google.gson.Gson;
import java.io.*;
import java.lang.reflect.*;
import java.nio.FloatBuffer;
import java.nio.file.*;
import java.util.*;
import net.minecraft.client.model.*;
import net.minecraft.client.renderer.GlStateManager;
import net.minecraft.client.renderer.entity.*;
import net.minecraft.entity.Entity;
import net.minecraft.entity.item.*;
import net.minecraft.util.BlockPos;
import net.minecraft.init.Blocks;
import net.minecraft.world.*;
import net.minecraft.world.chunk.Chunk;
import org.lwjgl.BufferUtils;
import org.lwjgl.opengl.GL11;

/** Capture the actual 1.8.9 model matrices at the point RenderBoat/RenderMinecart submit their mesh. */
public final class VehicleModelOracle {
    static float[] matrix(){
        FloatBuffer b=BufferUtils.createFloatBuffer(16);GL11.glGetFloat(GL11.GL_MODELVIEW_MATRIX,b);
        float[] m=new float[16];b.get(m);return m;
    }
    static class Probe extends ModelBase {
        float[] last;
        public void render(Entity e,float a,float b,float c,float d,float f,float g){last=matrix();}
    }
    static List<Object> panels(ModelRenderer[] parts)throws Exception{
        Field quads=ModelBox.class.getDeclaredField("quadList");quads.setAccessible(true);
        List<Object> faces=new ArrayList<Object>();
        for(ModelRenderer part:parts){
            if(part==null)continue;
            GlStateManager.pushMatrix();GlStateManager.loadIdentity();part.postRender(0.0625f);
            float[] m=matrix();GlStateManager.popMatrix();
            for(Object box:part.cubeList)for(TexturedQuad q:(TexturedQuad[])quads.get(box)){
                List<Object> face=new ArrayList<Object>();
                for(PositionTextureVertex v:q.vertexPositions){double x=v.vector3D.xCoord/16,y=v.vector3D.yCoord/16,z=v.vector3D.zCoord/16;
                    int[] p=new int[5];for(int i=0;i<3;i++)p[i]=(int)Math.round((m[i]*x+m[i+4]*y+m[i+8]*z+m[i+12])*40960);
                    p[3]=Math.round(v.texturePositionX*part.textureWidth);p[4]=Math.round(v.texturePositionY*part.textureHeight);face.add(p);
                }faces.add(face);
            }
        }return faces;
    }
    public static void main(String[] args){
        try{
            VanillaFrameBenchmark.Client mc=MenuOracle.startClient(new File(args[0]),new File(args[1]),"1.8.9 vehicle model oracle");
            mc.gameSettings.renderDistanceChunks=2;mc.launchIntegratedServer("fixture","Fixture",new WorldSettings(189,WorldSettings.GameType.CREATIVE,false,false,WorldType.FLAT));
            java.lang.invoke.MethodHandle loop=VanillaFrameBenchmark.privateMethod(mc,"runGameLoop");long start=System.nanoTime();
            while(mc.thePlayer==null||mc.currentScreen!=null){VanillaFrameBenchmark.runFrame(loop);if(System.nanoTime()-start>75_000_000_000L)throw new IOException("No world");}
            GlStateManager.matrixMode(GL11.GL_MODELVIEW);GlStateManager.pushMatrix();GlStateManager.loadIdentity();
            EntityBoat boat=new EntityBoat(mc.theWorld);EntityMinecartEmpty cart=new EntityMinecartEmpty(mc.theWorld);
            Map<String,Object> out=new LinkedHashMap<String,Object>();RenderBoat rb=new RenderBoat(mc.getRenderManager());
            RenderMinecart<EntityMinecartEmpty> rc=new RenderMinecart<EntityMinecartEmpty>(mc.getRenderManager());
            Field bf=RenderBoat.class.getDeclaredField("modelBoat");bf.setAccessible(true);Field cf=RenderMinecart.class.getDeclaredField("modelMinecart");cf.setAccessible(true);
            ModelBoat b=(ModelBoat)bf.get(rb); ModelMinecart c=(ModelMinecart)cf.get(rc);
            c.render(cart,0,0,-.1f,0,0,.0625f);
            out.put("boat",panels(b.boatSides));out.put("cart",panels(c.sideModels));
            Probe probe=new Probe();bf.set(rb,probe);cf.set(rc,probe);
            mc.theWorld.doPreChunk(0,0,true);Chunk chunk=mc.theWorld.getChunkFromChunkCoords(0,0);
            BlockPos rail=new BlockPos(8,150,8);List<Object> tests=new ArrayList<Object>();out.put("frames",tests);
            for(int k=-2;k<10;k++)for(int j=0;j<3;j++){
                boolean isBoat=k==-2;Entity e=isBoat?boat:cart;
                chunk.setBlockState(rail,k<0?Blocks.air.getDefaultState():Blocks.rail.getStateFromMeta(k));
                int id=j==0?0:j==1?Integer.MAX_VALUE:-189;
                float a=new float[]{0,.375f,1}[j];double[] from={8.15,150.125,8.8},to={8.94,150.327,8.11};
                e.setEntityId(id);e.lastTickPosX=from[0];e.lastTickPosY=from[1];e.lastTickPosZ=from[2];e.setPosition(to[0],to[1],to[2]);
                float yaw=-53.25f,prevPitch=3.75f,pitch=12.5f;e.prevRotationPitch=prevPitch;e.rotationPitch=pitch;
                int rolling=j==0?0:j==1?8:5;float damage=j==0?0:j==1?17.5f:.375f;int dir=j==1?-1:1;
                double x=from[0]+(to[0]-from[0])*a,y=from[1]+(to[1]-from[1])*a,z=from[2]+(to[2]-from[2])*a;
                if(isBoat){boat.setTimeSinceHit(rolling);boat.setDamageTaken(damage);boat.setForwardDirection(dir);rb.doRender(boat,x,y,z,yaw,a);}
                else{cart.setRollingAmplitude(rolling);cart.setDamage(damage);cart.setRollingDirection(dir);rc.doRender(cart,x,y,z,yaw,a);}
                if(probe.last==null)throw new AssertionError("missing renderer call");
                tests.add(Arrays.asList(k,id,a,from,to,yaw,prevPitch,pitch,rolling,damage,dir,probe.last));probe.last=null;
            }
            GlStateManager.popMatrix();try(Writer w=Files.newBufferedWriter(Paths.get(args[2]))){new Gson().toJson(out,w);}
            System.exit(0);
        }catch(Throwable ex){ex.printStackTrace();System.exit(1);}
    }
}
