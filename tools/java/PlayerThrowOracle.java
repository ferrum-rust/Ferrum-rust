import java.io.*;
import java.lang.reflect.Constructor;
import java.util.*;
import com.google.gson.GsonBuilder;
import net.minecraft.entity.item.EntityItem;
import net.minecraft.init.Bootstrap;
import net.minecraft.inventory.Slot;
import net.minecraft.item.ItemStack;
import net.minecraft.server.MinecraftServer;
import net.minecraft.world.WorldServer;

/** Reads actual Q and window throws before EntityItem.onUpdate, starting with known player RNG. */
public final class PlayerThrowOracle {
    public static void main(String[] args)throws Exception {
        Bootstrap.register();
        File dir=new File(args[1]);dir.mkdirs();
        Constructor<?> constructor=Class.forName("VanillaWorldBridge$Host").getDeclaredConstructor(File.class);
        constructor.setAccessible(true);
        MinecraftServer host=(MinecraftServer)constructor.newInstance(dir);
        RuntimeItemOracle.Fixture world=new RuntimeItemOracle.Fixture(host);
        host.worldServers=new WorldServer[]{world,null,null};world.init();
        world.theChunkProviderServer.loadChunk(-1,0);
        RuntimeItemOracle.Player player=new RuntimeItemOracle.Player(world);
        player.setPosition(-1.000000123456,71.23456789,8.7654321);
        List<Object> cases=new ArrayList<>();
        for(int seed:new int[]{0,189,-913})for(float[] look:new float[][]{{0,0},{137.3721f,-68.25f},{270,90}})for(boolean sneak:new boolean[]{false,true})for(String kind:new String[]{"Q","CtrlQ","slotQ","slotCtrlQ","outsideLeft","outsideRight","creative"}){
            RuntimeItemOracle.entityRandom(player).setSeed(seed);
            player.rotationYaw=look[0];player.rotationPitch=look[1];player.setSneaking(sneak);
            player.inventory.currentItem=3;player.events.clear();
            ItemStack stack=RuntimeItemOracle.stack(264,17,0,191);player.inventory.mainInventory[3]=null;player.inventory.setItemStack(null);
            int before=world.loadedEntityList.size();
            if(kind.equals("Q")||kind.equals("CtrlQ")){
                player.inventory.mainInventory[3]=stack;player.dropOneItem(kind.equals("CtrlQ"));
            }else if(kind.startsWith("slot")){
                player.inventory.mainInventory[3]=stack;
                player.inventoryContainer.slotClick(39,kind.equals("slotQ")?0:1,4,player);
            }else if(kind.startsWith("outside")){
                player.inventory.setItemStack(stack);
                player.inventoryContainer.slotClick(-999,kind.equals("outsideLeft")?0:1,0,player);
            }else{
                EntityItem i=player.dropPlayerItemWithRandomChoice(stack,true);i.setAgeToCreativeDespawnTime();
            }
            if(world.loadedEntityList.size()!=before+1)throw new AssertionError(kind);
            EntityItem i=(EntityItem)world.loadedEntityList.get(before);
            Map<String,Object> row=new LinkedHashMap<>();row.put("seed",seed);row.put("look",look);row.put("sneak",sneak);row.put("kind",kind);
            row.put("position",Arrays.asList(i.posX,i.posY,i.posZ));row.put("velocity",Arrays.asList(i.motionX,i.motionY,i.motionZ));
            row.put("state",RuntimeItemOracle.state(RuntimeItemOracle.entityRandom(player)));row.put("age",i.getAge());
            row.put("delay",RuntimeItemOracle.DELAY.getInt(i));row.put("thrower",i.getThrower());row.put("owner",i.getOwner());
            row.put("count",i.getEntityItem().stackSize);row.put("tag",i.getEntityItem().getTagCompound().getInteger("variant"));
            row.put("slot",player.inventory.mainInventory[3]==null?0:player.inventory.mainInventory[3].stackSize);
            row.put("cursor",player.inventory.getItemStack()==null?0:player.inventory.getItemStack().stackSize);
            row.put("stats",player.events.size());cases.add(row);
        }
        try(Writer out=new java.io.OutputStreamWriter(new java.util.zip.GZIPOutputStream(new FileOutputStream(args[0])),"UTF-8")){
            new GsonBuilder().serializeNulls().create().toJson(cases,out);
        }
        System.out.println("Original player throws: "+cases.size()+" inventory/pose/RNG combinations");
    }
}
