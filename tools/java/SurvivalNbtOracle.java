// Original NBT numeric conversion and direct active-effect Map.put loading.
import com.google.gson.Gson;
import java.io.*;
import java.util.*;
import net.minecraft.init.Bootstrap;
import net.minecraft.nbt.*;
import net.minecraft.potion.PotionEffect;

public final class SurvivalNbtOracle {
    public static void main(String[] args)throws Exception {
        Bootstrap.register();List<Object> out=new ArrayList<Object>();
        NBTBase[] numbers={new NBTTagByte((byte)-1),new NBTTagShort((short)-123),new NBTTagInt(-1),new NBTTagInt(22),new NBTTagLong(18014398509481987L),new NBTTagLong(-18014398509481987L),new NBTTagFloat(-1.75f),new NBTTagFloat(1.75f),new NBTTagFloat(-0f),new NBTTagFloat(Float.NEGATIVE_INFINITY),new NBTTagFloat(Float.POSITIVE_INFINITY),new NBTTagFloat(Float.NaN),new NBTTagDouble(-1.75),new NBTTagDouble(1.75),new NBTTagDouble(-2147483649.0),new NBTTagDouble(2147483648.0),new NBTTagDouble(Double.NaN),new NBTTagString("not numeric")};
        for(NBTBase number:numbers)for(boolean byteFlag:new boolean[]{false,true}){
            NBTTagCompound tag=new NBTTagCompound();for(String key:new String[]{"foodLevel","foodTickTimer","foodSaturationLevel","foodExhaustionLevel","AbsorptionAmount"})tag.setTag(key,number.copy());
            NBTTagList effects=new NBTTagList();NBTTagCompound old=new NBTTagCompound();old.setByte("Id",(byte)22);old.setInteger("Duration",2400);old.setByte("Amplifier",(byte)3);effects.appendTag(old);
            NBTTagCompound e=new NBTTagCompound();e.setByte("Id",(byte)22);e.setTag("Duration",number.copy());e.setTag("Amplifier",number.copy());e.setTag("Ambient",number.copy());if(byteFlag)e.setByte("ShowParticles",(byte)0);else e.setInteger("ShowParticles",0);effects.appendTag(e);tag.setTag("ActiveEffects",effects);
            FoodOracle.Player p=FoodOracle.player(2,true,false,false,20,20);p.readEntityFromNBT(tag);
            ByteArrayOutputStream bytes=new ByteArrayOutputStream();CompressedStreamTools.write(tag,new DataOutputStream(bytes));StringBuilder hex=new StringBuilder();for(byte b:bytes.toByteArray())hex.append(String.format("%02x",b&255));
            Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("nbt_hex",hex.toString());row.put("food",FoodOracle.snapshot(p));row.put("absorption",FoodOracle.bits(p.getAbsorptionAmount()));List<Object> loaded=new ArrayList<Object>();for(PotionEffect effect:p.getActivePotionEffects())loaded.add(Arrays.asList(effect.getPotionID(),effect.getDuration(),effect.getAmplifier(),effect.getIsAmbient(),effect.getIsShowParticles()));row.put("effects",loaded);out.add(row);
        }
        System.out.println(new Gson().toJson(out));
    }
}
