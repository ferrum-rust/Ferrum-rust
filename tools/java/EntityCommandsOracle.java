import com.google.gson.Gson;
import java.io.File;
import java.lang.reflect.Constructor;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import net.minecraft.command.*;
import net.minecraft.init.Bootstrap;
import net.minecraft.item.*;
import net.minecraft.potion.*;
import net.minecraft.server.MinecraftServer;
import net.minecraft.util.*;
import net.minecraft.world.WorldServer;

/** Command outputs and authoritative player state directly after each original command. */
public final class EntityCommandsOracle {
    static String message;
    static class Player extends RuntimeUseOracle.Player {
        Player(MinecraftServer h,RuntimeUseOracle.Fixture w){super(h,w);}
        public void addChatMessage(IChatComponent c){message=c instanceof ChatComponentTranslation?((ChatComponentTranslation)c).getKey():c.getUnformattedText();}
    }
    public static void main(String[] argv)throws Exception{
        Bootstrap.register();
        Constructor<?> c=Class.forName("VanillaWorldBridge$Host").getDeclaredConstructor(File.class);c.setAccessible(true);
        MinecraftServer h=(MinecraftServer)c.newInstance(new File(argv[1]));
        RuntimeUseOracle.field(MinecraftServer.class,"serverThread").set(h,Thread.currentThread());
        RuntimeUseOracle.Fixture w=new RuntimeUseOracle.Fixture(h);h.worldServers=new WorldServer[]{w};w.init();
        w.theChunkProviderServer.loadChunk(0,0);
        Player p=new Player(h,w);new RuntimeUseOracle.Handler(h,p);p.setPosition(8.5,42,8.5);
        h.getConfigurationManager().getPlayerList().add(p);
        ServerCommandManager commands=new ServerCommandManager();
        CommandBase.setAdminCommander((sender,cmd,flags,key,values)->{if(flags!=1)message=key;});
        String[] run={"give Player stone 5 0 {Custom:1b}","give Player stone 4","give Player 1","give Player dirt 65",
          "clear Player stone 0 0","clear Player stone 0 2 {Custom:1b}","clear Player stone 0 -1 {Custom:1b}","clear Player stone 0 1 {Custom:1b}","clear",
          "give Player diamond_sword","enchant Player sharpness 4","enchant Player 17","enchant Player 999","enchant Player flame",
          "effect Player speed 7 3 true","effect Player speed 4 0","effect Player 1 0","effect Player clear","effect Player instant_health","effect Player clear",
          "xp 19","xp 3L","xp -2l","xp -3","xp -999L","spawnpoint Player ~1 ~2 ~3",
          "tp Player ~1 ~2 ~3 70 120","tp 1 2.5 -3","replaceitem entity Player slot.armor.feet stone","replaceitem entity Player slot.armor.head stone",
          "replaceitem entity Player slot.hotbar.0 air","replaceitem entity Player slot.weapon diamond_sword","tell Player hello","tellraw Player {\"text\":\"Hello!\"}"};
        List<Object> out=new ArrayList<>();
        for(String line:run){
            String[] args=line.split(" ");ICommand cmd=commands.getCommands().get(args[0]);message=null;
            try{cmd.processCommand(p,Arrays.copyOfRange(args,1,args.length));}catch(CommandException e){message=e.getMessage();}
            List<Object> slots=new ArrayList<>();
            for(int i=0;i<40;i++){ItemStack s=p.inventory.getStackInSlot(i);if(s!=null)slots.add(Arrays.asList(i,Item.getIdFromItem(s.getItem()),s.stackSize,s.getMetadata()));}
            List<Object> effects=new ArrayList<>();
            for(PotionEffect e:p.getActivePotionEffects())effects.add(Arrays.asList(e.getPotionID(),e.getDuration(),e.getAmplifier(),e.getIsShowParticles()));
            out.add(Arrays.asList(line,message,slots,effects,p.experienceLevel,p.experienceTotal,p.posX,p.posY,p.posZ,p.rotationYaw,p.rotationPitch));
        }
        Files.write(Paths.get(argv[0]),new Gson().toJson(out).getBytes(StandardCharsets.UTF_8));
    }
}
