import com.google.gson.*;
import com.mojang.authlib.properties.PropertyMap;
import java.io.*;
import java.lang.invoke.*;
import java.lang.reflect.*;
import java.net.Proxy;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.security.Permission;
import java.util.*;
import java.util.concurrent.TimeUnit;
import net.minecraft.client.Minecraft;
import net.minecraft.client.main.GameConfiguration;
import net.minecraft.client.settings.GameSettings;
import net.minecraft.client.entity.EntityPlayerSP;
import net.minecraft.entity.player.EntityPlayerMP;
import net.minecraft.server.integrated.IntegratedServer;
import net.minecraft.util.*;
import net.minecraft.world.WorldServer;
import org.lwjgl.opengl.Display;
import org.lwjgl.opengl.GL11;

/** Measures the real 1.8.9 client runGameLoop, including Minecraft.updateDisplay
 * and Display.update. Camera setup, checks and screenshot happen outside sampling.
 * The vanilla integrated server and render workers continue running normally. */
public final class VanillaFrameBenchmark {
    static final class Client extends Minecraft {
        Client(GameConfiguration config) { super(config); }
        // The benchmark owns a fixed camera. Never capture the user's mouse.
        @Override public void setIngameFocus() { inGameHasFocus=false; }
    }
    static MethodHandle privateMethod(Client client,String name)throws Exception{
        Method fn=Minecraft.class.getDeclaredMethod(name);fn.setAccessible(true);
        return MethodHandles.lookup().unreflect(fn).bindTo(client);
    }
    static void runFrame(MethodHandle loop)throws Throwable{
        if(Display.isCloseRequested())throw new IOException("Benchmark window was closed");
        loop.invokeExact();
    }
    static void require(boolean ok,String reason){if(!ok)throw new IllegalStateException(reason);}
    static double seconds(long start){return (System.nanoTime()-start)*1e-9;}
    static void checkPose(EntityPlayerSP player,double[] p,float yaw,float pitch) {
        require(Math.abs(player.posX-p[0])<1e-7 && Math.abs(player.posY-p[1])<1e-7 && Math.abs(player.posZ-p[2])<1e-7 &&
                Math.abs(MathHelper.wrapAngleTo180_float(player.rotationYaw-yaw))<1e-4 && Math.abs(player.rotationPitch-pitch)<1e-4,
                "Camera changed: "+player.posX+","+player.posY+","+player.posZ+" rot "+player.rotationYaw+","+player.rotationPitch);
    }
    static int serverChunks(IntegratedServer server)throws Exception {
        final int[] result=new int[1];
        server.addScheduledTask(new Runnable(){public void run(){result[0]=server.worldServerForDimension(0).getChunkProvider().getLoadedChunkCount();}}).get(10,TimeUnit.SECONDS);
        return result[0];
    }
    static void options(Path game,JsonObject s)throws IOException {
        List<String> lines=Arrays.asList(
            "lang:en_US","renderDistance:"+s.get("render_distance"),"fancyGraphics:"+s.get("fancy"),
            "ao:"+s.get("smooth_lighting"),"mipmapLevels:"+s.get("mipmap_levels"),"gamma:"+s.get("gamma"),
            "renderClouds:false","enableVsync:false","useVbo:true","maxFps:260","difficulty:0",
            "snooperEnabled:false","pauseOnLostFocus:false","bobView:false","fboEnable:true","showInventoryAchievementHint:false",
            "soundCategory_master:0.0","fov:"+((s.get("fov_degrees").getAsFloat()-70f)/40f));
        Files.write(game.resolve("options.txt"),lines,StandardCharsets.UTF_8);
    }
    static void offlineOnly(final Path installation){
        System.setSecurityManager(new SecurityManager(){
            private void writable(String file){
                if("<<ALL FILES>>".equals(file))throw new SecurityException("No unrestricted writes");
                try {
                    if(new File(file).getCanonicalFile().toPath().startsWith(installation))
                        throw new SecurityException("Installed game files are read-only");
                } catch(IOException e){throw new SecurityException("Cannot resolve file before write",e);}
            }
            @Override public void checkWrite(String file){writable(file);}
            @Override public void checkDelete(String file){writable(file);}
            @Override public void checkPermission(Permission permission){
                if(permission instanceof FilePermission){
                    String actions=permission.getActions();
                    if(actions.contains("write") || actions.contains("delete"))writable(permission.getName());
                }
            }
            @Override public void checkPermission(Permission permission,Object context){checkPermission(permission);}
            @Override public void checkConnect(String host,int port){
                if(!"127.0.0.1".equals(host) && !"localhost".equalsIgnoreCase(host) && !"::1".equals(host))throw new SecurityException("Offline benchmark blocked outgoing connection");
            }
            @Override public void checkConnect(String host,int port,Object context){checkConnect(host,port);}
        });
        // Exercise the same permission checks as actual writes/deletes, without touching the installation.
        String untouched=installation.resolve("assets/skins/ferrum-denial-test").toString();
        for(String action:new String[]{"write","delete"}){
            try {System.getSecurityManager().checkPermission(new FilePermission(untouched,action));}
            catch(SecurityException denied){continue;}
            throw new AssertionError("Expected installed game "+action+" denial");
        }
    }
    static void measure(String[] args)throws Throwable{
        File game=new File(args[0]).getCanonicalFile();
        require(new File(game,"saves/benchmark/level.dat").isFile(),"Missing benchmark world");
        JsonObject scene=new JsonParser().parse(new String(Files.readAllBytes(Paths.get(args[2])),StandardCharsets.UTF_8)).getAsJsonObject();
        JsonArray dims=scene.getAsJsonArray("resolution"),feet=scene.getAsJsonArray("player_feet");
        int width=dims.get(0).getAsInt(),height=dims.get(1).getAsInt(),radius=scene.get("render_distance").getAsInt();
        final double[] p={feet.get(0).getAsDouble(),feet.get(1).getAsDouble(),feet.get(2).getAsDouble()};
        final float yaw=scene.get("yaw_degrees").getAsFloat(),pitch=scene.get("pitch_degrees").getAsFloat();
        Path installation=new File(args[1]).getCanonicalFile().toPath().getParent();
        require(!game.toPath().startsWith(installation),"Benchmark data must live outside installed game");
        offlineOnly(installation);options(game.toPath(),scene);
        final Session session=new Session("Bench189",UUID.nameUUIDFromBytes("OfflinePlayer:Bench189".getBytes(StandardCharsets.UTF_8)).toString(),"","legacy");
        final Client mc=new Client(new GameConfiguration(
            new GameConfiguration.UserInformation(session,new PropertyMap(),new PropertyMap(),Proxy.NO_PROXY),
            new GameConfiguration.DisplayInformation(width,height,false,false),
            new GameConfiguration.FolderInformation(game,new File(game,"resourcepacks"),new File(args[1]),"1.8"),
            new GameConfiguration.GameInformation(false,"1.8.9"),new GameConfiguration.ServerInformation(null,25565)));
        MethodHandle start=privateMethod(mc,"startGame"),loop=privateMethod(mc,"runGameLoop");
        net.minecraft.init.Bootstrap.register();
        start.invokeExact();
        Display.setTitle("1.8.9 OpenGL frame benchmark (offline)");Display.setVSyncEnabled(false);
        GameSettings opts=mc.gameSettings; opts.hideGUI=!scene.get("hud").getAsBoolean();
        opts.fovSetting=scene.get("fov_degrees").getAsFloat();opts.limitFramerate=260;
        opts.thirdPersonView=0;opts.viewBobbing=false;opts.enableVsync=false;opts.pauseOnLostFocus=false;
        require(opts.renderDistanceChunks==radius && opts.fancyGraphics==scene.get("fancy").getAsBoolean() &&
                opts.ambientOcclusion==scene.get("smooth_lighting").getAsInt() && opts.mipmapLevels==scene.get("mipmap_levels").getAsInt() &&
                opts.clouds==0 && opts.useVbo && opts.gammaSetting==scene.get("gamma").getAsFloat(),"Original options did not take effect");
        mc.launchIntegratedServer("benchmark","Benchmark",null);
        long loadStart=System.nanoTime();
        while(mc.thePlayer==null || mc.theWorld==null || mc.currentScreen!=null ||
                mc.theWorld.getChunkProvider().getLoadedChunkCount()<(2*radius+1)*(2*radius+1)){
            runFrame(loop);
            require(seconds(loadStart)<100,"Timed out receiving local chunks: "+(mc.theWorld==null?0:mc.theWorld.getChunkProvider().getLoadedChunkCount()));
        }
        final IntegratedServer server=mc.getIntegratedServer();
        server.addScheduledTask(new Runnable(){public void run(){
            EntityPlayerMP player=server.getConfigurationManager().getPlayerByUsername("Bench189");
            player.playerNetServerHandler.setPlayerLocation(p[0],p[1],p[2],yaw,pitch);
        }}).get(10,TimeUnit.SECONDS);
        mc.guiAchievement.clearAchievements();
        long warm=System.nanoTime();
        for(;;){
            runFrame(loop);
            double age=seconds(warm);
            if(age>=scene.get("warmup_seconds").getAsDouble() &&
                    mc.renderGlobal.getDebugInfoRenders().contains("pC: 000, pU: 0, aB: 5"))break;
            require(age<50,"Renderer did not settle: "+mc.renderGlobal.getDebugInfoRenders());
        }
        checkPose(mc.thePlayer,p,yaw,pitch);
        require(mc.displayWidth==width && mc.displayHeight==height,"Viewport differed from scene");
        require(mc.currentScreen==null && !mc.isGamePaused() && !mc.isFramerateLimitBelowMax(),"Scene is paused or frame-limited");
        final int clientBefore=mc.theWorld.getChunkProvider().getLoadedChunkCount(),serverBefore=serverChunks(server);
        final String renderBefore=mc.renderGlobal.getDebugInfoRenders();
        final int entitiesBefore=mc.theWorld.loadedEntityList.size();
        require(entitiesBefore==1,"Unexpected entities: "+entitiesBefore);
        double warmSecs=seconds(warm);
        final long target=(long)(1e9*scene.get("measure_seconds").getAsDouble());
        ArrayList<Double> intervals=new ArrayList<Double>(32768);
        long first=System.nanoTime(),previous=first,end=first;
        do {
            runFrame(loop);
            end=System.nanoTime();intervals.add((end-previous)*1e-9);previous=end;
        } while(end-first<target);
        final int clientAfter=mc.theWorld.getChunkProvider().getLoadedChunkCount(),serverAfter=serverChunks(server);
        checkPose(mc.thePlayer,p,yaw,pitch);
        require(clientBefore==clientAfter && serverBefore==serverAfter,"Chunk count changed during capture");
        require(mc.theWorld.loadedEntityList.size()==entitiesBefore,"Entity count changed");
        require(mc.theWorld.getWorldTime()%24000==scene.get("time").getAsLong() && !mc.theWorld.isRaining(),"Time or weather differed");
        ScreenShotHelper.saveScreenshot(game,"vanilla-view.png",width,height,mc.getFramebuffer());
        Map<String,Object> r=new LinkedHashMap<String,Object>();
        r.put("original_install_read_only",true);r.put("external_network_disabled",true);
        r.put("kind","vanilla_window_frame_intervals");r.put("version","1.8.9");r.put("java",System.getProperty("java.runtime.version"));
        r.put("gpu",GL11.glGetString(GL11.GL_RENDERER));r.put("gl_version",GL11.glGetString(GL11.GL_VERSION));r.put("backend","OpenGL");
        r.put("resolution_pixels",Arrays.asList(width,height));r.put("world_sha256",args[4]);r.put("scene",scene);
        r.put("camera_feet",p);r.put("yaw_degrees",mc.thePlayer.rotationYaw);r.put("pitch_degrees",mc.thePlayer.rotationPitch);r.put("eye_height",mc.thePlayer.getEyeHeight());
        r.put("client_loaded_chunks",Arrays.asList(clientBefore,clientAfter));r.put("server_loaded_chunks",Arrays.asList(serverBefore,serverAfter));
        r.put("render_info",Arrays.asList(renderBefore,mc.renderGlobal.getDebugInfoRenders()));r.put("entities_including_player",entitiesBefore);
        r.put("warmup_seconds",warmSecs);r.put("frames",intervals.size());r.put("measured_seconds",(end-first)*1e-9);
        r.put("average_fps",intervals.size()/((end-first)*1e-9));r.put("frame_seconds",intervals);
        ArrayList<Double> sorted=new ArrayList<Double>(intervals);Collections.sort(sorted);
        for(int pc:new int[]{50,95,99})r.put("p"+pc+"_frame_ms",sorted.get((int)((sorted.size()-1)*pc/100.))*1000.);
        int count=(int)Math.ceil(sorted.size()*0.01);double slow=0;for(int i=sorted.size()-count;i<sorted.size();i++)slow+=sorted.get(i);
        r.put("one_percent_low_fps",count/slow);
        Files.write(Paths.get(args[3]),new Gson().toJson(r).getBytes(StandardCharsets.UTF_8));
        mc.loadWorld(null);mc.shutdownMinecraftApplet();
    }
    public static void main(String[] args){
        int status=0;try{measure(args);}catch(Throwable failure){failure.printStackTrace();status=1;}finally{if(Display.isCreated())Display.destroy();}
        System.exit(status);
    }
}
