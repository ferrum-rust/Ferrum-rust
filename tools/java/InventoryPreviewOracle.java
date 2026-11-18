import java.io.File;
import java.lang.invoke.MethodHandle;
import java.util.LinkedHashMap;
import net.minecraft.client.gui.inventory.GuiInventory;
import net.minecraft.world.WorldSettings;
import net.minecraft.world.WorldType;

/** Capture GuiInventory after its cursor moved, using a disposable survival player. */
public final class InventoryPreviewOracle {
    public static void main(String[] args) throws Throwable {
        File game = new File(args[0]);
        VanillaFrameBenchmark.Client mc = MenuOracle.startClient(game, new File(args[1]), "1.8.9 inventory preview");
        mc.gameSettings.renderDistanceChunks = 2;
        mc.launchIntegratedServer("preview", "Preview", new WorldSettings(189, WorldSettings.GameType.SURVIVAL, false, false, WorldType.FLAT));
        MethodHandle loop = VanillaFrameBenchmark.privateMethod(mc, "runGameLoop");
        long start = System.nanoTime();
        while (mc.thePlayer == null || mc.currentScreen != null || mc.getRenderManager().livingPlayer != mc.thePlayer) {
            VanillaFrameBenchmark.runFrame(loop);
            if (System.nanoTime() - start > 75_000_000_000L) throw new IllegalStateException("Player renderer unavailable");
        }
        mc.thePlayer.inventory.clear();
        mc.thePlayer.ticksExisted = 0;
        GuiInventory gui = new GuiInventory(mc.thePlayer);
        mc.displayGuiScreen(gui);
        int[][] cursors = {{363,167}, {203,167}, {523,167}, {363,47}, {363,287}};
        String[] names = {"center","left","right","up","down"};
        for (int i = 0; i < cursors.length; i++) {
            gui.drawScreen(cursors[i][0], cursors[i][1], 0f);
            MenuOracle.record(mc, gui, names[i], game, new LinkedHashMap<String,Object>());
        }
        System.exit(0);
    }
}
