import com.google.gson.Gson;
import io.netty.buffer.Unpooled;
import java.util.*;
import net.minecraft.network.Packet;
import net.minecraft.network.PacketBuffer;
import net.minecraft.network.play.client.*;
import net.minecraft.network.play.server.*;
import net.minecraft.util.*;

public final class ChatPacketsOracle {
    static String bytes(Packet<?> packet) throws Exception {
        PacketBuffer b = new PacketBuffer(Unpooled.buffer());
        packet.writePacketData(b);
        StringBuilder hex = new StringBuilder();
        while(b.isReadable()) hex.append(String.format("%02x",b.readUnsignedByte()));
        return hex.toString();
    }
    public static void main(String[] args) throws Exception {
        Map<String,String> out = new LinkedHashMap<>();
        out.put("send",bytes(new C01PacketChatMessage("서버 chat")));
        out.put("request",bytes(new C14PacketTabComplete("/time set ",null)));
        out.put("target",bytes(new C14PacketTabComplete("안녕",new BlockPos(-12345,64,101))));
        out.put("choices",bytes(new S3APacketTabComplete(new String[]{"사과","사탕","한글"})));
        for(C16PacketClientStatus.EnumState state:C16PacketClientStatus.EnumState.values())
            out.put("status"+state.ordinal(),bytes(new C16PacketClientStatus(state)));
        out.put("spectate",bytes(new C18PacketSpectate(UUID.fromString("89122334-4556-4778-899a-abbccddeeff0"))));
        for(byte pos=0;pos<=2;pos++)
            out.put("incoming"+pos,bytes(new S02PacketChat(new ChatComponentText("안녕"),pos)));
        System.out.println(new Gson().toJson(out));
    }
}
