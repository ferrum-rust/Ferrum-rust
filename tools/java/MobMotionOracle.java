import com.google.gson.Gson;
import java.io.*;
import java.util.*;
import java.util.zip.GZIPOutputStream;
import net.minecraft.block.state.IBlockState;
import net.minecraft.entity.*;
import net.minecraft.init.Bootstrap;
import net.minecraft.potion.*;
import net.minecraft.util.*;
import net.minecraft.profiler.Profiler;
import net.minecraft.server.MinecraftServer;
import net.minecraft.world.*;
import net.minecraft.world.chunk.*;
import net.minecraft.world.storage.*;

/** Original server EntityLiving.onUpdate including MoveHelper, JumpHelper,
 * LivingBase travel, landing and limb swing; ambient/target goals are isolated. */
public final class MobMotionOracle {
    static MinecraftServer host;
    static final class World extends WorldServer {
        int layout;
        final Map<Long,Chunk> chunks=new HashMap<>();
        World(int layout){super(host,new SaveHandlerMP(),new WorldInfo(new WorldSettings(189,WorldSettings.GameType.SURVIVAL,false,false,WorldType.DEFAULT),"mob-motion"),0,new Profiler());this.layout=layout;}
        protected IChunkProvider createChunkProvider(){return null;}
        protected boolean isChunkLoaded(int x,int z,boolean allowEmpty){return true;}
        public Chunk getChunkFromChunkCoords(int x,int z){long key=(long)x<<32^(z&0xffffffffL);Chunk c=chunks.get(key);if(c==null){c=new Chunk(this,x,z);c.setChunkLoaded(true);chunks.put(key,c);}return c;}
        public List<Entity> getEntitiesWithinAABBExcludingEntity(Entity e,AxisAlignedBB b){return Collections.emptyList();}
        public void playSoundAtEntity(Entity e,String name,float volume,float pitch){}
        public boolean isRainingAt(BlockPos p){return false;}
        public IBlockState getBlockState(BlockPos p){
            int x=p.getX(),y=p.getY(),z=p.getZ();
            if(layout==16&&x==9&&y<=4&&y>=1)return FallingEntityOracle.state(65<<4|2);
            if(layout==17&&z==11&&y<=4&&y>=1)return FallingEntityOracle.state(16);
            return FallingEntityOracle.state(ItemMotionOracle.initial(x,y,z,layout));
        }
        public int getLightFromNeighbors(BlockPos p){return 15;}
    }
    static final class Body extends EntityLiving {
        double[] command;double speed;boolean forceJump,voidHit;
        Body(World w,float width,float height){super(w);setSize(width,height);}
        protected void despawnEntity(){}
        protected void updateAITasks(){
            if(command!=null)getMoveHelper().setMoveTo(command[0],command[1],command[2],speed);
            if(forceJump)getJumpHelper().setJumping();
        }
        public boolean attackEntityFrom(DamageSource source,float amount){if(source==DamageSource.outOfWorld)voidHit=true;return false;}
        boolean web(){return isInWeb;}
        void live(){
            voidHit=false;ticksExisted++;prevPosX=posX;prevPosY=posY;prevPosZ=posZ;
            super.onUpdate();
        }
    }
    static Object run(int layout,int kind,int mode)throws Exception{
        World w=new World(layout);float width=new float[]{.6f,.9f,1.4f,.3f}[kind],height=new float[]{1.8f,1.3f,2.9f,.9f}[kind];
        float yaw=new float[]{0f,90f,137.75f,-156.4f}[mode];
        Body e=new Body(w,width,height);
        double x=layout==12?-1.25:8.5,y=layout==13?-1.:mode==0?1.:3.,z=8.5;
        e.setPositionAndRotation(x,y,z,yaw,0);e.onGround=(mode&1)==0;
        double attr=new double[]{.23f,.2f,.25,.25}[kind];e.getEntityAttribute(SharedMonsterAttributes.movementSpeed).setBaseValue(attr);
        List<Object> frames=new ArrayList<>();
        Map<String,Object> row=new LinkedHashMap<>();row.put("layout",layout);row.put("kind",kind);row.put("mode",mode);row.put("start",new double[]{x,y,z});row.put("yaw",MobMoveOracle.bits(yaw));row.put("ground",e.onGround);row.put("attribute",attr);
        for(int t=1;t<=64&&!e.isDead;t++){
            e.command=t%7==0?null:new double[]{x+(t<29?2.5:-2.25),1.+(t%9<5?0:.75),z+(t<18?3.25:t<45?-2.:2.)};
            e.speed=new double[]{1.,1.25,1.4,2.}[(t/10)%4];e.forceJump=(mode==1||mode==3)&&t%19<4;
            e.live();AxisAlignedBB b=e.getEntityBoundingBox();
            frames.add(Arrays.asList(e.command,e.speed,e.forceJump,
                FallingEntityOracle.bits(e.posX,e.posY,e.posZ,e.motionX,e.motionY,e.motionZ,b.minX,b.minY,b.minZ,b.maxX,b.maxY,b.maxZ),
                Arrays.asList(e.onGround,e.isCollidedHorizontally,e.isInWater(),e.web(),e.voidHit),
                Arrays.asList(MobMoveOracle.bits(e.rotationYaw),MobMoveOracle.bits(e.getAIMoveSpeed()),MobMoveOracle.bits(e.moveForward),MobMoveOracle.bits(e.limbSwing),MobMoveOracle.bits(e.limbSwingAmount),MobMoveOracle.bits(e.prevLimbSwingAmount))));
        }
        row.put("frames",frames);return row;
    }
    public static void main(String[]args)throws Exception {
        Bootstrap.register();
        java.lang.reflect.Constructor<?> ctor=Class.forName("VanillaWorldBridge$Host").getDeclaredConstructor(File.class);
        ctor.setAccessible(true);host=(MinecraftServer)ctor.newInstance(new File(args[1]));
        List<Object> result=new ArrayList<>();int n=0;
        for(int layout=0;layout<18;layout++)for(int kind=0;kind<4;kind++)for(int mode=0;mode<4;mode++){
            Object row=run(layout,kind,mode);n+=((List<?>)((Map<?,?>)row).get("frames")).size();result.add(row);
        }
        try(Writer out=new OutputStreamWriter(new GZIPOutputStream(new FileOutputStream(args[0])),"UTF-8")){new Gson().toJson(result,out);}
        System.out.println("Original server living motion "+result.size()+" traces / "+n+" ticks");
    }
}
