import com.google.gson.Gson;
import java.lang.reflect.Field;
import java.util.*;
import sun.misc.Unsafe;
import net.minecraft.entity.*;
import net.minecraft.entity.projectile.*;
import net.minecraft.entity.item.EntityExpBottle;
import net.minecraft.init.*;
import net.minecraft.util.*;
import net.minecraft.world.*;
import net.minecraft.world.storage.WorldInfo;

/** Original projectile constructors, heading RNG and onUpdate travel callbacks. */
public final class ProjectileMotionOracle {
    static long headingSeed;
    static class Space extends FoodOracle.OracleWorld {
        @Override public List<Entity> getEntitiesWithinAABBExcludingEntity(Entity entity,AxisAlignedBB box) {
            return Collections.emptyList();
        }
        @Override public MovingObjectPosition rayTraceBlocks(Vec3 start,Vec3 end) { return null; }
        @Override public MovingObjectPosition rayTraceBlocks(Vec3 start,Vec3 end,boolean liquids,boolean empty,boolean last) { return null; }
        @Override public void spawnParticle(EnumParticleTypes particle,double x,double y,double z,double vx,double vy,double vz,int... data) {}
    }
    static Space space() throws Exception {
        Field field=Unsafe.class.getDeclaredField("theUnsafe");
        field.setAccessible(true);
        Space world=(Space)((Unsafe)field.get(null)).allocateInstance(Space.class);
        world.rules=new GameRules();
        world.difficulty=EnumDifficulty.NORMAL;
        FoodOracle.set(world,World.class,"rand",new Random(189));
        FoodOracle.set(world,World.class,"provider",new WorldProviderSurface());
        FoodOracle.set(world,World.class,"worldInfo",new WorldInfo(
            new WorldSettings(189,WorldSettings.GameType.SURVIVAL,false,false,WorldType.DEFAULT),"ProjectileMotionOracle"));
        return world;
    }
    static void previous(Entity entity) {
        entity.prevPosX=entity.posX;
        entity.prevPosY=entity.posY;
        entity.prevPosZ=entity.posZ;
        entity.prevRotationYaw=entity.rotationYaw;
        entity.prevRotationPitch=entity.rotationPitch;
    }
    static final class Arrow extends EntityArrow {
        boolean water;
        Arrow(World world,EntityLivingBase player,float velocity) { super(world,player,velocity); }
        @Override public void setThrowableHeading(double x,double y,double z,float velocity,float spread) {
            rand.setSeed(headingSeed);
            super.setThrowableHeading(x,y,z,velocity,spread);
        }
        @Override public void onEntityUpdate() { previous(this); }
        @Override public boolean isInWater() { return water; }
        @Override public boolean isWet() { return water; }
        @Override protected void doBlockCollisions() {}
    }
    static final class Ball extends EntitySnowball {
        boolean water;
        Ball(World world,EntityLivingBase player) { super(world,player); }
        @Override public void setThrowableHeading(double x,double y,double z,float velocity,float spread) {
            rand.setSeed(headingSeed);
            super.setThrowableHeading(x,y,z,velocity,spread);
        }
        @Override public void onEntityUpdate() { previous(this); }
        @Override public boolean isInWater() { return water; }
    }
    static double[] frame(Entity entity) {
        return new double[]{entity.posX,entity.posY,entity.posZ,entity.motionX,entity.motionY,entity.motionZ,
            entity.rotationYaw,entity.rotationPitch};
    }
    static final class Bottle extends EntityExpBottle {
        boolean water;
        Bottle(World world, EntityLivingBase player) { super(world, player); }
        @Override public void setThrowableHeading(double x,double y,double z,float velocity,float spread) {
            rand.setSeed(headingSeed);super.setThrowableHeading(x,y,z,velocity,spread);
        }
        @Override public void onEntityUpdate() { previous(this); }
        @Override public boolean isInWater() { return water; }
    }
    static Object run(long seed,float[] look,boolean water,float constructorSpeed) throws Exception {
        Space world=space();
        FoodOracle.Player player=new FoodOracle.Player(world);
        player.setLocationAndAngles(8.123456789,64.5,-4.333333333,look[0],look[1]);
        headingSeed=seed;
        Entity projectile;
        if (constructorSpeed<0) {
            Bottle bottle=new Bottle(world,player);bottle.water=water;projectile=bottle;
        } else if (constructorSpeed==0) {
            Ball ball=new Ball(world,player);
            ball.water=water;
            projectile=ball;
        } else {
            Arrow arrow=new Arrow(world,player,constructorSpeed);
            arrow.water=water;
            projectile=arrow;
        }
        List<double[]> frames=new ArrayList<>();
        frames.add(frame(projectile));
        for(int tick=0;tick<20;tick++) {
            projectile.onUpdate();
            frames.add(frame(projectile));
        }
        Map<String,Object> result=new LinkedHashMap<>();
        result.put("kind",constructorSpeed<0 ? "experience_bottle" : constructorSpeed==0 ? "snowball" : "arrow");
        result.put("seed",seed);
        result.put("player",new double[]{player.posX,player.posY,player.posZ});
        result.put("look",look);
        result.put("speed",constructorSpeed<0 ? 0.7f : constructorSpeed==0 ? 1.5f : constructorSpeed*1.5f);
        result.put("water",water);
        result.put("frames",frames);
        return result;
    }
    public static void main(String[] args) throws Exception {
        Bootstrap.register();
        List<Object> cases=new ArrayList<>();
        for(long seed:new long[]{189,-741}) {
            for(float[] look:new float[][]{{0,0},{37.25f,-18.5f},{-90,89.5f},{180,-89.5f}}) {
                for(boolean water:new boolean[]{false,true}) {
                    for(float speed:new float[]{0,0.2f,1f,2f,-1}) cases.add(run(seed,look,water,speed));
                }
            }
        }
        System.out.println(new Gson().toJson(cases));
    }
}
