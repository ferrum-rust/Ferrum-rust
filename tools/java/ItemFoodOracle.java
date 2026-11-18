// Original ItemFood finish/right-click and EntityPlayer use-countdown bytecode.
import com.google.gson.Gson;
import java.lang.reflect.*;
import java.util.*;
import sun.misc.Unsafe;
import net.minecraft.init.*;
import net.minecraft.entity.*;
import net.minecraft.item.*;
import net.minecraft.potion.*;
import net.minecraft.world.*;
import net.minecraft.world.storage.WorldInfo;

public final class ItemFoodOracle {
    static final class Boundary extends RuntimeException {}
    static final class Player extends FoodOracle.Player {
        boolean stopAfterUse;List<Object> added=new ArrayList<Object>();List<Integer> animations=new ArrayList<Integer>();
        Player(World world){super(world);}
        @Override public boolean isPlayerSleeping(){if(stopAfterUse)throw new Boundary();return super.isPlayerSleeping();}
        @Override protected void updateItemUse(ItemStack stack,int count){animations.add(count);}
        @Override public void addPotionEffect(PotionEffect effect){added.add(Arrays.asList(effect.getPotionID(),effect.getDuration(),effect.getAmplifier()));super.addPotionEffect(effect);}
        void usePrefix(){stopAfterUse=true;try{onUpdate();throw new AssertionError("Missing use boundary");}catch(Boundary stop){}finally{stopAfterUse=false;}}
    }
    static final class OracleWorld extends FoodOracle.OracleWorld {
        List<Long> pitches=new ArrayList<Long>();
        @Override public void playSoundAtEntity(Entity e,String sound,float volume,float pitch){if(sound.equals("random.burp"))pitches.add(FoodOracle.bits(pitch));}
    }
    static Player player(long seed,boolean immune,boolean remote,int level)throws Exception {
        Field f=Unsafe.class.getDeclaredField("theUnsafe");f.setAccessible(true);OracleWorld w=(OracleWorld)((Unsafe)f.get(null)).allocateInstance(OracleWorld.class);w.pitches=new ArrayList<Long>();
        w.difficulty=EnumDifficulty.NORMAL;w.rules=new GameRules();FoodOracle.set(w,World.class,"rand",new Random(seed));FoodOracle.set(w,World.class,"isRemote",remote);FoodOracle.set(w,World.class,"provider",new WorldProviderSurface());FoodOracle.set(w,World.class,"worldInfo",new WorldInfo(new WorldSettings(189L,WorldSettings.GameType.SURVIVAL,false,false,WorldType.DEFAULT),"ItemFoodOracle"));
        Player p=new Player(w);p.capabilities.disableDamage=immune;p.capabilities.isCreativeMode=immune;p.getFoodStats().readNBT(FoodOracle.food(level,0,0f,0f,0));return p;
    }
    static Object stack(ItemStack s){return s==null?null:Arrays.asList(Item.getIdFromItem(s.getItem()),s.getMetadata(),s.stackSize);}
    static Object useState(Player p){return Arrays.asList(p.isUsingItem(),p.getItemInUseCount(),stack(p.inventory.getCurrentItem()));}
    static Map<String,Object> result(Player p){Map<String,Object> out=new LinkedHashMap<String,Object>();out.put("food",FoodOracle.snapshot(p));out.put("effects",p.added);out.put("pitches",((OracleWorld)p.worldObj).pitches);out.put("tail",Long.toString(p.worldObj.rand.nextLong()));out.put("absorption",FoodOracle.bits(p.getAbsorptionAmount()));return out;}
    static Map<String,Object> finish(int id,int meta,long seed,boolean immune,boolean remote,int level)throws Exception{
        Player p=player(seed,immune,remote,level);ItemStack stack=new ItemStack(Item.getItemById(id),2,meta);ItemStack returned=stack.onItemUseFinish(p.worldObj,p);
        Map<String,Object> out=result(p);out.put("id",id);out.put("meta",meta);out.put("seed",Long.toString(seed));out.put("immune",immune);out.put("remote",remote);out.put("level",level);out.put("returned",stack(returned));return out;
    }
    static Map<String,Object> use(int id,int meta,boolean immune,boolean remote,int level,int pattern)throws Exception{
        Player p=player(189L,immune,remote,level);ItemStack held=new ItemStack(Item.getItemById(id),2,meta);p.inventory.mainInventory[0]=held;held.useItemRightClick(p.worldObj,p);
        List<Object> steps=new ArrayList<Object>();steps.add(useState(p));
        for(int tick=1;tick<=40;tick++){
            if(tick==12){
                if(pattern==1)p.inventory.mainInventory[0]=held.copy();
                if(pattern==2){p.inventory.mainInventory[1]=held;p.inventory.currentItem=1;}
                if(pattern==3)held.stackSize++;
                if(pattern==4)p.stopUsingItem();
                if(pattern==5)p.handleStatusUpdate((byte)9);
            }
            p.usePrefix();steps.add(useState(p));
        }
        Map<String,Object> out=result(p);out.put("id",id);out.put("meta",meta);out.put("immune",immune);out.put("remote",remote);out.put("level",level);out.put("pattern",pattern);out.put("steps",steps);out.put("animations",p.animations);return out;
    }
    static Object field(ItemFood food,String name)throws Exception{Field f=ItemFood.class.getDeclaredField(name);f.setAccessible(true);return f.get(food);}
    public static void main(String[] args)throws Exception{
        Bootstrap.register();List<Integer> ids=new ArrayList<Integer>();List<Object> properties=new ArrayList<Object>();
        for(Object item:Item.itemRegistry)if(item instanceof ItemFood){ItemFood food=(ItemFood)item;int id=Item.getIdFromItem(food);ids.add(id);properties.add(Arrays.asList(id,field(food,"potionId"),field(food,"potionDuration"),field(food,"potionAmplifier"),FoodOracle.bits((Float)field(food,"potionEffectProbability"))));}
        List<Object> finish=new ArrayList<Object>();for(int id:ids)for(int meta:new int[]{0,1,2,3,15})for(long seed:new long[]{0L,189L,-741L,107038380838084L})for(boolean immune:new boolean[]{false,true})for(boolean remote:new boolean[]{false,true})for(int level:new int[]{5,20})finish.add(finish(id,meta,seed,immune,remote,level));
        for(int id:new int[]{322,367,375,394,365})for(int seed=0;seed<128;seed++)finish.add(finish(id,0,seed*7919L,false,false,5));
        List<Object> use=new ArrayList<Object>();for(int[] item:new int[][]{{260,0},{322,0},{322,1},{282,0},{349,3}})for(boolean immune:new boolean[]{false,true})for(boolean remote:new boolean[]{false,true})for(int level:new int[]{0,20})for(int pattern=0;pattern<6;pattern++)use.add(use(item[0],item[1],immune,remote,level,pattern));
        Map<String,Object> out=new LinkedHashMap<String,Object>();out.put("properties",properties);out.put("finish",finish);out.put("use",use);System.out.println(new Gson().toJson(out));
    }
}
