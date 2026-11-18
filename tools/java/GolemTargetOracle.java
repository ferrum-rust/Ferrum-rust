import com.google.common.base.Predicate;
import com.google.gson.Gson;
import java.io.*;
import java.util.*;
import java.util.zip.GZIPOutputStream;
import net.minecraft.entity.*;
import net.minecraft.entity.monster.*;
import net.minecraft.entity.passive.EntityVillager;
import net.minecraft.init.Bootstrap;
import net.minecraft.pathfinding.*;
import net.minecraft.util.*;
import net.minecraft.village.Village;

/** The three real golem target goals. The navigator returns controlled final
 * nodes, separately tested against the original PathFinder and ground navigator. */
public final class GolemTargetOracle {
    static final class World extends FallingEntityOracle.TestWorld {
        List<EntityLivingBase> actors=new ArrayList<>();
        int tick;
        List<Object> paths=new ArrayList<>();
        World(){super(0);}
        public <T extends Entity> List<T> getEntitiesWithinAABB(Class<? extends T> cls,AxisAlignedBB box,Predicate<? super T> predicate){
            List<T> result=new ArrayList<>();
            for(Entity e:actors)if(cls.isInstance(e)&&e.getEntityBoundingBox().intersectsWith(box)&&predicate.apply(cls.cast(e)))result.add(cls.cast(e));
            return result;
        }
    }
    static final class Navigator extends PathNavigateGround {
        final World w;
        Navigator(EntityLiving mob,World world){super(mob,world);w=world;}
        public PathEntity getPathToEntityLiving(Entity target){
            int i=w.actors.indexOf(target);
            int delta=(i+w.tick/11)%5;
            w.paths.add(Arrays.asList(i,delta));
            if(delta==4)return null;
            BlockPos p=new BlockPos(target);
            return new PathEntity(new PathPoint[]{new PathPoint(p.getX()+delta,p.getY(),p.getZ())});
        }
    }
    static final class Golem extends EntityIronGolem {
        Golem(World w){super(w);}
        protected PathNavigate getNewNavigator(net.minecraft.world.World w){return new Navigator(this,(World)w);}
        void step(){targetTasks.onUpdateTasks();}
    }
    static Object run(int sample)throws Exception{
        World w=new World();Golem g=new Golem(w);g.setPosition(.5,1,.5);
        long seed=sample*287161L-189;g.getRNG().setSeed(seed);g.setPlayerCreated((sample&1)!=0);
        Village v=new Village(w);
        EntityLivingBase[] actors={new EntityZombie(w),new EntityCreeper(w),new VillagerGoalsOracle.Player(w),new EntitySkeleton(w),new EntityVillager(w,0),new EntityZombie(w)};
        for(EntityLivingBase e:actors)w.actors.add(e);
        w.playerEntities.add((net.minecraft.entity.player.EntityPlayer)actors[2]);
        List<Object> frames=new ArrayList<>();
        for(int t=1;t<=94;t++){
            w.tick=t;g.ticksExisted=t;w.paths.clear();
            boolean member=(sample%3==0||t>11)&&t%29!=0;FallingEntityOracle.field(EntityIronGolem.class,"villageObj").set(g,member?v:null);
            boolean home=(t/17)%2==1;if(home)g.setHomePosAndDistance(new BlockPos(0,1,0),8);else g.detachHome();
            int revenge=t%17==1?(sample+t/17)%6:-1;if(revenge>=0)g.setRevengeTarget(actors[revenge]);
            int aggressor=t%13==1?(sample+t/13+2)%6:-1;if(aggressor>=0)v.addOrRenewAgressor(actors[aggressor]);
            int rep=t==1?-16:t%22==0?4:0;if(rep!=0)v.setReputationForPlayer(actors[2].getName(),rep);
            List<Object> targets=new ArrayList<>();
            for(int k=0;k<6;k++){
                int dx=Math.floorMod(sample*5+k*3+t/9,23)-11,dz=(k%3-1)*3;
                double y=(k==5&&t%31<9)?7.5:1.;
                actors[k].setPosition(dx+.5,y,dz+.5);
                actors[k].setHealth((sample+k*5+t/7)%19==0?0f:20f);
                actors[k].setInvisible((sample+k+t/23)%5==0);
                targets.add(Arrays.asList(actors[k].posX,y,actors[k].posZ,actors[k].isEntityAlive(),actors[k].isInvisible()));
            }
            boolean immune=(sample+t/15)%5==0;
            ((net.minecraft.entity.player.EntityPlayer)actors[2]).capabilities.disableDamage=immune;
            g.step();
            frames.add(Arrays.asList(member,home,revenge,aggressor,rep,immune,targets,w.actors.indexOf(g.getAttackTarget()),new ArrayList<Object>(w.paths),Long.toString(ItemEntityOracle.seed(g.getRNG()))));
        }
        return Arrays.asList(Long.toString(seed),g.isPlayerCreated(),frames);
    }
    public static void main(String[]args)throws Exception{
        Bootstrap.register();List<Object> rows=new ArrayList<>();for(int i=0;i<96;i++)rows.add(run(i));
        try(Writer writer=new OutputStreamWriter(new GZIPOutputStream(new FileOutputStream(args[0])),"UTF-8")){new Gson().toJson(rows,writer);}
        System.out.println("Original iron golem target selectors 96 sessions / 9024 updates");
    }
}
