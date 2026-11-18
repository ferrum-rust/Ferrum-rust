// Original binary NBT for all 1.8 tag kinds, enchant order and modified UTF.
import com.google.gson.Gson;
import java.io.*;
import java.util.*;
import net.minecraft.init.Bootstrap;
import net.minecraft.nbt.*;
public final class ItemNbtOracle {
    public static void main(String[]args)throws Exception {
        Bootstrap.register();List<Object> out=new ArrayList<Object>();
        for(int pattern=0;pattern<16;pattern++){
            NBTTagCompound tag=new NBTTagCompound();tag.setByte("Byte",(byte)-127);tag.setShort("Short",(short)-32767);tag.setInteger("Int",Integer.MIN_VALUE);tag.setLong("Long",Long.MAX_VALUE-1);tag.setFloat("Float",Float.intBitsToFloat(pattern%2==0?0x7fc01234:0x80000000));tag.setDouble("Double",Double.longBitsToDouble(pattern%2==0?0x7ff8000000001234L:0x8000000000000000L));tag.setByteArray("Bytes",new byte[]{0,-1,-128,127});tag.setIntArray("Ints",new int[]{Integer.MIN_VALUE,0,Integer.MAX_VALUE});tag.setString("Name","NUL"+(char)0+" snowman "+(char)0x2603+" emoji "+new String(Character.toChars(0x1f600)));tag.setTag("Empty",new NBTTagList());
            NBTTagList ench=new NBTTagList();for(int[] entry:new int[][]{{34,pattern},{0,4},{99,-1},{0,2}}){NBTTagCompound e=new NBTTagCompound();e.setShort("id",(short)entry[0]);e.setShort("lvl",(short)entry[1]);e.setString("unknown","retained");ench.appendTag(e);}tag.setTag("ench",ench);tag.setBoolean("Unbreakable",pattern%2==0);NBTTagCompound nested=new NBTTagCompound();nested.setString("unknown", "payload");tag.setTag("mod:unknown",nested);
            ByteArrayOutputStream bytes=new ByteArrayOutputStream();CompressedStreamTools.write(tag,new DataOutputStream(bytes));StringBuilder hex=new StringBuilder();for(byte b:bytes.toByteArray())hex.append(String.format("%02x",b&255));out.add(hex.toString());
        }
        System.out.println(new Gson().toJson(out));
    }
}
