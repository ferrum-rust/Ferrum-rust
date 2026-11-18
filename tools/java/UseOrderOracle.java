// Execute the actual server use callback, then EntityPlayer.onUpdate's original
// item-use prefix. The exporter independently inspects runTick bytecode order.
import com.google.gson.GsonBuilder;
import java.util.*;
import net.minecraft.init.Bootstrap;
import net.minecraft.item.*;
import net.minecraft.server.management.ItemInWorldManager;
import net.minecraft.world.WorldSettings;

public final class UseOrderOracle {
    static Object trace(int id,int meta,int pattern)throws Exception {
        ItemFoodOracle.Player player=ItemFoodOracle.player(189L,false,false,6);
        ItemInWorldManager manager=new ItemInWorldManager(player.worldObj);
        FoodOracle.set(manager,ItemInWorldManager.class,"gameType",WorldSettings.GameType.SURVIVAL);
        player.inventory.mainInventory[0]=new ItemStack(Item.getItemById(id),2,meta);
        int delay=pattern==1||pattern==2?4:0;boolean previous=delay!=0;
        List<Object> steps=new ArrayList<Object>();
        for(int tick=1;tick<=48;tick++){
            boolean right=!(pattern==2&&tick==1||pattern==3&&tick==12);
            if(pattern==4&&tick==12)player.inventory.mainInventory[0]=player.inventory.mainInventory[0].copy();
            if(pattern==5&&tick==12){player.inventory.mainInventory[1]=player.inventory.mainInventory[0];player.inventory.mainInventory[0]=null;player.inventory.currentItem=1;}
            if(delay>0)delay--;
            boolean pressed=right&&!previous;previous=right;
            boolean using=player.isUsingItem();
            if(using&&!right)player.stopUsingItem();
            boolean click=right&&!using&&(pressed||delay==0);
            if(click){delay=4;ItemStack held=player.inventory.getCurrentItem();if(held!=null)manager.tryUseItem(player,player.worldObj,held);}
            player.usePrefix();
            steps.add(Arrays.asList(right,delay,player.isUsingItem(),player.getItemInUseCount(),ItemFoodOracle.stack(player.inventory.getCurrentItem()),player.getFoodStats().getFoodLevel()));
        }
        return Arrays.asList(id,meta,pattern,steps);
    }
    public static void main(String[]args)throws Exception {
        Bootstrap.register();List<Object> rows=new ArrayList<Object>();
        for(int[] item:new int[][]{{260,0},{322,1},{282,0},{349,3}})for(int pattern=0;pattern<6;pattern++)rows.add(trace(item[0],item[1],pattern));
        System.out.println(new GsonBuilder().serializeNulls().create().toJson(rows));
    }
}
