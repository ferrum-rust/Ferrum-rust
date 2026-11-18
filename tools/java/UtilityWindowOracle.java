import com.google.gson.Gson;
import net.minecraft.client.gui.GuiHopper;
import net.minecraft.client.gui.inventory.*;
import net.minecraft.client.renderer.entity.RendererLivingEntity;
import net.minecraft.entity.passive.EntityHorse;
import net.minecraft.entity.player.InventoryPlayer;
import net.minecraft.inventory.*;
import net.minecraft.world.*;
import java.io.*;
import java.nio.file.*;
import java.util.*;

/** Captures the installed 1.8.9 container screens and their active slot hit boxes. */
public final class UtilityWindowOracle {
    static void writeHorseModels(VanillaFrameBenchmark.Client mc,Path output)throws Exception {
        Map<Integer,Object> models=new TreeMap<>();
        for(int type=0;type<3;type++)for(int bits=0;bits<16;bits++){
            boolean saddled=(bits&1)!=0, chested=(bits&2)!=0, ridden=(bits&4)!=0, foal=(bits&8)!=0;
            if(type==0&&chested||ridden&&!saddled||foal&&(saddled||chested||ridden))continue;
            EntityHorse h=new EntityHorse(mc.theWorld);h.setHorseType(type);h.setHorseVariant(0);
            h.setChested(chested);h.setHorseSaddled(saddled);h.setGrowingAge(foal?-24000:0);
            if(ridden)h.riddenByEntity=mc.thePlayer;
            RendererLivingEntity renderer=(RendererLivingEntity)mc.getRenderManager().getEntityRenderObject(h);
            List<int[]> rest=RemoteMobOracle.frame(h,renderer,renderer.getMainModel(),0,0);
            h.prevRotationYawHead=h.rotationYawHead=20;h.prevRotationPitch=h.rotationPitch=27;
            List<int[]> look=RemoteMobOracle.frame(h,renderer,renderer.getMainModel(),0,0);
            List<Object> triangles=new ArrayList<>();
            for(int i=0;i<rest.size();i++){
                boolean head=false;for(int n=0;n<15;n++)if(n%5<3&&Math.abs(rest.get(i)[n]-look.get(i)[n])>2)head=true;
                triangles.add(Arrays.asList(head,rest.get(i)));
            }
            models.put(type*16+bits,triangles);
        }
        try(Writer writer=Files.newBufferedWriter(output)){new Gson().toJson(models,writer);}
    }
    public static void main(String[] args) {
        try {
            File game = new File(args[0]);
            VanillaFrameBenchmark.Client mc = MenuOracle.startClient(game,new File(args[1]),"1.8.9 utility inventories");
            mc.gameSettings.renderDistanceChunks=2;
            mc.launchIntegratedServer("fixture","Fixture",new WorldSettings(189,WorldSettings.GameType.CREATIVE,false,false,WorldType.FLAT));
            java.lang.invoke.MethodHandle loop=VanillaFrameBenchmark.privateMethod(mc,"runGameLoop");
            long start=System.nanoTime();
            while(mc.thePlayer==null||mc.currentScreen!=null||mc.getRenderManager().livingPlayer!=mc.thePlayer) {
                VanillaFrameBenchmark.runFrame(loop);
                if(System.nanoTime()-start>70_000_000_000L)throw new IllegalStateException("player unavailable");
            }
            InventoryPlayer inv=mc.thePlayer.inventory;
            inv.clear();
            Map<String,Object> layouts=new LinkedHashMap<>(), captures=new LinkedHashMap<>();
            for(String name:new String[]{"dispenser","dropper","brewing","hopper","horse-inventory","donkey-inventory"}){
                GuiContainer screen;
                if(name.endsWith("-inventory")){
                    boolean donkey=name.startsWith("donkey");
                    EntityHorse horse=new EntityHorse(mc.theWorld);
                    horse.setHorseType(donkey?1:0);
                    horse.setHorseVariant(0);
                    horse.setChested(donkey);
                    screen=new GuiScreenHorseInventory(inv,new AnimalChest(horse.getDisplayName(),donkey?17:2),horse);
                } else if(name.equals("hopper")) {
                    screen=new GuiHopper(inv,new InventoryBasic("container.hopper",false,5));
                } else if(name.equals("brewing")) {
                    InventoryBasic stand=new InventoryBasic("container.brewing",false,4) {
                        public int getField(int id){return id==0?200:0;}
                    };
                    screen=new GuiBrewingStand(inv,stand);
                } else {
                    screen=new GuiDispenser(inv,new InventoryBasic("container."+name,false,9));
                }
                mc.displayGuiScreen(screen);
                List<Object> slots=new ArrayList<>();
                for(Slot slot:screen.inventorySlots.inventorySlots) if(slot.canBeHovered())
                    slots.add(Arrays.asList(slot.slotNumber,slot.xDisplayPosition,slot.yDisplayPosition));
                layouts.put(name,slots);
                // A normal EntityRenderer frame starts GUI drawing with world/item lighting disabled.
                // This harness draws several GuiContainers in one frame; the previous one leaves it enabled.
                net.minecraft.client.renderer.RenderHelper.disableStandardItemLighting();
                net.minecraft.client.renderer.GlStateManager.disableLighting();
                MenuOracle.record(mc,screen,name,game,captures);
            }
            try(Writer writer=Files.newBufferedWriter(Paths.get(args[2]))){new Gson().toJson(layouts,writer);}
            writeHorseModels(mc,Paths.get(args[2]).resolveSibling("horse-models.json"));
            System.exit(0);
        }catch(Throwable e){e.printStackTrace();System.exit(1);}
    }
}
