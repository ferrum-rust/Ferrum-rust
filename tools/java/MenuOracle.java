import com.google.gson.Gson;
import com.mojang.authlib.properties.PropertyMap;
import java.io.*;
import java.lang.reflect.Field;
import java.net.Proxy;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.*;
import net.minecraft.client.gui.stream.GuiStreamUnavailable;
import net.minecraft.client.main.GameConfiguration;
import net.minecraft.client.multiplayer.ServerData;
import net.minecraft.client.multiplayer.ServerList;
import net.minecraft.client.renderer.GlStateManager;
import net.minecraft.client.resources.I18n;
import net.minecraft.util.ScreenShotHelper;
import net.minecraft.util.Session;
import org.lwjgl.opengl.Display;
import org.lwjgl.opengl.GL11;

/** Real 1.8.9 menu initGui/drawScreen calls in a disposable, network-isolated game directory. */
public final class MenuOracle {
    static Field field(Class<?> cls,String name) throws Exception {
        Field f=cls.getDeclaredField(name); f.setAccessible(true); return f;
    }
    static void capture(VanillaFrameBenchmark.Client mc,GuiScreen screen,String name,
                        File game,Map<String,Object> result) throws Exception {
        mc.displayGuiScreen(screen);
        if(screen instanceof GuiMainMenu) {
            field(GuiMainMenu.class,"panoramaTimer").setInt(screen,80);
            field(GuiMainMenu.class,"splashText").set(screen,"Minecraft!");
        }
        if(screen instanceof GuiMultiplayer) {
            ServerList list=((GuiMultiplayer)screen).getServerList();
            for(int i=0;i<list.countServers();i++){
                ServerData server=list.getServerData(i);
                server.field_78841_f=true;
                server.populationInfo=i==0?"3/20":"0/16";
                server.pingToServer=i==0?42L:180L;
                server.serverMOTD="로컬 서버 미리보기";
            }
            if(name.equals("multiplayer-selected")) ((GuiMultiplayer)screen).selectServer(0);
        }
        record(mc,screen,name,game,result);
    }

    static void record(VanillaFrameBenchmark.Client mc,GuiScreen screen,String name,
                       File game,Map<String,Object> result) throws Exception {
        // renderSkybox draws its cube to the window backbuffer before copying
        // it into the GUI framebuffer. Clear the window depth buffer as well.
        mc.getFramebuffer().unbindFramebuffer();
        GlStateManager.depthMask(true);
        GlStateManager.clearDepth(1.0);
        GlStateManager.clear(GL11.GL_COLOR_BUFFER_BIT|GL11.GL_DEPTH_BUFFER_BIT);
        mc.getFramebuffer().bindFramebuffer(true);
        GlStateManager.clear(GL11.GL_COLOR_BUFFER_BIT|GL11.GL_DEPTH_BUFFER_BIT);
        mc.entityRenderer.setupOverlayRendering();
        GlStateManager.color(1f,1f,1f,1f);
        screen.drawScreen(0,0,0f);
        List<?> buttons=(List<?>)field(GuiScreen.class,"buttonList").get(screen);
        List<Object> rows=new ArrayList<>();
        for(Object object:buttons) {
            GuiButton b=(GuiButton)object;
            rows.add(Arrays.asList(b.id,b.xPosition,b.yPosition,
                field(GuiButton.class,"width").getInt(b),field(GuiButton.class,"height").getInt(b),b.enabled,b.visible,b.displayString));
        }
        Map<String,Object> row=new LinkedHashMap<>();
        row.put("size",Arrays.asList(screen.width,screen.height));row.put("buttons",rows);
        result.put(name,row);
        ScreenShotHelper.saveScreenshot(game,name+".png",mc.displayWidth,mc.displayHeight,mc.getFramebuffer());
    }
    static VanillaFrameBenchmark.Client startClient(File game, File assets, String title) throws Throwable {
        VanillaFrameBenchmark.offlineOnly(assets.toPath().getParent());
        List<String> options=Arrays.asList("lang:ko_KR","guiScale:2","enableVsync:false","useVbo:true",
            "snooperEnabled:false","realmsNotifications:false","pauseOnLostFocus:false","soundCategory_master:0.0","showInventoryAchievementHint:false");
        Files.write(game.toPath().resolve("options.txt"),options,StandardCharsets.UTF_8);
        Session session=new Session("Menu189",UUID.nameUUIDFromBytes("OfflinePlayer:Menu189".getBytes(StandardCharsets.UTF_8)).toString(),"","legacy");
        VanillaFrameBenchmark.Client mc=new VanillaFrameBenchmark.Client(new GameConfiguration(
            new GameConfiguration.UserInformation(session,new PropertyMap(),new PropertyMap(),Proxy.NO_PROXY),
            new GameConfiguration.DisplayInformation(1600,900,false,false),
            new GameConfiguration.FolderInformation(game,new File(game,"resourcepacks"),assets,"1.8"),
            new GameConfiguration.GameInformation(false,"1.8.9"),new GameConfiguration.ServerInformation(null,25565)));
        net.minecraft.init.Bootstrap.register();
        VanillaFrameBenchmark.privateMethod(mc,"startGame").invokeExact();
        Display.setTitle(title);
        java.lang.invoke.MethodHandle loop=VanillaFrameBenchmark.privateMethod(mc,"runGameLoop");
        for(int i=0;i<3;i++) VanillaFrameBenchmark.runFrame(loop);
        return mc;
    }

