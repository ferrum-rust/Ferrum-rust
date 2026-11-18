// Actual EntityVillager.useRecipe/updateAITasks callbacks, with village
// discovery held at a fixed boundary so only trading and restocking advance.
import com.google.gson.GsonBuilder;
import java.lang.reflect.Field;
import java.util.*;
import sun.misc.Unsafe;
import net.minecraft.entity.*;
import net.minecraft.entity.item.EntityXPOrb;
import net.minecraft.entity.passive.EntityVillager;
import net.minecraft.init.Bootstrap;
import net.minecraft.item.ItemStack;
import net.minecraft.potion.PotionEffect;
import net.minecraft.village.*;
import net.minecraft.world.*;
import net.minecraft.world.storage.WorldInfo;

public final class VillagerRestockOracle {
    static final class CaptureWorld extends FoodOracle.OracleWorld {
        List<Integer> experience;
        List<Integer> statuses;
        @Override public boolean spawnEntityInWorld(Entity entity) {
            if(entity instanceof EntityXPOrb)experience.add(((EntityXPOrb)entity).getXpValue());
            return true;
        }
        @Override public void setEntityState(Entity entity,byte state) {statuses.add((int)state);}
    }
    static CaptureWorld world()throws Exception {
        Field field=Unsafe.class.getDeclaredField("theUnsafe");field.setAccessible(true);
        CaptureWorld world=(CaptureWorld)((Unsafe)field.get(null)).allocateInstance(CaptureWorld.class);
        world.experience=new ArrayList<Integer>();world.statuses=new ArrayList<Integer>();world.rules=new GameRules();world.difficulty=EnumDifficulty.NORMAL;
        FoodOracle.set(world,World.class,"rand",new Random(189));
        FoodOracle.set(world,World.class,"provider",new WorldProviderSurface());
        FoodOracle.set(world,World.class,"worldInfo",new WorldInfo(new WorldSettings(189L,WorldSettings.GameType.SURVIVAL,false,false,WorldType.DEFAULT),"VillagerOracle"));
        FoodOracle.set(world,World.class,"worldAccesses",new ArrayList<Object>());
        return world;
    }
    static final class Villager extends EntityVillager {
        List<Object> sounds=new ArrayList<Object>();
        List<Object> effects=new ArrayList<Object>();
        Villager(World world,int profession){super(world,profession);}
        @Override public void playSound(String sound,float volume,float pitch){if(sounds!=null)sounds.add(Arrays.asList(sound,FoodOracle.bits(volume),FoodOracle.bits(pitch)));}
        @Override public void addPotionEffect(PotionEffect effect){effects.add(Arrays.asList(effect.getPotionID(),effect.getDuration(),effect.getAmplifier()));super.addPotionEffect(effect);}
        void tickTrade(){super.updateAITasks();}
    }
    static Object field(Villager villager,String name)throws Exception {
        Field f=EntityVillager.class.getDeclaredField(name);f.setAccessible(true);return f.get(villager);
    }
    static final class Case {
        final CaptureWorld world;
        final FoodOracle.Player player;
        final Villager villager;
        final Village village;
        final Random random;
        final MerchantRecipeList recipes;
        final List<Object> steps=new ArrayList<Object>();
        Case(int profession,long seed,boolean member)throws Exception {
            world=world();player=new FoodOracle.Player(world);villager=new Villager(world,profession);village=new Village(world);
            random=new Random(seed);FoodOracle.set(villager,Entity.class,"rand",random);
            FoodOracle.set(villager,EntityVillager.class,"randomTickDivider",1000000);
            if(member)FoodOracle.set(villager,EntityVillager.class,"villageObj",village);
            recipes=villager.getRecipes(player);
        }
        void record(String op,int arg)throws Exception {
            List<Object> stock=new ArrayList<Object>();
            for(Object object:recipes){MerchantRecipe r=(MerchantRecipe)object;stock.add(Arrays.asList(r.getToolUses(),r.getMaxTradeUses()));}
            Map<String,Object> state=new LinkedHashMap<String,Object>();
            state.put("op",Arrays.asList(op,arg));state.put("stock",stock);state.put("career",field(villager,"careerId"));state.put("level",field(villager,"careerLevel"));
            state.put("wealth",field(villager,"wealth"));state.put("willing",field(villager,"isWillingToMate"));state.put("timer",field(villager,"timeUntilReset"));state.put("init",field(villager,"needsInitilization"));state.put("buyer",field(villager,"lastBuyingPlayer"));
            state.put("rep",village.getReputationForPlayer(player.getName()));state.put("sounds",new ArrayList<Object>(villager.sounds));state.put("effects",new ArrayList<Object>(villager.effects));state.put("experience",new ArrayList<Integer>(world.experience));state.put("statuses",new ArrayList<Integer>(world.statuses));state.put("tail",VillagerCareerOracle.tail(random));
            steps.add(state);villager.sounds.clear();villager.effects.clear();world.experience.clear();world.statuses.clear();
        }
        void tick(int ticks)throws Exception {for(int i=0;i<ticks;i++)villager.tickTrade();record("tick",ticks);}
        void customer(boolean open)throws Exception {villager.setCustomer(open?player:null);record("customer",open?1:0);}
    }
    static Object run(int profession,int pattern,long seed)throws Exception {
        boolean member=(pattern&1)!=0,rewards=(pattern&2)!=0;
        Case c=new Case(profession,seed,member);
        int selected=pattern%c.recipes.size();MerchantRecipe recipe=(MerchantRecipe)c.recipes.get(selected);
        FoodOracle.set(recipe,MerchantRecipe.class,"rewardsExp",rewards);
        c.record("init",selected);c.customer(true);
        for(int i=0;i<1+pattern%6;i++){c.villager.useRecipe(recipe);c.record("use",selected);}
        int disabled=(selected+1)%c.recipes.size();((MerchantRecipe)c.recipes.get(disabled)).compensateToolUses();c.record("disable",disabled);
        c.tick(8);c.customer(false);c.tick(13);c.customer(true);c.tick(7);c.customer(false);c.tick(26);c.tick(1);c.tick(1);
        return Arrays.asList(profession,Long.toString(seed),member,rewards,c.steps);
    }
    public static void main(String[]args)throws Exception {
        Bootstrap.register();List<Object> rows=new ArrayList<Object>();
        for(int profession=0;profession<5;profession++)for(int pattern=0;pattern<32;pattern++)rows.add(run(profession,pattern,pattern*918273645L+189));
        System.out.println(new GsonBuilder().serializeNulls().create().toJson(rows));
    }
}
