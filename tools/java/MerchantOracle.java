// Original MerchantRecipeList, InventoryMerchant and SlotMerchantResult calls.
import com.google.gson.GsonBuilder;
import java.io.*;
import java.util.*;
import net.minecraft.entity.IMerchant;
import net.minecraft.entity.player.EntityPlayer;
import net.minecraft.init.Bootstrap;
import net.minecraft.inventory.InventoryMerchant;
import net.minecraft.inventory.SlotMerchantResult;
import net.minecraft.item.*;
import net.minecraft.nbt.*;
import net.minecraft.util.*;
import net.minecraft.village.*;

public final class MerchantOracle {
    static final List<String> TAGS=new ArrayList<String>();
    static final Map<String,Integer> TAG_INDEX=new HashMap<String,Integer>();
    static NBTTagCompound tag(int kind) {
        if(kind==0)return null;
        NBTTagCompound root=new NBTTagCompound();
        if(kind>=2&&kind<=4){NBTTagCompound display=new NBTTagCompound();display.setString("Name",kind==4?"other":"required");if(kind==3){display.setInteger("extra",9);root.setLong("custom",17);}root.setTag("display",display);}
        if(kind>=5&&kind<=7){NBTTagList list=new NBTTagList();if(kind>=6){list.appendTag(new NBTTagInt(1));list.appendTag(new NBTTagInt(2));}if(kind==7)list.appendTag(new NBTTagInt(3));root.setTag("list",list);}
        if(kind==8)root.setByte("n",(byte)1);if(kind==9)root.setInteger("n",1);if(kind==10)root.setFloat("n",Float.NaN);
        if(kind==11){NBTTagList list=new NBTTagList();NBTTagCompound e=new NBTTagCompound();e.setShort("id",(short)16);e.setShort("lvl",(short)3);list.appendTag(e);root.setTag("StoredEnchantments",list);}
        return root;
    }
    static Integer intern(NBTTagCompound tag)throws Exception {
        if(tag==null)return null;
        ByteArrayOutputStream bytes=new ByteArrayOutputStream();CompressedStreamTools.write(tag,new DataOutputStream(bytes));StringBuilder hex=new StringBuilder();for(byte b:bytes.toByteArray())hex.append(String.format("%02x",b&255));
        String key=hex.toString();Integer id=TAG_INDEX.get(key);if(id==null){id=TAGS.size();TAGS.add(key);TAG_INDEX.put(key,id);}return id;
    }
    static ItemStack item(int id,int meta,int count,int shape){ItemStack s=new ItemStack(Item.getItemById(id),count,meta);s.setTagCompound(tag(shape));return s;}
    static Object stack(ItemStack s)throws Exception{return s==null?null:Arrays.asList(Item.getIdFromItem(s.getItem()),s.getMetadata(),s.stackSize,intern(s.getTagCompound()));}
    static MerchantRecipeList recipes(int variant){
        MerchantRecipeList list=new MerchantRecipeList();
        list.add(new MerchantRecipe(item(388,0,2,2),null,item(403,0,1,11),(variant&1)!=0?2:0,2));
        list.add(new MerchantRecipe(item(388,0,1,0),null,item(297,0,3,0),0,7));
        list.add(new MerchantRecipe(item(260,0,2,0),item(388,0,5,0),item(265,0,4,3),0,7));
        list.add(new MerchantRecipe(item(388,1,3,0),item(388,2,5,0),item(35,14,2,0),(variant&2)!=0?7:0,7));
        list.add(new MerchantRecipe(item(388,0,2,6),null,item(263,1,1,0),0,7));
        return list;
    }
    static final class Merchant implements IMerchant {
        final EntityPlayer customer;MerchantRecipeList recipes;
        Merchant(EntityPlayer customer,MerchantRecipeList recipes){this.customer=customer;this.recipes=recipes;}
        public void setCustomer(EntityPlayer p){}
        public EntityPlayer getCustomer(){return customer;}
        public MerchantRecipeList getRecipes(EntityPlayer p){return recipes;}
        public void setRecipes(MerchantRecipeList list){recipes=list;}
        public void useRecipe(MerchantRecipe recipe){recipe.incrementToolUses();}
        public void verifySellingItem(ItemStack stack){}
        public IChatComponent getDisplayName(){return new ChatComponentText("Oracle");}
    }
    static Object offers(MerchantRecipeList list)throws Exception {List<Object> result=new ArrayList<Object>();for(Object o:list){MerchantRecipe r=(MerchantRecipe)o;result.add(Arrays.asList(stack(r.getItemToBuy()),stack(r.getSecondItemToBuy()),stack(r.getItemToSell()),r.getToolUses(),r.getMaxTradeUses()));}return result;}
    static Object trade(EntityPlayer player,int variant,int mode,int shape,int selected,int count)throws Exception {
        Merchant merchant=new Merchant(player,recipes(variant));InventoryMerchant inv=new InventoryMerchant(player,merchant);SlotMerchantResult result=new SlotMerchantResult(player,merchant,inv,2,0,0);
        ItemStack left=null,right=null;
        switch(mode){
            case 0:left=item(388,0,count,shape);break;
            case 1:right=item(388,0,count,shape);break;
            case 2:left=item(260,0,count,shape);right=item(388,0,5,shape);break;
            case 3:left=item(388,0,5,shape);right=item(260,0,count,shape);break;
            case 4:left=item(388,1,3,shape);right=item(388,2,5,shape);break;
            case 5:left=item(388,2,5,shape);right=item(388,1,3,shape);break;
            case 6:left=item(388,0,count,shape);right=item(263,0,1,0);break;
            default:left=item(388,1,count,shape);break;
        }
        List<Object> inputs=Arrays.asList(stack(left),stack(right));inv.setCurrentRecipeIndex(selected);inv.setInventorySlotContents(0,left);inv.setInventorySlotContents(1,right);
        MerchantRecipe matched=inv.getCurrentRecipe();int index=matched==null?-1:merchant.recipes.indexOf(matched);Object before=stack(inv.getStackInSlot(2));boolean copy=matched!=null&&matched.getItemToSell()!=inv.getStackInSlot(2);
        if(matched!=null){ItemStack taken=result.decrStackSize(1);result.onPickupFromSlot(player,taken);}
        List<Integer> uses=new ArrayList<Integer>();for(Object r:merchant.recipes)uses.add(((MerchantRecipe)r).getToolUses());
        return Arrays.asList(variant,selected,inputs,index,before,copy,Arrays.asList(stack(inv.getStackInSlot(0)),stack(inv.getStackInSlot(1)),stack(inv.getStackInSlot(2))),uses);
    }
    public static void main(String[]args)throws Exception {
        Bootstrap.register();List<Object> comparisons=new ArrayList<Object>(),trades=new ArrayList<Object>(),variants=new ArrayList<Object>();
        for(int needed=0;needed<12;needed++)for(int given=0;given<12;given++)for(int meta=0;meta<2;meta++)for(int count:new int[]{1,2}){
            ItemStack required=item(388,1,2,needed),actual=item(388,meta,count,given);MerchantRecipe recipe=new MerchantRecipe(required,item(297,0,1,0));MerchantRecipeList list=new MerchantRecipeList();list.add(recipe);
            comparisons.add(Arrays.asList(stack(required),stack(actual),list.canRecipeBeUsed(actual,null,0)==recipe));
        }
        EntityPlayer player=FoodOracle.player(2,true,false,false,20,20);
        for(int variant=0;variant<4;variant++){variants.add(offers(recipes(variant)));for(int selected:new int[]{-1,0,1,2,3,4,9})for(int mode=0;mode<8;mode++)for(int shape:new int[]{0,2,3,4,6,7,11})for(int count:new int[]{1,3})trades.add(trade(player,variant,mode,shape,selected,count));}
        Map<String,Object> out=new LinkedHashMap<String,Object>();out.put("tags",TAGS);out.put("comparisons",comparisons);out.put("offers",variants);out.put("trades",trades);System.out.println(new GsonBuilder().serializeNulls().create().toJson(out));
    }
}
