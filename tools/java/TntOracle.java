import com.google.gson.Gson;
import java.io.File;
import java.io.PrintWriter;
import java.lang.reflect.Constructor;
import java.util.*;
import net.minecraft.block.Block;
import net.minecraft.entity.Entity;
import net.minecraft.entity.item.EntityTNTPrimed;
import net.minecraft.init.Bootstrap;
import net.minecraft.server.MinecraftServer;
import net.minecraft.util.*;
import net.minecraft.world.*;

/** Observe the installed game's EntityTNTPrimed tick and Explosion.doExplosionA directly. */
public final class TntOracle {
    static final class TestWorld extends RuntimeUseOracle.Fixture {
        int detonations;
        TestWorld(MinecraftServer host) { super(host); }
        @Override public Explosion newExplosion(Entity e, double x, double y, double z, float power,
                                                  boolean fire, boolean terrain) {
            if (power != 4 || fire || !terrain) throw new AssertionError();
            detonations++;
            return new Explosion(this, e, x, y, z, power, fire, terrain);
        }
    }
    static void put(TestWorld w, int x, int y, int z, int id) {
        w.setBlockState(new BlockPos(x, y, z), Block.getBlockById(id).getDefaultState(), 2);
    }
    public static void main(String[] args) throws Exception {
        Bootstrap.register();
        Constructor<?> ctor = Class.forName("VanillaWorldBridge$Host").getDeclaredConstructor(File.class);
        ctor.setAccessible(true);
        MinecraftServer host = (MinecraftServer)ctor.newInstance(new File(args[1]));
        TestWorld w = new TestWorld(host);
        host.worldServers = new WorldServer[]{w, null, null};
        w.init();
        for (int x=-1;x<=1;x++) for (int z=-1;z<=1;z++) w.theChunkProviderServer.loadChunk(x,z);
        Map<String,Object> data = new LinkedHashMap<>();
        float[] resistance = new float[256];
        for(int i=0;i<256;i++) resistance[i] = Block.getBlockById(i).getExplosionResistance(null);
        data.put("resistance", resistance);
        for (int x=0;x<16;x++) for (int z=0;z<16;z++) put(w,x,5,z,4);
        EntityTNTPrimed t = new EntityTNTPrimed(w,8.5,6,8.5,null);
        t.motionX = 0.0125; t.motionZ = -0.013;
        List<Object> motion = new ArrayList<>();
        for(int i=0;i<81;i++) {
            t.onUpdate();
            motion.add(Arrays.asList(t.posX,t.posY,t.posZ,t.motionX,t.motionY,t.motionZ,t.fuse,t.isDead));
        }
        if(w.detonations!=1) throw new AssertionError();
        data.put("motion",motion);
        List<Object> scenes=new ArrayList<>();
        int[] materials={0,1,3,5,20,46,49,9,7};
        for(int i=0;i<materials.length;i++) {
            for(int x=4;x<13;x++) for(int z=4;z<13;z++) {
                put(w,x,17,z,materials[i]);
                put(w,x,16,z,1);
                put(w,x,18,z,x==9 ? materials[i] : 0);
            }
            w.rand.setSeed(4189+i);
            Explosion e = new Explosion(w,null,8.25,18.06125,8.5,4,false,true);
            e.doExplosionA();
            List<int[]> hits = new ArrayList<>();
            for(BlockPos p : e.getAffectedBlockPositions()) hits.add(new int[]{p.getX(),p.getY(),p.getZ()});
            float exposure=w.getBlockDensity(new Vec3(8.25,18.06125,8.5),new AxisAlignedBB(10,17,8,10.6,18.8,8.6));
            scenes.add(Arrays.asList(materials[i],hits,exposure));
        }
        data.put("blasts",scenes);
        try(PrintWriter out=new PrintWriter(args[0],"UTF-8")){out.print(new Gson().toJson(data));}
    }
}
