import com.google.gson.Gson;
import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import java.util.concurrent.atomic.AtomicLong;
import java.util.zip.GZIPOutputStream;
import net.minecraft.block.Block;
import net.minecraft.entity.Entity;
import net.minecraft.entity.item.EntityTNTPrimed;
import net.minecraft.init.*;
import net.minecraft.server.MinecraftServer;
import net.minecraft.util.BlockPos;
import net.minecraft.world.*;
import net.minecraft.world.biome.BiomeGenBase;

/** Observe real scheduled block phases. Capture TNT before the separate entity phase. */
public final class FireTickOracle {
    static class World extends RuntimeUseOracle.Fixture {
        final List<Object> primed=new ArrayList<>();
        World(MinecraftServer host){super(host);}
        @Override public boolean spawnEntityInWorld(Entity e){
            if(e instanceof EntityTNTPrimed){
                List<String> motion=new ArrayList<>();
                for(double n:new double[]{e.posX,e.posY,e.posZ,e.motionX,e.motionY,e.motionZ})motion.add(Long.toHexString(Double.doubleToLongBits(n)));
                primed.add(Arrays.asList(motion,((EntityTNTPrimed)e).fuse));return true;
            }
            return super.spawnEntityInWorld(e);
        }
    }
    static Object frame(World w,int base)throws Exception{
        List<Object> queue=new ArrayList<>();
        for(NextTickListEntry e:(Set<NextTickListEntry>)RuntimeUseOracle.field(WorldServer.class,"pendingTickListEntriesTreeSet").get(w))
            queue.add(Arrays.asList(e.position.getX(),e.position.getY(),e.position.getZ(),Block.getIdFromBlock(e.getBlock()),e.scheduledTime,e.priority));
        long hash=0xcbf29ce484222325L;
        for(int x=0;x<16;x++)for(int z=0;z<16;z++)for(int y=base-2;y<=base+8;y++){
            hash^=RuntimeUseOracle.state(w.getBlockState(new BlockPos(x,y,z)));hash*=0x100000001b3L;
        }
        return Arrays.asList(Long.toUnsignedString(hash),queue,FirePortalOracle.seed(w.rand),new ArrayList<>(w.primed));
    }
    static List<int[]> edits(int scene,int age,int base){
        List<int[]> v=new ArrayList<>();BlockPos at=new BlockPos(8,base,8);
        for(int x=-3;x<=3;x++)for(int z=-3;z<=3;z++)FirePortalOracle.edit(v,at.add(x,-1,z),16);
        if(scene!=0&&scene!=5)FirePortalOracle.edit(v,at.down(),87<<4);
        if(scene>=2){
            int[] fuel={5,17,35,47,170,173,125,85};
            for(int x=-2;x<=2;x++)for(int z=-2;z<=2;z++){
                if(x==0&&z==0)continue;
                if(scene==3&&(x*x+z*z)<=1)continue;
                int y=scene==3?(x+2+z+2)%4:0;
                FirePortalOracle.edit(v,at.add(x,y,z),fuel[(x*5+z+32)&7]<<4);
            }
        }
        if(scene==4)for(int x=-2;x<=2;x++)for(int z=-2;z<=2;z++)FirePortalOracle.edit(v,at.add(x,3,z),16);
        if(scene==6)for(int[] d:new int[][]{{1,0},{0,1},{-1,0},{0,-1}})FirePortalOracle.edit(v,at.add(d[0],0,d[1]),46<<4);
        FirePortalOracle.edit(v,at,51<<4|age);
        return v;
    }
    static Object scene(MinecraftServer host,int index)throws Exception{
        World w=new World(host);host.worldServers=new WorldServer[]{w,null,null};w.init();
        int variant=index/7,scene=index%7,biome=new int[]{1,6,2,12,34,21,37,4}[variant%8],difficulty=variant%4,base=variant%8==4?145:5;
        boolean rain=variant%8>=1&&variant%8<=4,enabled=variant!=8;
        for(int x=-1;x<=1;x++)for(int z=-1;z<=1;z++)Arrays.fill(w.theChunkProviderServer.loadChunk(x,z).getBiomeArray(),(byte)biome);
        w.getWorldInfo().setDifficulty(EnumDifficulty.getDifficultyEnum(difficulty));w.getGameRules().setOrCreateGameRule("doFireTick",Boolean.toString(enabled));w.setRainStrength(rain?1f:0f);
        w.rand.setSeed(1889+index);w.setTotalWorldTime(0);
        Random math=(Random)RuntimeUseOracle.field(Class.forName("java.lang.Math$RandomNumberGeneratorHolder"),"randomNumberGenerator").get(null);math.setSeed(8189+index);
        List<int[]> edits=edits(scene,new int[]{0,3,4,12,15}[variant%5],base);
        for(int[] e:edits)FirePortalOracle.set(w,new BlockPos(e[0],e[1],e[2]),e[3],2);
        List<Object> frames=new ArrayList<>();
        for(int tick=0;tick<=150;tick++){
            if(tick!=0){w.setTotalWorldTime(tick);w.tickUpdates(false);}
            if(tick%10==0||!w.primed.isEmpty())frames.add(Arrays.asList(tick,frame(w,base),FirePortalOracle.seed(math)));
            if(!w.primed.isEmpty())break;
        }
        Map<String,Object> r=new LinkedHashMap<>();r.put("index",index);r.put("biome",biome);r.put("base",base);r.put("difficulty",difficulty);r.put("rain",rain);r.put("enabled",enabled);r.put("edits",edits);r.put("frames",frames);return r;
    }
    public static void main(String[] args)throws Exception{
        Bootstrap.register();File dir=new File(args[1]);dir.mkdirs();Constructor<?> ctor=Class.forName("VanillaWorldBridge$Host").getDeclaredConstructor(File.class);ctor.setAccessible(true);MinecraftServer host=(MinecraftServer)ctor.newInstance(dir);
        Method encour=Blocks.fire.getClass().getDeclaredMethod("getEncouragement",Block.class),flamm=Blocks.fire.getClass().getDeclaredMethod("getFlammability",Block.class);encour.setAccessible(true);flamm.setAccessible(true);
        int[][] fuels=new int[256][2];boolean[] rains=new boolean[256];
        for(int i=0;i<256;i++){Block b=Block.getBlockById(i);fuels[i][0]=(int)encour.invoke(Blocks.fire,b);fuels[i][1]=(int)flamm.invoke(Blocks.fire,b);BiomeGenBase bio=BiomeGenBase.getBiomeGenArray()[i];rains[i]=bio!=null&&bio.canRain();}
        List<Object> cases=new ArrayList<>();for(int i=0;i<112;i++)cases.add(scene(host,i));
        Map<String,Object> data=new LinkedHashMap<>();data.put("fuel",fuels);data.put("rain",rains);data.put("cases",cases);
        try(Writer out=new OutputStreamWriter(new GZIPOutputStream(new FileOutputStream(args[0])),"UTF-8")){new Gson().toJson(data,out);}
    }
}
