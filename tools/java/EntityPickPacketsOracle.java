import com.google.gson.Gson;
import io.netty.buffer.Unpooled;
import java.io.PrintStream;
import java.util.*;
import net.minecraft.world.*;
import net.minecraft.entity.*;
import net.minecraft.entity.item.*;
import net.minecraft.entity.projectile.*;
import net.minecraft.init.*;
import net.minecraft.network.PacketBuffer;
import net.minecraft.network.play.server.*;
import net.minecraft.util.*;

/** Original player list and attachment payloads used to resolve the spectator/riding
 * predicates, plus the actual spawn-position and AABB constructors used by S0E. */
public final class EntityPickPacketsOracle {
    static String hex(PacketBuffer bytes) {
        StringBuilder s=new StringBuilder();
        for(int i=0;i<bytes.writerIndex();i++) s.append(String.format("%02x",bytes.getUnsignedByte(i)));
        return s.toString();
    }
    static Map<String,Object> playerList(int action,int n) throws Exception {
        PacketBuffer wire=new PacketBuffer(Unpooled.buffer());
        wire.writeVarIntToBuffer(action);
        wire.writeVarIntToBuffer(n);
        for(int i=0;i<n;i++) {
            wire.writeUuid(new UUID(-189L*i,4321L+i));
            if(action==0) {
                wire.writeString("Player"+i);
                wire.writeVarIntToBuffer(2);
                wire.writeString("textures");wire.writeString("skin"+i);wire.writeBoolean(true);wire.writeString("signed");
                wire.writeString("extra");wire.writeString("value");wire.writeBoolean(false);
            }
            if(action<=1) wire.writeVarIntToBuffer(new int[]{3,1,2,0,-1,99}[i%6]);
            if(action==0||action==2) wire.writeVarIntToBuffer(i==0?-8:i*35);
            if(action==0||action==3) {
                wire.writeBoolean(i%2==0);
                if(i%2==0) wire.writeString("{\"text\":\"Display "+i+"\"}");
            }
        }
        S38PacketPlayerListItem packet=new S38PacketPlayerListItem(); packet.readPacketData(wire);
        List<Object> entries=new ArrayList<Object>();
        for(S38PacketPlayerListItem.AddPlayerData e:packet.getEntries()) {
            Map<String,Object> row=new LinkedHashMap<String,Object>();
            row.put("uuid",e.getProfile().getId().toString().replace("-",""));row.put("name",e.getProfile().getName());
            row.put("props",e.getProfile().getProperties().size());
            row.put("mode",e.getGameMode()==null?null:e.getGameMode().getID());row.put("ping",e.getPing());
            row.put("display",e.getDisplayName()==null?null:e.getDisplayName().getUnformattedText());
            entries.add(row);
        }
        Map<String,Object> out=new LinkedHashMap<String,Object>();
        out.put("wire",hex(wire));out.put("action",packet.getAction().ordinal());out.put("entries",entries);return out;
    }
    static Map<String,Object> attach(int player,int vehicle,int leash) throws Exception {
        PacketBuffer wire=new PacketBuffer(Unpooled.buffer());wire.writeInt(player);wire.writeInt(vehicle);wire.writeByte(leash);
        S1BPacketEntityAttach packet=new S1BPacketEntityAttach();packet.readPacketData(wire);
        Map<String,Object> out=new LinkedHashMap<String,Object>();
        out.put("wire",hex(wire));
        out.put("result",Arrays.asList(packet.getEntityId(),packet.getVehicleEntityId(),packet.getLeash()));return out;
    }
    static Map<String,Object> object(World w,int kind,int data,double x,double y,double z) {
        Entity e=null;
        switch(kind) {
            case 1:e=new EntityBoat(w,x,y,z);break;
            case 10:e=EntityMinecart.getMinecart(w,x,y,z,EntityMinecart.EnumMinecartType.byNetworkID(data));break;
            case 50:e=new EntityTNTPrimed(w,x,y,z,null);break;
            case 51:e=new EntityEnderCrystal(w,x,y,z);break;
            case 63:e=new EntityLargeFireball(w,x,y,z,.1,-.2,.3);break;
            case 64:e=new EntitySmallFireball(w,x,y,z,.1,-.2,.3);break;
            case 66:e=new EntityWitherSkull(w,x,y,z,.1,-.2,.3);break;
            case 70:e=new EntityFallingBlock(w,x,y,z,Blocks.sand.getDefaultState());break;
            case 71:e=new EntityItemFrame(w,new BlockPos(x,y,z),EnumFacing.getHorizontal(data));break;
            case 77:e=new EntityLeashKnot(w,new BlockPos(x,y,z));break;
            case 78:e=new EntityArmorStand(w,x,y,z);break;
        }
        Map<String,Object> row=new LinkedHashMap<String,Object>();
        row.put("kind",kind);row.put("data",data);row.put("pose",MovementOracle.bits(x,y,z));
        row.put("feet",MovementOracle.bits(e.posX,e.posY,e.posZ));row.put("bounds",EntityPickOracle.box(e.getEntityBoundingBox()));
        row.put("allow",e.canBeCollidedWith());row.put("pointed",e instanceof EntityLivingBase || e instanceof EntityItemFrame);
        row.put("border",e.getCollisionBorderSize());
        return row;
    }
    static List<Object> headTurns() throws Exception {
        net.minecraft.client.Minecraft mc=MovementOracle.minecraft();
        World world=new MovementOracle.TestWorld(0,0,0);
        net.minecraft.client.network.NetHandlerPlayClient net=new net.minecraft.client.network.NetHandlerPlayClient(mc,null,null,
            new com.mojang.authlib.GameProfile(new UUID(0,189),"Head"));
        mc.playerController=new net.minecraft.client.multiplayer.PlayerControllerMP(mc,net);
        MovementOracle.Player p=new MovementOracle.Player(mc,world,net);
        mc.thePlayer=p;
        p.setPosition(8,1,8);
        p.rotationYaw=p.rotationYawHead=0f;p.rotationPitch=0f;
        List<Object> rows=new ArrayList<Object>();
        for(int i=0;i<14;i++) {
            float dx=(i%4-1)*77.125f,dy=(i%3-1)*11.625f;
            p.setAngles(dx,-dy);
            Map<String,Object> row=new LinkedHashMap<String,Object>();
            row.put("dx",dx);row.put("dy",dy);row.put("before",EntityPickOracle.vec(p.getLook(1f)));
            p.onLivingUpdate();
            row.put("after",EntityPickOracle.vec(p.getLook(1f)));
            rows.add(row);
        }
        return rows;
    }
    public static void main(String[] args) throws Exception {
        PrintStream out=System.out;System.setOut(System.err);Bootstrap.register();
        Map<String,Object> output=new LinkedHashMap<String,Object>();
        List<Object> players=new ArrayList<Object>(),attach=new ArrayList<Object>(),objects=new ArrayList<Object>();
        for(int action=0;action<5;action++)for(int n:new int[]{0,1,2,6,29})players.add(playerList(action,n));
        for(int a:new int[]{0,1,345,-37,Integer.MAX_VALUE})for(int b:new int[]{-1,8,543,Integer.MIN_VALUE})
            for(int c:new int[]{0,1,255})attach.add(attach(a,b,c));
        World w=new MovementOracle.TestWorld(0,0,0);
        for(int kind:new int[]{1,10,50,51,63,64,66,70,71,77,78})
          for(int data=-4;data<=6;data++)for(double x:new double[]{-.0625,15.9375,16000000.03125})
            objects.add(object(w,kind,data,x,15.125,-x-.25));
        output.put("players",players);output.put("attach",attach);output.put("objects",objects);
        output.put("head_turns",headTurns());
        out.println(new Gson().toJson(output));
    }
}
