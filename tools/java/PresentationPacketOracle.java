import com.google.gson.Gson;
import io.netty.buffer.Unpooled;
import java.lang.reflect.Field;
import java.util.*;
import net.minecraft.init.Bootstrap;
import net.minecraft.network.Packet;
import net.minecraft.network.PacketBuffer;
import net.minecraft.network.play.server.*;
import net.minecraft.scoreboard.*;
import net.minecraft.util.*;

public final class PresentationPacketOracle {
    private static final List<Map<String,Object>> packets=new ArrayList<>();
    private static void capture(int id, Packet<?> packet) throws Exception {
        PacketBuffer buffer=new PacketBuffer(Unpooled.buffer());
        try {
            packet.writePacketData(buffer);
            StringBuilder hex=new StringBuilder();while(buffer.isReadable())hex.append(String.format("%02x",buffer.readUnsignedByte()));
            Map<String,Object> row=new LinkedHashMap<>();row.put("id",id);row.put("payload",hex.toString());packets.add(row);
        } finally { buffer.release(); }
    }
    public static void main(String[] args) throws Exception {
        java.io.PrintStream output=System.out; Bootstrap.register();
        capture(0x28,new S28PacketEffect(2001,new BlockPos(-37,74,191),0x1003,false));
        capture(0x29,new S29PacketSoundEffect("note.harp",-1.25,74.5,3.75,0.75f,1.25f));
        for (EnumParticleTypes kind:EnumParticleTypes.values()) {
            int[] argsForKind=new int[kind.getArgumentCount()];if(argsForKind.length>0)argsForKind[0]=kind==EnumParticleTypes.ITEM_CRACK?264:4099;
            capture(0x2a,new S2APacketParticles(kind,true,1.25f,2.5f,-3.0f,0.1f,0.2f,0.3f,0.5f,10,argsForKind));
        }
        Scoreboard board=new Scoreboard();ScoreObjective obj=board.addScoreObjective("points",IScoreObjectiveCriteria.DUMMY);
        obj.setDisplayName("Points");capture(0x3b,new S3BPacketScoreboardObjective(obj,0));
        Score score=board.getValueFromObjective("Alex",obj);score.setScorePoints(-2);capture(0x3c,new S3CPacketUpdateScore(score));
        capture(0x3d,new S3DPacketDisplayScoreboard(1,obj));
        ScorePlayerTeam team=board.createTeam("t");team.setNamePrefix("§a");team.setNameSuffix("!");board.addPlayerToTeam("Alex","t");
        capture(0x3e,new S3EPacketTeams(team,0));
        capture(0x45,new S45PacketTitle(2,5,1));capture(0x45,new S45PacketTitle(S45PacketTitle.Type.TITLE,new ChatComponentText("Hello")));
        S47PacketPlayerListHeaderFooter list=new S47PacketPlayerListHeaderFooter(new ChatComponentText("Header"));
        Field footer=S47PacketPlayerListHeaderFooter.class.getDeclaredField("footer");footer.setAccessible(true);footer.set(list,new ChatComponentText("Footer"));capture(0x47,list);
        output.println(new Gson().toJson(packets));
    }
}
