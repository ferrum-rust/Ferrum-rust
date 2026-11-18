import com.google.gson.Gson;
import java.io.*;
import java.lang.reflect.Constructor;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import net.minecraft.block.Block;
import net.minecraft.block.state.IBlockState;
import net.minecraft.command.*;
import net.minecraft.command.server.*;
import net.minecraft.init.Bootstrap;
import net.minecraft.server.MinecraftServer;
import net.minecraft.util.*;
import net.minecraft.world.WorldServer;

/** Run command implementations in the installed JAR against a real, disposable WorldServer. */
public final class WorldCommandsOracle {
    static String message;
    static class Player extends RuntimeUseOracle.Player {
        Player(MinecraftServer h, RuntimeUseOracle.Fixture w){super(h,w);}
        public void addChatMessage(IChatComponent c){message=c instanceof ChatComponentTranslation?((ChatComponentTranslation)c).getKey():c.getUnformattedText();}
    }
    public static void main(String[] argv)throws Exception{
        Bootstrap.register();
        Constructor<?> c=Class.forName("VanillaWorldBridge$Host").getDeclaredConstructor(File.class);c.setAccessible(true);
        MinecraftServer h=(MinecraftServer)c.newInstance(new File(argv[1]));
        RuntimeUseOracle.field(MinecraftServer.class,"serverThread").set(h,Thread.currentThread());
        RuntimeUseOracle.Fixture w=new RuntimeUseOracle.Fixture(h);h.worldServers=new WorldServer[]{w};w.init();
        for(int z=0;z<=1;z++)for(int x=0;x<=1;x++)w.theChunkProviderServer.loadChunk(x,z);
        Player p=new Player(h,w);new RuntimeUseOracle.Handler(h,p);p.setPosition(8.5,42,8.5);
        ServerCommandManager commands=new ServerCommandManager();CommandBase.setAdminCommander((sender,cmd,flags,key,values)->message=key);
        List<Object> rows=new ArrayList<>();
        for(String line:new String[]{
            "setblock 8 42 8 stone 3","setblock 8 42 8 stone 3","setblock 8 42 8 wool 2 keep",
            "fill 8 42 8 10 44 10 stone","fill 8 42 8 10 44 10 glass 0 hollow",
            "testforblock 9 43 9 air","testforblock 8 42 8 glass","testforblock 8 42 8 wool",
            "fill 8 42 8 10 44 10 wool 3 replace glass 0",
            "clone 8 42 8 10 44 10 9 42 8","clone 8 42 8 10 44 10 12 42 8 masked",
            "testforblocks 8 42 8 10 44 10 12 42 8 masked","setblock 12 42 8 gold_block",
            "testforblocks 8 42 8 10 44 10 12 42 8","setblock 8 42 8 chest 0 replace {CustomName:Before}",
            "blockdata 8 42 8 {CustomName:After}","testforblock 8 42 8 chest -1 {CustomName:After}",
            "blockdata 8 42 8 {CustomName:After}","clone 8 42 8 8 42 8 11 42 8 replace move",
            "testforblock 11 42 8 chest -1 {CustomName:After}","setblock 8 42 8 air 0 destroy",
            "fill 0 0 0 50 100 50 stone","setblock 8 256 8 stone","setblock 8 257 8 stone",
            "time set night","time add 27","time query daytime","time set -1","weather thunder 5","weather clear 1",
            "toggledownfall","gamerule keepInventory true","gamerule keepInventory","gamerule keepInventory True","gamerule custom hello world","gamerule custom",
            "worldborder set 20","worldborder add 3","worldborder center ~ ~","worldborder damage buffer 2","worldborder warning distance 3","worldborder get",
            "setworldspawn 8 42 7","difficulty hard","particle smoke ~ ~ ~ 0 0 0 0 3","particle wrong ~ ~ ~ 0 0 0 0 1"}){
            String[] cmd=line.split(" ");ICommand command=commands.getCommands().get(cmd[0]);
            message=null;
            try{command.processCommand(p,Arrays.copyOfRange(cmd,1,cmd.length));}catch(CommandException e){message=e.getMessage();}
            if(message==null)throw new AssertionError(line+" returned no outcome");
            List<Integer> states=new ArrayList<>();
            for(int z=8;z<=10;z++)for(int y=42;y<=44;y++)for(int x=8;x<=14;x++){
                IBlockState state=w.getBlockState(new BlockPos(x,y,z));states.add(Block.getIdFromBlock(state.getBlock())*16+state.getBlock().getMetaFromState(state));
            }
            rows.add(Arrays.asList(line,message,states,w.getWorldTime()));
        }
        Files.write(Paths.get(argv[0]),new Gson().toJson(rows).getBytes(StandardCharsets.UTF_8));System.out.println("Verified "+rows.size()+" original world command actions");
    }
}

