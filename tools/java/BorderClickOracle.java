import com.google.gson.Gson;
import io.netty.buffer.Unpooled;
import java.io.PrintStream;
import java.lang.reflect.Field;
import java.util.*;
import net.minecraft.block.Block;
import net.minecraft.init.*;
import net.minecraft.item.ItemStack;
import net.minecraft.nbt.*;
import net.minecraft.network.PacketBuffer;
import net.minecraft.network.play.server.S44PacketWorldBorder;
import net.minecraft.util.BlockPos;
import net.minecraft.world.border.WorldBorder;
import net.minecraft.world.storage.WorldInfo;

public final class BorderClickOracle {
    static Map<String,Object> one(int n,int action)throws Exception{
        PacketBuffer b=new PacketBuffer(Unpooled.buffer());b.writeVarIntToBuffer(action);
        double x=n%2==0?-15.25:29999981.5,z=n%3==0?31.03125:-13.75;
        double start=n%3==0?64.:4.25,end=n%2==0?7.75:30.125;
        long duration=new long[]{0L,17L,123456L,1L<<43}[n%4];
        if(action==0)b.writeDouble(end);
        if(action==1){b.writeDouble(start);b.writeDouble(end);b.writeVarLong(duration);}
        if(action==2){b.writeDouble(x);b.writeDouble(z);}
        if(action==3){b.writeDouble(x);b.writeDouble(z);b.writeDouble(start);b.writeDouble(end);b.writeVarLong(duration);
            b.writeVarIntToBuffer(n%2==0?100:29999984);b.writeVarIntToBuffer(n-2);b.writeVarIntToBuffer(n+17);}
        if(action>=4)b.writeVarIntToBuffer(n-2);
        String wire=MiddlePickOracle.hex(b);
        S44PacketWorldBorder packet=new S44PacketWorldBorder();packet.readPacketData(b);
        WorldBorder border=new WorldBorder();packet.func_179788_a(border);
        Field started=WorldBorder.class.getDeclaredField("startTime"),ended=WorldBorder.class.getDeclaredField("endTime");started.setAccessible(true);ended.setAccessible(true);
        long time=System.currentTimeMillis();
        long elapsed=(n+1)*5;
        if(action==1 || action==3&&duration>0){started.setLong(border,time-elapsed);ended.setLong(border,time-elapsed+duration);}
        List<Object> hits=new ArrayList<Object>();
        for(int attempt=0;attempt<64;attempt++){
            long before=System.currentTimeMillis();hits.clear();double diameter=border.getDiameter();
            for(int dx=-35;dx<=35;dx++)for(int dz:new int[]{-5,0,9}){
                BlockPos p=new BlockPos((int)Math.floor(border.getCenterX())+dx,63,(int)Math.floor(border.getCenterZ())+dz);
                hits.add(Arrays.asList(p.getX(),p.getZ(),border.contains(p)));
            }
            if(System.currentTimeMillis()!=before)continue;
            Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("wire",wire);row.put("elapsed",before-time+elapsed);
            row.put("diameter",MovementOracle.bits(diameter));row.put("center",MovementOracle.bits(border.getCenterX(),border.getCenterZ()));
            row.put("warnings",Arrays.asList(border.getWarningTime(),border.getWarningDistance()));row.put("contains",hits);return row;
        }
        throw new IllegalStateException("Unable to sample border inside one millisecond");
    }
    public static void main(String[] args)throws Exception{
        PrintStream out=System.out;System.setOut(System.err);Bootstrap.register();List<Object> rows=new ArrayList<Object>(),tools=new ArrayList<Object>(),names=new ArrayList<Object>();
        for(int i=0;i<15;i++)for(int action=0;action<6;action++)rows.add(one(i,action));
        for(Block b:Block.blockRegistry){
            int id=Block.getIdFromBlock(b);String key=Block.blockRegistry.getNameForObject(b).toString();
            names.add(Arrays.asList(id,key));for(String value:new String[]{key,key.substring(10),key.toUpperCase(),Integer.toString(id),"minecraft:"+id,"a:"+key.substring(10),"no-such-block"}) {
                ItemStack tool=new ItemStack(Items.diamond_pickaxe);NBTTagList l=new NBTTagList();l.appendTag(new NBTTagString(value));tool.setTagInfo("CanDestroy",l);
                tools.add(Arrays.asList(id,value,tool.canDestroy(b)));
            }
        }
        List<Object> cached=new ArrayList<Object>(),levels=new ArrayList<Object>();
        ItemStack held=new ItemStack(Items.diamond_pickaxe);
        for(int i=0;i<60;i++){
            String value=new String[]{"minecraft:stone","wool","gold_block"}[i%3];
            NBTTagList l=new NBTTagList();l.appendTag(new NBTTagString(value));held.setTagInfo("CanDestroy",l);
            Block target=Block.getBlockById(new int[]{1,1,35,35,41,41,1}[i%7]);
            if(i%11==0)held=held.copy();
            cached.add(Arrays.asList(i%11==0,value,Block.getIdFromBlock(target),held.canDestroy(target)));
        }
        for(int n=0;n<20;n++){
            NBTTagCompound input=new NBTTagCompound();
            if(n%5!=0){input.setFloat("BorderCenterX",n+.125f);input.setDouble("BorderCenterZ",-n-.75);
                input.setDouble("BorderSize",1234.5+n);input.setLong("BorderSizeLerpTime",n%3==0?0:345678L);
                input.setDouble("BorderSizeLerpTarget",10.5+n);input.setFloat("BorderWarningBlocks",n-.25f);input.setShort("BorderWarningTime",(short)(n+9));}
            WorldInfo info=new WorldInfo(input);
            levels.add(Arrays.asList(MiddlePickOracle.tag(input),MovementOracle.bits(info.getBorderCenterX(),info.getBorderCenterZ(),info.getBorderSize(),info.getBorderLerpTarget()),
                info.getBorderLerpTime(),info.getBorderWarningTime(),info.getBorderWarningDistance()));
        }
        Map<String,Object> result=new LinkedHashMap<String,Object>();result.put("border",rows);result.put("tools",tools);result.put("names",names);result.put("cache",cached);result.put("levels",levels);
        out.println(new Gson().toJson(result));
    }
}