    static void run(String[] args) throws Throwable {
        File game=new File(args[0]).getCanonicalFile();
        File assets=new File(args[1]).getCanonicalFile();
        VanillaFrameBenchmark.Client mc=startClient(game,assets,"Original 1.8.9 menu reference");
        Map<String,Object> result=new LinkedHashMap<>();
        GuiMainMenu main=new GuiMainMenu();
        GuiOptions settings=new GuiOptions(main,mc.gameSettings);
        capture(mc,main,"main",game,result);
        ServerList servers=new ServerList(mc);
        servers.addServerData(new ServerData("Ferrum Survival","survival.example.invalid",false));
        servers.addServerData(new ServerData("Ferrum Creative","creative.example.invalid",false));
        servers.saveServerList();
        GuiMultiplayer multi=new GuiMultiplayer(main);
        capture(mc,multi,"multiplayer",game,result);
        capture(mc,multi,"multiplayer-selected",game,result);
        ServerData draft=new ServerData(I18n.format("selectServer.defaultName"),"",false);
        capture(mc,new GuiScreenAddServer(multi,draft),"add-server",game,result);
        capture(mc,new GuiScreenAddServer(multi,servers.getServerData(0)),"edit-server",game,result);
        capture(mc,new GuiScreenServerList(multi,draft),"direct-connect",game,result);
        capture(mc,new GuiYesNo(multi,I18n.format("selectServer.deleteQuestion"),"'Ferrum Survival' "+I18n.format("selectServer.deleteWarning"),
            I18n.format("selectServer.deleteButton"),I18n.format("gui.cancel"),0),"delete-server",game,result);
        capture(mc,settings,"options",game,result);
        capture(mc,new GuiCustomizeSkin(settings),"skin",game,result);
        capture(mc,new GuiScreenOptionsSounds(settings,mc.gameSettings),"sounds",game,result);
        capture(mc,new GuiControls(settings,mc.gameSettings),"controls",game,result);
        capture(mc,new GuiLanguage(settings,mc.gameSettings,mc.getLanguageManager()),"languages",game,result);
        capture(mc,new ScreenChatOptions(settings,mc.gameSettings),"chat",game,result);
        capture(mc,new GuiSnooper(settings,mc.gameSettings),"snooper",game,result);
        capture(mc,new GuiScreenResourcePacks(settings),"packs",game,result);
        GuiStreamUnavailable.func_152321_a(settings);
        capture(mc,mc.currentScreen,"broadcast",game,result);
        try(Writer w=new OutputStreamWriter(new FileOutputStream(args[2]),StandardCharsets.UTF_8)) {
            new Gson().toJson(result,w);
        }
    }
    public static void main(String[] args) {
        try {run(args);System.exit(0);}
        catch(Throwable e){e.printStackTrace();System.exit(1);}
    }
}
