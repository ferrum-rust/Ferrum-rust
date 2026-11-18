import com.google.gson.Gson;
import java.io.*;
import java.util.*;
import java.util.zip.GZIPOutputStream;
import net.minecraft.entity.*;
import net.minecraft.entity.monster.*;
import net.minecraft.entity.passive.EntityVillager;
import net.minecraft.init.Bootstrap;
import net.minecraft.util.*;
import net.minecraft.village.Village;

/** Original setRevengeTarget/onDeath village callbacks, age, player proximity
 * and reputation clamping, plus ordered aggressor-list renewal/expiry. */
public final class VillageReputationOracle {
    static final class World extends FallingEntityOracle.TestWorld {
        final List<Integer> statuses=new ArrayList<>();
        World(){super(0);getGameRules().setOrCreateGameRule("doMobLoot","false");}
        public void setEntityState(Entity e,byte status){statuses.add((int)status);}
    }
    static Object state(Village village,World world,EntityVillager golem,EntityLivingBase[] actors)throws Exception{
        EntityLivingBase nearest=village.findNearestVillageAggressor(golem);
        return Arrays.asList(village.getReputationForPlayer("Player"),FallingEntityOracle.field(Village.class,"noBreedTicks").getInt(village),
            Arrays.asList(actors).indexOf(nearest),new ArrayList<Integer>(world.statuses));
    }
    static Object run(int i)throws Exception{
        World w=new World();Village v=new Village(w);FallingEntityOracle.field(Village.class,"tickCounter").setInt(v,147);
        VillagerGoalsOracle.Player p=new VillagerGoalsOracle.Player(w);
        p.setPosition(i%3==0?15.9999:i%3==1?16:16.001,1,0);w.playerEntities.add(p);
        EntityVillager a=new EntityVillager(w,0);a.setPosition(0,1,0);a.setGrowingAge(i%2==0?-12000:0);a.setHealth(i%7==0?0f:20f);
        EntityZombie zombie=new EntityZombie(w);zombie.setPosition(-3,1,0);
        EntitySpider spider=new EntitySpider(w);spider.setPosition(3,1,0);
        EntityLivingBase[] actors={p,zombie,spider};
        boolean member=(i&4)==0;FallingEntityOracle.field(EntityVillager.class,"villageObj").set(a,member?v:null);
        int initial=new int[]{-30,-15,-1,0,8,10}[i%6];v.setReputationForPlayer("Player",initial);
        List<Object> frames=new ArrayList<>();
        for(int t=0;t<9;t++){
            int id=(i+t*3)%4;w.statuses.clear();a.setRevengeTarget(id==3?null:actors[id]);frames.add(Arrays.asList(id,state(v,w,a,actors)));
        }
        w.statuses.clear();
        int source=i%4;
        DamageSource damage=source==0?DamageSource.causePlayerDamage(p):source==1?DamageSource.causeMobDamage(spider):source==2?DamageSource.fall:DamageSource.causeMobDamage(new EntityVillager(w,0));
        a.onDeath(damage);
        return Arrays.asList(i,member,a.isChild(),i%7!=0,initial,p.posX,frames,source,state(v,w,a,actors));
    }
    public static void main(String[]args)throws Exception{
        Bootstrap.register();List<Object> rows=new ArrayList<>();for(int i=0;i<192;i++)rows.add(run(i));
        try(Writer writer=new OutputStreamWriter(new GZIPOutputStream(new FileOutputStream(args[0])),"UTF-8")){new Gson().toJson(rows,writer);}
        System.out.println("Original villager reputation 192 sessions / 1728 hits +192 deaths");
    }
}
