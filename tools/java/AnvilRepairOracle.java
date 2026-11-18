import com.google.gson.Gson;
import java.io.File;
import java.lang.reflect.*;
import java.nio.file.*;
import java.nio.charset.StandardCharsets;
import java.util.*;
import net.minecraft.init.*;
import net.minecraft.item.*;
import net.minecraft.enchantment.*;
import net.minecraft.nbt.*;
import net.minecraft.inventory.*;
import net.minecraft.server.MinecraftServer;
import net.minecraft.world.WorldServer;

public final class AnvilRepairOracle {
    static List<Object> stack(ItemStack s){
        if(s==null)return null;
        List<Object> ench=new ArrayList<>();for(Map.Entry<Integer,Integer> e:EnchantmentHelper.getEnchantments(s).entrySet())ench.add(Arrays.asList(e.getKey(),e.getValue()));
        return Arrays.asList(Item.getIdFromItem(s.getItem()),s.stackSize,s.getItemDamage(),s.getRepairCost(),s.hasDisplayName()?s.getDisplayName():null,ench);
    }
    public static void main(String[] args)throws Exception {
        Bootstrap.register();
        TreeMap<Integer,List<Integer>> materials=new TreeMap<>();
        for(Item item:Item.itemRegistry)if(item.getMaxDamage()>0)for(Item source:Item.itemRegistry){
            if(item.getIsRepairable(new ItemStack(item),new ItemStack(source)))materials.computeIfAbsent(Item.getIdFromItem(item),k->new ArrayList<>()).add(Item.getIdFromItem(source));
        }
        Files.write(Paths.get(args[2]),new Gson().toJson(materials).getBytes(StandardCharsets.UTF_8));
        Constructor<?> c=Class.forName("VanillaWorldBridge$Host").getDeclaredConstructor(File.class);c.setAccessible(true);
        MinecraftServer h=(MinecraftServer)c.newInstance(new File(args[1]));RuntimeUseOracle.field(MinecraftServer.class,"serverThread").set(h,Thread.currentThread());
        RuntimeUseOracle.Fixture w=new RuntimeUseOracle.Fixture(h);h.worldServers=new WorldServer[]{w};w.init();w.theChunkProviderServer.loadChunk(0,0);
        RuntimeUseOracle.Player player=new RuntimeUseOracle.Player(h,w);new RuntimeUseOracle.Handler(h,player);
        List<Object> cases=new ArrayList<>();Field m=ContainerRepair.class.getDeclaredField("materialCost");m.setAccessible(true);
        for(int type=0;type<15;++type)for(int rep:new int[]{0,1,38,64})for(boolean creative:new boolean[]{false,true}){
            ContainerRepair r=new ContainerRepair(player.inventory,w,player);player.capabilities.isCreativeMode=creative;
            int id=type==9?1:type==13?403:type==7?258:type==12?310:276;
            ItemStack left=new ItemStack(Item.getItemById(id),type==9?8:1,(id==1||id==403||type==14)?0:1200);
            left.setRepairCost(rep);left.setTagInfo("kept",new NBTTagByte((byte)42));
            if(type==13)Items.enchanted_book.addEnchantment(left,new EnchantmentData(Enchantment.sharpness,2));
            else if(type==4||type==5||type==6)left.addEnchantment(Enchantment.sharpness,2);
            if(type==1||type==8)left.setStackDisplayName("before");
            String name=type==0||type==1||type==7||type==9||type==14?"renamed":"";
            ItemStack right=null;
            if(type==2||type==14)right=new ItemStack(Items.diamond,4);
            if(type==3||type==4)right=new ItemStack(Items.diamond_sword,1,300);
            if(type==4)right.addEnchantment(Enchantment.sharpness,2);
            if(type==10)right=new ItemStack(Items.apple);
            if(type==5||type==6||type==7||type==11||type==12||type==13){
                right=new ItemStack(Items.enchanted_book);
                Items.enchanted_book.addEnchantment(right,new EnchantmentData(type==5?Enchantment.smite:type==12?Enchantment.thorns:type==11?Enchantment.flame:Enchantment.sharpness,2));
                if(type==6)Items.enchanted_book.addEnchantment(right,new EnchantmentData(Enchantment.looting,3));
            }
            if(right!=null)right.setRepairCost(type%3);
            r.putStackInSlot(0,left);if(right!=null)r.putStackInSlot(1,right);r.updateItemName(name);
            cases.add(Arrays.asList(stack(left),stack(right),name,creative,r.maximumCost,m.getInt(r),stack(r.getSlot(2).getStack())));
        }
        Files.write(Paths.get(args[0]),new Gson().toJson(cases).getBytes(StandardCharsets.UTF_8));
    }
}
