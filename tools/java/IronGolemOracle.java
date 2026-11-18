import com.google.gson.Gson;
import java.io.*;
import java.util.*;
import java.util.zip.GZIPOutputStream;
import net.minecraft.client.model.*;
import net.minecraft.entity.*;
import net.minecraft.entity.item.EntityItem;
import net.minecraft.entity.monster.*;
import net.minecraft.entity.player.EntityPlayer;
import net.minecraft.init.Bootstrap;
import net.minecraft.item.Item;
import net.minecraft.util.*;

/** EntityIronGolem.attackEntityAsMob/dropFewItems/status and ModelIronGolem
 * animation are original calls; the target records accepted/rejected hits. */
public final class IronGolemOracle {
    static final class World extends FallingEntityOracle.TestWorld {
        World(){super(0);}
        public void setEntityState(Entity e,byte status){events.add(Arrays.asList("status",(int)status));}
    }
    static final class Target extends FallingEntityOracle.Target {
        boolean accepts;
        Target(World world,boolean accepts){super(world,1);this.accepts=accepts;}
        public boolean attackEntityFrom(DamageSource source,float amount){
            ((World)worldObj).events.add(Arrays.asList("hit",source.damageType,(int)amount));
            return accepts;
        }
    }
    static class Golem extends EntityIronGolem {
        Golem(World world){super(world);}
        long state()throws Exception{return ItemEntityOracle.seed(rand);}
        void reset(long seed){rand.setSeed(seed);}
        public void playSound(String name,float volume,float pitch){((World)worldObj).events.add(Arrays.asList("sound",name,bits(volume),bits(pitch)));}
        public EntityItem dropItemWithOffset(Item item,int count,float offset){
            ((World)worldObj).events.add(Arrays.asList("drop",Item.getIdFromItem(item),count,bits(offset)));return null;
        }
        void loot(boolean hit,int fortune){dropFewItems(hit,fortune);}
        int air(int supply){return decreaseAirSupply(supply);}
    }
    static long bits(float x){return Integer.toUnsignedLong(Float.floatToRawIntBits(x));}
    static Object combat(int i) throws Exception {
        long seed=i*817163L-189;
        World w=new World();Golem g=new Golem(w);g.reset(seed);g.setPlayerCreated((i&1)!=0);
        Target t=new Target(w,(i&2)!=0);t.motionY=i%7*.18;
        boolean success=g.attackEntityAsMob(t);
        List<Object> attack=Arrays.asList(success,FallingEntityOracle.bits(t.motionY),g.getAttackTimer(),Long.toString(g.state()),new ArrayList<Object>(w.events));
        w.events.clear();g.loot((i&4)!=0,i%5);
        List<Object> loot=Arrays.asList(Long.toString(g.state()),new ArrayList<Object>(w.events));
        w.events.clear();g.setHoldingRose((i&8)!=0);int serverRose=g.getHoldRoseTick();
        g.handleStatusUpdate((byte)(i%3==0?4:11));
        return Arrays.asList(Long.toString(seed),i,attack,loot,serverRose,g.getHoldRoseTick(),g.getAttackTimer(),new ArrayList<Object>(w.events),
            Arrays.asList(g.canAttackClass(EntityPlayer.class),g.canAttackClass(EntityCreeper.class),g.canAttackClass(EntityZombie.class),g.air(i%301),g.canAttackClass(EntityGhast.class)));
    }
    static Object model(int i) throws Exception {
        ModelIronGolem m=new ModelIronGolem();World w=new World();Golem g=new Golem(w);
        Random r=new Random(i*61897L-189);
        float swing=r.nextFloat()*130-35,amount=r.nextFloat()*1.8f,partial=r.nextFloat(),yaw=r.nextFloat()*720-360,pitch=r.nextFloat()*180-90;
        int attack=i%4==0?1+r.nextInt(10):0,rose=i%3==0?1+r.nextInt(400):0;
        FallingEntityOracle.field(EntityIronGolem.class,"attackTimer").setInt(g,attack);FallingEntityOracle.field(EntityIronGolem.class,"holdRoseTick").setInt(g,rose);
        m.setLivingAnimations(g,swing,amount,partial);m.setRotationAngles(swing,amount,0,yaw,pitch,.0625f,g);
        return Arrays.asList(Arrays.asList(bits(swing),bits(amount),bits(partial),bits(yaw),bits(pitch),attack,rose),
            Arrays.asList(bits(m.ironGolemHead.rotateAngleX),bits(m.ironGolemHead.rotateAngleY),
                bits(m.ironGolemRightArm.rotateAngleX),bits(m.ironGolemLeftArm.rotateAngleX),
                bits(m.ironGolemRightLeg.rotateAngleX),bits(m.ironGolemLeftLeg.rotateAngleX)));
    }
    static Object geometry()throws Exception{
        ModelIronGolem m=new ModelIronGolem();List<Object> parts=new ArrayList<>();
        for(ModelRenderer part:new ModelRenderer[]{m.ironGolemHead,m.ironGolemBody,m.ironGolemRightArm,m.ironGolemLeftArm,m.ironGolemRightLeg,m.ironGolemLeftLeg}){
            List<Object> quads=new ArrayList<>();
            for(Object object:part.cubeList){
                TexturedQuad[] boxes=(TexturedQuad[])FallingEntityOracle.field(ModelBox.class,"quadList").get(object);
                for(TexturedQuad q:boxes){
                    List<Object> vertices=new ArrayList<>();
                    for(PositionTextureVertex v:q.vertexPositions){vertices.add(Arrays.asList(v.vector3D.xCoord,v.vector3D.yCoord,v.vector3D.zCoord,v.texturePositionX,v.texturePositionY));}
                    quads.add(vertices);
                }
            }
            parts.add(Arrays.asList(new float[]{part.rotationPointX,part.rotationPointY,part.rotationPointZ},quads));
        }
        return parts;
    }
    public static void main(String[]args)throws Exception{
        Bootstrap.register();List<Object> combat=new ArrayList<>(),models=new ArrayList<>();
        for(int i=0;i<512;i++){combat.add(combat(i));models.add(model(i));}
        Map<String,Object> data=new LinkedHashMap<>();data.put("combat",combat);data.put("models",models);data.put("mesh",geometry());
        try(Writer writer=new OutputStreamWriter(new GZIPOutputStream(new FileOutputStream(args[0])),"UTF-8")){new Gson().toJson(data,writer);}
        System.out.println("Original iron golem 512 attacks/drop/status +512 poses, six model parts");
    }
}
