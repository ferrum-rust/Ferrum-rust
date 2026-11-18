import com.google.gson.Gson;
import java.io.PrintStream;
import java.util.*;
import net.minecraft.enchantment.Enchantment;
import net.minecraft.init.Bootstrap;
import net.minecraft.item.*;
import net.minecraft.potion.Potion;
import net.minecraft.util.ResourceLocation;

/** Command argument registries and applicability of the installed 1.8.9 items. */
public final class CommandItemOracle {
    public static void main(String[] args) {
        PrintStream out=System.out;System.setOut(System.err);Bootstrap.register();
        Map<String,Object> items=new TreeMap<>(),effects=new TreeMap<>(),enchantments=new TreeMap<>(),data=new LinkedHashMap<>();
        for(Item item:Item.itemRegistry)
            items.put(Item.itemRegistry.getNameForObject(item).toString(),Item.getIdFromItem(item));
        for(ResourceLocation name:Potion.getPotionLocations()){
            Potion p=Potion.getPotionFromResourceLocation(name.toString());
            effects.put(name.toString(),Arrays.asList(p.id,p.getName(),p.isInstant()));
        }
        for(ResourceLocation name:Enchantment.func_181077_c()){
            Enchantment e=Enchantment.getEnchantmentByLocation(name.toString());
            List<Integer> applicable=new ArrayList<>(),conflicts=new ArrayList<>();
            for(Item item:Item.itemRegistry)if(e.canApply(new ItemStack(item)))applicable.add(Item.getIdFromItem(item));
            for(ResourceLocation k:Enchantment.func_181077_c()){
                Enchantment other=Enchantment.getEnchantmentByLocation(k.toString());
                if(!other.canApplyTogether(e))conflicts.add(other.effectId);
            }
            Collections.sort(applicable);Collections.sort(conflicts);
            enchantments.put(name.toString(),Arrays.asList(e.effectId,e.getName(),e.getMinLevel(),e.getMaxLevel(),applicable,conflicts));
        }
        data.put("items",items);data.put("effects",effects);data.put("enchantments",enchantments);
        out.println(new Gson().toJson(data));
    }
}
