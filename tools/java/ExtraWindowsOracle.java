import com.google.gson.GsonBuilder;
import java.io.File;
import java.lang.reflect.Constructor;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import net.minecraft.client.player.inventory.ContainerLocalMenu;
import net.minecraft.util.ChatComponentText;
import net.minecraft.entity.passive.EntityHorse;
import net.minecraft.init.Bootstrap;
import net.minecraft.inventory.*;
import net.minecraft.item.*;
import net.minecraft.server.MinecraftServer;
import net.minecraft.tileentity.*;
import net.minecraft.world.WorldServer;

/** Records actual Container.slotClick on a private WorldServer; player slots use server wire order. */
public final class ExtraWindowsOracle {
    static ItemStack item(int id,int count,int meta){return new ItemStack(Item.getItemById(id),count,meta);}
    static Map<String,Object> sample(MinecraftServer host,RuntimeUseOracle.Fixture world,String kind,int variant,boolean server)throws Exception {
        RuntimeUseOracle.Player player=new RuntimeUseOracle.Player(host,world);
        new RuntimeUseOracle.Handler(host,player);
        Container c;
        IInventory backing=new ContainerLocalMenu("minecraft:"+kind,new ChatComponentText(kind),
            kind.equals("brewing")?4:kind.equals("hopper")?5:9);
        switch(kind){
            case "brewing":c=new ContainerBrewingStand(player.inventory,server?new TileEntityBrewingStand():backing);break;
            case "hopper":c=new ContainerHopper(player.inventory,backing,player);break;
            case "dispenser":c=new ContainerDispenser(player.inventory,backing);break;
            default:
                boolean donkey=kind.equals("donkey");
                EntityHorse horse=new EntityHorse(world);horse.setHorseType(donkey?1:0);horse.setChested(donkey);
                IInventory chest=new AnimalChest(new ChatComponentText("Horse"),donkey?17:2);
                c=new ContainerHorseInventory(player.inventory,chest,horse,player);
        }
        int top=c.inventorySlots.size()-36;
        int[][] clicks;
        if(kind.equals("brewing")){
            if(variant==0){
                player.inventory.mainInventory[9]=item(374,6,0);player.inventory.mainInventory[10]=item(373,1,8193);
                player.inventory.mainInventory[11]=item(349,3,3);player.inventory.mainInventory[12]=item(349,4,0);player.inventory.mainInventory[0]=item(331,4,0);
                clicks=new int[][]{{top,0,1},{top+1,0,1},{top+2,0,1},{top+3,0,1},{top+27,0,1},{3,0,1},{0,1,0},{2,1,0},{3,0,0},{top+4,0,0}};
            }else{
                player.inventory.setItemStack(item(374,6,0));player.inventory.mainInventory[9]=item(350,3,3);c.getSlot(3).putStack(item(331,58,0));
                clicks=new int[][]{{3,0,0},{0,0,0},{1,1,0},{2,1,0},{top+1,0,0},{top,0,1},{3,0,1},{top+27,0,1},{1,0,1}};
            }
        }else if(kind.equals("horse")||kind.equals("donkey")){
            player.inventory.mainInventory[9]=item(329,1,0);player.inventory.mainInventory[10]=item(419,1,0);
            player.inventory.mainInventory[11]=item(418,1,0);player.inventory.mainInventory[12]=item(1,32,0);
            if(variant==0) clicks=new int[][]{{top,0,1},{top+1,0,1},{top+2,0,1},{top+3,0,1},{0,0,1},{1,0,1},{top+35,0,1}};
            else{player.inventory.setItemStack(item(1,5,0));clicks=new int[][]{{0,1,0},{1,0,0},{top+4,0,0},{top,0,0},{0,0,0},{top+1,0,0},{1,0,0},{top+5,0,0},{top+3,1,0},{top+4,0,0}};}
        }else {
            c.getSlot(0).putStack(item(1,12,0));player.inventory.mainInventory[8]=item(1,61,0);
            if(variant==0)clicks=new int[][]{{0,0,1},{top+35,0,1},{0,1,0},{2,1,0},{0,0,0},{2,0,1}};
            else{for(int i=0;i<36;i++)player.inventory.mainInventory[i]=item(4,64,0);player.inventory.mainInventory[8]=item(1,63,0);
                clicks=new int[][]{{0,0,1},{0,0,1},{top+35,0,1},{0,1,0},{1,1,0}};}
        }
        Map<String,Object> row=new LinkedHashMap<>();row.put("kind",kind);row.put("variant",variant);
        row.put("initial",RemoteWindowsOracle.snapshot(c,player));List<Object> steps=new ArrayList<>();
        for(int[] click:clicks){
            ItemStack result=c.slotClick(click[0],click[1],click[2],player);
            Map<String,Object> frame=RemoteWindowsOracle.snapshot(c,player);frame.put("click",click);frame.put("returned",RemoteWindowsOracle.stack(result));steps.add(frame);
        }
        row.put("steps",steps);return row;
    }
    public static void main(String[] args)throws Exception {
        Bootstrap.register();
        Constructor<?> c=Class.forName("VanillaWorldBridge$Host").getDeclaredConstructor(File.class);c.setAccessible(true);
        MinecraftServer host=(MinecraftServer)c.newInstance(new File(args[1]));
        RuntimeUseOracle.field(MinecraftServer.class,"serverThread").set(host,Thread.currentThread());
        RuntimeUseOracle.Fixture world=new RuntimeUseOracle.Fixture(host);host.worldServers=new WorldServer[]{world};world.init();world.theChunkProviderServer.loadChunk(0,0);
        List<Object> scenarios=new ArrayList<>();for(String kind:new String[]{"brewing","horse","donkey","hopper","dispenser"})for(int i=0;i<2;i++)scenarios.add(sample(host,world,kind,i,false));
        Map<Integer,List<Integer>> ingredients=new TreeMap<>();
        for(Item item:Item.itemRegistry){
            int start=-1;List<Integer> ranges=new ArrayList<>();ItemStack s=new ItemStack(item);
            for(int meta=0;meta<=32768;meta++){
                s.setItemDamage(meta);boolean ingredient=meta<32768 && item.isPotionIngredient(s);
                if(ingredient && start==-1)start=meta;
                if(!ingredient && start!=-1){ranges.add(start);ranges.add(meta-1);start=-1;}
            }
            if(!ranges.isEmpty())ingredients.put(Item.getIdFromItem(item),ranges);
        }
        Map<String,Object> data=new LinkedHashMap<>();data.put("ingredients",ingredients);data.put("cases",scenarios);
        // The original server's tile inventory overrides decrStackSize and ignores
        // the requested quantity. Its client instead uses ContainerLocalMenu.
        // Both sides still send the full original clicked stack in C0E.
        Map<String,Object> serverBrewing=sample(host,world,"brewing",0,true);
        data.put("server_brewing_split",((List<?>)serverBrewing.get("steps")).get(6));
        Files.write(Paths.get(args[0]),new GsonBuilder().serializeNulls().create().toJson(data).getBytes(StandardCharsets.UTF_8));
        System.out.println(ingredients);
    }
}
