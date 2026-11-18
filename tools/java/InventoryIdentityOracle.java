// Identity relationships from actual ContainerChest.slotClick/InventoryPlayer.
import com.google.gson.GsonBuilder;
import java.util.*;
import net.minecraft.init.Bootstrap;
import net.minecraft.inventory.*;
import net.minecraft.item.*;

public final class InventoryIdentityOracle {
    static final class Fixture {
        final FoodOracle.Player player;
        final InventoryBasic chest;
        final ContainerChest window;
        final IdentityHashMap<ItemStack,Integer> identities=new IdentityHashMap<ItemStack,Integer>();
        final List<Object> steps=new ArrayList<Object>();
        final Map<String,Object> out=new LinkedHashMap<String,Object>();
        Fixture(String name)throws Exception {
            player=FoodOracle.player(2,true,false,false,20,20);player.capabilities.isCreativeMode=true;
            chest=new InventoryBasic("Oracle",false,27);window=new ContainerChest(player.inventory,chest,player);
            out.put("name",name);out.put("steps",steps);
        }
        Fixture p(int slot,int id,int count){player.inventory.mainInventory[slot]=new ItemStack(Item.getItemById(id),count);return this;}
        Fixture c(int slot,int id,int count){chest.setInventorySlotContents(slot,new ItemStack(Item.getItemById(id),count));return this;}
        Fixture cursor(int id,int count){player.inventory.setItemStack(new ItemStack(Item.getItemById(id),count));return this;}
        Object item(ItemStack s){if(s==null)return null;Integer identity=identities.get(s);if(identity==null){identity=identities.size()+1;identities.put(s,identity);}return Arrays.asList(Item.getIdFromItem(s.getItem()),s.stackSize,s.getMetadata(),identity);}
        Object snapshot(){List<Object> p=new ArrayList<Object>(),c=new ArrayList<Object>();for(ItemStack s:player.inventory.mainInventory)p.add(item(s));for(int i=0;i<27;i++)c.add(item(chest.getStackInSlot(i)));return Arrays.asList(p,c,item(player.inventory.getItemStack()));}
        Fixture begin(){out.put("initial",snapshot());return this;}
        Fixture click(int slot,int button,int mode){window.slotClick(slot,button,mode,player);steps.add(Arrays.asList(Arrays.asList("click",slot,button,mode),snapshot()));return this;}
        Fixture add(int id,int count){player.inventory.addItemStackToInventory(new ItemStack(Item.getItemById(id),count));steps.add(Arrays.asList(Arrays.asList("add",id,count),snapshot()));return this;}
    }
    public static void main(String[]args)throws Exception {
        Bootstrap.register();List<Object> cases=new ArrayList<Object>();
        cases.add(new Fixture("pickup-full-split-place-merge").p(0,260,12).p(1,260,60).begin().click(54,0,0).click(0,1,0).click(1,0,0).click(1,1,0).click(55,0,0).click(2,0,0).out);
        cases.add(new Fixture("hotbar-copies-then-moves").p(0,260,8).p(1,367,4).c(0,296,10).begin().click(55,0,2).click(1,0,2).click(0,1,2).out);
        cases.add(new Fixture("quickmove-copies-empty-retains-merged").p(8,260,60).c(0,260,12).begin().click(0,0,1).click(62,0,1).click(61,0,1).out);
        cases.add(new Fixture("drag-replaces-existing-and-cursor").c(1,260,60).cursor(260,17).begin().click(-999,0,5).click(0,1,5).click(1,1,5).click(-999,2,5).click(2,1,0).out);
        cases.add(new Fixture("swap-and-clone").c(0,367,3).cursor(260,4).begin().click(0,0,0).click(1,0,0).click(0,2,3).click(2,0,0).out);
        cases.add(new Fixture("outside-right-and-whole-throw").cursor(260,3).begin().click(-999,1,0).click(-999,0,0).out);
        cases.add(new Fixture("q-partial-and-whole").p(0,260,3).begin().click(54,0,4).click(54,1,4).out);
        cases.add(new Fixture("collect-retains-cursor").p(0,260,3).c(0,260,4).cursor(260,1).begin().click(1,0,6).out);
        cases.add(new Fixture("pickup-add-copies-empty").p(0,260,60).begin().add(260,12).add(276,1).out);
        System.out.println(new GsonBuilder().serializeNulls().create().toJson(cases));
    }
}
