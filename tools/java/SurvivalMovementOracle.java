import com.google.gson.Gson;
import com.mojang.authlib.GameProfile;
import java.io.*;
import java.util.*;
import net.minecraft.block.material.Material;
import net.minecraft.client.Minecraft;
import net.minecraft.client.multiplayer.PlayerControllerMP;
import net.minecraft.client.network.NetHandlerPlayClient;
import net.minecraft.client.settings.KeyBinding;
import net.minecraft.init.Bootstrap;
import net.minecraft.util.DamageSource;

/** Original local movement and block callbacks, feeding the original server player's
 * damage, movement stats, food and peaceful tick. Positions and effects come from the SP world. */
public class SurvivalMovementOracle {
    static List<Object> run(int scenario, double height, int flags, int boost, int difficulty, boolean creative, int ticks) throws Exception {
        MovementOracle.TestWorld world = new MovementOracle.TestWorld(scenario,0,0);
        Minecraft mc = MovementOracle.minecraft();
        NetHandlerPlayClient net = new NetHandlerPlayClient(mc,null,null,new GameProfile(new UUID(0,189),"FallOracle"));
        mc.playerController = new PlayerControllerMP(mc,net);
        FoodOracle.Player server = FoodOracle.player(difficulty,true,creative,false,20,20);
        MovementOracle.Player p = new MovementOracle.Player(mc,world,net) {
            public void playSound(String sound,float volume,float pitch) {}
            public boolean attackEntityFrom(DamageSource source,float amount) {return server.attackEntityFrom(source,amount);}
        };
        p.setPosition(8.5,height,scenario==7 || scenario==8 ? 7.5 : 8.5);
        p.onGround=height==1;
        p.capabilities.allowFlying=creative;
        p.capabilities.isCreativeMode=creative;
        if(boost>=0) MovementOracle.effect(p,8,boost);
        for(int i=0;i<7;i++) KeyBinding.setKeyBindState(i+1,(flags&(1<<i))!=0);
        List<Object> snapshots = new ArrayList<>();
        for(int i=0; i<ticks; i++) {
            p.jumped=false;
            double x=p.posX,y=p.posY,z=p.posZ;
            server.hurtResistantTime=Math.max(0,server.hurtResistantTime-1);
            server.ticksExisted++;
            server.peacefulPrefix();
            p.getFoodStats().setFoodLevel(server.getFoodStats().getFoodLevel());
            p.handleWaterMovement();
            boolean lava = p.isInLava();
            // Entity.onEntityUpdate applies lava before moveEntity deals inFire contact.
            if(lava) {
                server.attackEntityFrom(DamageSource.lava,4);
                p.fallDistance*=0.5f;
            }
            p.onLivingUpdate();
            server.sprint=p.isSprinting();
            if(p.jumped) {server.sprint=p.jumpSprint;server.jump();server.sprint=p.isSprinting();}
            server.onGround=p.onGround;
            server.water=p.isInWater();server.eyesWater=p.isInsideOfMaterial(Material.water);server.ladder=p.isOnLadder();
            server.addMovementStat(p.posX-x,p.posY-y,p.posZ-z);
            server.getFoodStats().onUpdate(server);
            Map<String,Object> food=FoodOracle.snapshot(server);
            if(!creative && p.onGround || i==ticks-1 || i%5==0)
                snapshots.add(Arrays.asList(i,FoodOracle.bits(p.fallDistance),food.get("health"),food.get("food"),food.get("saturation"),food.get("exhaustion")));
        }
        return Arrays.asList(scenario,height,flags,boost,difficulty,creative,ticks,snapshots);
    }
    public static void main(String[] args) throws Exception {
        PrintStream output=System.out;System.setOut(System.err);Bootstrap.register();
        List<Object> cases = new ArrayList<>();
        if(args.length>0 && args[0].equals("melee")) {
            for(int difficulty=0; difficulty<4; difficulty++) for(int id:new int[]{54,52}) {
                FoodOracle.Player server=FoodOracle.player(difficulty,true,false,false,20,20);
                net.minecraft.entity.monster.EntityMob mob = id==54 ? new net.minecraft.entity.monster.EntityZombie(server.worldObj)
                    : new net.minecraft.entity.monster.EntitySpider(server.worldObj);
                float base = (float)mob.getEntityAttribute(net.minecraft.entity.SharedMonsterAttributes.attackDamage).getAttributeValue();
                mob.setPosition(1.,1.,0.);
                // Two melee hits in the same tick exercise the original hurt-resistance window.
                server.attackEntityFrom(DamageSource.causeMobDamage(mob),base);
                server.attackEntityFrom(DamageSource.causeMobDamage(mob),base);
                cases.add(Arrays.asList(difficulty,id,base,server.getHealth()));
            }
            output.println(new Gson().toJson(cases));return;
        }
        for(double h:new double[]{3.9,4.,4.1,5.,6.,9.,10.8,19.}) cases.add(run(0,h,0,-1,2,false,38));
        for(int surface:new int[]{4,5,7,8,15,16,17}) cases.add(run(surface,12,0,-1,2,false,44));
        cases.add(run(16,12,32,-1,2,false,44));
        cases.add(run(0,12,0,1,2,false,42));
        cases.add(run(0,12,0,-1,2,true,42));
        for(int difficulty=0;difficulty<4;difficulty++) cases.add(run(0,1,1|16|64,-1,difficulty,false,400));
        output.println(new Gson().toJson(cases));
    }
}
