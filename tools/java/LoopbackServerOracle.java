import java.io.File;
import java.lang.invoke.MethodHandle;
import java.net.InetAddress;
import java.nio.file.*;
import java.util.concurrent.TimeUnit;
import net.minecraft.server.integrated.IntegratedServer;
import net.minecraft.world.*;

/** Real local-only TCP endpoint on the integrated server bundled with the installed client. */
public final class LoopbackServerOracle {
    // Change the selected item only inside the first click callback. The original
    // NetHandlerPlayServer then compares C0E with the returned ItemStack, rejects
    // that stale prediction, disables crafting and sends S30 plus S2F(-1,-1).
    static class PluginInventory extends net.minecraft.inventory.InventoryBasic implements net.minecraft.world.IInteractionObject {
        boolean substituted,rejected,unlocked,second;
        PluginInventory(){
            super("§6서버 메뉴",true,9);
            setInventorySlotContents(0,new net.minecraft.item.ItemStack(net.minecraft.init.Items.diamond).setStackDisplayName("§d선택하기"));
            setInventorySlotContents(1,new net.minecraft.item.ItemStack(net.minecraft.init.Items.paper).setStackDisplayName("§b다음 메뉴"));
        }
        public String getGuiID(){return "minecraft:chest";}
        public net.minecraft.inventory.Container createContainer(net.minecraft.entity.player.InventoryPlayer bag,net.minecraft.entity.player.EntityPlayer owner){
            return new net.minecraft.inventory.ContainerChest(bag,this,owner){
                public net.minecraft.item.ItemStack slotClick(int slot,int button,int mode,net.minecraft.entity.player.EntityPlayer who){
                    if(slot==0 && button==0 && mode==0 && !substituted){
                        substituted=true;
                        PluginInventory.this.setInventorySlotContents(0,new net.minecraft.item.ItemStack(net.minecraft.init.Items.emerald,3).setStackDisplayName("§a서버 선택"));
                    }else if(slot==1 && button==0 && mode==0 && rejected && unlocked){second=true;}
                    return super.slotClick(slot,button,mode,who);
                }
                public void setCanCraft(net.minecraft.entity.player.EntityPlayer who,boolean allow){
                    super.setCanCraft(who,allow);
                    if(!allow && substituted)rejected=true;
                    if(allow && rejected)unlocked=true;
                }
            };
        }
    }
    static void run(String[] args) throws Throwable {
        File game = new File(args[0]).getCanonicalFile();
        // The local Java host needs LWJGL frames but does not need desktop focus.
        org.lwjgl.opengl.Display.setLocation(32000, 32000);
        VanillaFrameBenchmark.Client mc = MenuOracle.startClient(game,new File(args[1]).getCanonicalFile(),"1.8.9 loopback protocol test");
        mc.gameSettings.renderDistanceChunks=2;
        mc.gameSettings.limitFramerate=30;
        mc.gameSettings.difficulty=EnumDifficulty.NORMAL;
        mc.launchIntegratedServer("test","Loopback",new WorldSettings(189,WorldSettings.GameType.SURVIVAL,false,false,WorldType.FLAT));
        MethodHandle loop=VanillaFrameBenchmark.privateMethod(mc,"runGameLoop");
        long start=System.nanoTime();
        while(mc.thePlayer==null || mc.theWorld==null || mc.currentScreen!=null) {
            VanillaFrameBenchmark.runFrame(loop);
            VanillaFrameBenchmark.require(VanillaFrameBenchmark.seconds(start)<70,"Local host failed to join");
        }
        IntegratedServer server=mc.getIntegratedServer();
        final int port=Integer.parseInt(args[3]);
        server.addScheduledTask(() -> {
            server.setOnlineMode(false);
            server.worldServerForDimension(0).getGameRules().setOrCreateGameRule("doMobSpawning","false");
            try {
                server.getNetworkSystem().addLanEndpoint(InetAddress.getByName("127.0.0.1"),port);
            } catch(Exception error) { throw new RuntimeException(error); }
        }).get(15,TimeUnit.SECONDS);
        Path ready=Paths.get(args[2]);
        Files.write(ready,new byte[]{1});
        start=System.nanoTime();
        net.minecraft.entity.monster.EntityIronGolem[] golem={null};
        boolean[] hit={false};
        PluginInventory[] menu={null};boolean[] confirmNotice={false},successNotice={false};
        net.minecraft.entity.passive.EntityHorse[] horse={null};
        double[] ridingStart={0,0,0};boolean[] mounted={false},jumped={false},advanced={false};
        net.minecraft.util.BlockPos[] anvil={null};
        boolean[] input={false}, collected={false}, repaired={false};
        while(!Files.exists(ready.resolveSibling("stop"))) {
            VanillaFrameBenchmark.runFrame(loop);
            if(Boolean.getBoolean("ferrum.pluginProbe")){
                server.addScheduledTask(()->{
                    net.minecraft.entity.player.EntityPlayerMP p=server.getConfigurationManager().getPlayerByUsername("PluginProbe");
                    if(p==null)return;
                    if(menu[0]==null){
                        menu[0]=new PluginInventory();p.displayGUIChest(menu[0]);
                    }else if(!confirmNotice[0] && menu[0].unlocked && menu[0].rejected && p.openContainer.getCanCraft(p)){
                        net.minecraft.item.ItemStack cursor=p.inventory.getItemStack();
                        if(cursor!=null && cursor.getItem()==net.minecraft.init.Items.emerald && cursor.stackSize==3 && "§a서버 선택".equals(cursor.getDisplayName())){
                            p.addChatMessage(new net.minecraft.util.ChatComponentText("plugin-original-rejected-and-confirmed"));confirmNotice[0]=true;
                        }
                    }else if(!successNotice[0] && menu[0].second && p.openContainer.getCanCraft(p)){
                        net.minecraft.item.ItemStack cursor=p.inventory.getItemStack(),stack=menu[0].getStackInSlot(1);
                        if(cursor!=null && stack!=null && cursor.getItem()==net.minecraft.init.Items.paper && "§b다음 메뉴".equals(cursor.getDisplayName())
                                && stack.getItem()==net.minecraft.init.Items.emerald && stack.stackSize==3 && "§a서버 선택".equals(stack.getDisplayName())){
                            p.addChatMessage(new net.minecraft.util.ChatComponentText("plugin-original-next-click-accepted"));successNotice[0]=true;
                        }
                    }
                }).get(2,TimeUnit.SECONDS);
            }
            if(Boolean.getBoolean("ferrum.vehicleProbe")){
                server.addScheduledTask(()->{
                    net.minecraft.entity.player.EntityPlayerMP p=server.getConfigurationManager().getPlayerByUsername("HorseProbe");
                    if(p==null)return;
                    if(horse[0]==null){
                        WorldServer world=server.worldServerForDimension(0);int x=(int)Math.floor(p.posX)+3,z=(int)Math.floor(p.posZ);
                        world.getChunkFromBlockCoords(new net.minecraft.util.BlockPos(x,0,z));
                        double y=world.getHeight(new net.minecraft.util.BlockPos(x,0,z)).getY();
                        p.playerNetServerHandler.setPlayerLocation(x+.5,y,z+.5,0,0);
                        net.minecraft.entity.passive.EntityHorse h=new net.minecraft.entity.passive.EntityHorse(world);
                        h.setLocationAndAngles(x+.5,y,z+.5,0,0);h.setTamedBy(p);h.setHorseSaddled(true);
                        world.spawnEntityInWorld(h);horse[0]=h;ridingStart[0]=h.posX;ridingStart[1]=h.posY;ridingStart[2]=h.posZ;
                    }else if(!mounted[0] && horse[0].ticksExisted>=8){
                        p.mountEntity(horse[0]);mounted[0]=true;
                    }else if(p.ridingEntity==horse[0]){
                        if(!advanced[0] && Math.hypot(horse[0].posX-ridingStart[0],horse[0].posZ-ridingStart[2])>1.2){
                            advanced[0]=true;p.addChatMessage(new net.minecraft.util.ChatComponentText("oracle-horse-travel"));
                        }
                        if(!jumped[0] && horse[0].posY>ridingStart[1]+0.5){
                            jumped[0]=true;p.addChatMessage(new net.minecraft.util.ChatComponentText("oracle-horse-jump"));
                        }
                    }
                }).get(2,TimeUnit.SECONDS);
            }
            if(Boolean.getBoolean("ferrum.windowProbe")){
                server.addScheduledTask(()->{
                    net.minecraft.entity.player.EntityPlayerMP player=server.getConfigurationManager().getPlayerByUsername("WindowProbe");
                    if(player==null)return;
                    if(anvil[0]==null){
                        WorldServer world=server.worldServerForDimension(0);
                        int x=(int)Math.floor(player.posX)+4,z=(int)Math.floor(player.posZ);
                        net.minecraft.util.BlockPos feet=world.getHeight(new net.minecraft.util.BlockPos(x,0,z));
                        anvil[0]=feet.add(0,0,2);
                        world.setBlockState(anvil[0].down(),net.minecraft.init.Blocks.stone.getDefaultState());
                        world.setBlockState(anvil[0],net.minecraft.init.Blocks.anvil.getDefaultState());
                        player.addExperienceLevel(5);
                        player.playerNetServerHandler.setPlayerLocation(x+.5,feet.getY(),z+.5,0,25);
                    } else if(!input[0] && player.openContainer instanceof net.minecraft.inventory.ContainerRepair) {
                        // The Rust client opened the original GUI by looking at the block and using it.
                        player.openContainer.putStackInSlot(0,new net.minecraft.item.ItemStack(net.minecraft.init.Items.diamond_sword));
                        player.openContainer.detectAndSendChanges();
                        input[0]=true;
                    } else if(input[0] && !collected[0]) {
                        net.minecraft.item.ItemStack cursor=player.inventory.getItemStack();
                        if(cursor!=null && cursor.getItem()==net.minecraft.init.Items.diamond_sword
                            && "모루 189".equals(cursor.getDisplayName()) && player.experienceLevel==4) {
                            player.addChatMessage(new net.minecraft.util.ChatComponentText("oracle-anvil-collected-4"));
                            collected[0]=true;
                            player.inventory.addItemStackToInventory(cursor.copy());
                            player.inventory.setItemStack(null);
                            player.updateHeldItem();
                            player.openContainer.putStackInSlot(0,new net.minecraft.item.ItemStack(net.minecraft.init.Items.iron_pickaxe,1,140));
                            player.openContainer.putStackInSlot(1,new net.minecraft.item.ItemStack(net.minecraft.init.Items.iron_ingot,6));
                            player.openContainer.detectAndSendChanges();
                        }
                    } else if(collected[0] && !repaired[0]) {
                        net.minecraft.item.ItemStack cursor=player.inventory.getItemStack();
                        net.minecraft.item.ItemStack ingots=player.openContainer.getSlot(1).getStack();
                        if(cursor!=null && cursor.getItem()==net.minecraft.init.Items.iron_pickaxe && cursor.getItemDamage()==0
                            && !cursor.hasDisplayName() && ingots!=null && ingots.stackSize==3 && player.experienceLevel==1) {
                            player.addChatMessage(new net.minecraft.util.ChatComponentText("oracle-anvil-repaired-1"));
                            repaired[0]=true;
                        }
                    }
                }).get(2,TimeUnit.SECONDS);
            }
            if(Boolean.getBoolean("ferrum.mobProbe")){
                server.addScheduledTask(()->{
                    net.minecraft.entity.player.EntityPlayerMP player=server.getConfigurationManager().getPlayerByUsername("MobProbe");
                    if(player==null)return;
                    if(golem[0]==null){
                        WorldServer world=server.worldServerForDimension(0);
                        int x=(int)Math.floor(player.posX)+4,z=(int)Math.floor(player.posZ);
                        world.getChunkFromBlockCoords(new net.minecraft.util.BlockPos(x,0,z));
                        double y=world.getHeight(new net.minecraft.util.BlockPos(x,0,z)).getY();
                        player.playerNetServerHandler.setPlayerLocation(x+0.5,y,z+0.5,0,0);
                        golem[0]=new net.minecraft.entity.monster.EntityIronGolem(world);
                        golem[0].setNoAI(true);golem[0].setLocationAndAngles(x+0.5,y,z+2.75,180,0);
                        world.spawnEntityInWorld(golem[0]);
                    }else if(!hit[0] && golem[0].getHealth()<golem[0].getMaxHealth()){
                        player.addChatMessage(new net.minecraft.util.ChatComponentText("oracle-golem-hit-"+golem[0].getHealth()));hit[0]=true;
                    }
                }).get(2,TimeUnit.SECONDS);
            }
            VanillaFrameBenchmark.require(VanillaFrameBenchmark.seconds(start)<120,"Rust client did not finish");
        }
        mc.loadWorld(null);
        mc.shutdownMinecraftApplet();
    }
    public static void main(String[] args) {
        try { run(args); System.exit(0); }
        catch(Throwable t){ t.printStackTrace(); System.exit(1); }
    }
}
