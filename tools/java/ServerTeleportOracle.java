import com.google.gson.Gson;
import com.mojang.authlib.GameProfile;
import io.netty.buffer.Unpooled;
import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import net.minecraft.entity.player.*;
import net.minecraft.init.Bootstrap;
import net.minecraft.network.*;
import net.minecraft.network.play.client.C03PacketPlayer;
import net.minecraft.network.play.server.S08PacketPlayerPosLook;
import net.minecraft.profiler.Profiler;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.management.*;
import net.minecraft.stats.StatisticsFile;
import net.minecraft.world.*;
import net.minecraft.world.storage.*;

/** Observe original S08 creation and C03 admission up to the call into the
 * live player update. Collision, stats, and gameplay belong to that next phase. */
public final class ServerTeleportOracle {
    static final class Admitted extends RuntimeException {}
    static final class Player extends EntityPlayerMP {
        Player(MinecraftServer host, WorldServer w) {
            super(host,w,new GameProfile(new UUID(0,1),"TeleportProbe"),new ItemInWorldManager(w));
        }
        @Override public void onUpdateEntity(){ throw new Admitted(); }
        @Override public void sendPlayerAbilities(){}
    }
    static final class Handler extends NetHandlerPlayServer {
        String kick;
        List<String> sent = new ArrayList<>();
        Handler(MinecraftServer host, Player p) { super(host,new NetworkManager(EnumPacketDirection.SERVERBOUND),p); }
        @Override public void sendPacket(Packet packet) {
            if (!(packet instanceof S08PacketPlayerPosLook)) return;
            try {
                PacketBuffer buf = new PacketBuffer(Unpooled.buffer());
                try {
                    packet.writePacketData(buf);
                    StringBuilder hex = new StringBuilder();
                    while(buf.isReadable()) hex.append(String.format(Locale.ROOT,"%02x",buf.readUnsignedByte()));
                    sent.add(hex.toString());
                } finally { buf.release(); }
            } catch(IOException e) { throw new IllegalStateException(e); }
        }
        @Override public void kickPlayerFromServer(String message){kick=message;}
    }
    static Field field(Class<?> type, String name) throws Exception {
        Field f=type.getDeclaredField(name);f.setAccessible(true);return f;
    }
    static String bits(double d) {return Long.toUnsignedString(Double.doubleToRawLongBits(d));}
    static List<String> doubles(double x,double y,double z) {
        return Arrays.asList(bits(x),bits(y),bits(z));
    }
    public static void main(String[] args) throws Exception {
        Bootstrap.register(); File dir=new File(args[0]); dir.mkdirs();
        Constructor<?> ctor=Class.forName("VanillaWorldBridge$Host").getDeclaredConstructor(File.class);
        ctor.setAccessible(true);MinecraftServer host=(MinecraftServer)ctor.newInstance(dir);
        field(MinecraftServer.class,"serverThread").set(host,Thread.currentThread());
        WorldSettings settings=new WorldSettings(189,WorldSettings.GameType.SURVIVAL,false,false,WorldType.DEFAULT);
        WorldServer w=new WorldServer(host,new SaveHandlerMP(),new WorldInfo(settings,"admission"),0,new Profiler());
        host.worldServers=new WorldServer[]{w,null,null}; w.init();
        host.setConfigManager(new ServerConfigurationManager(host){
            @Override public StatisticsFile getPlayerStatsFile(EntityPlayer p){return new StatisticsFile(host,new File(dir,"stats.json"));}
        });
        Player p=new Player(host,w);
        Field moved=field(NetHandlerPlayServer.class,"hasMoved"), tick=field(NetHandlerPlayServer.class,"networkTickCount");
        Field lastTick=field(NetHandlerPlayServer.class,"field_175090_f");
        List<Object> cases=new ArrayList<>();
        for(int flags=0;flags<32;flags++)for(int scenario=0;scenario<20;scenario++){
            Handler h=new Handler(host,p);
            p.playerConqueredTheEnd=scenario==18;
            double x=flags%3==0?29999980.12345678:-16.625, y=85.125, z=32.75;
            float yaw=317.125f,pitch=-12.25f;
            p.setPositionAndRotation(x,y,z,yaw,pitch);
            double tx=-1.875,ty=.375,tz=1.625;
            float ry=451.75f,rp=-376.5f;
            h.setPlayerLocation(tx,ty,tz,ry,rp,S08PacketPlayerPosLook.EnumFlags.func_180053_a(flags));
            List<String> target=doubles(p.posX,p.posY,p.posZ);
            List<Float> rotation=Arrays.asList(p.rotationYaw,p.rotationPitch);
            double px=p.posX,py=p.posY,pz=p.posZ;float inputYaw=123,inputPitch=-32;
            int type=6;
            switch(scenario){
                case 0: type=3; break;
                case 1: type=5; break;
                case 2: px+=.5; break;
                case 3: px=Math.nextAfter(px+.5,px); break;
                case 4: px=Math.nextUp(px+.5); break;
                case 5: py-=.5; break;
                case 6: pz-=.5; break;
                case 7: px+=.25;py+=.25;pz+=.25;break;
                case 8: px+=1.;break;
                case 9: px=Double.NaN;break;
                case 10: inputYaw=Float.POSITIVE_INFINITY;break;
                case 11: type=4;break;
                case 12: type=4; px+=1.;break;
                case 13: type=5; break;
                case 14: type=3; break;
                case 15: px=Double.NEGATIVE_INFINITY;break;
                case 16: type=5;inputPitch=Float.NaN;break;
                case 17: py=-999.;break;
                case 19: px-=.125;pz+=.375;break;
            }
            C03PacketPlayer packet=type==3?new C03PacketPlayer(true):type==4?new C03PacketPlayer.C04PacketPlayerPosition(px,py,pz,false):
                type==5?new C03PacketPlayer.C05PacketPlayerLook(inputYaw,inputPitch,true):new C03PacketPlayer.C06PacketPlayerPosLook(px,py,pz,inputYaw,inputPitch,false);
            int now=scenario==12||scenario==13||scenario==14?21:20;
            tick.setInt(h,now);
            boolean admitted=false;
            try { h.processPlayer(packet); } catch(Admitted hook){admitted=true;}
            Map<String,Object> row=new LinkedHashMap<>();
            row.put("flags",flags);row.put("from",doubles(x,y,z));row.put("from_rot",Arrays.asList(yaw,pitch));
            row.put("request",doubles(tx,ty,tz));row.put("request_rot",Arrays.asList(ry,rp));
            row.put("position",target);row.put("rotation",rotation);row.put("sent",h.sent);
            row.put("type",type);row.put("packet",doubles(packet.getPositionX(),packet.getPositionY(),packet.getPositionZ()));
            row.put("packet_rot",Arrays.asList(Integer.toUnsignedLong(Float.floatToRawIntBits(packet.getYaw())),Integer.toUnsignedLong(Float.floatToRawIntBits(packet.getPitch()))));
            row.put("now",now);row.put("end",p.playerConqueredTheEnd);row.put("admitted",admitted);
            row.put("ack",moved.getBoolean(h));row.put("last_tick",lastTick.getInt(h));row.put("kick",h.kick);
            cases.add(row);
        }
        try(Writer out=new OutputStreamWriter(new FileOutputStream(args[1]),"UTF-8")){new Gson().toJson(cases,out);}
        System.out.println("Server teleport/C03 admission: "+cases.size()+" cases.");
    }
}
