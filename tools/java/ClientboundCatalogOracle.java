import com.google.gson.*;
import io.netty.buffer.Unpooled;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import net.minecraft.init.Bootstrap;
import net.minecraft.network.*;
import net.minecraft.util.ChatComponentText;

/** Resolve every clientbound Play ID from the installed JAR and round-trip its own reader/writer. */
public final class ClientboundCatalogOracle {
    static void prefix(int id, PacketBuffer b) throws Exception {
        switch(id){
            case 1: b.writeInt(1);b.writeByte(0);b.writeByte(0);b.writeByte(2);b.writeByte(20);b.writeString("flat");b.writeBoolean(false);break;
            case 4:b.writeVarIntToBuffer(1);b.writeShort(0);b.writeShort(-1);break;
            case 7:b.writeInt(0);b.writeByte(2);b.writeByte(0);b.writeString("flat");break;
            case 8:b.writeDouble(.5);b.writeDouble(8);b.writeDouble(.5);b.writeFloat(0);b.writeFloat(0);b.writeByte(0);break;
            case 12:b.writeZero(33);b.writeByte(127);break;
            case 15:b.writeByte(1);b.writeByte(92);b.writeZero(21);b.writeByte(127);break;
            case 28:b.writeByte(1);b.writeByte(127);break;
            case 45:b.writeByte(1);b.writeString("minecraft:chest");b.writeChatComponent(new ChatComponentText("Container probe"));b.writeByte(27);break;
            case 47:b.writeByte(1);b.writeShort(0);b.writeShort(-1);break;
            case 51:b.writeLong(0);for(int i=0;i<4;i++)b.writeChatComponent(new ChatComponentText("line"));break;
            case 53:b.writeLong(0);b.writeByte(1);b.writeByte(10);b.writeShort(0);b.writeByte(0);break;
            case 58:b.writeVarIntToBuffer(1);b.writeString("/help");break;
            case 64:b.writeChatComponent(new ChatComponentText("catalog-74-complete"));break;
            case 70:b.writeVarIntToBuffer(-1);break;
            case 72:b.writeString("https://example.invalid/catalog.zip");b.writeString("");break;
            default:break;
        }
    }
    public static void main(String[] args)throws Exception{
        Bootstrap.register();
        JsonObject seeds=new JsonParser().parse(new String(Files.readAllBytes(Paths.get(args[0])),StandardCharsets.UTF_8)).getAsJsonObject();
        List<Object> rows=new ArrayList<>();
        for(int id=0;;id++){
            Packet<?> p=EnumConnectionState.PLAY.getPacket(EnumPacketDirection.CLIENTBOUND,id);
            if(p==null)break;
            PacketBuffer buf=PlayEventsOracle.buf();
            try{
                String key=Integer.toString(id);
                if(seeds.has(key)){
                    String hex=seeds.get(key).getAsString();
                    for(int i=0;i<hex.length();i+=2)buf.writeByte(Integer.parseInt(hex.substring(i,i+2),16));
                }else{prefix(id,buf);buf.writeZero(1024);}
                p.readPacketData(buf);
                // These two readers populate a List, but their writers use the spawning entity's DataWatcher.
                if(id==12||id==15)for(java.lang.reflect.Field field:p.getClass().getDeclaredFields()){
                    if(field.getType()==net.minecraft.entity.DataWatcher.class){
                        field.setAccessible(true);field.set(p,new net.minecraft.entity.DataWatcher(null));
                    }
                }
                byte[] data=PlayEventsOracle.write(p);
                PacketBuffer round=new PacketBuffer(Unpooled.wrappedBuffer(data));
                EnumConnectionState.PLAY.getPacket(EnumPacketDirection.CLIENTBOUND,id).readPacketData(round);
                if(round.isReadable())throw new AssertionError("trailing bytes");
                round.release();rows.add(Arrays.asList(id,p.getClass().getSimpleName(),PlayEventsOracle.hex(data)));
            }catch(Exception failure){throw new Exception("ID "+id+" "+p.getClass(),failure);}finally{buf.release();}
        }
        if(rows.size()!=74)throw new AssertionError(rows.size());
        Files.write(Paths.get(args[1]),new Gson().toJson(rows).getBytes(StandardCharsets.UTF_8));
        System.out.println("Verified "+rows.size()+" registry-dispatched original writers/readers");
    }
}
