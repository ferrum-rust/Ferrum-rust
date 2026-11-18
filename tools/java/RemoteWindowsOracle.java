import com.google.gson.GsonBuilder;
import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import net.minecraft.init.Bootstrap;
import net.minecraft.inventory.*;
import net.minecraft.item.*;
import net.minecraft.network.play.client.C0EPacketClickWindow;
import net.minecraft.server.MinecraftServer;
import net.minecraft.tileentity.TileEntityFurnace;
import net.minecraft.util.BlockPos;
import net.minecraft.world.WorldServer;

/** Captures the vanilla prediction return that PlayerControllerMP places in C0E. */
public final class RemoteWindowsOracle {
    static List<Integer> stack(ItemStack item) {
        return item == null ? null : Arrays.asList(Item.getIdFromItem(item.getItem()),item.stackSize,item.getItemDamage());
    }
    static ItemStack item(int id,int count){return new ItemStack(Item.getItemById(id),count,0);}
    static Map<String,Object> snapshot(Container c,RuntimeUseOracle.Player p) {
        Map<String,Object> frame=new LinkedHashMap<>();List<Object> slots=new ArrayList<>();
        for(int i=0;i<c.inventorySlots.size();i++)slots.add(stack(c.getSlot(i).getStack()));
        frame.put("slots",slots);frame.put("cursor",stack(p.inventory.getItemStack()));return frame;
    }
    static Map<String,Object> sample(MinecraftServer host,RuntimeUseOracle.Fixture world,String name) {
        RuntimeUseOracle.Player p=new RuntimeUseOracle.Player(host,world);
        new RuntimeUseOracle.Handler(host,p);
        String kind=name.split("_")[0]; Container c;
        if(kind.equals("chest"))c=new ContainerChest(p.inventory,new InventoryBasic("Chest",false,27),p);
        else if(kind.equals("furnace"))c=new ContainerFurnace(p.inventory,new TileEntityFurnace());
        else if(kind.equals("crafting"))c=new ContainerWorkbench(p.inventory,world,BlockPos.ORIGIN);
        else c=p.inventoryContainer;
        int total=c.inventorySlots.size(),bin=total-36;int[][] actions;
        switch(name){
            case "chest_collect":
                c.getSlot(0).putStack(item(1,64));c.getSlot(2).putStack(item(1,2));c.getSlot(bin+33).putStack(item(1,3));
                p.inventory.setItemStack(item(1,1));actions=new int[][]{{1,0,6},{0,0,6}};break;
            case "chest_noop":
                for(int i=0;i<36;i++)p.inventory.mainInventory[i]=item(4,64);
                c.getSlot(0).putStack(item(1,8));actions=new int[][]{{0,0,1},{1,0,1},{0,1,0},{0,0,0}};break;
            case "chest_moves":
                c.getSlot(0).putStack(item(265,12));p.inventory.mainInventory[8]=item(265,60);
                actions=new int[][]{{0,0,1},{total-1,0,1},{0,1,0},{3,0,0},{0,1,2},{3,0,4}};break;
            case "furnace_output":
                c.getSlot(2).putStack(item(20,12));p.inventory.mainInventory[8]=item(20,61);
                actions=new int[][]{{2,0,1},{total-1,0,0},{2,0,0},{0,1,0}};break;
            case "furnace_partial":
                for(int i=0;i<36;i++)p.inventory.mainInventory[i]=item(4,64);
                p.inventory.mainInventory[8]=item(20,63);c.getSlot(2).putStack(item(20,12));
                actions=new int[][]{{2,0,1},{2,0,1},{2,1,0}};break;
            case "furnace_inputs":
                p.inventory.mainInventory[0]=item(15,5);p.inventory.mainInventory[9]=item(263,4);
                actions=new int[][]{{bin+27,0,1},{bin,0,1},{0,0,1},{1,0,1}};break;
            case "crafting_partial":case "personal_partial":
                for(int i=0;i<36;i++)p.inventory.mainInventory[i]=item(4,64);
                p.inventory.mainInventory[8]=item(5,63);c.getSlot(1).putStack(item(17,3));
                actions=new int[][]{{0,0,1},{0,0,1},{0,1,0},{0,0,0}};break;
            case "crafting_place":case "personal_place":
                p.inventory.setItemStack(item(17,2));actions=new int[][]{{1,1,0},{2,1,0},{1,0,0},{1,0,0},{2,0,0},{3,0,0},{0,0,0}};break;
            default:throw new AssertionError(name);
        }
        Map<String,Object> row=new LinkedHashMap<>();row.put("name",name);row.put("kind",kind);
        row.put("initial",snapshot(c,p));List<Object> steps=new ArrayList<>();
        int id=kind.equals("personal")?0:5;
        for(int[] a:actions){
            short txn=c.getNextTransactionID(p.inventory);
            ItemStack result=c.slotClick(a[0],a[1],a[2],p);
            C0EPacketClickWindow packet=new C0EPacketClickWindow(id,a[0],a[1],a[2],result,txn);
            Map<String,Object> step=snapshot(c,p);step.put("click",a);step.put("txn",packet.getActionNumber());
            step.put("clicked",stack(packet.getClickedItem()));steps.add(step);
        }
        row.put("steps",steps);return row;
    }
    public static void main(String[] args)throws Exception {
        Bootstrap.register();
        Constructor<?> constructor=Class.forName("VanillaWorldBridge$Host").getDeclaredConstructor(File.class);
        constructor.setAccessible(true);MinecraftServer host=(MinecraftServer)constructor.newInstance(new File(args[1]));
        RuntimeUseOracle.field(MinecraftServer.class,"serverThread").set(host,Thread.currentThread());
        RuntimeUseOracle.Fixture world=new RuntimeUseOracle.Fixture(host);
        host.worldServers=new WorldServer[]{world,null,null};world.init();world.theChunkProviderServer.loadChunk(0,0);
        List<Object> result=new ArrayList<>();
        for(String name:new String[]{"chest_collect","chest_noop","chest_moves","furnace_output","furnace_partial","furnace_inputs","crafting_partial","personal_partial","crafting_place","personal_place"})
            result.add(sample(host,world,name));
        try(Writer out=new OutputStreamWriter(new FileOutputStream(args[0]),"UTF-8")){new GsonBuilder().serializeNulls().create().toJson(result,out);}
        System.out.println("Generated "+result.size()+" original container traces");
    }
}
