import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import com.google.gson.Gson;
import com.mojang.authlib.GameProfile;
import net.minecraft.entity.player.*;
import net.minecraft.init.Bootstrap;
import net.minecraft.nbt.*;
import net.minecraft.profiler.Profiler;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.management.*;
import net.minecraft.stats.StatisticsFile;
import net.minecraft.world.*;
import net.minecraft.world.storage.*;

/** Actual server player entity NBT read, including game-type capabilities. */
public final class PlayerPoseOracle {
    static class Player extends EntityPlayerMP {
        Player(MinecraftServer h,WorldServer w,int id){super(h,w,new GameProfile(new UUID(0,id),"PoseOracle"),new ItemInWorldManager(w));}
        @Override public void sendPlayerAbilities(){}
    }
    static NBTTagList doubles(double... values){NBTTagList l=new NBTTagList();for(double v:values)l.appendTag(new NBTTagDouble(v));return l;}
    static NBTTagList floats(float... values){NBTTagList l=new NBTTagList();for(float v:values)l.appendTag(new NBTTagFloat(v));return l;}
    static String binary(NBTTagCompound n)throws Exception {
        ByteArrayOutputStream out=new ByteArrayOutputStream();CompressedStreamTools.write(n,new DataOutputStream(out));
        StringBuilder hex=new StringBuilder();for(byte b:out.toByteArray())hex.append(String.format(Locale.ROOT,"%02x",b&255));return hex.toString();
    }
    public static void main(String[] args)throws Exception {
        Bootstrap.register();File dir=new File(args[0]);dir.mkdirs();
        Constructor<?> ctor=Class.forName("VanillaWorldBridge$Host").getDeclaredConstructor(File.class);ctor.setAccessible(true);
        MinecraftServer host=(MinecraftServer)ctor.newInstance(dir);
        WorldSettings settings=new WorldSettings(189,WorldSettings.GameType.SURVIVAL,false,false,WorldType.DEFAULT);
        WorldServer world=new WorldServer(host,new SaveHandlerMP(),new WorldInfo(settings,"pose"),0,new Profiler());host.worldServers=new WorldServer[]{world,null,null};world.init();
        host.setConfigManager(new ServerConfigurationManager(host){@Override public StatisticsFile getPlayerStatsFile(EntityPlayer p){return new StatisticsFile(host,new File(dir,"pose-stats.json"));}});
        List<Object> rows=new ArrayList<>();
        for(int i=0;i<64;i++){
            Player p=new Player(host,world,i+1);NBTTagCompound n=new NBTTagCompound();
            n.setTag("Pos",doubles(i%2==0?29999999.12345678:-32.000000001,63.123456789+i,i%3==0?-29999999.7654321:16.0000000001));
            if(i%7==0)n.setTag("Motion",floats(1,2,3));else n.setTag("Motion",doubles(i%2==0?10:10.00000001,i%3==0?-10:-10.00000001,i%5==0?-.0:.0734567890123));
            if(i%9==0)n.setTag("Rotation",doubles(90,-30));else n.setTag("Rotation",floats(i%2==0?450.25f:-720.75f,i%3==0?450:-62.125f));
            n.setBoolean("OnGround",(i&1)!=0);n.setInteger("Dimension",0);n.setInteger("playerGameType",i%2);
            if(i%7==2)n.setShort("OnGround",(short)256);
            if(i%7==3)n.setDouble("OnGround",-.25);
            n.setLong("SpawnX",(1L<<40)-i); n.setFloat("SpawnY",63.875f);
            if(i%5==1)n.setString("SpawnZ","missing");else if(i%5!=2)n.setDouble("SpawnZ",-16.125-i);
            if(i%3==1)n.setShort("SpawnForced",(short)256);else n.setDouble("SpawnForced",-.25);
            NBTTagCompound abilities=new NBTTagCompound();abilities.setBoolean("flying",(i&2)!=0);abilities.setBoolean("mayfly",(i&4)!=0);
            if(i%11==1)abilities.setShort("flying",(short)256);
            if(i%11==3)abilities.setFloat("flying",.25f);
            if(i%4!=0)abilities.setFloat("flySpeed",.03125f+i*.001f);
            if(i%6!=0)abilities.setFloat("walkSpeed",.078125f);
            n.setTag("abilities",abilities);p.readFromNBT(n);
            Map<String,Object> row=new LinkedHashMap<>();row.put("nbt",binary(n));
            if(p.getBedLocation()!=null){
                net.minecraft.util.BlockPos loc=p.getBedLocation();
                row.put("bed",Arrays.asList(loc.getX(),loc.getY(),loc.getZ(),p.isSpawnForced()?1:0));
            }
            row.put("pos",Arrays.asList(p.posX,p.posY,p.posZ));row.put("velocity",Arrays.asList(p.motionX,p.motionY,p.motionZ));
            row.put("rotation",Arrays.asList(p.rotationYaw,p.rotationPitch));row.put("grounded",p.onGround);
            row.put("flying",p.capabilities.isFlying);row.put("allow_flying",p.capabilities.allowFlying);row.put("creative",p.capabilities.isCreativeMode);
            row.put("fly_speed",Integer.toUnsignedLong(Float.floatToRawIntBits(p.capabilities.getFlySpeed())));
            row.put("walk_speed",Integer.toUnsignedLong(Float.floatToRawIntBits(p.capabilities.getWalkSpeed())));rows.add(row);
        }
        try(Writer out=new OutputStreamWriter(new FileOutputStream(args[1]),"UTF-8")){new Gson().toJson(rows,out);}
        System.out.println("Original player pose NBT: "+rows.size()+" records");
    }
}
