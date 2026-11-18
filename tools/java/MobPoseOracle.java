import com.google.gson.Gson;
import java.io.*;
import java.nio.file.*;
import java.util.*;
import net.minecraft.client.model.*;
import net.minecraft.client.renderer.entity.*;
import net.minecraft.entity.*;
import net.minecraft.entity.boss.EntityDragon;
import net.minecraft.entity.monster.EntityGuardian;
import net.minecraft.entity.passive.*;
import net.minecraft.world.*;

/** Original articulated meshes for animations driven by entity state rather than limbSwing. */
public final class MobPoseOracle {
    public static void main(String[] args) {
        try {
            VanillaFrameBenchmark.Client mc = MenuOracle.startClient(new File(args[0]), new File(args[1]), "1.8.9 creature poses");
            mc.gameSettings.renderDistanceChunks = 2;
            mc.launchIntegratedServer("fixture", "Fixture", new WorldSettings(189, WorldSettings.GameType.CREATIVE, false, false, WorldType.FLAT));
            java.lang.invoke.MethodHandle loop = VanillaFrameBenchmark.privateMethod(mc, "runGameLoop");
            while (mc.thePlayer == null || mc.currentScreen != null || mc.getRenderManager().livingPlayer != mc.thePlayer)
                VanillaFrameBenchmark.runFrame(loop);
            Map<Integer,Object> data = new TreeMap<>();
            for (int id : new int[]{54,55,56,60,61,63,64,65,67,68,94,101}) {
                EntityLivingBase entity = (EntityLivingBase)EntityList.createEntityByID(id,mc.theWorld);
                RendererLivingEntity renderer = (RendererLivingEntity)mc.getRenderManager().getEntityRenderObject(entity);
                ModelBase model = id == 55 ? new ModelSlime(0)
                    : id == 54 ? new ModelZombieVillager() : renderer.getMainModel();
                List<Object> frames = new ArrayList<>();
                int samples = id == 55 ? 1 : id == 54 ? 9 : id == 61 ? 80 : 8;
                for (int i=0; i<samples; i++) {
                    float phase = (float)i/samples, angle = 0;
                    float speed = 0;
                    if (id == 54) {
                        angle = (float)((i-1)*Math.PI/4/0.6662);
                        speed = i==0 ? 0 : 1;
                    } else if (id == 56 || id == 60 || id == 64 || id == 67) {
                        float frequency = id == 56 ? .3f : id == 64 ? .1f : .9f;
                        angle = (float)(phase*Math.PI*2/frequency);
                    } else if (id == 61) {
                        angle = phase*200; // common period of the three rod orbits
                    } else if (id == 63) {
                        EntityDragon dragon = (EntityDragon)entity;
                        dragon.prevAnimTime = dragon.animTime = phase;
                    } else if (id == 65) {
                        ((EntityBat)entity).setIsBatHanging(false);
                        angle = (float)(phase*Math.PI*2/1.3);
                    } else if (id == 68) {
                        float tailPhase = (float)(phase*Math.PI*2);
                        MenuOracle.field(EntityGuardian.class, "field_175484_c").setFloat(entity,tailPhase);
                        MenuOracle.field(EntityGuardian.class, "field_175482_b").setFloat(entity,tailPhase);
                    } else if (id == 94) {
                        angle = phase < .5f ? (float)(Math.sin(4*phase*phase*Math.PI)*Math.PI*.25) : 0;
                    } else if (id == 101) {
                        MenuOracle.field(EntityRabbit.class,"field_175535_bn").setInt(entity,10);
                        angle = phase*10;
                    }
                    frames.add(RemoteMobOracle.frame(entity,renderer,model,angle,speed));
                }
                data.put(id,frames);
            }
            try (Writer out = Files.newBufferedWriter(Paths.get(args[2]))) { new Gson().toJson(data,out); }
            System.exit(0);
        } catch(Throwable t) { t.printStackTrace(); System.exit(1); }
    }
}
