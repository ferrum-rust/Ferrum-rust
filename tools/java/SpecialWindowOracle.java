import com.google.gson.Gson;
import io.netty.buffer.Unpooled;
import java.nio.file.*;
import java.nio.charset.StandardCharsets;
import java.util.*;
import net.minecraft.init.*;
import net.minecraft.item.*;
import net.minecraft.nbt.*;
import net.minecraft.network.*;
import net.minecraft.network.play.client.*;
import net.minecraft.util.*;

public final class SpecialWindowOracle {
    static PacketBuffer buf(){return new PacketBuffer(Unpooled.buffer());}
    static String write(Packet p)throws Exception{PacketBuffer b=buf();p.writePacketData(b);StringBuilder s=new StringBuilder();while(b.isReadable())s.append(String.format("%02x",b.readUnsignedByte()));return s.toString();}
    public static void main(String[] a)throws Exception{
        Bootstrap.register();
        Map<String,String> rows=new TreeMap<>();
        rows.put("rename",write(new C17PacketCustomPayload("MC|ItemName",buf().writeString("모루"))));
        PacketBuffer beacon=buf();beacon.writeInt(1);beacon.writeInt(10);rows.put("beacon",write(new C17PacketCustomPayload("MC|Beacon",beacon)));
        rows.put("enchant",write(new C11PacketEnchantItem(9,2)));
        for(boolean signed:new boolean[]{false,true}){
            ItemStack item=new ItemStack(signed?Items.written_book:Items.writable_book);
            NBTTagList pages=new NBTTagList();String line="책";pages.appendTag(new NBTTagString(signed?IChatComponent.Serializer.componentToJson(new ChatComponentText(line)):line));item.setTagInfo("pages",pages);
            if(signed){item.setTagInfo("title",new NBTTagString("게임"));item.setTagInfo("author",new NBTTagString("Player"));}
            PacketBuffer b=buf();b.writeItemStackToBuffer(item);rows.put(signed?"sign":"edit",write(new C17PacketCustomPayload(signed?"MC|BSign":"MC|BEdit",b)));
        }
        Files.write(Paths.get(a[0]),new Gson().toJson(rows).getBytes(StandardCharsets.UTF_8));
    }
}
