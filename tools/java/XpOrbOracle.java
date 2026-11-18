import com.google.gson.Gson;
import com.mojang.authlib.GameProfile;
import java.io.*;
import java.util.*;
import java.util.zip.GZIPOutputStream;
import net.minecraft.block.state.IBlockState;
import net.minecraft.entity.Entity;
import net.minecraft.entity.item.EntityXPOrb;
import net.minecraft.entity.player.EntityPlayer;
import net.minecraft.init.Bootstrap;
import net.minecraft.util.*;

/** Runs original EntityXPOrb constructors, update, targeting and player pickup. */
public final class XpOrbOracle {
    static final class World extends FallingEntityOracle.TestWorld {
        final int layout;
        int time;
        World(int layout) { super(0); this.layout = layout; }
        public IBlockState getBlockState(BlockPos p) {
            return FallingEntityOracle.state(ItemMotionOracle.initial(p.getX(), p.getY(), p.getZ(), layout));
        }
        public boolean isRainingAt(BlockPos p) { return layout == 10 && time > 4; }
    }
    static final class Player extends EntityPlayer {
        boolean spectator;
        Player(World world) { super(world, new GameProfile(new UUID(0, 189), "Player")); }
        public boolean isSpectator() { return spectator; }
        public void onItemPickup(Entity entity, int count) {
            ((World)worldObj).events.add(Arrays.asList("pickup", count));
        }
    }
    static final class Orb extends EntityXPOrb {
        Orb(World world, double x, double y, double z, int value, long seed) {
            super(world, x, y, z, value);
            rand.setSeed(seed);
        }
        long state() throws Exception { return ItemEntityOracle.seed(rand); }
        boolean web() { return isInWeb; }
    }
    static Object snapshot(Orb e, World w) throws Exception {
        AxisAlignedBB b = e.getEntityBoundingBox();
        return Arrays.asList(
            FallingEntityOracle.bits(e.posX,e.posY,e.posZ,e.prevPosX,e.prevPosY,e.prevPosZ,e.motionX,e.motionY,e.motionZ,b.minX,b.minY,b.minZ,b.maxX,b.maxY,b.maxZ),
            e.xpOrbAge, e.delayBeforeCanPickup, e.xpColor,
            ItemEntityOracle.field(EntityXPOrb.class,"xpTargetColor").getInt(e),
            ItemEntityOracle.field(EntityXPOrb.class,"closestPlayer").get(e) != null,
            ItemEntityOracle.field(EntityXPOrb.class,"xpOrbHealth").getInt(e),
            ItemEntityOracle.field(Entity.class,"fire").getInt(e),
            e.onGround, e.web(), e.isDead, Long.toString(e.state()), new ArrayList<Object>(w.events));
    }
    static Object cases(Random math) throws Exception {
        List<Object> result=new ArrayList<>();
        for (int layout=0; layout<16; layout++) for (int mode=0; mode<5; mode++) {
            World w=new World(layout);
            Player p=new Player(w);
            w.playerEntities.add(p);
            double x=layout==12?-2.25:8.5, y=layout==13?-62:layout==11?3.125:mode==2?2.5:6., z=8.5;
            long seed=-189L+layout*31+mode*7;
            int id=mode==4?-1:mode*26+layout;
            math.setSeed(seed^137);
            Orb e=new Orb(w,x,y,z,3+mode*2,seed);
            e.setEntityId(id);
            List<String> generated=FallingEntityOracle.bits(e.motionX,e.motionY,e.motionZ);
            if(mode==1){e.motionX=.45;e.motionY=-.12;e.motionZ=-.22;}
            if(mode==2){e.motionX=-.18;e.motionY=.16;e.motionZ=.05;}
            if(mode==3){e.motionX=0;e.motionY=0;e.motionZ=0;}
            e.xpOrbAge=mode==3?5991:0;
            e.delayBeforeCanPickup=mode+1;
            if(layout==10) ItemEntityOracle.field(Entity.class,"fire").setInt(e,60);
            Map<String,Object> row=new LinkedHashMap<>();
            row.put("layout",layout);row.put("mode",mode);row.put("id",id);row.put("seed",Long.toString(seed));
            row.put("position",new double[]{x,y,z});row.put("generated",generated);
            row.put("yaw",Integer.toUnsignedLong(Float.floatToRawIntBits(e.rotationYaw)));
            row.put("math",Long.toString(ItemEntityOracle.seed(math)));
            row.put("motion",new double[]{e.motionX,e.motionY,e.motionZ});
            List<Object> frames=new ArrayList<>();
            for(int t=1;t<=80 && !e.isDead;t++){
                w.time=t;
                p.setPosition(x+(t<=12?10:mode==4?8:t<=47?4.2:1.8), y-2.,z+1.);
                p.spectator=mode==2&&t>=30&&t<=40;
                p.setSneaking(mode==1&&t>=50);
                w.events.clear();
                e.ticksExisted++;
                e.onUpdate();
                frames.add(snapshot(e,w));
            }
            row.put("frames",frames);
            result.add(row);
        }
        return result;
    }
    static Object pickups(Random math) throws Exception {
        List<Object> rows=new ArrayList<>();
        for(int i=0; i<512;i++) {
            World w=new World(0);
            Player p=new Player(w);
            int value=new int[]{-5,0,1,3,7,17,37,73,149,307,617,1237,2477,9000}[i%14];
            long seed=i*8172781L-189;
            math.setSeed(seed);
            Orb e=new Orb(w,0,2,0,value,seed);
            p.experienceLevel=new int[]{0,4,14,15,29,30,34}[i%7];
            p.experienceTotal=i%19==0?Integer.MAX_VALUE-5:i*3;
            p.experience=new float[]{0f,.25f,.95f,.99999994f}[i%4];
            p.xpCooldown=i%5==0?1:0;e.delayBeforeCanPickup=i%7==0?2:0;
            p.ticksExisted=145;
            List<Object> before=Arrays.asList(p.experienceLevel,p.experienceTotal,Integer.toUnsignedLong(Float.floatToRawIntBits(p.experience)),p.xpCooldown,e.delayBeforeCanPickup);
            w.events.clear();e.onCollideWithPlayer(p);
            rows.add(Arrays.asList(value,Long.toString(seed),before,p.experienceLevel,p.experienceTotal,Integer.toUnsignedLong(Float.floatToRawIntBits(p.experience)),p.getScore(),p.xpCooldown,e.isDead,Long.toString(e.state()),new ArrayList<Object>(w.events)));
        }
        return rows;
    }
    public static void main(String[] args) throws Exception {
        Bootstrap.register();
        Random math=(Random)ItemEntityOracle.field(Class.forName("java.lang.Math$RandomNumberGeneratorHolder"),"randomNumberGenerator").get(null);
        Map<String,Object> output=new LinkedHashMap<>();
        output.put("cases",cases(math));output.put("pickups",pickups(math));
        try(Writer writer=new OutputStreamWriter(new GZIPOutputStream(new FileOutputStream(args[0])),"UTF-8")){
            new Gson().toJson(output,writer);
        }
        System.out.println("Original XP orb 80 motion scenarios / 512 pickup cases");
    }
}
