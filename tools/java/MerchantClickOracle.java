// Actual ContainerMerchant.slotClick and onContainerClosed traces.
import com.google.gson.GsonBuilder;
import java.util.*;
import net.minecraft.entity.item.EntityItem;
import net.minecraft.init.Bootstrap;
import net.minecraft.inventory.ContainerMerchant;
import net.minecraft.item.ItemStack;
import net.minecraft.village.*;
import net.minecraft.world.World;

public final class MerchantClickOracle {
    static final class Player extends FoodOracle.Player {
        final List<Object> dropped=new ArrayList<Object>();
        Player(World world){super(world);}
        @Override public EntityItem dropPlayerItemWithRandomChoice(ItemStack item,boolean random){
            try{dropped.add(MerchantOracle.stack(item));}catch(Exception e){throw new AssertionError(e);}
            return null;
        }
    }
    static final class Case {
        final Player player;
        final MerchantOracle.Merchant merchant;
        final ContainerMerchant container;
        final Map<String,Object> result=new LinkedHashMap<String,Object>();
        final List<Object> steps=new ArrayList<Object>();
        Case(String name,int variant,boolean creative)throws Exception {
            player=new Player(FoodOracle.player(2,true,false,false,20,20).worldObj);
            player.capabilities.isCreativeMode=creative;
            merchant=new MerchantOracle.Merchant(player,MerchantOracle.recipes(variant));
            container=new ContainerMerchant(player.inventory,merchant,player.worldObj);
            result.put("name",name);result.put("offers",MerchantOracle.offers(merchant.recipes));result.put("creative",creative);result.put("steps",steps);
        }
        Case player(int slot,ItemStack item){player.inventory.setInventorySlotContents(slot,item);return this;}
        Case input(int slot,ItemStack item){container.getMerchantInventory().setInventorySlotContents(slot,item);return this;}
        Case cursor(ItemStack item){player.inventory.setItemStack(item);return this;}
        Case select(int selected){container.setCurrentRecipeIndex(selected);result.put("selected",selected);return this;}
        Object state()throws Exception {
            List<Object> inventory=new ArrayList<Object>(),slots=new ArrayList<Object>(),stock=new ArrayList<Object>();
            for(int i=0;i<36;i++)inventory.add(MerchantOracle.stack(player.inventory.getStackInSlot(i)));
            for(int i=0;i<3;i++)slots.add(MerchantOracle.stack(container.getMerchantInventory().getStackInSlot(i)));
            for(Object recipe:merchant.recipes)stock.add(((MerchantRecipe)recipe).getToolUses());
            return Arrays.asList(inventory,slots,MerchantOracle.stack(player.inventory.getItemStack()),stock,new ArrayList<Object>(player.dropped));
        }
        Case begin()throws Exception {result.put("initial",state());return this;}
        Case click(int slot,int button,int mode)throws Exception {
            container.slotClick(slot,button,mode,player);steps.add(Arrays.asList(Arrays.asList("click",slot,button,mode),state()));return this;
        }
        Case close()throws Exception {
            container.onContainerClosed(player);steps.add(Arrays.asList(Arrays.asList("close"),state()));return this;
        }
    }
    static ItemStack item(int id,int count,int tag){return MerchantOracle.item(id,0,count,tag);}
    public static void main(String[]args)throws Exception {
        Bootstrap.register();List<Object> cases=new ArrayList<Object>();
        cases.add(new Case("pickup-tagged-result",0,false).player(0,item(388,6,3)).select(0).begin().click(30,0,0).click(0,0,0).click(2,1,0).click(3,0,0).click(2,0,0).close().result);
        cases.add(new Case("shift-repeats-until-sold-out",0,false).input(0,item(388,6,3)).select(0).begin().click(2,0,1).close().result);
        Case partial=new Case("shift-partial-result",0,false).input(0,item(388,6,0)).select(1);
        for(int i=0;i<36;i++)partial.player(i,item(1,64,0));partial.player(8,item(297,63,0));
        cases.add(partial.begin().click(2,0,1).close().result);
        cases.add(new Case("hotbar-result-displaces-old-stack",0,false).input(0,item(388,2,3)).player(0,item(260,4,0)).select(0).begin().click(2,0,2).close().result);
        cases.add(new Case("hotbar-empty-result",0,false).input(0,item(388,2,3)).select(0).begin().click(2,3,2).close().result);
        cases.add(new Case("shift-input-and-player",0,false).input(0,item(388,5,3)).player(0,item(260,2,0)).select(0).begin().click(0,0,1).click(30,0,1).click(3,0,1).close().result);
        cases.add(new Case("reversed-same-id-costs",0,false).input(0,MerchantOracle.item(388,2,5,0)).input(1,MerchantOracle.item(388,1,3,0)).select(3).begin().click(2,0,0).close().result);
        cases.add(new Case("throw-result-whole",0,false).input(0,item(388,5,0)).select(1).begin().click(2,0,4).close().result);
        cases.add(new Case("creative-clone-preview",0,true).input(0,item(388,5,0)).select(1).begin().click(2,2,3).click(5,0,0).close().result);
        cases.add(new Case("drag-inputs",0,false).cursor(item(388,9,3)).select(0).begin().click(-999,0,5).click(0,1,5).click(1,1,5).click(2,1,5).click(-999,2,5).close().result);
        cases.add(new Case("collect-result-partially",0,false).input(0,item(388,5,0)).cursor(item(297,63,0)).select(1).begin().click(5,0,6).close().result);
        Map<String,Object> result=new LinkedHashMap<String,Object>();result.put("tags",MerchantOracle.TAGS);result.put("cases",cases);
        System.out.println(new GsonBuilder().serializeNulls().create().toJson(result));
    }
}
