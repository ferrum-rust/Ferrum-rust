import com.google.gson.Gson;
import com.mojang.authlib.GameProfile;
import io.netty.buffer.Unpooled;
import java.io.PrintStream;
import java.lang.reflect.*;
import java.nio.ByteBuffer;
import java.util.*;
import net.minecraft.block.*;
import net.minecraft.block.state.IBlockState;
import net.minecraft.client.Minecraft;
import net.minecraft.client.multiplayer.*;
import net.minecraft.client.network.NetHandlerPlayClient;
import net.minecraft.entity.*;
import net.minecraft.entity.item.*;
import net.minecraft.entity.player.InventoryPlayer;
import net.minecraft.init.*;
import net.minecraft.item.*;
import net.minecraft.nbt.*;
import net.minecraft.network.*;
import net.minecraft.network.play.client.C10PacketCreativeInventoryAction;
import net.minecraft.tileentity.*;
import net.minecraft.util.*;
import net.minecraft.world.*;
import net.minecraft.world.chunk.storage.RegionFile;
import org.lwjgl.input.Keyboard;

/** Runs the real private middleClickMouse and captures the actual C10 packet.
 * A synthetic keyboard buffer supplies ctrl without touching the system input. */
public final class MiddlePickOracle {
    static class Net extends NetHandlerPlayClient {
        C10PacketCreativeInventoryAction last;
        Net(Minecraft mc) {super(mc,null,null,new GameProfile(new UUID(0,189),"MiddlePick"));}
        public void addToSendQueue(Packet packet) {
            if(packet instanceof C10PacketCreativeInventoryAction) last=(C10PacketCreativeInventoryAction)packet;
        }
    }
    static class MapWorld extends EntityPickOracle.Scene {
        final Map<BlockPos,TileEntity> tiles=new HashMap<BlockPos,TileEntity>();
        MapWorld(Net net){super(net);}
        public TileEntity getTileEntity(BlockPos p){return tiles==null?null:tiles.get(p);}
    }
    final Minecraft mc;
    final Net net;
    final MapWorld w;
    final MovementOracle.Player p;
    final Method middle;
    final ByteBuffer keys;
    final BlockPos pos=new BlockPos(-3,63,19);
    MiddlePickOracle() throws Exception {
        mc=MovementOracle.minecraft();net=new Net(mc);w=new MapWorld(net);
        p=new MovementOracle.Player(mc,w,net);mc.thePlayer=p;mc.theWorld=w;
        mc.playerController=new PlayerControllerMP(mc,net);
        middle=Minecraft.class.getDeclaredMethod("middleClickMouse");middle.setAccessible(true);
        Field created=Keyboard.class.getDeclaredField("created");created.setAccessible(true);created.setBoolean(null,true);
        Field f=Keyboard.class.getDeclaredField("keyDownBuffer");f.setAccessible(true);keys=(ByteBuffer)f.get(null);
    }
    static String hex(PacketBuffer b) {
        StringBuilder s=new StringBuilder();
        for(int i=0;i<b.writerIndex();i++)s.append(String.format("%02x",b.getUnsignedByte(i)));
        return s.toString();
    }
    static String tag(NBTTagCompound n)throws Exception{
        PacketBuffer b=new PacketBuffer(Unpooled.buffer());b.writeNBTTagCompoundToBuffer(n);return hex(b);
    }
    static List<Object> stack(ItemStack s)throws Exception {
        if(s==null)return null;
        return Arrays.<Object>asList(Item.getIdFromItem(s.getItem()),s.stackSize,s.getMetadata(),s.hasTagCompound()?tag(s.getTagCompound()):null);
    }
    void reset(boolean creative,boolean ctrl) {
        Arrays.fill(p.inventory.mainInventory,null);p.inventory.currentItem=0;
        mc.playerController.setGameType(creative?WorldSettings.GameType.CREATIVE:WorldSettings.GameType.SURVIVAL);
        keys.put(29,(byte)(ctrl?1:0));net.last=null;
    }
    void click() throws Exception {middle.invoke(mc);}
    ItemStack block(int raw,int below,TileEntity te,boolean ctrl) throws Exception {
        reset(true,ctrl);w.blocks.clear();w.tiles.clear();
        w.blocks.put(pos,raw);w.blocks.put(pos.down(),below);
        if(te!=null){te.setWorldObj(w);te.setPos(pos);w.tiles.put(pos,te);}
        mc.objectMouseOver=new MovingObjectPosition(new Vec3(pos),EnumFacing.UP,pos);click();
        return net.last==null?null:net.last.getStack();
    }
    Map<String,Object> output() throws Exception {
        Map<String,Object> out=new LinkedHashMap<String,Object>();
        List<Object> canonical=new ArrayList<Object>(), plants=new ArrayList<Object>(), tiles=new ArrayList<Object>(), items=new ArrayList<Object>();
        Set<Integer> providers=new TreeSet<Integer>();
        for(int raw=0;raw<4096;raw++){
            IBlockState s=Block.BLOCK_STATE_IDS.getByValue(raw);if(s==null) continue;
            ItemStack pick=block(raw,0,null,false);
            canonical.add(Arrays.asList(raw,pick==null?0:Item.getIdFromItem(pick.getItem()),pick==null?0:pick.getMetadata(),pick!=null&&pick.getItem().getHasSubtypes()));
            int id=raw>>4;
            if(id==175 && (raw&8)!=0) for(int meta=0;meta<6;meta++){
                ItemStack lower=block(raw,(175<<4)|meta,null,false);
                plants.add(Arrays.asList(raw,meta,lower==null?-1:lower.getMetadata()));
            }
            if(s.getBlock() instanceof ITileEntityProvider && providers.add(id)) {
                TileEntity te=((ITileEntityProvider)s.getBlock()).createNewTileEntity(w,raw&15);
                if(te==null && id==36) te=new TileEntityPiston(Blocks.stone.getDefaultState(),EnumFacing.EAST,true,false);
                if(te!=null) tile(tiles,raw,te);
            }
        }
        for(Item item:Item.itemRegistry){
            ItemStack one=new ItemStack(item,1,0),two=new ItemStack(item,2,0);
            items.add(Arrays.asList(Item.getIdFromItem(item),item.getHasSubtypes(),one.isItemEnchantable(),two.isItemEnchantable(),Item.itemRegistry.getNameForObject(item).toString()));
        }
        for(int i=0;i<18;i++) {
            TileEntitySkull skull=new TileEntitySkull();skull.setType(i%6);
            if(i%6==3) skull.setPlayerProfile(new GameProfile(new UUID(189,i),"Owner"+i));
            tile(tiles,(144<<4)|1,skull);
            TileEntityFlowerPot pot=new TileEntityFlowerPot(i%2!=0?Items.apple:Item.getItemFromBlock(Blocks.sapling),i%6);
            tile(tiles,140<<4,pot);
            TileEntityBanner banner=new TileEntityBanner();
            NBTTagCompound n=new NBTTagCompound();n.setInteger("Base",i%16);banner.readFromNBT(n);
            tile(tiles,(176<<4)|2,banner);
        }
        List<Object> imports=new ArrayList<Object>();
        for(int id:providers){
            int raw=0;for(int s=id<<4;s<(id+1)<<4;s++)if(Block.BLOCK_STATE_IDS.getByValue(s)!=null){raw=s;break;}
            IBlockState state=Block.BLOCK_STATE_IDS.getByValue(raw);if(state==null)continue;
            TileEntity original=((ITileEntityProvider)state.getBlock()).createNewTileEntity(w,raw&15);
            if(original==null&&id==36)original=new TileEntityPiston(Blocks.stone.getDefaultState(),EnumFacing.EAST,true,false);
            if(original==null)continue;
            original.setWorldObj(w);original.setPos(pos);
            NBTTagCompound saved=new NBTTagCompound();original.writeToNBT(saved);
            for(int n=0;n<4;n++){
                NBTTagCompound input=(NBTTagCompound)saved.copy();
                if(n==1){input.setString("unknown","ignored by original");}
                if(n==2){for(String key:new String[]{"Patterns","Lock","Data","CustomName","CookTimeTotal","Owner","Items","Base"})input.removeTag(key);}
                if(n==3){
                    if(id==140){input.setInteger("Item",6);input.setShort("Data",(short)4);}
                    if(id==144){input.setByte("SkullType",(byte)3);input.setString("ExtraType","ImportedOwner");}
                    if(id==176||id==177){input.setByte("Base",(byte)13);}
                    if(id==63||id==68){input.setString("Text1","legacy plain");input.setString("Text2","{\"text\":\"Styled\",\"bold\":true}");}
                }
                TileEntity loaded=TileEntity.createAndLoadEntity(input);
                if(loaded==null)continue;
                loaded.setWorldObj(w);
                NBTTagCompound result=new NBTTagCompound();loaded.writeToNBT(result);
                imports.add(Arrays.asList(raw,tag(input),tag(result)));
            }
        }
        out.put("imports",imports);
        TileEntityChest chest=new TileEntityChest();
        chest.setCustomName("Supplies");
        ItemStack wool=new ItemStack(Blocks.wool,4,14);
        wool.setTagInfo("ench",new NBTTagList());
        chest.setInventorySlotContents(5,wool);
        tile(tiles,(54<<4)|2,chest);
        TileEntityFurnace furnace=new TileEntityFurnace();
        furnace.setInventorySlotContents(1,new ItemStack(Items.coal,7,1));
        furnace.setField(0,29);furnace.setField(2,88);furnace.setField(3,301);
        tile(tiles,(61<<4)|4,furnace);
        out.put("registry",canonical);out.put("plants",plants);out.put("tiles",tiles);out.put("items",items);
        out.put("inventory",inventories());out.put("entities",entityPicks());
        List<Object> frames=new ArrayList<Object>();
        EntityItemFrame frame=new EntityItemFrame(w,pos,EnumFacing.NORTH);
        for(int id:new int[]{6,35,267,260,383})for(int damage:new int[]{0,1,13,27}){
            ItemStack shown=new ItemStack(Item.getItemById(id),8,damage);
            shown.setStackDisplayName("Displayed");frame.setDisplayedItem(shown);
            reset(true,false);mc.objectMouseOver=new MovingObjectPosition(frame);click();
            frames.add(Arrays.asList(id,damage,stack(net.last.getStack()),hexPacket(net.last)));
        }
        out.put("frames",frames);
        return out;
    }
    static String hexPacket(C10PacketCreativeInventoryAction p)throws Exception{
        PacketBuffer b=new PacketBuffer(Unpooled.buffer());p.writePacketData(b);return hex(b);
    }
    void region(java.io.File target)throws Exception{
        NBTTagCompound root=new NBTTagCompound(),level=new NBTTagCompound(),section=new NBTTagCompound();
        byte[] blocks=new byte[4096],meta=new byte[2048];
        NBTTagList nbt=new NBTTagList();
        for(int i=0;i<5;i++){
            int packed=new int[]{(144<<4)|1,(54<<4)|2,(61<<4)|4,16,(140<<4)}[i];
            int index=(15<<8)|(3<<4)|(12+i)%16;
            blocks[index]=(byte)(packed>>4);meta[index/2]|=(byte)((packed&15)<<((index&1)*4));
            TileEntity te=i==0?new TileEntitySkull():i==2?new TileEntityFurnace():i==4?new TileEntityFlowerPot(Items.apple,5):new TileEntityChest();
            te.setWorldObj(w);te.setPos(new BlockPos(-16+(12+i)%16,63,19));
            if(i==0)((TileEntitySkull)te).setPlayerProfile(new GameProfile(new UUID(189,3),"Owner"));
            if(i==1)((TileEntityChest)te).setInventorySlotContents(4,new ItemStack(Items.apple,3));
            NBTTagCompound tile=new NBTTagCompound();te.writeToNBT(tile);nbt.appendTag(tile);
            if(i==1){NBTTagCompound forged=(NBTTagCompound)tile.copy();forged.setInteger("x",33);nbt.appendTag(forged);}
        }
        section.setByte("Y",(byte)3);section.setByteArray("Blocks",blocks);section.setByteArray("Data",meta);
        NBTTagList sections=new NBTTagList();sections.appendTag(section);level.setTag("Sections",sections);
        level.setTag("TileEntities",nbt);level.setInteger("xPos",-1);level.setInteger("zPos",1);root.setTag("Level",level);
        target.getParentFile().mkdirs();RegionFile region=new RegionFile(target);
        java.io.DataOutputStream stream=region.getChunkDataOutputStream(31,1);CompressedStreamTools.write(root,stream);stream.close();region.close();
    }
    void tile(List<Object> rows,int raw,TileEntity te)throws Exception{
        te.setWorldObj(w);te.setPos(pos);
        NBTTagCompound n=new NBTTagCompound();te.writeToNBT(n);
        ItemStack plain=block(raw,0,te,false),ctrl=block(raw,0,te,true);
        Map<String,Object> row=new LinkedHashMap<String,Object>();
        row.put("raw",raw);row.put("tile",tag(n));row.put("name",n.getString("id"));row.put("plain",stack(plain));row.put("ctrl",stack(ctrl));
        rows.add(row);
    }
    List<Object> inventories() throws Exception {
        Random random=new Random(189);List<Object> rows=new ArrayList<Object>();
        int[] ids={1,3,17,35,260,267,276,340,383};
        for(int nr=0;nr<640;nr++){
            boolean creative=(nr&1)!=0,match=(nr&2)!=0;
            reset(creative,false);
            p.inventory.currentItem=random.nextInt(9);
            List<Object> before=new ArrayList<Object>();
            for(int i=0;i<36;i++){
                if(random.nextInt(10)>((nr&4)!=0?0:4)){
                    ItemStack item=new ItemStack(Item.getItemById(ids[random.nextInt(ids.length)]),1+random.nextInt(3),random.nextInt(4));
                    if(random.nextInt(12)==0){item.setTagInfo("ench",new NBTTagList());}
                    p.inventory.mainInventory[i]=item;before.add(Arrays.asList(i,stack(item)));
                }
            }
            int id=ids[random.nextInt(ids.length)],meta=random.nextInt(4),sel=p.inventory.currentItem;
            p.inventory.setCurrentItem(Item.getItemById(id),meta,match,creative);
            List<Object> changes=new ArrayList<Object>();
            for(int i=0;i<36;i++){
                Object old=null;for(Object entry:before){List<?> v=(List<?>)entry;if(((Integer)v.get(0))==i)old=v.get(1);}
                Object updated=stack(p.inventory.mainInventory[i]);if(!Objects.equals(old,updated))changes.add(Arrays.asList(i,updated));
            }
            rows.add(Arrays.asList(id,meta,match,creative,sel,before,p.inventory.currentItem,changes));
        }
        return rows;
    }
    List<Object> entityPicks() throws Exception {
        List<Object> rows=new ArrayList<Object>();
        for(int id=0;id<256;id++){
            String name=EntityList.getStringFromID(id);if(name==null) continue;
            Entity entity=EntityList.createEntityByID(id,w);if(entity==null)continue;
            reset(true,false);mc.objectMouseOver=new MovingObjectPosition(entity);click();
            rows.add(Arrays.asList(id,stack(net.last==null?null:net.last.getStack())));
        }
        return rows;
    }
    public static void main(String[] args)throws Exception{
        PrintStream out=System.out;System.setOut(System.err);Bootstrap.register();
        MiddlePickOracle oracle=new MiddlePickOracle();
        if(args.length>0)oracle.region(new java.io.File(args[0]));
        out.println(new Gson().toJson(oracle.output()));
    }
}
