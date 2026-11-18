import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import java.util.zip.GZIPOutputStream;
import com.google.gson.Gson;
import net.minecraft.block.state.IBlockState;
import net.minecraft.entity.*;
import net.minecraft.entity.item.EntityItem;
import net.minecraft.init.Bootstrap;
import net.minecraft.item.*;
import net.minecraft.util.*;

/** Original EntityItem.onUpdate including both water queries, fire and moves. */
public final class ItemMotionOracle {
    static final class World extends FallingEntityOracle.TestWorld {
        final int layout;int time;
        World(int n){super(0);layout=n;}
        public IBlockState getBlockState(BlockPos p){return FallingEntityOracle.state(initial(p.getX(),p.getY(),p.getZ(),layout));}
        public <T extends Entity> List<T> getEntitiesWithinAABB(Class<? extends T> cls,AxisAlignedBB b){events.add(Arrays.asList("merge",FallingEntityOracle.bits(b.minX,b.minY,b.minZ,b.maxX,b.maxY,b.maxZ)));return Collections.emptyList();}
        public boolean isBlockFullCube(BlockPos p){boolean value=super.isBlockFullCube(p);events.add(Arrays.asList("full",p.getX(),p.getY(),p.getZ(),value));return value;}
        public boolean isRainingAt(BlockPos p){return layout==10&&time>4;}
        public void spawnParticle(EnumParticleTypes particle,double x,double y,double z,double vx,double vy,double vz,int... args){events.add(Arrays.asList("particle",particle.name(),FallingEntityOracle.bits(x,y,z,vx,vy,vz)));}
    }
    static int initial(int x,int y,int z,int s){
        if(y<0||y>=256||s==13)return 0;
        if(y==0)return (s==1?79:s==2?174:s==3?165:s==4?88:s==5?44:1)<<4;
        if(s==6&&y>=2&&y<=4)return 30<<4;
        if(s==7&&y<=3)return 9<<4;
        if(s==8&&y<=3)return 8<<4|Math.floorMod(x,8);
        if(s==9&&y<=3)return 10<<4;
        if((s==10||s==14)&&y<=3)return 51<<4;
        if(s==11&&x==8&&z==8&&y>=2&&y<=4)return 1<<4;
        if(s==12&&x==0&&y<=6)return 1<<4;
        if(s==15&&x==8&&z==8&&y<=3)return 81<<4;
        return 0;
    }
    static class FallingItem extends EntityItem {
        FallingItem(World w,double x,double y,double z,long seed){super(w,x,y,z,new ItemStack(Item.getItemById(264),3));rand.setSeed(seed);}
        boolean web(){return isInWeb;}boolean first(){return firstUpdate;}
        long randomState()throws Exception{return ItemEntityOracle.seed(rand);}
    }
    static List<Object> frame(FallingItem e,World w)throws Exception {
        AxisAlignedBB b=e.getEntityBoundingBox();return Arrays.asList(e.ticksExisted,
            FallingEntityOracle.bits(e.posX,e.posY,e.posZ,e.prevPosX,e.prevPosY,e.prevPosZ,e.motionX,e.motionY,e.motionZ,b.minX,b.minY,b.minZ,b.maxX,b.maxY,b.maxZ),
            Arrays.asList(e.onGround,e.isCollidedHorizontally,e.isCollidedVertically,e.noClip,e.isInWater(),e.web(),e.first(),e.isDead),
            ItemEntityOracle.field(EntityItem.class,"age").getInt(e),ItemEntityOracle.field(EntityItem.class,"delayBeforeCanPickup").getInt(e),ItemEntityOracle.field(EntityItem.class,"health").getInt(e),ItemEntityOracle.field(Entity.class,"fire").getInt(e),Integer.toUnsignedLong(Float.floatToRawIntBits(e.fallDistance)),Long.toString(e.randomState()),new ArrayList<>(w.events));
    }
    public static void main(String[]args)throws Exception {
        Bootstrap.register();Random math=(Random)ItemEntityOracle.field(Class.forName("java.lang.Math$RandomNumberGeneratorHolder"),"randomNumberGenerator").get(null);List<Object> cases=new ArrayList<>();int count=0;
        for(int s=0;s<16;s++)for(int mode=0;mode<4;mode++){
            World w=new World(s);double x=s==12?-2.25:8.5,y=s==13?-62:s==11?3.125:mode==2?2.5:6.,z=8.5;long seed=-189L+s*31+mode*7;math.setSeed(seed^137);
            FallingItem e=new FallingItem(w,x,y,z,seed);double[] velocity={e.motionX,e.motionY,e.motionZ};
            if(mode==1){e.motionX=.45;e.motionY=-.12;e.motionZ=-.22;}
            if(mode==2){e.motionX=-.18;e.motionY=.16;e.motionZ=.05;}
            if(mode==3){e.motionX=0;e.motionY=0;e.motionZ=0;}
            int age=mode==3?5991:mode==2?-32768:0,delay=mode==2?32767:10;
            ItemEntityOracle.age(e,age,delay);if(s==10)ItemEntityOracle.field(Entity.class,"fire").setInt(e,60);
            Map<String,Object> row=new LinkedHashMap<>();row.put("scenario",s);row.put("mode",mode);row.put("seed",Long.toString(seed));row.put("position",new double[]{x,y,z});row.put("motion",new double[]{e.motionX,e.motionY,e.motionZ});row.put("age",age);row.put("delay",delay);row.put("fire",s==10?60:0);
            List<Object> frames=new ArrayList<>();for(int t=1;t<=80&&!e.isDead;t++){w.time=t;e.ticksExisted++;w.events.clear();e.onUpdate();frames.add(frame(e,w));count++;}row.put("frames",frames);cases.add(row);
        }
        try(Writer out=new OutputStreamWriter(new GZIPOutputStream(new FileOutputStream(args[0])),"UTF-8")){new Gson().toJson(cases,out);}
        System.out.println("Original EntityItem motion "+cases.size()+" scenarios / "+count+" ticks");
    }
}
