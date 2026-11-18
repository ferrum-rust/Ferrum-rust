// Execute the original career tables and the private populateBuyingList method.
// Constructors have independent random state. The fixture sets a reproducible
// post-constructor entity RNG before the first call to getRecipes.
import com.google.gson.GsonBuilder;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.util.*;
import java.util.concurrent.atomic.AtomicLong;
import net.minecraft.enchantment.Enchantment;
import net.minecraft.entity.Entity;
import net.minecraft.entity.passive.EntityVillager;
import net.minecraft.init.Bootstrap;
import net.minecraft.item.Item;
import net.minecraft.item.ItemStack;
import net.minecraft.util.Tuple;
import net.minecraft.village.MerchantRecipe;
import net.minecraft.village.MerchantRecipeList;

public final class VillagerCareerOracle {
    static Object field(Object object, String name) throws Exception {
        Field field=object.getClass().getDeclaredField(name);
        field.setAccessible(true);
        return field.get(object);
    }
    static int integer(EntityVillager villager,String name) throws Exception {
        Field field=EntityVillager.class.getDeclaredField(name);
        field.setAccessible(true);
        return field.getInt(villager);
    }
    static String tail(Random random) throws Exception {
        Field field=Random.class.getDeclaredField("seed");
        field.setAccessible(true);
        long internal=((AtomicLong)field.get(random)).get();
        return Long.toString(new Random(internal^0x5deece66dL).nextLong());
    }
    static List<Integer> price(Object value) {
        if(value==null)return Arrays.asList(1,1);
        Tuple<?,?> tuple=(Tuple<?,?>)value;
        return Arrays.asList((Integer)tuple.getFirst(),(Integer)tuple.getSecond());
    }
    static List<Integer> item(Object value) {
        ItemStack stack=value instanceof ItemStack?(ItemStack)value:new ItemStack((Item)value);
        return Arrays.asList(Item.getIdFromItem(stack.getItem()),stack.getMetadata());
    }
    static Object rule(Object entry) throws Exception {
        String kind=entry.getClass().getSimpleName();
        switch(kind){
            case "EmeraldForItems":return Arrays.asList("buy",item(field(entry,"sellItem")),price(field(entry,"price")));
            case "ListItemForEmeralds":return Arrays.asList("sell",item(field(entry,"itemToBuy")),price(field(entry,"priceInfo")));
            case "ListEnchantedItemForEmeralds":return Arrays.asList("gear",item(field(entry,"enchantedItemStack")),price(field(entry,"priceInfo")));
            case "ItemAndEmeraldToItem":return Arrays.asList("exchange",item(field(entry,"buyingItemStack")),price(field(entry,"buyingPriceInfo")),item(field(entry,"sellingItemstack")),price(field(entry,"field_179408_d")));
            case "ListEnchantedBookForEmeralds":return Arrays.asList("book");
            default:throw new AssertionError(kind);
        }
    }
    static Object registry(Object[][][][] table) throws Exception {
        List<Object> professions=new ArrayList<Object>();
        for(Object[][][] profession:table){
            List<Object> careers=new ArrayList<Object>();
            for(Object[][] career:profession){
                List<Object> levels=new ArrayList<Object>();
                for(Object[] level:career){
                    List<Object> rules=new ArrayList<Object>();
                    for(Object entry:level)rules.add(rule(entry));
                    levels.add(rules);
                }
                careers.add(levels);
            }
            professions.add(careers);
        }
        return professions;
    }
    static Object offer(MerchantRecipe r) throws Exception {
        return Arrays.asList(MerchantOracle.stack(r.getItemToBuy()),MerchantOracle.stack(r.getSecondItemToBuy()),MerchantOracle.stack(r.getItemToSell()),r.getToolUses(),r.getMaxTradeUses(),r.getRewardsExp());
    }
    static Object career(int profession,int levels,long seed,Method populate) throws Exception {
        FoodOracle.Player player=FoodOracle.player(2,true,false,false,20,20);
        EntityVillager villager=new EntityVillager(player.worldObj,profession);
        Random random=new Random(seed);
        FoodOracle.set(villager,Entity.class,"rand",random);
        List<Object> steps=new ArrayList<Object>();
        int previous=0;
        for(int level=0;level<levels+2;level++){
            if(level!=0)populate.invoke(villager);
            MerchantRecipeList recipes=villager.getRecipes(player);
            List<Object> added=new ArrayList<Object>();
            for(int i=previous;i<recipes.size();i++)added.add(offer((MerchantRecipe)recipes.get(i)));
            previous=recipes.size();
            steps.add(Arrays.asList(integer(villager,"careerId"),integer(villager,"careerLevel"),added,tail(random)));
        }
        return Arrays.asList(profession,Long.toString(seed),steps);
    }
    public static void main(String[]args)throws Exception {
        Bootstrap.register();
        Field tableField=EntityVillager.class.getDeclaredField("DEFAULT_TRADE_LIST_MAP");
        tableField.setAccessible(true);
        Object[][][][] table=(Object[][][][])tableField.get(null);
        Method populate=EntityVillager.class.getDeclaredMethod("populateBuyingList");
        populate.setAccessible(true);
        List<Object> cases=new ArrayList<Object>();
        for(int profession=0;profession<table.length;profession++){
            for(int career=0;career<table[profession].length;career++){
                int found=0;
                for(long n=0;found<32;n++){
                    long seed=n*918273645L+189;
                    if(new Random(seed).nextInt(table[profession].length)!=career)continue;
                    cases.add(career(profession,table[profession][career].length,seed,populate));
                    found++;
                }
            }
        }
        List<Object> items=new ArrayList<Object>(),bookPool=new ArrayList<Object>();
        for(Object object:Item.itemRegistry){
            Item item=(Item)object;
            if(item.getItemEnchantability()<=0)continue;
            List<Integer> applicable=new ArrayList<Integer>();
            for(Enchantment enchantment:Enchantment.enchantmentsBookList){
                if(item==net.minecraft.init.Items.book||enchantment.type.canEnchantItem(item))applicable.add(enchantment.effectId);
            }
            items.add(Arrays.asList(Item.getIdFromItem(item),item.getItemEnchantability(),applicable));
        }
        for(Enchantment enchantment:Enchantment.enchantmentsBookList)bookPool.add(Arrays.asList(enchantment.effectId,enchantment.getMinLevel(),enchantment.getMaxLevel()));
        Map<String,Object> out=new LinkedHashMap<String,Object>();
        out.put("registry",registry(table));out.put("items",items);out.put("book_pool",bookPool);out.put("tags",MerchantOracle.TAGS);out.put("careers",cases);
        System.out.println(new GsonBuilder().serializeNulls().create().toJson(out));
    }
}
