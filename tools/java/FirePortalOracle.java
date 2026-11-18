import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import java.util.concurrent.atomic.AtomicLong;
import java.util.zip.GZIPOutputStream;
import com.google.gson.Gson;
import net.minecraft.block.Block;
import net.minecraft.init.Bootstrap;
import net.minecraft.server.MinecraftServer;
import net.minecraft.util.BlockPos;
import net.minecraft.world.*;

/** Original BlockFire.onBlockAdded, support and complete BlockPortal.Size checks. */
public final class FirePortalOracle {
    static final BlockPos AT=new BlockPos(8,5,8);
    static void set(WorldServer w,BlockPos p,int s,int flags){Block b=Block.getBlockById(s>>4);w.setBlockState(p,b.getStateFromMeta(s&15),flags);}
    static void edit(List<int[]> edits,BlockPos p,int s){edits.add(new int[]{p.getX(),p.getY(),p.getZ(),s});}
    static String seed(Random r)throws Exception{return Long.toString(((AtomicLong)RuntimeUseOracle.field(Random.class,"seed").get(r)).get());}
    static List<Object> frame(WorldServer w)throws Exception{
        List<int[]> blocks=new ArrayList<>();for(int x=5;x<=32;x++)for(int z=5;z<=32;z++)for(int y=3;y<=28;y++){
            BlockPos p=new BlockPos(x,y,z);int s=RuntimeUseOracle.state(w.getBlockState(p));if(s>>4==51||s>>4==90)blocks.add(new int[]{x,y,z,s});
        }
        List<Object> ticks=new ArrayList<>();for(NextTickListEntry entry:(Set<NextTickListEntry>)RuntimeUseOracle.field(WorldServer.class,"pendingTickListEntriesTreeSet").get(w)){
            if(Block.getIdFromBlock(entry.getBlock())==51)ticks.add(Arrays.asList(entry.position.getX(),entry.position.getY(),entry.position.getZ(),entry.scheduledTime,entry.priority));
        }
        return Arrays.asList(blocks,ticks,seed(w.rand));
    }
    static Map<String,Object> run(MinecraftServer host,int axis,int width,int height,int variant)throws Exception{
        RuntimeUseOracle.Fixture w=new RuntimeUseOracle.Fixture(host);host.worldServers=new WorldServer[]{w,null,null};w.init();
        for(int x=-1;x<=2;x++)for(int z=-1;z<=2;z++)w.theChunkProviderServer.loadChunk(x,z);
        List<int[]> edits=new ArrayList<>();BlockPos cut=AT.down();int dx=axis==1?1:0,dz=axis==1?0:1;
        if(axis==0){
            if(variant!=0)edit(edits,variant<6?AT.down():AT.east(),new int[]{0,16,5<<4,44<<4,44<<4|8,54<<4|2,18<<4,1<<4,35<<4}[variant]);
            cut=variant<6?AT.down():AT.east();
        }else{
            for(int i=0;i<width;i++){BlockPos bottom=AT.add(dx*i,-1,dz*i);edit(edits,bottom,49<<4);if(!(variant==1&&i==1))edit(edits,AT.add(dx*i,height,dz*i),49<<4);}
            for(int j=0;j<height;j++){edit(edits,AT.add(-dx,j,-dz),49<<4);if(!(variant==2&&j==1))edit(edits,AT.add(dx*width,j,dz*width),49<<4);}
            if(variant==3)edit(edits,AT.add(dx,1,dz),90<<4|axis);if(variant==4)edit(edits,AT.add(dx,1,dz),1<<4);
            cut=AT.add(dx*(width-1),height,dz*(width-1));
        }
        for(int[] e:edits)set(w,new BlockPos(e[0],e[1],e[2]),e[3],2);
        w.setTotalWorldTime(0);w.rand.setSeed(731);set(w,AT,51<<4,3);
        List<Object> frames=new ArrayList<>();frames.add(frame(w));set(w,cut,0,3);frames.add(frame(w));
        Map<String,Object> row=new LinkedHashMap<>();row.put("description",new int[]{axis,width,height,variant});row.put("edits",edits);row.put("cut",new int[]{cut.getX(),cut.getY(),cut.getZ()});row.put("frames",frames);return row;
    }
    public static void main(String[] argv)throws Exception{
        Bootstrap.register();File dir=new File(argv[1]);dir.mkdirs();Constructor<?> c=Class.forName("VanillaWorldBridge$Host").getDeclaredConstructor(File.class);c.setAccessible(true);MinecraftServer host=(MinecraftServer)c.newInstance(dir);
        List<Object> cases=new ArrayList<>();for(int v=0;v<9;v++)cases.add(run(host,0,0,0,v));for(int axis=1;axis<=2;axis++)for(int width:new int[]{1,2,4,21,22})for(int height:new int[]{2,3,5,21,22})for(int v=0;v<5;v++)cases.add(run(host,axis,width,height,v));
        try(Writer out=new OutputStreamWriter(new GZIPOutputStream(new FileOutputStream(argv[0])),"UTF-8")){new Gson().toJson(cases,out);}
        System.out.println("Original fire/portal starts "+cases.size()+", checkpoints "+(cases.size()*2));
    }
}
