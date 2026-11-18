// End-to-end original anvil/falling-block hits, including helmet wear before
// the stronger-hit test and the three independent random streams.
import com.google.gson.Gson;
import java.lang.reflect.Field;
import java.util.*;
import net.minecraft.entity.EntityLivingBase;
import net.minecraft.init.*;
import net.minecraft.item.*;
import net.minecraft.potion.PotionEffect;
import net.minecraft.util.DamageSource;

public final class ArmorImpactOracle {
    static Map<String,Object> hit(int pattern,boolean anvil,float amount,long seed)throws Exception {
        boolean creative=pattern%13==0,unbreakable=(pattern&2)!=0,blocking=(pattern&4)!=0;
        ArmorOracle.Player p=ArmorOracle.player(seed,creative);
        int[] ids={301,300,311,new int[]{0,310,299,86,267,397}[pattern%6]};
        List<Object> initial=new ArrayList<Object>();
        for(int slot=0;slot<4;slot++){
            int id=ids[slot];if(id==0||slot<3&&(pattern&1)==0){initial.add(null);continue;}
            int maximum=Item.getItemById(id).getMaxDamage();int damage=Math.max(0,maximum-((pattern&8)!=0?0:3));
            int level=pattern%4;ItemStack s=ArmorOracle.item(id,1,damage,level,unbreakable);
            ArmorOracle.enchant(s,0,1+pattern%5);ArmorOracle.enchant(s,99,-1);p.inventory.armorInventory[slot]=s;
            initial.add(Arrays.asList(id,damage,Arrays.asList(Arrays.asList(34,level),Arrays.asList(0,1+pattern%5),Arrays.asList(99,-1))));
        }
        int resistance=new int[]{-99,0,1,4}[pattern>>4&3];if(resistance!=-99)p.addPotionEffect(new PotionEffect(11,100,resistance));
        float absorption=2f*(pattern%3);p.setAbsorptionAmount(absorption);
        if(blocking){p.inventory.mainInventory[0]=new ItemStack(Items.iron_sword);p.setItemInUse(p.inventory.mainInventory[0],72000);}
        int timer=new int[]{0,10,16,20}[pattern>>3&3];float last=new float[]{0,1,4,9}[pattern%4];p.hurtResistantTime=timer;
        Field previous=EntityLivingBase.class.getDeclaredField("lastDamage");previous.setAccessible(true);previous.setFloat(p,last);
        Random enchantment=ArmorOracle.protectionRandom();enchantment.setSeed(seed^0x55555L);
        boolean accepted=p.attackEntityFrom(anvil?DamageSource.anvil:DamageSource.fallingBlock,amount);
        Map<String,Object> row=ArmorOracle.result(p);row.put("initial",initial);row.put("creative",creative);row.put("unbreakable",unbreakable);row.put("blocking",blocking);row.put("resistance",resistance);row.put("initial_absorption",FoodOracle.bits(absorption));row.put("initial_timer",timer);row.put("initial_last",FoodOracle.bits(last));row.put("anvil",anvil);row.put("amount",FoodOracle.bits(amount));row.put("seed",Long.toString(seed));row.put("accepted",accepted);row.put("armor",ArmorOracle.stacks(p.inventory.armorInventory));row.put("health",FoodOracle.bits(p.getHealth()));row.put("absorption",FoodOracle.bits(p.getAbsorptionAmount()));row.put("exhaustion",FoodOracle.snapshot(p).get("exhaustion"));row.put("timer",p.hurtResistantTime);row.put("last",FoodOracle.bits(previous.getFloat(p)));row.put("enchantment_tail",Long.toString(enchantment.nextLong()));return row;
    }
    public static void main(String[]args)throws Exception {
        Bootstrap.register();List<Object> cases=new ArrayList<Object>();
        for(int pattern=0;pattern<96;pattern++)for(boolean anvil:new boolean[]{false,true})for(float amount:new float[]{0f,1f,4f,7.5f,40f})cases.add(hit(pattern,anvil,amount,pattern*7919L));
        System.out.println(new Gson().toJson(cases));
    }
}
