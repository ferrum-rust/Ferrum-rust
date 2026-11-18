// Actual WorldServer queue management; the inert probe callback records dispatch.
import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import sun.misc.Unsafe;
import com.google.gson.Gson;
import net.minecraft.block.*;
import net.minecraft.block.material.Material;
import net.minecraft.block.state.IBlockState;
import net.minecraft.init.*;
import net.minecraft.profiler.Profiler;
import net.minecraft.util.BlockPos;
import net.minecraft.world.*;
import net.minecraft.world.storage.WorldInfo;

public class ScheduledTickOracle {
    static Field field(Class<?> c,String n)throws Exception {Field f=c.getDeclaredField(n);f.setAccessible(true);return f;}
    static void set(Object o,Class<?> c,String n,Object v)throws Exception {field(c,n).set(o,v);}
    static class Probe extends Block {
        Probe(){super(Material.rock);}
        @Override public void updateTick(World world,BlockPos p,IBlockState state,Random random) {
            Fixture w=(Fixture)world;w.called.add(Arrays.asList(w.getTotalWorldTime(),p.getX(),p.getY(),p.getZ()));
            if(w.nested&&p.getX()==0){w.scheduleUpdate(new BlockPos(1,64,0),this,0);w.scheduleUpdate(p,this,0);}
        }
    }
    static final Probe probe=new Probe();
    static class Fixture extends WorldServer {
        boolean loaded,nested;Set<BlockPos> stale;List<Object> called;
        Fixture(){super(null,null,null,0,null);}
        @Override public boolean isAreaLoaded(BlockPos a,BlockPos b){return loaded&&a.getY()>=0&&b.getY()<256;}
        @Override public IBlockState getBlockState(BlockPos p){return stale.contains(p)?Blocks.air.getDefaultState():probe.getDefaultState();}
    }
    static Fixture fixture()throws Exception {
        Unsafe unsafe=(Unsafe)field(Unsafe.class,"theUnsafe").get(null);Fixture w=(Fixture)unsafe.allocateInstance(Fixture.class);
        set(w,World.class,"worldInfo",new WorldInfo(new WorldSettings(189L,WorldSettings.GameType.SURVIVAL,false,false,WorldType.DEFAULT),"TickOracle"));
        set(w,World.class,"theProfiler",new Profiler());set(w,World.class,"rand",new Random(189));
        set(w,WorldServer.class,"pendingTickListEntriesTreeSet",new TreeSet<NextTickListEntry>());
        set(w,WorldServer.class,"pendingTickListEntriesHashSet",new HashSet<NextTickListEntry>());
        set(w,WorldServer.class,"pendingTickListEntriesThisTick",new ArrayList<NextTickListEntry>());
        w.loaded=true;w.stale=new HashSet<>();w.called=new ArrayList<>();return w;
    }
    @SuppressWarnings("unchecked")
    static Object snapshot(Fixture w)throws Exception {
        List<Object> queue=new ArrayList<>();
        for(NextTickListEntry e:(Set<NextTickListEntry>)field(WorldServer.class,"pendingTickListEntriesTreeSet").get(w))queue.add(Arrays.asList(e.position.getX(),e.position.getY(),e.position.getZ(),e.scheduledTime,field(NextTickListEntry.class,"priority").getInt(e)));
        return Arrays.asList(new ArrayList<Object>(w.called),queue);
    }
    static Object run(int mode)throws Exception {
        Fixture w=fixture();List<Object> actions=new ArrayList<>(),states=new ArrayList<>();
        if(mode<4) {
            int count=mode==0?24:mode==1?1001:mode==2?2057:15;
            for(int i=0;i<count;i++){
                int x=mode==3?i%5:i,y=64,z=0,delay=mode==2?i%4:mode==3?15-i:1,priority=(i*17)%7-3;
                w.scheduleBlockUpdate(new BlockPos(x,y,z),probe,delay,priority);actions.add(Arrays.asList(0,x,y,z,delay,priority));
            }
        } else if(mode==4) {
            w.nested=true;
            for(int x=0;x<3;x++){w.scheduleBlockUpdate(new BlockPos(x,64,0),probe,1,0);actions.add(Arrays.asList(0,x,64,0,1,0));}
        } else {
            for(int x=0;x<12;x++){w.scheduleBlockUpdate(new BlockPos(x,64,0),probe,x%3,0);actions.add(Arrays.asList(0,x,64,0,x%3,0));}
        }
        states.add(snapshot(w));
        for(int tick=1;tick<=6;tick++) {
            if(mode==5&&tick==1){w.loaded=false;actions.add(Arrays.asList(1,0));}
            if(mode==5&&tick==3){w.loaded=true;actions.add(Arrays.asList(1,1));}
            if(mode==6&&tick==1){for(int x=0;x<12;x+=2){w.stale.add(new BlockPos(x,64,0));actions.add(Arrays.asList(2,x,64,0));}}
            w.setTotalWorldTime(tick);w.tickUpdates(false);actions.add(Arrays.asList(3));states.add(snapshot(w));
        }
        Map<String,Object> row=new LinkedHashMap<>();row.put("mode",mode);row.put("actions",actions);row.put("snapshots",states);return row;
    }
    public static void main(String[] args)throws Exception {
        Bootstrap.register();List<Object> rows=new ArrayList<>();for(int mode=0;mode<7;mode++)rows.add(run(mode));
        try(Writer writer=new OutputStreamWriter(new FileOutputStream(args[0]),"UTF-8")){new Gson().toJson(rows,writer);}
        System.out.println("Original WorldServer scheduled queue: 7 traces / 49 checkpoints");
    }
}
