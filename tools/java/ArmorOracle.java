// Original item durability, armor damage and enchantment protection callbacks.
import com.google.gson.Gson;
import java.lang.reflect.*;
import java.util.*;
import net.minecraft.init.*;
import net.minecraft.entity.*;
import net.minecraft.item.*;
import net.minecraft.nbt.*;
import net.minecraft.enchantment.*;
import net.minecraft.util.*;
import net.minecraft.world.World;

public final class ArmorOracle {
    static final class Source extends DamageSource {Source(String name){super(name);}}
    static final class Player extends FoodOracle.Player {
        List<Object> sounds=new ArrayList<Object>();
        List<Object> breaks=new ArrayList<Object>();
        Player(World world){super(world);}
        @Override public void playSound(String name,float volume,float pitch){sounds.add(Arrays.asList(name,FoodOracle.bits(volume),FoodOracle.bits(pitch)));}
        @Override public void renderBrokenItemStack(ItemStack stack){breaks.add(stack(stack));super.renderBrokenItemStack(stack);}
        void defend(DamageSource source,float amount){damageEntity(source,amount);}
    }
    static Player player(long seed,boolean creative)throws Exception {
        FoodOracle.Player base=FoodOracle.player(2,true,creative,false,20,20);FoodOracle.set(base.worldObj,World.class,"worldAccesses",new ArrayList<Object>());FoodOracle.set(base.worldObj,World.class,"rand",new Random(seed^0x123456789abcdefL));
        Player p=new Player(base.worldObj);p.capabilities.isCreativeMode=creative;p.capabilities.disableDamage=creative;FoodOracle.set(p,Entity.class,"rand",new Random(seed));return p;
    }
    static ItemStack item(int id,int count,int damage,int unbreaking,boolean unbreakable){
        ItemStack s=new ItemStack(Item.getItemById(id),count,damage);NBTTagCompound tag=new NBTTagCompound();tag.setBoolean("Unbreakable",unbreakable);NBTTagList list=new NBTTagList();NBTTagCompound e=new NBTTagCompound();e.setShort("id",(short)34);e.setShort("lvl",(short)unbreaking);list.appendTag(e);tag.setTag("ench",list);s.setTagCompound(tag);return s;
    }
    static void enchant(ItemStack stack,int id,int level){NBTTagList list=stack.getEnchantmentTagList();NBTTagCompound e=new NBTTagCompound();e.setShort("id",(short)id);e.setShort("lvl",(short)level);list.appendTag(e);}
    static Object stack(ItemStack s){return s==null?null:Arrays.asList(Item.getIdFromItem(s.getItem()),s.stackSize,s.getItemDamage());}
    static Object stacks(ItemStack[] items){List<Object> out=new ArrayList<Object>();for(ItemStack s:items)out.add(stack(s));return out;}
    static Map<String,Object> result(Player p){Map<String,Object> out=new LinkedHashMap<String,Object>();out.put("breaks",p.breaks);out.put("sounds",p.sounds);out.put("entity_tail",Long.toString(p.getRNG().nextLong()));out.put("world_tail",Long.toString(p.worldObj.rand.nextLong()));return out;}
    static Object damage(int id,int initial,int amount,int level,boolean unbreakable,boolean creative,long seed)throws Exception {
        Player p=player(seed,creative);ItemStack s=item(id,2,initial,level,unbreakable);p.inventory.mainInventory[0]=s;s.damageItem(amount,p);Map<String,Object> out=result(p);out.put("input",Arrays.asList(id,initial,amount,level,unbreakable,creative,Long.toString(seed)));out.put("stack",stack(s));return out;
    }
    static Object armor(int[] ids,float amount,int pattern,long seed)throws Exception {
        boolean creative=(pattern&1)!=0,unbreakable=(pattern&2)!=0;int level=new int[]{0,1,3,10}[pattern>>2&3];Player p=player(seed,creative);
        List<Object> initial=new ArrayList<Object>();for(int i=0;i<4;i++){int id=ids[i];int max=Item.getItemById(id).getMaxDamage();int damage=(pattern&16)!=0?max:Math.max(0,max-2);p.inventory.armorInventory[i]=item(id,1,damage,level,unbreakable);initial.add(Arrays.asList(id,damage));}
        p.inventory.damageArmor(amount);Map<String,Object> out=result(p);out.put("initial",initial);out.put("amount",FoodOracle.bits(amount));out.put("creative",creative);out.put("unbreakable",unbreakable);out.put("level",level);out.put("seed",Long.toString(seed));out.put("armor",stacks(p.inventory.armorInventory));return out;
    }
    static DamageSource source(int id){switch(id){case 0:return DamageSource.cactus;case 1:return DamageSource.lava;case 2:return DamageSource.fall;case 3:return new Source("explosion").setExplosion();case 4:return new Source("arrow").setProjectile();case 5:return DamageSource.magic;case 6:return DamageSource.starve;default:return DamageSource.outOfWorld;}}
    static Random protectionRandom()throws Exception {Field f=EnchantmentHelper.class.getDeclaredField("enchantmentRand");f.setAccessible(true);return (Random)f.get(null);}
    static Object protection(int pattern,int source,long seed)throws Exception {
        ItemStack[] armor=new ItemStack[4];List<Object> entries=new ArrayList<Object>();Random data=new Random(pattern*7919L);
        for(int slot=0;slot<4;slot++){if((pattern&3)==slot){entries.add(null);continue;}armor[slot]=item(310+slot,1,0,0,false);List<Object> levels=new ArrayList<Object>();for(int n=0;n<1+pattern%7;n++){int id=new int[]{0,1,2,3,4,7,16,99}[data.nextInt(8)];int level=new int[]{-32768,-1,0,1,4,10,32767}[data.nextInt(7)];enchant(armor[slot],id,level);levels.add(Arrays.asList(id,level));}entries.add(levels);}
        Random random=protectionRandom();random.setSeed(seed);int result=EnchantmentHelper.getEnchantmentModifierDamage(armor,source(source));return Arrays.asList(entries,source,Long.toString(seed),result,Long.toString(random.nextLong()));
    }
    static Object defense(int pattern,int source,float amount,long seed)throws Exception {
        boolean creative=(pattern&1)!=0,unbreakable=(pattern&2)!=0,blocking=(pattern&4)!=0;int level=pattern%4;Player p=player(seed,creative);List<Object> initial=new ArrayList<Object>();
        for(int slot=0;slot<4;slot++){int id=new int[]{301,300,311,306}[slot];int damage=Item.getItemById(id).getMaxDamage()-((pattern&8)!=0?0:2);ItemStack s=item(id,1,damage,level,unbreakable);enchant(s,(pattern+slot)%5,1+pattern%5);enchant(s,99,32767);p.inventory.armorInventory[slot]=s;initial.add(Arrays.asList(id,damage,Arrays.asList(Arrays.asList(34,level),Arrays.asList((pattern+slot)%5,1+pattern%5),Arrays.asList(99,32767))));}
        int resistance=new int[]{-99,0,1,4}[pattern>>4&3];if(resistance!=-99)p.addPotionEffect(new net.minecraft.potion.PotionEffect(11,100,resistance));p.setAbsorptionAmount((pattern&32)!=0?4f:0f);
        if(blocking){p.inventory.mainInventory[0]=new ItemStack(Items.iron_sword);p.setItemInUse(p.inventory.mainInventory[0],72000);}
        Random ench=protectionRandom();ench.setSeed(seed^0x55555L);p.defend(source(source),amount);
        Map<String,Object> out=result(p);out.put("initial",initial);out.put("creative",creative);out.put("unbreakable",unbreakable);out.put("blocking",blocking);out.put("resistance",resistance);out.put("initial_absorption",FoodOracle.bits((pattern&32)!=0?4f:0f));out.put("source",source);out.put("amount",FoodOracle.bits(amount));out.put("seed",Long.toString(seed));out.put("armor",stacks(p.inventory.armorInventory));out.put("health",FoodOracle.bits(p.getHealth()));out.put("absorption",FoodOracle.bits(p.getAbsorptionAmount()));out.put("exhaustion",FoodOracle.snapshot(p).get("exhaustion"));out.put("enchantment_tail",Long.toString(ench.nextLong()));return out;
    }
    public static void main(String[] args)throws Exception {
        Bootstrap.register();List<Object> registry=new ArrayList<Object>(),damage=new ArrayList<Object>(),armor=new ArrayList<Object>(),protection=new ArrayList<Object>();List<Integer> ids=new ArrayList<Integer>();
        for(Object o:Item.itemRegistry){Item item=(Item)o;int id=Item.getIdFromItem(item);if(item.getMaxDamage()>0){ids.add(id);registry.add(Arrays.asList(id,item.getMaxDamage(),item instanceof ItemArmor,item instanceof ItemArmor?((ItemArmor)item).damageReduceAmount:0));}}
        for(int id:ids){int max=Item.getItemById(id).getMaxDamage();for(int pattern=0;pattern<24;pattern++){int initial=new int[]{0,max-1,max,max+1}[pattern%4];int amount=new int[]{-2,0,1,2,4,17}[pattern%6];int level=new int[]{-1,0,1,3,10}[pattern%5];damage.add(damage(id,initial,amount,level,(pattern%11)==0,(pattern%7)==0,pattern*7919L));}}
        for(int pattern=0;pattern<32;pattern++)for(float amount:new float[]{0f,1f,3.9999998f,4f,7.9999995f,8f,16f,39f})for(int[] set:new int[][]{{301,300,299,298},{309,308,307,306},{313,312,311,310},{317,316,315,314},{276,86,1,397}})armor.add(armor(set,amount,pattern,pattern*189L+Float.floatToRawIntBits(amount)));
        for(int pattern=0;pattern<128;pattern++)for(int source=0;source<8;source++)for(long seed:new long[]{0,189,-741})protection.add(protection(pattern,source,seed));
        List<Object> defense=new ArrayList<Object>();for(int pattern=0;pattern<64;pattern++)for(int source=0;source<8;source++)for(float amount:new float[]{0f,1f,4f,12.5f,40f})defense.add(defense(pattern,source,amount,pattern*7919L));
        Map<String,Object> out=new LinkedHashMap<String,Object>();out.put("registry",registry);out.put("damage",damage);out.put("armor",armor);out.put("protection",protection);out.put("defense",defense);System.out.println(new Gson().toJson(out));
    }
}
