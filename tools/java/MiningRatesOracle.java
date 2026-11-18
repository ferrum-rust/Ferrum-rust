import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import java.util.zip.GZIPOutputStream;
import com.google.gson.Gson;
import com.mojang.authlib.GameProfile;
import net.minecraft.block.Block;
import net.minecraft.block.material.Material;
import net.minecraft.block.state.IBlockState;
import net.minecraft.enchantment.Enchantment;
import net.minecraft.entity.player.EntityPlayer;
import net.minecraft.init.Bootstrap;
import net.minecraft.item.*;
import net.minecraft.potion.*;
import net.minecraft.stats.StatList;
import net.minecraft.tileentity.TileEntity;
import net.minecraft.util.*;
import net.minecraft.world.World;

/** Actual tool-strength table, silk outputs and EntityPlayer dig modifiers. */
public final class MiningRatesOracle {
    static final BlockPos POS=new BlockPos(8,4,8);
    static final class Fixture extends FallingEntityOracle.TestWorld {
        IBlockState block;
        Fixture(){super(0);}
        public IBlockState getBlockState(BlockPos p){return POS.equals(p)&&block!=null?block:super.getBlockState(p);}
    }
    static final class Player extends EntityPlayer{
        boolean water;
        Player(World world){super(world,new GameProfile(new UUID(0,189),"Player"));}
        public boolean isSpectator(){return false;}
        public boolean isInsideOfMaterial(Material material){return material==Material.water&&water;}
    }
    static Method method(Class<?> c,String n,Class<?>... args)throws Exception{Method m=c.getDeclaredMethod(n,args);m.setAccessible(true);return m;}
    static String owner(Block b,String name,Class<?>... args)throws Exception{return b.getClass().getMethod(name,args).getDeclaringClass().getSimpleName();}
    public static void main(String[] argv)throws Exception{
        Bootstrap.register();Fixture w=new Fixture();Player p=new Player(w);Random r=new Random(189);
        Map<String,Object> data=new LinkedHashMap<>();List<Object> blocks=new ArrayList<>(),strength=new ArrayList<>(),silk=new ArrayList<>(),traces=new ArrayList<>();List<Integer> items=new ArrayList<>();
        for(Item i:Item.itemRegistry)items.add(Item.getIdFromItem(i));
        Method canSilk=method(Block.class,"canSilkHarvest"),stacked=method(Block.class,"createStackedBlock",IBlockState.class);
        for(int id=0;id<256;id++){
            Block b=Block.getBlockById(id);w.block=b.getDefaultState();
            List<Object> owners=Arrays.asList(owner(b,"onBlockClicked",World.class,BlockPos.class,EntityPlayer.class),owner(b,"onBlockHarvested",World.class,BlockPos.class,IBlockState.class,EntityPlayer.class),owner(b,"onBlockDestroyedByPlayer",World.class,BlockPos.class,IBlockState.class),owner(b,"harvestBlock",World.class,EntityPlayer.class,BlockPos.class,IBlockState.class,TileEntity.class));
            blocks.add(Arrays.asList(Float.floatToRawIntBits(b.getBlockHardness(w,POS)),(boolean)canSilk.invoke(b),owners,StatList.mineBlockStatArray[id]==null?"":StatList.mineBlockStatArray[id].statId));
            for(Item item:Item.itemRegistry){ItemStack s=new ItemStack(item);float f=s.getStrVsBlock(b);if(f!=1)strength.add(new int[]{Item.getIdFromItem(item),id,Float.floatToRawIntBits(f)});}
            if(Block.getIdFromBlock(b)!=id)continue;
            Set<Integer> metas=new TreeSet<>();for(IBlockState s:b.getBlockState().getValidStates())metas.add(b.getMetaFromState(s));
            for(int meta:metas){
                w.block=b.getStateFromMeta(meta);ItemStack s=(ItemStack)stacked.invoke(b,w.block);if(s!=null&&s.getItem()!=null)silk.add(new int[]{id<<4|meta,Item.getIdFromItem(s.getItem()),s.getMetadata()});
                for(int n=0;n<24;n++){
                    int[] selected={0,257,278,270,256,258,276,359,285,277,271,294};
                    int item=n%3==0?items.get(r.nextInt(items.size())):selected[n%selected.length];
                    int eff=r.nextInt(8)-1,haste=r.nextInt(5)-2,fatigue=r.nextInt(7)-2;boolean water=r.nextBoolean(),ground=r.nextBoolean(),aqua=r.nextBoolean();
                    ItemStack held=item==0?null:new ItemStack(Item.getItemById(item));if(held!=null&&eff>0)held.addEnchantment(Enchantment.efficiency,eff);
                    p.inventory.mainInventory[0]=held;p.inventory.armorInventory[3]=new ItemStack(Item.getItemById(310));if(aqua)p.inventory.armorInventory[3].addEnchantment(Enchantment.aquaAffinity,1);
                    p.clearActivePotions();if(haste>=0)p.addPotionEffect(new PotionEffect(Potion.digSpeed.id,90,haste));if(fatigue>=0)p.addPotionEffect(new PotionEffect(Potion.digSlowdown.id,90,fatigue));p.water=water;p.onGround=ground;
                    traces.add(new int[]{id<<4|meta,item,eff,haste,fatigue,water?1:0,ground?1:0,aqua?1:0,Float.floatToRawIntBits(b.getPlayerRelativeBlockHardness(p,w,POS)),p.canHarvestBlock(b)?1:0});
                }
            }
        }
        List<Object> itemStats=new ArrayList<>();for(int id:items)itemStats.add(Arrays.asList(id,StatList.objectUseStats[id]==null?"":StatList.objectUseStats[id].statId,StatList.objectBreakStats[id]==null?"":StatList.objectBreakStats[id].statId));
        data.put("blocks",blocks);data.put("strength",strength);data.put("silk",silk);data.put("item_stats",itemStats);
        try(Writer out=new OutputStreamWriter(new FileOutputStream(argv[0]),"UTF-8")){new Gson().toJson(data,out);}
        try(Writer out=new OutputStreamWriter(new GZIPOutputStream(new FileOutputStream(argv[1])),"UTF-8")){new Gson().toJson(traces,out);}
        System.out.println("Original mining strength item/block registry "+items.size()+"x256, real hardness/harvest cases "+traces.size());
    }
}
