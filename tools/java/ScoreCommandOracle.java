import com.google.gson.Gson;
import java.io.*;
import java.util.*;
import net.minecraft.command.*;
import net.minecraft.init.Bootstrap;
import net.minecraft.scoreboard.*;
import net.minecraft.server.MinecraftServer;
import net.minecraft.util.*;
import net.minecraft.world.WorldServer;

/** Execute nested commands via the stock command manager, including score operations and result bindings. */
public final class ScoreCommandOracle {
    static class Sender extends RuntimeUseOracle.Player{
        Sender(MinecraftServer s,RuntimeUseOracle.Fixture w){super(s,w);}
        public boolean canCommandSenderUseCommand(int n,String c){return true;}
        public void addChatMessage(IChatComponent c){}
    }
    public static void main(String[] a)throws Exception{
        PrintStream out=System.out;System.setOut(System.err);Bootstrap.register();
        java.lang.reflect.Constructor<?> build=Class.forName("VanillaWorldBridge$Host").getDeclaredConstructor(File.class);build.setAccessible(true);
        MinecraftServer server=(MinecraftServer)build.newInstance(new File(a[0]));
        RuntimeUseOracle.field(MinecraftServer.class,"serverThread").set(server,Thread.currentThread());
        RuntimeUseOracle.Fixture world=new RuntimeUseOracle.Fixture(server);server.worldServers=new WorldServer[]{world};world.init();world.theChunkProviderServer.loadChunk(0,0);
        Sender p=new Sender(server,world);new RuntimeUseOracle.Handler(server,p);p.setPosition(8.5,42,8.5);server.getConfigurationManager().getPlayerList().add(p);world.spawnEntityInWorld(p);
        ServerCommandManager commands=new ServerCommandManager();CommandBase.setAdminCommander((s,c,f,k,v)->{});
        RuntimeUseOracle.field(MinecraftServer.class,"commandManager").set(server,commands);
        Scoreboard b=world.getScoreboard();List<Object> rows=new ArrayList<>();
        for(String cmd:new String[]{
            "scoreboard objectives add wins dummy Wins","scoreboard objectives add button trigger","scoreboard objectives add read health","scoreboard players set Player wins 8",
            "scoreboard players set Other wins 3","scoreboard players operation Player wins *= Other wins","scoreboard players operation Player wins %= Other wins",
            "scoreboard players operation Player wins >< Other wins","scoreboard players operation Player wins /= Other wins","scoreboard players set Player read 5",
            "scoreboard players test Player wins 3 3","scoreboard players test Player wins 6","scoreboard objectives setdisplay sidebar wins",
            "scoreboard teams add blueTeam Blue Team","scoreboard teams join blueTeam Player","scoreboard teams option blueTeam color blue",
            "scoreboard players enable Player button","trigger button add 5","trigger button set 9","scoreboard players enable Player button","trigger button set -8",
            "stats entity Player set QueryResult Player wins","time set 120","time query daytime","scoreboard players test Player wins 120",
            "execute Player ~ ~ ~ scoreboard players add Player wins 4","scoreboard players add * wins 2","scoreboard teams leave Player",
            "stats entity Player clear QueryResult","scoreboard players reset Other wins","scoreboard objectives remove wins","scoreboard objectives remove button"}){
            int count=commands.executeCommand(p,cmd);Map<String,Integer> scores=new TreeMap<>();
            for(Score s:b.getScores())scores.put(s.getPlayerName()+"/"+s.getObjective().getName(),s.getScorePoints());
            ScoreObjective sidebar=b.getObjectiveInDisplaySlot(1);
            rows.add(Arrays.asList(cmd,count,scores,sidebar==null?"":sidebar.getName(),b.getPlayersTeam("Player")==null?"":b.getPlayersTeam("Player").getRegisteredName()));
        }
        out.println(new Gson().toJson(rows));
    }
}
