import com.google.gson.Gson;
import io.netty.buffer.Unpooled;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import net.minecraft.block.Block;
import net.minecraft.init.*;
import net.minecraft.item.ItemStack;
import net.minecraft.nbt.NBTTagCompound;
import net.minecraft.network.*;
import net.minecraft.network.play.client.C12PacketUpdateSign;
import net.minecraft.network.play.server.*;
import net.minecraft.stats.*;
import net.minecraft.util.*;
import net.minecraft.village.*;
import net.minecraft.world.EnumDifficulty;

/** Emit the original writer's packet payloads. Each vector must be consumed by its original reader. */
public final class PlayEventsOracle {
    static final List<Object> rows = new ArrayList<>();
    static PacketBuffer buf(){return new PacketBuffer(Unpooled.buffer());}
    static String hex(byte[] data){StringBuilder s=new StringBuilder();for(byte b:data)s.append(String.format("%02x",b&255));return s.toString();}
    static byte[] write(Packet<?> p) throws Exception {
        PacketBuffer b=buf();p.writePacketData(b);byte[] data=new byte[b.readableBytes()];b.readBytes(data);b.release();return data;
    }
    interface Seed {void fill(PacketBuffer b)throws Exception;}
    static Packet<?> wire(Packet<?> packet,Seed seed)throws Exception{
        PacketBuffer b=buf();seed.fill(b);packet.readPacketData(b);
        if(b.isReadable())throw new AssertionError("unread seed");b.release();return packet;
    }
    static void add(String name,int id,Packet<?> p)throws Exception {
        byte[] data=write(p);
        PacketBuffer b=new PacketBuffer(Unpooled.wrappedBuffer(data));
        Packet<?> decoded=p.getClass().newInstance();decoded.readPacketData(b);
        if(b.isReadable())throw new AssertionError(name+" trailing original bytes");
        b.release();
        rows.add(Arrays.asList(name,id,hex(data)));
    }
    public static void main(String[] args)throws Exception{
        Bootstrap.register();
        BlockPos pos=new BlockPos(-12345,255,56789);
        add("bed",10,wire(new S0APacketUseBed(),b->{b.writeVarIntToBuffer(16384);b.writeBlockPos(pos);}));
        add("animation",11,wire(new S0BPacketAnimation(),b->{b.writeVarIntToBuffer(300);b.writeByte(5);}));
        add("painting",16,wire(new S10PacketSpawnPainting(),b->{b.writeVarIntToBuffer(31);b.writeString("SkullAndRoses");b.writeBlockPos(pos);b.writeByte(3);}));
        add("orb",17,wire(new S11PacketSpawnExperienceOrb(),b->{b.writeVarIntToBuffer(128);b.writeInt(-65);b.writeInt(8191);b.writeInt(1);b.writeShort(2477);}));
        add("block",36,new S24PacketBlockAction(pos,Blocks.piston,1,5));
        add("break",37,new S25PacketBlockBreakAnim(333,pos,-1));
        add("explosion",39,new S27PacketExplosion(-10.75,22.75,-93.25,4.5f,
            Arrays.asList(new BlockPos(-138,149,-92),new BlockPos(117,-106,-94)),new Vec3(-0.25,1.5,0.75)));
        add("global",44,wire(new S2CPacketSpawnGlobalEntity(),b->{b.writeVarIntToBuffer(170);b.writeByte(1);b.writeInt(65);b.writeInt(-8191);b.writeInt(-1);}));
        add("property",49,new S31PacketWindowProperty(254,-32768,32767));
        List<Vec4b> icons=Arrays.asList(new Vec4b((byte)13,(byte)-128,(byte)127,(byte)15));
        byte[] colors=new byte[16384];for(int i=0;i<colors.length;i++)colors[i]=(byte)(i%251);
        add("map",52,new S34PacketMaps(150,(byte)3,icons,colors,126,125,2,3));
        add("icons",52,new S34PacketMaps(151,(byte)0,icons,colors,0,0,0,0));
        add("sign",54,new S36PacketSignEditorOpen(pos));
        Map<StatBase,Integer> stats=new LinkedHashMap<>();stats.put(StatList.jumpStat,32769);stats.put(StatList.deathsStat,0);
        add("stats",55,new S37PacketStatistics(stats));
        add("empty-stats",55,new S37PacketStatistics(Collections.<StatBase,Integer>emptyMap()));
        PacketBuffer brand=buf();brand.writeString("원본 1.8.9");add("brand",63,new S3FPacketCustomPayload("MC|Brand",brand));
        PacketBuffer offers=buf();offers.writeInt(513);MerchantRecipeList trades=new MerchantRecipeList();
        ItemStack sword=new ItemStack(Items.diamond_sword,1,23);sword.setStackDisplayName("칼");
        trades.add(new MerchantRecipe(new ItemStack(Items.emerald,7),null,sword,3,3));
        trades.add(new MerchantRecipe(new ItemStack(Items.diamond),new ItemStack(Items.book),new ItemStack(Items.emerald,9),1,10));
        trades.writeToBuf(offers);add("offers",63,new S3FPacketCustomPayload("MC|TrList",offers));
        add("book",63,new S3FPacketCustomPayload("MC|BOpen",buf()));
        PacketBuffer other=buf();other.writeByte(0xca);add("other",63,new S3FPacketCustomPayload("test",other));
        add("difficulty",65,new S41PacketServerDifficulty(EnumDifficulty.HARD,true));
        add("combat-start",66,wire(new S42PacketCombatEvent(),b->b.writeVarIntToBuffer(0)));
        add("combat-end",66,wire(new S42PacketCombatEvent(),b->{b.writeVarIntToBuffer(1);b.writeVarIntToBuffer(301);b.writeInt(-1);}));
        add("combat-death",66,wire(new S42PacketCombatEvent(),b->{b.writeVarIntToBuffer(2);b.writeVarIntToBuffer(301);b.writeInt(-1);b.writeString("\"사망\"");}));
        add("camera",67,wire(new S43PacketCamera(),b->b.writeVarIntToBuffer(100000)));
        NBTTagCompound tag=new NBTTagCompound();tag.setString("CustomName","테스트");tag.setByte("Silent",(byte)1);
        add("entity-nbt",73,new S49PacketUpdateEntityNBT(193,tag));
        IChatComponent[] lines={new ChatComponentText("1"),new ChatComponentText("서버"),new ChatComponentText("&<'>="),new ChatComponentText("\"")};
        add("update-sign",18,new C12PacketUpdateSign(pos,lines));
        Files.write(Paths.get(args[0]),(new Gson().toJson(rows)+"\n").getBytes(StandardCharsets.UTF_8));
    }
}
