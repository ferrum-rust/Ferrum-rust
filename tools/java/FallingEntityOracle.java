// Executes the original EntityFallingBlock and World collision/placement code.
// Item-entity construction and target health are represented by recorded dispatches.
import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import java.util.concurrent.atomic.AtomicLong;
import java.util.zip.GZIPOutputStream;
import com.google.gson.Gson;
import net.minecraft.block.*;
import net.minecraft.block.state.IBlockState;
import net.minecraft.entity.*;
import net.minecraft.entity.item.*;
import net.minecraft.init.*;
import net.minecraft.item.ItemStack;
import net.minecraft.nbt.NBTTagCompound;
import net.minecraft.profiler.Profiler;
import net.minecraft.util.*;
import net.minecraft.world.*;
import net.minecraft.world.chunk.*;
import net.minecraft.world.storage.WorldInfo;

public final class FallingEntityOracle {
    static Field field(Class<?> c,String name)throws Exception {Field f=c.getDeclaredField(name);f.setAccessible(true);return f;}
    static int state(IBlockState s){return Block.getIdFromBlock(s.getBlock())<<4|s.getBlock().getMetaFromState(s);}
    static IBlockState state(int s){return Block.getBlockById(s>>4).getStateFromMeta(s&15);}
    static List<String> bits(double... values){List<String> result=new ArrayList<>();for(double v:values)result.add(Long.toHexString(Double.doubleToRawLongBits(v)));return result;}
    static class TestWorld extends World {
        final int scenario;int tick;
        final Map<BlockPos,IBlockState> edits=new HashMap<>();
        final Map<Long,Chunk> chunks=new HashMap<>();
        final List<Object> events=new ArrayList<>();
        final List<Entity> targets=new ArrayList<>();
        TestWorld(int scenario){super(null,new WorldInfo(new WorldSettings(189,WorldSettings.GameType.SURVIVAL,false,false,WorldType.DEFAULT),"falling"),new WorldProviderSurface(),new Profiler(),scenario==15);this.scenario=scenario;provider.registerWorld(this);getGameRules().setOrCreateGameRule("doEntityDrops",scenario==18?"false":"true");}
        protected IChunkProvider createChunkProvider(){return null;}
        protected int getRenderDistanceChunks(){return 8;}
        protected boolean isChunkLoaded(int x,int z,boolean allowEmpty){return true;}
        public Chunk getChunkFromChunkCoords(int x,int z){long k=(long)x<<32^(z&0xffffffffL);Chunk c=chunks.get(k);if(c==null){c=new Chunk(this,x,z);c.setChunkLoaded(true);chunks.put(k,c);}return c;}
        public IBlockState getBlockState(BlockPos p){IBlockState s=edits.get(p);return s!=null?s:state(initial(p.getX(),p.getY(),p.getZ(),scenario));}
        public boolean setBlockState(BlockPos p,IBlockState s,int flags){boolean ok=scenario!=16||s.getBlock()==Blocks.air;events.add(Arrays.asList("set",p.getX(),p.getY(),p.getZ(),state(s),flags,ok));if(ok)edits.put(new BlockPos(p.getX(),p.getY(),p.getZ()),s);return ok;}
        public List<Entity> getEntitiesWithinAABBExcludingEntity(Entity e,AxisAlignedBB b){List<Entity> out=new ArrayList<>();for(Entity target:targets)if(target!=e&&target.getEntityBoundingBox().intersectsWith(b))out.add(target);return out;}
        public boolean isRainingAt(BlockPos p){return scenario==17&&tick>4;}
        public void playSoundAtEntity(Entity e,String name,float volume,float pitch){events.add(Arrays.asList("sound",name,Integer.toUnsignedLong(Float.floatToRawIntBits(volume)),Integer.toUnsignedLong(Float.floatToRawIntBits(pitch))));}
        public void playAuxSFX(int id,BlockPos p,int data){events.add(Arrays.asList("effect",id,p.getX(),p.getY(),p.getZ(),data));}
    }
    static int initial(int x,int y,int z,int s){
        if(y<0||y>=256)return 0;
        if(s==12||s==14||s==20)return 0;
        if(y==0)return(s==2?44:s==7?88:s==8?165:s==9?85:1)<<4;
        if((s==1||s==3||s==18)&&y==1)return 50<<4|5;
        if(s==4&&y<=3)return 9<<4;
        if((s==5||s==17)&&y<=3)return 11<<4;
        if(s==6&&y>=2&&y<=4)return 30<<4;
        if(s==10&&y==1)return 36<<4;
        if(s==19&&x==10&&y<15)return 1<<4;
        return 0;
    }
    static class Target extends Entity {
        final int index;
        Target(TestWorld w,int index){super(w);this.index=index;setEntityBoundingBox(new AxisAlignedBB(-100,0,-100,100,4,100));}
        protected void entityInit(){}
        protected void readEntityFromNBT(NBTTagCompound n){}
        protected void writeEntityToNBT(NBTTagCompound n){}
        public boolean attackEntityFrom(DamageSource source,float amount){((TestWorld)worldObj).events.add(Arrays.asList("damage",index,source.damageType,Integer.toUnsignedLong(Float.floatToRawIntBits(amount))));return true;}
    }
    static class Falling extends EntityFallingBlock {
        Falling(TestWorld w,double x,double y,double z,IBlockState s,long seed){super(w,x,y,z,s);rand.setSeed(seed);}
        public EntityItem entityDropItem(ItemStack stack,float offset){((TestWorld)worldObj).events.add(Arrays.asList("drop",bits(posX,posY+(double)offset,posZ),net.minecraft.item.Item.getIdFromItem(stack.getItem()),stack.stackSize,stack.getMetadata(),10));return null;}
        boolean web(){return isInWeb;}
        long random()throws Exception{return((AtomicLong)field(Random.class,"seed").get(rand)).get();}
    }
    static Map<String,Object> snapshot(Falling p)throws Exception {
        AxisAlignedBB b=p.getEntityBoundingBox();Map<String,Object> row=new LinkedHashMap<>();
        row.put("bits",bits(p.posX,p.posY,p.posZ,p.motionX,p.motionY,p.motionZ,b.minX,b.minY,b.minZ,b.maxX,b.maxY,b.maxZ,p.prevPosX,p.prevPosY,p.prevPosZ));
        row.put("flags",new boolean[]{p.onGround,p.isCollidedHorizontally,p.isCollidedVertically,p.isDead,p.web(),field(EntityFallingBlock.class,"canSetAsBlock").getBoolean(p)});
        row.put("age",p.fallTime);row.put("fall_distance",Integer.toUnsignedLong(Float.floatToRawIntBits(p.fallDistance)));row.put("state",state(p.getBlock()));row.put("fire",field(Entity.class,"fire").getInt(p));row.put("random",Long.toString(p.random()));
        row.put("events",new ArrayList<>(((TestWorld)p.worldObj).events));((TestWorld)p.worldObj).events.clear();return row;
    }
    public static void main(String[] args)throws Exception {
        Bootstrap.register();List<Object> rows=new ArrayList<>();int ticks=0;
        for(int scenario=0;scenario<=20;scenario++)for(int block:new int[]{12<<4,12<<4|1,13<<4,145<<4,145<<4|5,145<<4|10,1<<4,0})for(int motion=0;motion<3;motion++){
            TestWorld w=new TestWorld(scenario);w.targets.add(new Target(w,1));w.targets.add(new Target(w,2));
            double y=scenario==13?300:scenario==12||scenario==14?80:8;long seed=189L+scenario*17+block*31+motion;
            Falling p=new Falling(w,8.5,y,8.5,state(block),seed);p.setHurtEntities((block>>4)==145||motion==2);p.shouldDropItem=motion!=1;p.motionX=motion==0?0:motion==1?.3:1.25;p.motionY=motion==2?-.24:0;p.motionZ=motion==1?-.15:0;
            if(scenario==14)p.fallTime=599;if(scenario==20)p.noClip=true;
            if(scenario!=11&&scenario!=15)w.edits.put(new BlockPos(p),state(block));
            Map<String,Object> row=new LinkedHashMap<>();row.put("scenario",scenario);row.put("motion",motion);row.put("seed",Long.toString(seed));row.put("initial",snapshot(p));List<Object> frames=new ArrayList<>();
            for(int t=0;t<625&&!p.isDead;t++){w.tick=t;p.onUpdate();frames.add(snapshot(p));ticks++;}
            row.put("ticks",frames);rows.add(row);
        }
        try(Writer out=new OutputStreamWriter(new GZIPOutputStream(new FileOutputStream(args[0])),"UTF-8")){new Gson().toJson(rows,out);}
        System.out.println("Original falling entity: "+rows.size()+" sequences / "+ticks+" ticks");
    }
}
