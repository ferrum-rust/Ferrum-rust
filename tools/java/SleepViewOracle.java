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
import net.minecraft.client.entity.EntityPlayerSP;
import net.minecraft.client.renderer.EntityRenderer;
import net.minecraft.client.renderer.GlStateManager;
import net.minecraft.client.renderer.entity.RenderManager;
import net.minecraft.client.renderer.entity.RenderPlayer;
import net.minecraft.entity.player.EntityPlayer;
import net.minecraft.init.Blocks;
import net.minecraft.util.BlockPos;
import net.minecraft.util.EnumFacing;
import net.minecraft.world.WorldSettings;
import net.minecraft.world.WorldType;
import org.lwjgl.BufferUtils;
import org.lwjgl.opengl.GL11;

/** Capture the installed client's model and view transforms while its player actually uses a bed. */
public final class SleepViewOracle {
    static float[] matrix() {
        FloatBuffer data = BufferUtils.createFloatBuffer(16);
        GL11.glGetFloat(GL11.GL_MODELVIEW_MATRIX, data);
        float[] result = new float[16];
        data.get(result);
        return result;
    }
    static final class PlayerProbe extends RenderPlayer {
        PlayerProbe(RenderManager manager) { super(manager); }
        float[] sleepingBody(EntityPlayerSP player) {
            GlStateManager.loadIdentity();
            renderLivingAt(player, 0, 0, 0);
            rotateCorpse(player, 10, 47, .625f);
            return matrix();
        }
    }
    static final class Sample {
        int facing, perspective;
        float[] view, body;
    }
    static void run(String[] args) throws Throwable {
        VanillaFrameBenchmark.Client mc = MenuOracle.startClient(new File(args[0]), new File(args[1]), "1.8.9 sleeping player");
        mc.gameSettings.renderDistanceChunks = 2;
        mc.launchIntegratedServer("fixture", "Fixture", new WorldSettings(189, WorldSettings.GameType.SURVIVAL, false, false, WorldType.FLAT));
        MethodHandle loop = VanillaFrameBenchmark.privateMethod(mc, "runGameLoop");
        long start = System.nanoTime();
        while (mc.thePlayer == null || mc.currentScreen != null || mc.getRenderViewEntity() != mc.thePlayer) {
            VanillaFrameBenchmark.runFrame(loop);
            if (System.nanoTime() - start > 75000000000L) throw new IllegalStateException("No player");
        }
        EntityPlayerSP p = mc.thePlayer;
        BlockPos head = new BlockPos(p.posX, 75, p.posZ);
        PlayerProbe probe = new PlayerProbe(mc.getRenderManager());
        Method orient = EntityRenderer.class.getDeclaredMethod("orientCamera", float.class);
        orient.setAccessible(true);
        GlStateManager.matrixMode(GL11.GL_MODELVIEW);
        List<Sample> samples = new ArrayList<>();
        for (int i = 0; i < 4; i++) {
            EnumFacing facing = EnumFacing.getHorizontal(i);
            mc.theWorld.setBlockState(head.offset(facing.getOpposite()), Blocks.bed.getStateFromMeta(i), 2);
            mc.theWorld.setBlockState(head, Blocks.bed.getStateFromMeta(i | 8), 2);
            if (p.trySleep(head) != EntityPlayer.EnumStatus.OK) throw new AssertionError("bed " + i);
            p.prevRotationYaw = 30; p.rotationYaw = 63;
            p.prevRotationPitch = -16; p.rotationPitch = 39;
            for (int perspective = 0; perspective < 3; perspective++) {
                mc.gameSettings.thirdPersonView = perspective;
                Sample s = new Sample();
                s.facing = i; s.perspective = perspective;
                s.body = probe.sleepingBody(p);
                GlStateManager.loadIdentity();
                orient.invoke(mc.entityRenderer, .625f);
                s.view = matrix();
                samples.add(s);
            }
            p.wakeUpPlayer(true, false, false);
        }
        try (Writer out = Files.newBufferedWriter(Paths.get(args[2]))) { new Gson().toJson(samples, out); }
    }
    public static void main(String[] args) {
        try { run(args); System.exit(0); }
        catch (Throwable t) { t.printStackTrace(); System.exit(1); }
    }
}
