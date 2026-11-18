import com.google.gson.GsonBuilder;
import java.io.*;
import java.lang.invoke.MethodHandle;
import java.lang.reflect.Method;
import java.nio.file.*;
import java.nio.charset.StandardCharsets;
import java.util.*;
import net.minecraft.client.gui.*;
import net.minecraft.client.gui.achievement.*;
import net.minecraft.world.*;
import net.minecraft.stats.*;
import net.minecraft.item.Item;
import net.minecraft.util.ChatComponentTranslation;

public class PauseMenusOracle {
    static void run(String[] args)throws Throwable{
        File game=new File(args[0]);org.lwjgl.opengl.Display.setLocation(32000,32000);
        VanillaFrameBenchmark.Client mc=MenuOracle.startClient(game,new File(args[1]),"Pause UI oracle");
        Map<String,Object> result=new LinkedHashMap<>();
        MenuOracle.capture(mc,new GuiIngameMenu(),"pause-remote",game,result);
        mc.launchIntegratedServer("fixture","Pause",new WorldSettings(189,WorldSettings.GameType.SURVIVAL,false,false,WorldType.FLAT));
        MethodHandle loop=VanillaFrameBenchmark.privateMethod(mc,"runGameLoop");long start=System.nanoTime();
        while(mc.thePlayer==null || mc.theWorld==null || mc.currentScreen!=null){
            VanillaFrameBenchmark.runFrame(loop);VanillaFrameBenchmark.require(VanillaFrameBenchmark.seconds(start)<70,"Join failed");}
        // Compare both menu renderers over the same headless framebuffer clear.
        net.minecraft.client.renderer.GlStateManager.clearColor(0f,0f,0f,1f);
        GuiIngameMenu pause=new GuiIngameMenu();
        MenuOracle.capture(mc,pause,"pause",game,result);
        GuiShareToLan lan=new GuiShareToLan(pause);
        MenuOracle.capture(mc,lan,"share-lan",game,result);
        Method apply=GuiShareToLan.class.getDeclaredMethod("actionPerformed",GuiButton.class);apply.setAccessible(true);
        List<?> buttons=(List<?>)MenuOracle.field(GuiScreen.class,"buttonList").get(lan);
        for(int i=1;i<=4;i++){apply.invoke(lan,buttons.get(2));apply.invoke(lan,buttons.get(3));MenuOracle.record(mc,lan,"share-lan-"+i,game,result);}
        GuiAchievements tree=new GuiAchievements(pause,mc.thePlayer.getStatFileWriter());
        mc.displayGuiScreen(tree);tree.doneLoading();MenuOracle.record(mc,tree,"achievements",game,result);
        List<Object> nodes=new ArrayList<>();
        for(Achievement a:AchievementList.achievementList)nodes.add(Arrays.asList(a.statId, a.displayColumn,a.displayRow,
            Item.getIdFromItem(a.theItemStack.getItem()),a.theItemStack.getMetadata(),a.parentAchievement==null?null:a.parentAchievement.statId,a.getSpecial()));
        Path registry = args.length > 3 ? Paths.get(args[3]) : Paths.get(args[2]).resolveSibling("achievements.json");
        Files.write(registry,new GsonBuilder().serializeNulls().create().toJson(nodes).getBytes(StandardCharsets.UTF_8));
        Files.write(Paths.get(args[2]),new GsonBuilder().create().toJson(result).getBytes(StandardCharsets.UTF_8));
        mc.loadWorld(null);mc.shutdownMinecraftApplet();
    }
    public static void main(String[] args){try{run(args);System.exit(0);}catch(Throwable e){e.printStackTrace();System.exit(1);}}
}
