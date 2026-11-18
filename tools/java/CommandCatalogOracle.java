import java.io.File;
import java.io.PrintStream;
import java.util.*;
import com.google.gson.Gson;
import net.minecraft.command.*;
import net.minecraft.init.Bootstrap;
import net.minecraft.scoreboard.IScoreObjectiveCriteria;
import net.minecraft.stats.*;

/** Export the integrated server's registered commands and its statistic/score registries. */
public final class CommandCatalogOracle {
    public static void main(String[] args) throws Exception {
        PrintStream out = System.out;
        System.setOut(System.err);
        Bootstrap.register();
        java.lang.reflect.Constructor<?> constructor =
            Class.forName("VanillaWorldBridge$Host").getDeclaredConstructor(File.class);
        constructor.setAccessible(true);
        net.minecraft.server.MinecraftServer server = (net.minecraft.server.MinecraftServer)
            constructor.newInstance(new File(args[0]));
        ServerCommandManager manager = new ServerCommandManager();
        Map<String,Object> result = new TreeMap<>(), commands = new TreeMap<>(), stats = new TreeMap<>();
        for (ICommand command : new HashSet<ICommand>(manager.getCommands().values())) {
            CommandBase base = (CommandBase) command;
            List<Integer> targets = new ArrayList<>();
            String[] words = {"", "", "", "", "", "", "", "", "", ""};
            for (int i=0;i<words.length;i++) if (base.isUsernameIndex(words,i)) targets.add(i);
            commands.put(base.getCommandName(), Arrays.asList(base.getCommandUsage(server),base.getRequiredPermissionLevel(),base.getCommandAliases(),targets));
        }
        for (StatBase stat : StatList.allStats) {
            Achievement parent = stat instanceof Achievement ? ((Achievement) stat).parentAchievement : null;
            stats.put(stat.statId, parent == null ? "" : parent.statId);
        }
        result.put("commands", commands);
        result.put("statistics", stats);
        result.put("entities",new TreeSet<>(net.minecraft.entity.EntityList.getEntityNameList()));
        result.put("criteria",new TreeSet<>(IScoreObjectiveCriteria.INSTANCES.keySet()));
        out.println(new Gson().toJson(result));
    }
}
