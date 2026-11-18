import com.google.gson.Gson;
import java.lang.reflect.*;
import java.nio.file.*;
import java.util.*;
import net.minecraft.block.material.Material;
import net.minecraft.entity.Entity;
import net.minecraft.entity.item.*;
import net.minecraft.entity.projectile.EntityFishHook;
import net.minecraft.init.*;
import net.minecraft.item.*;
import net.minecraft.enchantment.Enchantment;
import net.minecraft.nbt.*;
import net.minecraft.util.*;
import net.minecraft.world.*;
import net.minecraft.world.storage.WorldInfo;
import sun.misc.Unsafe;

/** Runs the installed entity bytecode with empty-world collision callbacks. */
public final class FishingEyeOracle {
    static Field field(Class<?> owner,String name)throws Exception {Field f=owner.getDeclaredField(name);f.setAccessible(true);return f;}
    static Random random(Entity entity)throws Exception {return (Random)field(Entity.class,"rand").get(entity);}
    static class Space extends FoodOracle.OracleWorld {
        @Override public boolean spawnEntityInWorld(Entity e){spawned.add(e);return true;}
        List<Entity> spawned;
        @Override public List<Entity> getEntitiesWithinAABBExcludingEntity(Entity e,AxisAlignedBB b){return Collections.emptyList();}
        @Override public MovingObjectPosition rayTraceBlocks(Vec3 a,Vec3 b){return null;}
        @Override public boolean isAABBInMaterial(AxisAlignedBB b,Material m){return false;}
        @Override public void spawnParticle(EnumParticleTypes kind,double x,double y,double z,double vx,double vy,double vz,int... args){}
        @Override public void playAuxSFX(int id,BlockPos at,int data){}
    }
    static Space space()throws Exception {
        Field u=Unsafe.class.getDeclaredField("theUnsafe");u.setAccessible(true);
        Space w=(Space)((Unsafe)u.get(null)).allocateInstance(Space.class);
        w.rules=new GameRules();w.difficulty=EnumDifficulty.NORMAL;w.spawned=new ArrayList<>();
        FoodOracle.set(w,World.class,"rand",new Random(189));
        FoodOracle.set(w,World.class,"provider",new WorldProviderSurface());
        FoodOracle.set(w,World.class,"worldInfo",new WorldInfo(new WorldSettings(189,WorldSettings.GameType.SURVIVAL,false,false,WorldType.DEFAULT),"FishingEyeOracle"));
        return w;
    }
    static class Eye extends EntityEnderEye {
        Eye(World w,double x,double y,double z){super(w,x,y,z);}
        @Override public void onEntityUpdate(){}
        @Override public boolean isInWater(){return false;}
    }
    static long xorshift(long s){s^=s<<13;s^=s>>>7;s^=s<<17;return s;}
    static class Hook extends EntityFishHook {
        Hook(World w,FoodOracle.Player p){super(w,p);}
        @Override public void onEntityUpdate(){}
        @Override public void moveEntity(double x,double y,double z){setPosition(posX+x,posY+y,posZ+z);}
        @Override public void handleHookCasting(double x,double y,double z,float speed,float spread){
            try{random(this).setSeed(xorshift(189));}catch(Exception e){throw new RuntimeException(e);}
            super.handleHookCasting(x,y,z,speed,spread);
        }
    }
    static double[] pose(Entity e){return new double[]{e.posX,e.posY,e.posZ,e.motionX,e.motionY,e.motionZ};}
    public static void main(String[] args)throws Exception {
        Bootstrap.register();Map<String,Object> output=new LinkedHashMap<>();
        List<Object> eyes=new ArrayList<>();
        for(int[] target:new int[][]{{108,20,8},{-84,12,-76},{9,20,10},{8,65,8}}){
            Space w=space();Eye e=new Eye(w,8,64,8);random(e).setSeed(189);e.moveTowards(new BlockPos(target[0],target[1],target[2]));
            List<double[]> frames=new ArrayList<>();frames.add(pose(e));for(int tick=0;tick<81;tick++){e.onUpdate();frames.add(pose(e));}
            Map<String,Object> row=new LinkedHashMap<>();row.put("target",target);row.put("frames",frames);row.put("drop",!w.spawned.isEmpty());row.put("dead",e.isDead);eyes.add(row);
        }
        List<Object> hooks=new ArrayList<>();
        for(float[] aim:new float[][]{{0,0},{90,20},{-135,-30},{179,60}}){
            Space w=space();FoodOracle.Player p=new FoodOracle.Player(w);p.setPosition(8.5,64,8.5);p.rotationYaw=aim[0];p.rotationPitch=aim[1];p.inventory.mainInventory[0]=new ItemStack(Items.fishing_rod);
            Hook h=new Hook(w,p);List<double[]> frames=new ArrayList<>();frames.add(pose(h));for(int tick=0;tick<10;tick++){h.onUpdate();frames.add(pose(h));}
            Map<String,Object> row=new LinkedHashMap<>();row.put("aim",aim);row.put("frames",frames);hooks.add(row);
        }
        List<Object> catches=new ArrayList<>();
        Method catchItem=EntityFishHook.class.getDeclaredMethod("getFishingResult");catchItem.setAccessible(true);
        for(int enchant:new int[]{0,3})for(final float roll:new float[]{0,0.125f,0.5f})for(int seed=0;seed<16;seed++){
            Space w=space();FoodOracle.set(w,World.class,"rand",new Random(){@Override public float nextFloat(){return roll;}});
            FoodOracle.Player p=new FoodOracle.Player(w);ItemStack rod=new ItemStack(Items.fishing_rod);p.inventory.mainInventory[0]=rod;
            if(enchant>0){rod.addEnchantment(Enchantment.getEnchantmentById(61),enchant);rod.addEnchantment(Enchantment.getEnchantmentById(62),enchant);}
            Hook h=new Hook(w,p);random(h).setSeed(seed);
            ItemStack caught=(ItemStack)catchItem.invoke(h);
            List<int[]> enchants=new ArrayList<>();
            if(caught.hasTagCompound()){
                NBTTagList tags=caught.getTagCompound().getTagList(caught.getItem()==Items.enchanted_book?"StoredEnchantments":"ench",10);
                for(int i=0;i<tags.tagCount();i++)enchants.add(new int[]{tags.getCompoundTagAt(i).getShort("id"),tags.getCompoundTagAt(i).getShort("lvl")});
            }
            Map<String,Object> row=new LinkedHashMap<>();row.put("level",enchant);row.put("roll",roll);row.put("seed",seed);
            row.put("stack",new int[]{Item.getIdFromItem(caught.getItem()),caught.stackSize,caught.getItemDamage()});row.put("enchants",enchants);catches.add(row);
        }
        output.put("eyes",eyes);output.put("hooks",hooks);output.put("catches",catches);
        Files.write(Paths.get(args[0]),new Gson().toJson(output).getBytes("UTF-8"));
    }
}
