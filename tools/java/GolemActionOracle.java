import com.google.gson.Gson;
import java.io.*;
import java.util.*;
import java.util.zip.GZIPOutputStream;
import net.minecraft.entity.*;
import net.minecraft.entity.ai.*;
import net.minecraft.entity.monster.*;
import net.minecraft.entity.passive.EntityVillager;
import net.minecraft.init.Bootstrap;
import net.minecraft.nbt.*;
import net.minecraft.pathfinding.*;
import net.minecraft.util.*;
import net.minecraft.village.*;

/** The eight original action tasks together. Navigation results are supplied
 * as recorded controlled boundaries; actual PathFinder/Nav physics have
 * separate differential traces. No original goal is replaced or reordered. */
public final class GolemActionOracle {
    static class World extends FallingEntityOracle.TestWorld {
        boolean day;int time;List<Object> calls=new ArrayList<>();List<EntityLivingBase> actors=new ArrayList<>();
        World(){super(0);villageCollectionObj=new VillageCollection(this);}
        public boolean isDaytime(){return day;}
        public <T extends Entity> List<T> getEntitiesWithinAABB(Class<? extends T> cls,AxisAlignedBB box){
            List<T> out=new ArrayList<>();for(Entity e:actors)if(cls.isInstance(e)&&e.getEntityBoundingBox().intersectsWith(box))out.add(cls.cast(e));return out;
        }
        public void setEntityState(Entity e,byte b){calls.add(Arrays.asList("status",(int)b));}
    }
    static class Target extends EntityZombie {
        Target(World w){super(w);}
        public boolean attackEntityFrom(DamageSource source,float f){((World)worldObj).calls.add(Arrays.asList("hit",(int)f));return false;}
    }
    static class Nav extends PathNavigateGround {
        final World w;boolean hasPath,breakDoors;
        Nav(Golem g,World w){super(g,w);this.w=w;}
        public void setBreakDoors(boolean val){breakDoors=val;super.setBreakDoors(val);}
        public PathEntity getPathToPos(BlockPos p){
            boolean accepted=Math.floorMod(p.getX()*7+p.getY()+p.getZ()+w.time,5)!=1;
            w.calls.add(Arrays.asList("block",p.getX(),p.getY(),p.getZ(),breakDoors,accepted));
            return accepted?new PathEntity(new PathPoint[]{new PathPoint(p.getX(),p.getY(),p.getZ())}):null;
        }
        public PathEntity getPathToEntityLiving(Entity e){
            boolean accepted=w.time%7!=2;
            int index=w.actors.indexOf(e);w.calls.add(Arrays.asList("entity",index,accepted));
            BlockPos p=new BlockPos(e);return accepted?new PathEntity(new PathPoint[]{new PathPoint(p.getX(),p.getY(),p.getZ())}):null;
        }
        public boolean setPath(PathEntity p,double speed){hasPath=p!=null;w.calls.add(Arrays.asList("set",hasPath,speed));return hasPath;}
        public boolean noPath(){return !hasPath;}
        public void clearPathEntity(){hasPath=false;w.calls.add(Arrays.asList("clear"));}
    }
    static class Look extends EntityLookHelper {
        final World w;
        Look(Golem g,World w){super(g);this.w=w;}
        public void setLookPositionWithEntity(Entity e,float yaw,float pitch){w.calls.add(Arrays.asList("lookEntity",w.actors.indexOf(e),yaw,pitch));}
        public void setLookPosition(double x,double y,double z,float yaw,float pitch){w.calls.add(Arrays.asList("look",FallingEntityOracle.bits(x,y,z),yaw,pitch));}
    }
    static class Golem extends EntityIronGolem {
        final Look look;
        Golem(World w){super(w);look=new Look(this,w);}
        protected PathNavigate getNewNavigator(net.minecraft.world.World w){return new Nav(this,(World)w);}
        public EntityLookHelper getLookHelper(){return look;}
        public void playSound(String k,float volume,float pitch){((World)worldObj).calls.add(Arrays.asList("sound",k));}
        void updateGoals(){tasks.onUpdateTasks();}
        EntityAITasks selector(){return tasks;}
        void age(int n){entityAge=n;}
    }
    static Object run(int sample)throws Exception {
        World w=new World();Golem g=new Golem(w);g.setPosition(.5,1,.5);g.onGround=true;
        EntityLivingBase[] actors={new Target(w),new VillagerGoalsOracle.Player(w),new EntityVillager(w,0),new EntityVillager(w,0)};
        w.actors.addAll(Arrays.asList(actors));w.playerEntities.add((net.minecraft.entity.player.EntityPlayer)actors[1]);
        long seed=sample*317681L-189;
        if(sample%12==1){Random probe=new Random();for(;;seed++){probe.setSeed(seed);if(probe.nextInt(8000)==0)break;}}
        g.getRNG().setSeed(seed);
        Village v=new Village(w);
        for(int j=0;j<5;j++)v.addVillageDoorInfo(new VillageDoorInfo(new BlockPos(j*4-6,1,j%2*2),2,0,0));
        if(sample%4!=0)w.getVillageCollection().getVillageList().add(v);
        List<Object> frames=new ArrayList<>();
        Nav nav=(Nav)g.getNavigator();
        for(int t=1;t<=143;t++){
            w.time=t;w.calls.clear();w.day=(sample+t/27)%3!=0;
            double x=(sample%3==2&&t%37<15)?13.5:.5;g.setPosition(x,1,.5);
            boolean home=(sample+t/15)%3!=0;if(home)g.setHomePosAndDistance(new BlockPos(0,1,0),8);else g.detachHome();
            int age=(sample%5==0&&t%41>20)?130:t;g.age(age);
            actors[0].setPosition(t%19<7?x+1:x+7+t%3,1,.5);
            actors[1].setPosition(x+2+t%11,1,.5);
            actors[2].setPosition(x+3,1,1.5);actors[3].setPosition(x-3,1,-.5);
            int attacked=sample%5!=0 && (sample+t/19)%4==0?0:-1;
            g.setAttackTarget(attacked>=0?actors[attacked]:null);
            boolean exhausted=t%7==0;if(exhausted)nav.hasPath=false;
            g.updateGoals();
            List<Integer> active=new ArrayList<>();
            List<?> entries=(List<?>)FallingEntityOracle.field(EntityAITasks.class,"taskEntries").get(g.selector());
            for(Object e:(List<?>)FallingEntityOracle.field(EntityAITasks.class,"executingTaskEntries").get(g.selector()))active.add(entries.indexOf(e));
            frames.add(Arrays.asList(w.day,x,home,age,attacked,exhausted,active,nav.hasPath,g.getAttackTimer(),g.getHoldRoseTick(),new ArrayList<Object>(w.calls),Long.toString(ItemEntityOracle.seed(g.getRNG()))));
        }
        return Arrays.asList(sample,Long.toString(seed),frames);
    }
    public static void main(String[]args)throws Exception{
        Bootstrap.register();List<Object> rows=new ArrayList<>();for(int i=0;i<96;i++)rows.add(run(i));
        try(Writer writer=new OutputStreamWriter(new GZIPOutputStream(new FileOutputStream(args[0])),"UTF-8")){new Gson().toJson(rows,writer);}
        System.out.println("Original iron golem action selectors 96 sessions / 13,728 updates");
    }
}
