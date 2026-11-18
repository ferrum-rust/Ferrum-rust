import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import java.util.concurrent.atomic.AtomicLong;
import java.util.zip.GZIPOutputStream;
import com.google.gson.Gson;
import net.minecraft.block.Block;
import net.minecraft.entity.Entity;
import net.minecraft.entity.item.*;
import net.minecraft.enchantment.Enchantment;
import net.minecraft.init.Bootstrap;
import net.minecraft.item.*;
import net.minecraft.network.play.client.C07PacketPlayerDigging;
import net.minecraft.potion.*;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.management.ItemInWorldManager;
import net.minecraft.tileentity.*;
import net.minecraft.util.*;
import net.minecraft.world.*;

/** Original server C07, ItemInWorldManager ticks and harvest/drop callbacks. */
public final class RuntimeDigOracle {
    static Field field(Class<?> c,String n)throws Exception{return RuntimeUseOracle.field(c,n);}
    static final BlockPos POS=new BlockPos(8,4,8);
    static final class World extends RuntimeUseOracle.Fixture{
        final List<Object> crack=new ArrayList<>(),aux=new ArrayList<>(),spawned=new ArrayList<>();
        World(MinecraftServer h){super(h);}
        public void sendBlockBreakProgress(int id,BlockPos p,int stage){crack.add(Arrays.asList(p.getX(),p.getY(),p.getZ(),stage));}
        public void playAuxSFXAtEntity(net.minecraft.entity.player.EntityPlayer player,int effect,BlockPos p,int data){aux.add(Arrays.asList(effect,p.getX(),p.getY(),p.getZ(),data));}
        public boolean spawnEntityInWorld(Entity e){
            boolean ok=super.spawnEntityInWorld(e);if(ok){List<String> pos=RuntimeFallingOracle.bits(e.posX,e.posY,e.posZ);
                if(e instanceof EntityItem){ItemStack s=((EntityItem)e).getEntityItem();spawned.add(Arrays.asList("item",pos,Item.getIdFromItem(s.getItem()),s.stackSize,s.getMetadata()));}
                if(e instanceof EntityXPOrb)spawned.add(Arrays.asList("xp",pos,((EntityXPOrb)e).getXpValue()));
            }return ok;
        }
    }
    static String state(Random r)throws Exception{return Long.toString(((AtomicLong)field(Random.class,"seed").get(r)).get());}
    static Random math()throws Exception{return (Random)field(Class.forName("java.lang.Math$RandomNumberGeneratorHolder"),"randomNumberGenerator").get(null);}
    static List<Object> frame(World world,RuntimeUseOracle.Player p,RuntimeUseOracle.Handler net)throws Exception{
        ItemInWorldManager m=p.theItemInWorldManager;List<Object> manager=new ArrayList<>();
        for(String n:new String[]{"curblockDamage","isDestroyingBlock","field_180240_f","initialDamage","receivedFinishDiggingPacket","field_180241_i","initialBlockDamage","durabilityRemainingOnBlock"}){
            Object v=field(ItemInWorldManager.class,n).get(m);if(v instanceof BlockPos){BlockPos b=(BlockPos)v;v=new int[]{b.getX(),b.getY(),b.getZ()};}manager.add(v);
        }
        ItemStack held=p.inventory.getCurrentItem();List<Object> out=Arrays.asList(manager,RuntimeUseOracle.state(world.getBlockState(POS)),held==null?null:new int[]{Item.getIdFromItem(held.getItem()),held.stackSize,held.getMetadata()},
            new ArrayList<>(net.corrections),new ArrayList<>(world.crack),new ArrayList<>(world.aux),new ArrayList<>(world.spawned),new ArrayList<>(p.stats),Float.floatToRawIntBits(field(FoodStats.class,"foodExhaustionLevel").getFloat(p.getFoodStats())),state(world.rand),state((Random)field(Entity.class,"rand").get(p)),state(math()));
        net.corrections.clear();world.crack.clear();world.aux.clear();world.spawned.clear();p.stats.clear();return out;
    }
    static Map<String,Object> run(MinecraftServer host,int state,int v,int item)throws Exception{
        World w=new World(host);host.worldServers=new WorldServer[]{w,null,null};w.init();for(int x=-1;x<=1;x++)for(int z=-1;z<=1;z++)w.theChunkProviderServer.loadChunk(x,z);
        RuntimeUseOracle.Player p=new RuntimeUseOracle.Player(host,w);RuntimeUseOracle.Handler net=new RuntimeUseOracle.Handler(host,p);
        boolean creative=v==0||v==7;boolean ground=v!=5;
        p.theItemInWorldManager.setGameType(creative?WorldSettings.GameType.CREATIVE:WorldSettings.GameType.SURVIVAL);
        double distance=v==8?6.0000000001:v==9?6.:2.;
        p.setPosition(8.5,3.,8.5+distance);p.onGround=ground;
        List<int[]> edits=new ArrayList<>();edits.add(new int[]{8,3,8,1<<4});edits.add(new int[]{8,4,8,state});
        if(state>>4==64)edits.add(new int[]{8,5,8,64<<4|8});
        if(state>>4==26){EnumFacing f=EnumFacing.getHorizontal(state&3);BlockPos other=POS.offset((state&8)==0?f:f.getOpposite());edits.add(new int[]{other.getX(),other.getY(),other.getZ(),state^8});}
        if(state>>4==175){boolean upper=(state&8)!=0;edits.add(new int[]{8,upper?3:5,8,175<<4|(upper?3:8)});}
        if(v==6)edits.add(new int[]{8,5,8,51<<4});for(int[] edit:edits){Block b=Block.getBlockById(edit[3]>>4);net.minecraft.block.state.IBlockState s=b.getStateFromMeta(edit[3]&15);edit[3]=RuntimeUseOracle.state(s);w.setBlockState(new BlockPos(edit[0],edit[1],edit[2]),s,2);}
        if(state>>4==140)((TileEntityFlowerPot)w.getTileEntity(POS)).setFlowerPotData(Item.getItemById(6),3);
        if(state>>4==144)((TileEntitySkull)w.getTileEntity(POS)).setType(1);
        ItemStack s=item==0?null:new ItemStack(Item.getItemById(item));
        if(s!=null){if(v==2)s.addEnchantment(Enchantment.efficiency,4);if(v==2)s.addEnchantment(Enchantment.fortune,3);if(v==3)s.addEnchantment(Enchantment.silkTouch,1);if(v==10&&s.isItemStackDamageable())s.setItemDamage(s.getMaxDamage());p.inventory.mainInventory[0]=s;}
        w.rand.setSeed(731);((Random)field(Entity.class,"rand").get(p)).setSeed(189+v);math().setSeed(1167+v);
        net.corrections.clear();w.crack.clear();w.aux.clear();w.spawned.clear();p.stats.clear();
        List<Object> frames=new ArrayList<>();
        for(int t=0;t<32;t++){
            if(t>0)p.theItemInWorldManager.updateBlockRemoving();
            C07PacketPlayerDigging.Action action=t==0?C07PacketPlayerDigging.Action.START_DESTROY_BLOCK:
                t==2&&v==4?C07PacketPlayerDigging.Action.ABORT_DESTROY_BLOCK:
                t==(v==1?12:3)?C07PacketPlayerDigging.Action.STOP_DESTROY_BLOCK:null;
            if(action!=null)net.processPlayerDigging(new C07PacketPlayerDigging(action,POS,EnumFacing.UP));frames.add(frame(w,p,net));
        }
        Map<String,Object> row=new LinkedHashMap<>();row.put("state",state);row.put("item",item);row.put("v",v);row.put("creative",creative);row.put("ground",ground);row.put("distance",distance);row.put("edits",edits);row.put("frames",frames);return row;
    }
    public static void main(String[] argv)throws Exception{
        Bootstrap.register();File dir=new File(argv[1]);dir.mkdirs();Constructor<?> c=Class.forName("VanillaWorldBridge$Host").getDeclaredConstructor(File.class);c.setAccessible(true);MinecraftServer host=(MinecraftServer)c.newInstance(dir);
        field(MinecraftServer.class,"serverThread").set(host,Thread.currentThread());List<Object> rows=new ArrayList<>();
        int[][] pairs={{16,278},{16,0},{3<<4,256},{7<<4,278},{56<<4,278},{16<<4,257},{14<<4,274},{78<<4|3,256},{79<<4,278},{18<<4|2,359},{161<<4|1,359},{31<<4|1,359},{32<<4,359},{54<<4,258},{52<<4,278},{35<<4|4,359},{58<<4,258},{64<<4,258},{17<<4|2,276},{0,0},{26<<4|1,258},{26<<4|10,0},{175<<4|2,359},{175<<4|8,359},{175<<4|8,0},{73<<4,278},{74<<4,278},{46<<4,256},{140<<4,0},{144<<4|1,0}};
        for(int[] pair:pairs)for(int v=0;v<=10;v++)rows.add(run(host,pair[0],v,v==7?276:pair[1]));
        try(Writer out=new OutputStreamWriter(new GZIPOutputStream(new FileOutputStream(argv[0])),"UTF-8")){new Gson().toJson(rows,out);}
        System.out.println("Original C07 server dig/harvest "+rows.size()+" scenes; "+rows.size()*32+" checkpoints");
    }
}
