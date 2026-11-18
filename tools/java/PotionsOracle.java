// Actual PotionEffect/EntityLivingBase map methods; attack is an observed boundary.
import com.google.gson.Gson;
import java.util.*;
import net.minecraft.init.Bootstrap;
import net.minecraft.entity.EntityLivingBase;
import net.minecraft.potion.*;
import net.minecraft.util.DamageSource;
import net.minecraft.world.World;

public final class PotionsOracle {
    static final class Player extends FoodOracle.Player {
        List<Object> events=new ArrayList<Object>();
        Player(World w){super(w);}
        @Override public void heal(float amount){if(events!=null)events.add(Arrays.asList("heal",FoodOracle.bits(amount)));super.heal(amount);}
        @Override public boolean attackEntityFrom(DamageSource source,float amount){events.add(Arrays.asList(source==DamageSource.wither?"wither":"magic",FoodOracle.bits(amount)));return false;}
        @Override public void addExhaustion(float amount){events.add(Arrays.asList("exhaustion",FoodOracle.bits(amount)));super.addExhaustion(amount);}
        @Override protected void onNewPotionEffect(PotionEffect e){events.add(Arrays.asList("added",effect(e)));super.onNewPotionEffect(e);}
        @Override protected void onChangedPotionEffect(PotionEffect e,boolean flag){events.add(Arrays.asList("changed",effect(e),flag));super.onChangedPotionEffect(e,flag);}
        @Override protected void onFinishedPotionEffect(PotionEffect e){events.add(Arrays.asList("removed",effect(e)));super.onFinishedPotionEffect(e);}
        void tickEffects(){updatePotionEffects();}
        float reduced(DamageSource source,float amount){return applyPotionDamageCalculations(source,amount);}
    }
    static final class CombatPlayer extends FoodOracle.Player {
        int armor;
        CombatPlayer(World world){super(world);}
        @Override public int getTotalArmorValue(){return armor;}
        @Override protected void damageArmor(float amount){} // observed durability boundary
    }
    static Object effect(PotionEffect e){return Arrays.asList(e.getPotionID(),e.getDuration(),e.getAmplifier(),e.getIsAmbient(),e.getIsShowParticles());}
    static Object snapshot(Player p){List<Object> effects=new ArrayList<Object>();for(PotionEffect e:p.getActivePotionEffects())effects.add(effect(e));return Arrays.asList(effects,FoodOracle.bits(p.getHealth()),FoodOracle.bits(p.getAbsorptionAmount()),FoodOracle.bits(p.getFoodStats().getSaturationLevel()),p.getFoodStats().getFoodLevel(),FoodOracle.snapshot(p).get("exhaustion"));}
    static Player player(boolean immune,boolean remote,float health)throws Exception{FoodOracle.Player base=FoodOracle.player(2,true,immune,remote,health,20f);FoodOracle.set(base.worldObj,World.class,"worldAccesses",new ArrayList<Object>());Player p=new Player(base.worldObj);p.capabilities.disableDamage=immune;p.capabilities.isCreativeMode=immune;p.setHealth(health);return p;}
    static Map<String,Object> sequence(int seed,boolean immune,boolean remote,float health)throws Exception {
        Player p=player(immune,remote,health);Random random=new Random(seed);List<Object> ops=new ArrayList<Object>(),states=new ArrayList<Object>();int[] ids={3,4,8,9,10,11,12,13,14,15,16,17,19,20,22,23};
        for(int i=0;i<96;i++){
            if(i<ids.length || random.nextInt(5)<3){
                int id=i<ids.length?ids[i]:ids[random.nextInt(ids.length)];int duration=new int[]{-1,0,1,2,3,25,50,601}[random.nextInt(8)],amp=new int[]{-1,0,1,3,5,31,32}[random.nextInt(7)];
                boolean ambient=random.nextBoolean(),visible=random.nextBoolean();PotionEffect e=new PotionEffect(id,duration,amp,ambient,visible);ops.add(Arrays.asList("add",effect(e)));p.addPotionEffect(e);
            }else if(random.nextBoolean()) {int ticks=random.nextInt(8)+1;ops.add(Arrays.asList("tick",ticks));for(int j=0;j<ticks;j++)p.tickEffects();}
            else if(random.nextBoolean()){int id=ids[random.nextInt(ids.length)];ops.add(Arrays.asList("remove",id));p.removePotionEffect(id);}
            else {ops.add(Arrays.asList("clear"));p.clearActivePotions();}
            states.add(snapshot(p));
        }
        Map<String,Object> out=new LinkedHashMap<String,Object>();out.put("seed",seed);out.put("immune",immune);out.put("remote",remote);out.put("health",FoodOracle.bits(health));out.put("ops",ops);out.put("states",states);out.put("events",p.events);return out;
    }
    static Object combat(int armor,boolean immune,int resistance,boolean fire,int source,float absorption)throws Exception {
        FoodOracle.Player base=FoodOracle.player(2,true,immune,false,20,20);CombatPlayer p=new CombatPlayer(base.worldObj);p.armor=armor;p.capabilities.disableDamage=immune;p.setAbsorptionAmount(absorption);
        if(resistance!=-99)p.addPotionEffect(new PotionEffect(11,100,resistance));if(fire)p.addPotionEffect(new PotionEffect(12,100,0));
        DamageSource[] sources={DamageSource.magic,DamageSource.wither,DamageSource.starve,DamageSource.outOfWorld,DamageSource.lava,DamageSource.cactus};
        List<Object> states=new ArrayList<Object>();float[] amounts={1f,3f,0f,Float.intBitsToFloat(0x3f800001),8f,1f,1f,2f,2f,2f,2f,2f,1f,1f,1f,6f,3f,3f,3f,4f,4f,1f,2f,9f};
        for(float amount:amounts){p.hurtResistantTime=Math.max(0,p.hurtResistantTime-1);p.attackEntityFrom(sources[source],amount);states.add(Arrays.asList(FoodOracle.bits(p.getHealth()),FoodOracle.bits(p.getAbsorptionAmount()),FoodOracle.snapshot(p).get("exhaustion"),p.hurtResistantTime));}
        return Arrays.asList(armor,immune,resistance,fire,source,FoodOracle.bits(absorption),states);
    }
    public static void main(String[] args)throws Exception {
        Bootstrap.register();List<Object> sequences=new ArrayList<Object>();for(int seed=0;seed<16;seed++)for(boolean immune:new boolean[]{false,true})for(boolean remote:new boolean[]{false,true})for(float health:new float[]{0f,1f,19f,20f})sequences.add(sequence(seed,immune,remote,health));
        List<Object> resistance=new ArrayList<Object>();DamageSource[] sources={DamageSource.magic,DamageSource.wither,DamageSource.starve,DamageSource.outOfWorld,DamageSource.lava};
        for(int amp:new int[]{-1,0,1,4,5,31,32,Integer.MAX_VALUE})for(int source=0;source<sources.length;source++)for(float damage:new float[]{-1f,-0f,0f,1f,Float.intBitsToFloat(0x3f800001),3.5f,100f}){Player p=player(false,false,20);p.addPotionEffect(new PotionEffect(11,100,amp));resistance.add(Arrays.asList(amp,source,FoodOracle.bits(damage),FoodOracle.bits(p.reduced(sources[source],damage))));}
        List<Object> combat=new ArrayList<Object>();for(int armor:new int[]{0,3,7,20})for(boolean immune:new boolean[]{false,true})for(int resistanceAmp:new int[]{-99,0,4,5})for(boolean fire:new boolean[]{false,true})for(int source=0;source<6;source++)for(float absorption:new float[]{0f,4f})combat.add(combat(armor,immune,resistanceAmp,fire,source,absorption));
        Map<String,Object> out=new LinkedHashMap<String,Object>();out.put("sequences",sequences);out.put("resistance",resistance);out.put("combat",combat);System.out.println(new Gson().toJson(out));
    }
}
