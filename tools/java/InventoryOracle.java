// Test harness only. Executes the user's original Minecraft classes, not a copied slot algorithm.
import net.minecraft.init.Bootstrap;
import net.minecraft.entity.player.EntityPlayerMP;
import net.minecraft.entity.player.InventoryPlayer;
import net.minecraft.entity.player.PlayerCapabilities;
import net.minecraft.inventory.ContainerChest;
import net.minecraft.inventory.InventoryBasic;
import net.minecraft.item.Item;
import net.minecraft.item.ItemStack;
import com.google.gson.GsonBuilder;
import java.io.*;
import java.lang.reflect.Field;
import java.util.*;
import sun.misc.Unsafe;

public class InventoryOracle {
    static final List<Object> CASES = new ArrayList<Object>();
    static Object stack(ItemStack s) {
        if(s==null)return null;
        Map<String,Object> value=new LinkedHashMap<String,Object>();
        value.put("id",Item.getIdFromItem(s.getItem()));value.put("count",s.stackSize);value.put("damage",s.getItemDamage());
        return value;
    }
    static ItemStack item(int id,int count) {return new ItemStack(Item.getItemById(id),count,0);}
    static class Fixture {
        final EntityPlayerMP player;
        final InventoryBasic chest;
        final ContainerChest container;
        final Map<String,Object> output=new LinkedHashMap<String,Object>();
        final List<Object> steps=new ArrayList<Object>();
        Fixture(String name,int size,boolean creative) throws Exception {
            Field field=Unsafe.class.getDeclaredField("theUnsafe");field.setAccessible(true);
            player=(EntityPlayerMP)((Unsafe)field.get(null)).allocateInstance(EntityPlayerMP.class);
            player.inventory=new InventoryPlayer(player);player.capabilities=new PlayerCapabilities();player.capabilities.isCreativeMode=creative;
            chest=new InventoryBasic("Oracle",false,size);container=new ContainerChest(player.inventory,chest,player);
            output.put("name",name);output.put("size",size);output.put("creative",creative);output.put("steps",steps);
        }
        Map<String,Object> snapshot() {
            Map<String,Object> result=new LinkedHashMap<String,Object>();
            List<Object> p=new ArrayList<Object>(),c=new ArrayList<Object>();
            for(int i=0;i<36;i++)p.add(stack(player.inventory.getStackInSlot(i)));
            for(int i=0;i<chest.getSizeInventory();i++)c.add(stack(chest.getStackInSlot(i)));
            result.put("player",p);result.put("chest",c);result.put("cursor",stack(player.inventory.getItemStack()));return result;
        }
        Fixture c(int slot,int id,int count) {chest.setInventorySlotContents(slot,item(id,count));return this;}
        Fixture p(int slot,int id,int count) {player.inventory.setInventorySlotContents(slot,item(id,count));return this;}
        Fixture cursor(int id,int count) {player.inventory.setItemStack(item(id,count));return this;}
        Fixture begin() {output.put("initial",snapshot());return this;}
        Fixture click(int slot,int button,int mode) {
            container.slotClick(slot,button,mode,player);
            Map<String,Object> step=snapshot();step.put("slot",slot);step.put("button",button);step.put("mode",mode);steps.add(step);return this;
        }
        void end() {CASES.add(output);}
    }
    public static void main(String[] args) throws Exception {
        Bootstrap.register();
        new Fixture("split_merge",27,false).c(0,1,63).c(1,1,4).cursor(1,5).begin().click(0,0,0).click(2,1,0).click(1,0,0).click(0,1,0).click(3,0,0).end();
        new Fixture("chest_quickmove_reverse",27,false).c(0,265,12).p(8,265,60).begin().click(0,0,1).end();
        new Fixture("inventory_quickmove_forward",27,false).c(0,263,63).p(0,263,16).begin().click(54,0,1).end();
        new Fixture("hotbar_displaces_to_inventory",27,false).c(0,296,10).p(0,260,8).begin().click(0,0,2).end();
        Fixture full=new Fixture("hotbar_full_inventory_rejects",27,false).c(0,296,10);
        for(int i=0;i<36;i++)full.p(i,1,64);full.begin().click(0,0,2).end();
        new Fixture("collect_small_stacks_before_full",27,false).c(0,1,64).c(2,1,2).p(0,1,3).cursor(1,1).begin().click(1,0,6).end();
        new Fixture("collect_reverse",27,false).c(0,1,4).c(2,1,3).p(8,1,64).cursor(1,60).begin().click(1,1,6).end();
        new Fixture("drag_even_respects_capacity",27,false).c(1,1,63).cursor(1,11).begin().click(-999,0,5).click(0,1,5).click(1,1,5).click(-999,2,5).end();
        new Fixture("drag_right",27,false).cursor(1,5).begin().click(-999,4,5).click(0,5,5).click(1,5,5).click(2,5,5).click(-999,6,5).end();
        new Fixture("creative_clone",27,true).c(0,296,3).begin().click(0,2,3).end();
        new Fixture("nonstackable_sword",27,false).c(0,276,1).begin().click(0,1,0).click(1,1,0).end();
        new Fixture("double_chest_order",54,false).c(53,1,20).p(8,1,60).begin().click(53,0,1).click(88,0,1).end();
        File file=new File(args[0]);file.getParentFile().mkdirs();
        try(Writer writer=new OutputStreamWriter(new FileOutputStream(file),"UTF-8")) {new GsonBuilder().serializeNulls().setPrettyPrinting().create().toJson(CASES,writer);}
        System.out.println("Original ContainerChest oracle: "+CASES.size()+" cases -> "+file);
    }
}
