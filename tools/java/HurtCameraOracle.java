import com.google.gson.Gson;
import java.io.File;
import java.io.Writer;
import java.lang.invoke.MethodHandle;
import java.lang.reflect.Method;
import java.nio.FloatBuffer;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.List;
import net.minecraft.client.Minecraft;
import net.minecraft.client.entity.EntityPlayerSP;
import net.minecraft.client.renderer.EntityRenderer;
import net.minecraft.client.renderer.GlStateManager;
import net.minecraft.client.renderer.ItemRenderer;
import net.minecraft.world.WorldSettings;
import net.minecraft.world.WorldType;
import org.lwjgl.BufferUtils;
import org.lwjgl.opengl.GL11;

/** Sample the installed game's camera matrices, including the matrix received by the held item. */
public final class HurtCameraOracle {
    static float[] modelView() {
        FloatBuffer data = BufferUtils.createFloatBuffer(16);
        GL11.glGetFloat(GL11.GL_MODELVIEW_MATRIX, data);
        float[] result = new float[16];
        data.get(result);
        return result;
    }

    static final class HandProbe extends ItemRenderer {
        float[] matrix;
        HandProbe(Minecraft mc) { super(mc); }
        @Override public void renderItemInFirstPerson(float partial) { matrix = modelView(); }
        @Override public void renderOverlays(float partial) {}
    }

    static Method method(String name, Class<?>... arguments) throws ReflectiveOperationException {
        Method m = EntityRenderer.class.getDeclaredMethod(name, arguments);
        m.setAccessible(true);
        return m;
    }

    static final class Sample {
        int hurt, max, death, perspective;
        float yaw, health, partial, fov_scale;
        float[] effect, world_base, world, hand;
    }

    final VanillaFrameBenchmark.Client mc;
    final EntityRenderer renderer;
    final HandProbe hand;
    final Method hurt = method("hurtCameraEffect", float.class);
    final Method setup = method("setupCameraTransform", float.class, int.class);
    final Method renderHand = method("renderHand", float.class, int.class);
    final Method fov = method("getFOVModifier", float.class, boolean.class);
    final List<Sample> samples = new ArrayList<>();

    HurtCameraOracle(VanillaFrameBenchmark.Client client) throws Exception {
        mc = client;
        mc.gameSettings.viewBobbing = false;
        mc.gameSettings.anaglyph = false;
        mc.gameSettings.hideGUI = false;
        renderer = mc.entityRenderer;
        hand = new HandProbe(mc);
        MenuOracle.field(EntityRenderer.class, "itemRenderer").set(renderer, hand);
        EntityPlayerSP p = mc.thePlayer;
        p.setPositionAndRotation(8.5, 120, 8.5, 38f, -17f);
        p.prevPosX = p.lastTickPosX = p.posX;
        p.prevPosY = p.lastTickPosY = p.posY;
        p.prevPosZ = p.lastTickPosZ = p.posZ;
        p.prevRotationYaw = p.rotationYaw;
        p.prevRotationPitch = p.rotationPitch;
        p.prevTimeInPortal = p.timeInPortal = 0;
    }

    void sample(int ticks, int maximum, float angle, int deathTicks, float health, float partial,
                int perspective) throws ReflectiveOperationException {
        Sample row = new Sample();
        row.hurt = ticks; row.max = maximum; row.yaw = angle; row.death = deathTicks;
        row.health = health; row.partial = partial; row.perspective = perspective;
        EntityPlayerSP p = mc.thePlayer;
        p.setHealth(20);
        p.hurtTime = p.deathTime = 0;
        p.maxHurtTime = 10;
        p.attackedAtYaw = 0;
        mc.gameSettings.thirdPersonView = perspective;
        setup.invoke(renderer, partial, 0);
        row.world_base = modelView();
        float baseFov = (Float)fov.invoke(renderer, partial, false);

        p.setHealth(health);
        p.hurtTime = ticks;
        p.maxHurtTime = maximum;
        p.attackedAtYaw = angle;
        p.deathTime = deathTicks;
        GlStateManager.matrixMode(GL11.GL_MODELVIEW);
        GlStateManager.loadIdentity();
        hurt.invoke(renderer, partial);
        row.effect = modelView();
        setup.invoke(renderer, partial, 0);
        row.world = modelView();
        hand.matrix = null;
        renderHand.invoke(renderer, partial, 0);
        row.hand = hand.matrix;
        if ((perspective == 0) != (row.hand != null)) throw new AssertionError("Hand visibility");
        row.fov_scale = (Float)fov.invoke(renderer, partial, false) / baseFov;
        samples.add(row);
    }

    static void run(String[] args) throws Throwable {
        VanillaFrameBenchmark.Client mc = MenuOracle.startClient(new File(args[0]), new File(args[1]), "1.8.9 hurt camera oracle");
        mc.gameSettings.renderDistanceChunks = 2;
        mc.launchIntegratedServer("fixture", "Fixture", new WorldSettings(189, WorldSettings.GameType.SURVIVAL, false, false, WorldType.FLAT));
        MethodHandle loop = VanillaFrameBenchmark.privateMethod(mc, "runGameLoop");
        long start = System.nanoTime();
        while (mc.thePlayer == null || mc.currentScreen != null || mc.getRenderViewEntity() != mc.thePlayer) {
            VanillaFrameBenchmark.runFrame(loop);
            if (System.nanoTime() - start > 75000000000L) throw new IllegalStateException("No player");
        }
        HurtCameraOracle oracle = new HurtCameraOracle(mc);
        for (int perspective = 0; perspective < 3; perspective++) {
            for (int tick = 10; tick >= 0; tick--) {
                for (float alpha : new float[]{0f, .375f, 1f}) {
                    oracle.sample(tick, 10, 0f, 0, 18f, alpha, perspective);
                }
            }
            oracle.sample(9, 10, 0, 1, 0f, .5f, perspective);
            oracle.sample(5, 10, 0, 5, 0f, .5f, perspective);
            oracle.sample(0, 0, 0, 0, 20f, 0, perspective);
            for (int death : new int[]{0, 1, 10, 20}) {
                oracle.sample(0, 10, 0, death, 0f, .375f, perspective);
            }
        }
        try (Writer writer = Files.newBufferedWriter(Paths.get(args[2]))) {
            new Gson().toJson(oracle.samples, writer);
        }
    }

    public static void main(String[] args) {
        try { run(args); System.exit(0); }
        catch (Throwable t) { t.printStackTrace(); System.exit(1); }
    }
}
