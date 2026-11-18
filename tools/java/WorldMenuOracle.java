import com.google.gson.Gson;
import java.io.File;
import java.io.OutputStream;
import java.io.Writer;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import net.minecraft.client.gui.*;
import net.minecraft.nbt.CompressedStreamTools;
import net.minecraft.nbt.NBTTagCompound;
import net.minecraft.world.WorldSettings;
import net.minecraft.world.WorldType;
import net.minecraft.world.storage.WorldInfo;

/** Render the original singleplayer selection/creation flow against two disposable Anvil saves. */
public final class WorldMenuOracle {
    private final VanillaFrameBenchmark.Client mc;
    private final File game;
    private final Map<String,Object> layouts = new LinkedHashMap<>();

    WorldMenuOracle(VanillaFrameBenchmark.Client mc, File game) {
        this.mc = mc;
        this.game = game;
    }

    private void record(String name) throws Exception {
        MenuOracle.record(mc,mc.currentScreen,name,game,layouts);
    }
    private void press(int id) throws Exception {
        List<?> buttons = (List<?>) MenuOracle.field(GuiScreen.class,"buttonList").get(mc.currentScreen);
        for (Object item : buttons) {
            GuiButton button = (GuiButton)item;
            if (button.id == id && button.visible && button.enabled) {
                Method action = GuiScreen.class.getDeclaredMethod("actionPerformed",GuiButton.class);
                action.setAccessible(true);
                action.invoke(mc.currentScreen,button);
                return;
            }
        }
        throw new AssertionError("Missing enabled button " + id + " on " + mc.currentScreen);
    }
    private void writeWorld(String folder, String display, long seed, long played, WorldSettings.GameType mode) throws Exception {
        WorldSettings settings = new WorldSettings(seed,mode,true,false,WorldType.DEFAULT);
        if (mode == WorldSettings.GameType.CREATIVE) settings.enableCommands();
        WorldInfo info = new WorldInfo(settings,display);
        info.setSaveVersion(19133);
        NBTTagCompound data = info.getNBTTagCompound();
        data.setLong("LastPlayed",played);
        NBTTagCompound root = new NBTTagCompound();
        root.setTag("Data",data);
        Path directory = Files.createDirectory(game.toPath().resolve("saves").resolve(folder));
        try (OutputStream output = Files.newOutputStream(directory.resolve("level.dat"))) {
            CompressedStreamTools.writeCompressed(root,output);
        }
    }
    private void selectFirst() throws Exception {
        Object list = MenuOracle.field(GuiSelectWorld.class,"availableWorlds").get(mc.currentScreen);
        Method click = list.getClass().getDeclaredMethod("elementClicked",int.class,boolean.class,int.class,int.class);
        click.setAccessible(true);
        click.invoke(list,0,false,400,41);
    }
    private static Object value(Object object, String name) throws Exception {
        return MenuOracle.field(object.getClass(),name).get(object);
    }

    private Map<String,Object> presetData() throws Exception {
        Map<String,Object> data = new LinkedHashMap<>();
        List<Object> flat = new ArrayList<>();
        for (Object preset : (List<?>)MenuOracle.field(GuiFlatPresets.class,"FLAT_WORLD_PRESETS").get(null)) {
            net.minecraft.item.Item icon = (net.minecraft.item.Item)value(preset,"field_148234_a");
            Map<String,Object> row = new LinkedHashMap<>();
            row.put("name",value(preset,"field_148232_b"));
            row.put("item",net.minecraft.item.Item.getIdFromItem(icon));
            row.put("damage",value(preset,"field_179037_b"));
            row.put("settings",value(preset,"field_148233_c"));
            flat.add(row);
        }
        List<Object> custom = new ArrayList<>();
        for (Object preset : (List<?>)MenuOracle.field(GuiScreenCustomizePresets.class,"field_175310_f").get(null)) {
            Map<String,Object> row = new LinkedHashMap<>();
            row.put("name",value(preset,"field_178955_a"));
            row.put("texture",value(preset,"field_178953_b").toString());
            row.put("settings",value(preset,"field_178954_c").toString());
            custom.add(row);
        }
        data.put("flat",flat);
        data.put("customized",custom);
        data.put("defaults",new net.minecraft.world.gen.ChunkProviderSettings.Factory().toString());
        return data;
    }

    private void gather() throws Exception {
        GuiScreen worlds = new GuiSelectWorld(new GuiMainMenu());
        mc.displayGuiScreen(worlds);
        record("worlds-empty");
        writeWorld("Survival 189","서바이벌 월드",189L,1700000000000L,WorldSettings.GameType.SURVIVAL);
        writeWorld("Creative 189","Creative Test",-42L,1600000000000L,WorldSettings.GameType.CREATIVE);
        mc.displayGuiScreen(worlds);
        record("worlds");
        java.text.SimpleDateFormat format = (java.text.SimpleDateFormat)MenuOracle.field(GuiSelectWorld.class,"field_146633_h").get(worlds);
        System.out.println("World list dates (" + format.toPattern() + "): " +
            format.format(new Date(1700000000000L)) + ", " + format.format(new Date(1600000000000L)));
        selectFirst();
        record("worlds-selected");
        press(6); record("world-rename"); press(1);
        selectFirst(); press(2); record("world-delete"); press(1);
        selectFirst(); press(7); record("world-recreate"); press(3); record("world-recreate-more");
        mc.displayGuiScreen(worlds);
        press(3); record("create");
        press(2); record("create-hardcore"); press(3); record("create-hardcore-more"); press(3);
        press(2); record("create-creative"); press(3); record("create-creative-more"); press(3);
        press(2); press(3); record("create-more");
        press(5); record("create-flat"); press(8); record("flat");
        press(5); record("flat-presets"); press(1); press(0);
        press(5); record("create-large-biomes");
        press(5); record("create-amplified");
        press(5); record("create-customized"); press(8); record("customized");
        for (int page=2; page<=4; page++) {press(303); record("customized-" + page);}
        press(305); record("customized-presets");
    }
    public static void main(String[] args) {
        try {
            File game = new File(args[0]).getCanonicalFile();
            File assets = new File(args[1]).getCanonicalFile();
            WorldMenuOracle oracle = new WorldMenuOracle(MenuOracle.startClient(game,assets,"Original 1.8.9 singleplayer menus"),game);
            oracle.gather();
            try (Writer out=Files.newBufferedWriter(Paths.get(args[2]),StandardCharsets.UTF_8)) {
                new Gson().toJson(oracle.layouts,out);
            }
            try (Writer out=Files.newBufferedWriter(game.toPath().getParent().resolve("presets.json"),StandardCharsets.UTF_8)) {
                new Gson().toJson(oracle.presetData(),out);
            }
            System.exit(0);
        } catch (Throwable error) {
            error.printStackTrace();
            System.exit(1);
        }
    }
}
