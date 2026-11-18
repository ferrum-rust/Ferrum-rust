import com.google.gson.*;
import java.io.*;
import java.util.*;
import net.minecraft.init.Bootstrap;
import net.minecraft.item.*;
import net.minecraft.nbt.*;
public final class ItemNbtEqualityOracle {
    static String hex(NBTTagCompound tag)throws Exception{ByteArrayOutputStream out=new ByteArrayOutputStream();CompressedStreamTools.write(tag,new DataOutputStream(out));StringBuilder hex=new StringBuilder();for(byte b:out.toByteArray())hex.append(String.format("%02x",b&255));return hex.toString();}
    public static void main(String[]args)throws Exception {
        Bootstrap.register();JsonArray payloads=new JsonParser().parse(new InputStreamReader(new FileInputStream(args[0]),"UTF-8")).getAsJsonArray();List<Object> rows=new ArrayList<Object>();
        for(JsonElement payload:payloads){String hex=payload.getAsString();byte[] bytes=new byte[hex.length()/2];for(int i=0;i<bytes.length;i++)bytes[i]=(byte)Integer.parseInt(hex.substring(i*2,i*2+2),16);NBTTagCompound tag=CompressedStreamTools.read(new DataInputStream(new ByteArrayInputStream(bytes)));
            for(int mode=0;mode<8;mode++){ItemStack a=new ItemStack(Item.getItemById(310),1,7);a.setTagCompound((NBTTagCompound)tag.copy());ItemStack b=mode==0?a:a.copy();
                if(mode==2)b.getTagCompound().setShort("Byte",(short)-127);if(mode==3)b.getTagCompound().removeTag("mod:unknown");if(mode==4)b.stackSize=2;if(mode==5)b.setItemDamage(8);if(mode==6)b.setTagCompound(null);if(mode==7){a.setTagCompound(null);b.setTagCompound(null);}
                Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("alias",mode==0);row.put("a",Arrays.asList(a.stackSize,a.getItemDamage(),a.hasTagCompound()?hex(a.getTagCompound()):null));row.put("b",Arrays.asList(b.stackSize,b.getItemDamage(),b.hasTagCompound()?hex(b.getTagCompound()):null));row.put("tags_equal",ItemStack.areItemStackTagsEqual(a,b));row.put("equal",ItemStack.areItemStacksEqual(a,b));rows.add(row);
            }
        }
        System.out.println(new GsonBuilder().serializeNulls().create().toJson(rows));
    }
}
