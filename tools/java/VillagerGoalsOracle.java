import com.google.gson.Gson;
import com.mojang.authlib.GameProfile;
import java.io.*;
import java.util.*;
import java.util.zip.GZIPOutputStream;
import net.minecraft.entity.*;
import net.minecraft.entity.ai.*;
import net.minecraft.entity.passive.EntityVillager;
import net.minecraft.entity.player.EntityPlayer;
import net.minecraft.init.Bootstrap;
import net.minecraft.profiler.Profiler;

/** Two original merchant AI tasks sharing the real EntityAITasks scheduler. */
public final class VillagerGoalsOracle {
    static final class Player extends EntityPlayer {
        Player(net.minecraft.world.World world) { super(world,new GameProfile(new UUID(0,189),"Player")); }
        public boolean isSpectator() {return false;}
    }
    static Object run(int i)throws Exception {
        FallingEntityOracle.TestWorld world=new FallingEntityOracle.TestWorld(0);
        EntityVillager v=new EntityVillager(world,0);
        Player player=new Player(world);
        long seed=-189L+i*817276L;
        v.getRNG().setSeed(seed);v.setPosition(0,2,0);
        EntityAITradePlayer trade=new EntityAITradePlayer(v);
        EntityAILookAtTradePlayer look=new EntityAILookAtTradePlayer(v);
        EntityAITasks tasks=new EntityAITasks(new Profiler());
        tasks.addTask(1,trade);tasks.addTask(1,look);
        List<Object> frames=new ArrayList<>();
        for(int t=1;t<=138;t++){
            boolean grounded=(t+i)%19!=1 && (i%4==0||t>3);
            boolean wet=i%3==0&&t>=41&&t<=44;
            boolean velocity=i%5==0&&t>=20&&t<=21;
            double distance=t%31==0?8.5:t%17==0?4.00000000001:t%13==0?4.0:(i%3+1)*.5;
            int bind=t%23==1?1:t%53==0?-1:0;
            if(bind==1)v.setCustomer(player);if(bind==-1)v.setCustomer(null);
            v.onGround=grounded;v.velocityChanged=velocity;
            FallingEntityOracle.field(Entity.class,"inWater").setBoolean(v,wet);
            player.setPosition(distance,2,0);
            tasks.onUpdateTasks();
            List<Integer> active=new ArrayList<>();
            for(Object entry:(List<?>)FallingEntityOracle.field(EntityAITasks.class,"executingTaskEntries").get(tasks)){
                EntityAIBase action=(EntityAIBase)FallingEntityOracle.field(entry.getClass(),"action").get(entry);
                active.add(action==trade?0:1);
            }
            frames.add(Arrays.asList(grounded,wet,velocity,distance,bind,active,v.getCustomer()!=null,
                FallingEntityOracle.field(EntityAIWatchClosest.class,"lookTime").getInt(look),Long.toString(ItemEntityOracle.seed(v.getRNG()))));
        }
        return Arrays.asList(Long.toString(seed),frames);
    }
    public static void main(String[]args)throws Exception{
        Bootstrap.register();List<Object> rows=new ArrayList<>();for(int i=0;i<96;i++)rows.add(run(i));
        try(Writer writer=new OutputStreamWriter(new GZIPOutputStream(new FileOutputStream(args[0])),"UTF-8")){new Gson().toJson(rows,writer);}
        System.out.println("Original merchant AI 96 sessions / 13,248 customer/look updates");
    }
}
