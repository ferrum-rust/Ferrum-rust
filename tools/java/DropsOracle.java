// Executes original Block.dropBlockAsItemWithChance, including subclass overrides.
import com.google.gson.Gson;
import java.lang.reflect.*;
import java.util.*;
import sun.misc.Unsafe;
import net.minecraft.init.*;
import net.minecraft.block.*;
import net.minecraft.block.state.IBlockState;
import net.minecraft.entity.*;
import net.minecraft.entity.item.*;
import net.minecraft.item.*;
import net.minecraft.nbt.*;
import net.minecraft.tileentity.*;
import net.minecraft.util.*;
import net.minecraft.world.*;
import net.minecraft.world.storage.WorldInfo;

public final class DropsOracle {
    static final BlockPos POS=new BlockPos(-19,65,36);
    static IBlockState state(int raw){return Block.getBlockById(raw>>4).getStateFromMeta(raw&15);}
    static int raw(IBlockState state){return Block.getIdFromBlock(state.getBlock())<<4|state.getBlock().getMetaFromState(state);}
    static Object nbt(NBTBase tag){
        if(tag==null)return null;Map<String,Object> out=new LinkedHashMap<String,Object>();Object value=null;String key="End";
        switch(tag.getId()){
        case 1:key="Byte";value=((NBTBase.NBTPrimitive)tag).getByte();break;
        case 2:key="Short";value=((NBTBase.NBTPrimitive)tag).getShort();break;
        case 3:key="Int";value=((NBTBase.NBTPrimitive)tag).getInt();break;
        case 4:key="Long";value=((NBTBase.NBTPrimitive)tag).getLong();break;
        case 5:key="Float";value=((NBTBase.NBTPrimitive)tag).getFloat();break;
        case 6:key="Double";value=((NBTBase.NBTPrimitive)tag).getDouble();break;
        case 7:key="Bytes";value=((NBTTagByteArray)tag).getByteArray();break;
        case 8:key="String";value=((NBTTagString)tag).getString();break;
        case 9:{key="List";NBTTagList list=(NBTTagList)tag;List<Object> values=new ArrayList<Object>();for(int i=0;i<list.tagCount();i++)values.add(nbt(list.get(i)));Map<String,Object> v=new LinkedHashMap<String,Object>();v.put("element",list.getTagType());v.put("values",values);value=v;break;}
        case 10:{key="Compound";Map<String,Object> values=new TreeMap<String,Object>();NBTTagCompound c=(NBTTagCompound)tag;for(String name:c.getKeySet())values.put(name,nbt(c.getTag(name)));value=values;break;}
        case 11:key="Ints";value=((NBTTagIntArray)tag).getIntArray();break;
        }
        out.put(key,value);return out;
    }
    static final class OracleWorld extends WorldServer {
        List<Object> events;TileEntity tile;GameRules rules;IBlockState block;
        OracleWorld(){super(null,null,null,0,null);}
        @Override public GameRules getGameRules(){return rules;}
        @Override public TileEntity getTileEntity(BlockPos p){return tile;}
        @Override public IBlockState getBlockState(BlockPos p){return p.equals(POS)?block:Blocks.air.getDefaultState();}
        @Override public boolean spawnEntityInWorld(Entity entity){
            Map<String,Object> e=new LinkedHashMap<String,Object>();e.put("position_bits",Arrays.asList(Long.toUnsignedString(Double.doubleToRawLongBits(entity.posX)),Long.toUnsignedString(Double.doubleToRawLongBits(entity.posY)),Long.toUnsignedString(Double.doubleToRawLongBits(entity.posZ))));
            if(entity instanceof EntityItem){EntityItem item=(EntityItem)entity;ItemStack stack=item.getEntityItem();e.put("kind","Item");Map<String,Object>s=new LinkedHashMap<String,Object>();s.put("id",Item.getIdFromItem(stack.getItem()));s.put("count",stack.stackSize);s.put("meta",stack.getMetadata());s.put("nbt",nbt(stack.getTagCompound()));e.put("stack",s);e.put("pickup_delay",10);}
            else if(entity instanceof EntityXPOrb){e.put("kind","Experience");e.put("value",((EntityXPOrb)entity).getXpValue());}
            else{e.put("kind",EntityList.getEntityString(entity));e.put("yaw",entity.rotationYaw);}
            events.add(e);return true;
        }
        @Override public void setEntityState(Entity entity,byte status){Map<String,Object>e=new LinkedHashMap<String,Object>();e.put("kind","EntityStatus");e.put("status",status);events.add(e);}
    }
    static void set(Object o,Class<?> cls,String name,Object value)throws Exception{Field f=cls.getDeclaredField(name);f.setAccessible(true);f.set(o,value);}
    static OracleWorld world(long seed,boolean tiles,boolean remote)throws Exception{
        Field f=Unsafe.class.getDeclaredField("theUnsafe");f.setAccessible(true);OracleWorld w=(OracleWorld)((Unsafe)f.get(null)).allocateInstance(OracleWorld.class);
        w.events=new ArrayList<Object>();w.rules=new GameRules();w.rules.setOrCreateGameRule("doTileDrops",Boolean.toString(tiles));set(w,World.class,"rand",new Random(seed));set(w,World.class,"isRemote",remote);set(w,World.class,"provider",new WorldProviderSurface());set(w,World.class,"worldInfo",new WorldInfo(new WorldSettings(189L,WorldSettings.GameType.SURVIVAL,false,false,WorldType.DEFAULT),"DropsOracle"));return w;
    }
    static Map<String,Object> run(int raw,long seed,int fortune,float chance,boolean tiles,boolean remote,int special)throws Exception{
        OracleWorld w=world(seed,tiles,remote);w.block=state(raw);
        if(special==1){TileEntityBanner b=new TileEntityBanner();NBTTagCompound tag=new NBTTagCompound();tag.setInteger("Base",12);NBTTagList list=new NBTTagList();NBTTagCompound p=new NBTTagCompound();p.setString("Pattern","cre");p.setInteger("Color",6);p.setByte("ExtraByte",(byte)-3);p.setLong("ExtraLong",189000000001L);list.appendTag(p);tag.setTag("Patterns",list);b.readFromNBT(tag);b.setPos(POS);w.tile=b;}
        if(special==2)w.tile=new TileEntityPiston(Blocks.tallgrass.getStateFromMeta(1),EnumFacing.NORTH,true,false);
        w.block.getBlock().dropBlockAsItemWithChance(w,POS,w.block,chance,fortune);
        Map<String,Object> out=new LinkedHashMap<String,Object>();out.put("state",raw);out.put("seed",Long.toString(seed));out.put("fortune",fortune);out.put("chance",chance);out.put("tiles",tiles);out.put("remote",remote);out.put("special",special);out.put("events",w.events);out.put("tail",Long.toString(w.rand.nextLong()));return out;
    }
    public static void main(String[] args)throws Exception{
        Bootstrap.register();TreeMap<Integer,IBlockState> states=new TreeMap<Integer,IBlockState>();for(Object o:Block.blockRegistry){Block b=(Block)o;for(IBlockState s:b.getBlockState().getValidStates())states.put(raw(s),state(raw(s)));}
        List<Object> table=new ArrayList<Object>();for(Map.Entry<Integer,IBlockState> entry:states.entrySet()){IBlockState s=entry.getValue();Block b=s.getBlock();Item item=b.getItemDropped(s,new Random(0),0);table.add(new int[]{entry.getKey(),item==null?-1:Item.getIdFromItem(item),b.damageDropped(s)});}
        List<Object> cases=new ArrayList<Object>();
        for(int raw:states.keySet())for(long seed:new long[]{0L,189L,-741L,107038380838084L})for(int profile=0;profile<7;profile++)cases.add(run(raw,seed,profile==1?3:profile==2?1:profile==3?3:0,profile==1?0f:profile==2?.35f:profile==5?-.1f:profile==6?2f:1f,profile!=3,profile==4,0));
        for(int raw:new int[]{18<<4,18<<4|3,161<<4|1,31<<4|1,59<<4|7,141<<4|7,142<<4|7,175<<4|2,104<<4|7,115<<4|3})for(int seed=0;seed<1024;seed++)cases.add(run(raw,(long)seed*7919L,seed%4,.2f,true,false,0));
        for(int raw:new int[]{176<<4,177<<4|2,36<<4})for(long seed:new long[]{0L,189L,-741L})for(int profile=0;profile<3;profile++)cases.add(run(raw,seed,3,0f,profile!=1,profile==2,raw>>4==36?2:1));
        Map<String,Object> out=new LinkedHashMap<String,Object>();out.put("states",table);out.put("cases",cases);System.out.println(new Gson().toJson(out));
    }
}
