import java.util.*;
import com.google.gson.Gson;
import net.minecraft.init.Bootstrap;
import net.minecraft.profiler.Profiler;
import net.minecraft.world.*;
import net.minecraft.world.storage.WorldInfo;
import net.minecraft.world.chunk.IChunkProvider;
import net.minecraft.world.gen.structure.*;
import net.minecraft.nbt.*;

/** Layout-only original World/MapGenVillage calls, no world population emulator. */
public final class VillageOracle {
    static final class LayoutWorld extends World {
        LayoutWorld(long seed){super(null,new WorldInfo(new WorldSettings(seed,WorldSettings.GameType.SURVIVAL,true,false,WorldType.DEFAULT),"oracle"),WorldProvider.getProviderForDimension(0),new Profiler(),false);provider.registerWorld(this);}
        protected IChunkProvider createChunkProvider(){return null;}
        protected int getRenderDistanceChunks(){return 8;}
    }
    static final class Candidate extends MapGenVillage {
        Candidate(World world){this.worldObj=world;}
        boolean candidate(int x,int z){return canSpawnStructureAtCoords(x,z);}
    }
    static Object nbt(NBTBase value) {
        if(value instanceof NBTTagCompound){Map<String,Object> out=new TreeMap<String,Object>();NBTTagCompound c=(NBTTagCompound)value;for(String key:c.getKeySet())out.put(key,nbt(c.getTag(key)));return out;}
        if(value instanceof NBTTagList){List<Object> out=new ArrayList<Object>();NBTTagList list=(NBTTagList)value;for(int i=0;i<list.tagCount();i++)out.add(nbt(list.get(i)));return out;}
        if(value instanceof NBTTagIntArray)return ((NBTTagIntArray)value).getIntArray();
        if(value instanceof NBTTagString)return ((NBTTagString)value).getString();
        if(value instanceof NBTTagByte)return ((NBTTagByte)value).getByte();
        if(value instanceof NBTTagInt)return ((NBTTagInt)value).getInt();
        if(value instanceof NBTTagShort)return ((NBTTagShort)value).getShort();
        if(value instanceof NBTTagLong)return Long.toString(((NBTTagLong)value).getLong());
        throw new IllegalStateException("Unexpected layout NBT "+value.getClass());
    }
    static Map<String,Object> layout(LayoutWorld world,long seed,int x,int z) {
        Random random=new Random(seed);long a=random.nextLong(),b=random.nextLong();random.setSeed(x*a ^ z*b ^ seed);random.nextInt();
        MapGenVillage.Start start=new MapGenVillage.Start(world,random,x,z,0);
        Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("seed",Long.toString(seed));row.put("x",x);row.put("z",z);row.put("candidate",new Candidate(world).candidate(x,z));row.put("nbt",nbt(start.writeStructureComponentsToNBT(x,z)));row.put("next_long",Long.toString(random.nextLong()));return row;
    }
    public static void main(String[] args) {
        Bootstrap.register();List<Map<String,Object>> cases=new ArrayList<Map<String,Object>>();
        for(long seed:new long[]{0,1,-1,189,8675309,Long.MIN_VALUE,Long.MAX_VALUE}) {
            LayoutWorld world=new LayoutWorld(seed);Candidate candidate=new Candidate(world);
            for(int[] p:new int[][]{{0,0},{-1,-1},{15,-27},{-101,92}})cases.add(layout(world,seed,p[0],p[1]));
            int found=0;
            for(int gx=-8;gx<=8;gx++)for(int gz=-8;gz<=8;gz++) {
                Random r=world.setRandomSeed(gx,gz,10387312);int x=gx*32+r.nextInt(24),z=gz*32+r.nextInt(24);
                if(candidate.candidate(x,z) && found++<24)cases.add(layout(world,seed,x,z));
            }
        }
        Map<String,Object> out=new LinkedHashMap<String,Object>();out.put("cases",cases);System.out.println(new Gson().toJson(out));
    }
}
