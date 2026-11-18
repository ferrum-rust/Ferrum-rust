import com.google.gson.Gson;
import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import net.minecraft.block.*;
import net.minecraft.block.state.IBlockState;
import net.minecraft.entity.*;
import net.minecraft.entity.monster.*;
import net.minecraft.entity.player.*;
import net.minecraft.init.*;
import net.minecraft.item.ItemStack;
import net.minecraft.network.*;
import net.minecraft.profiler.Profiler;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.management.*;
import net.minecraft.stats.StatisticsFile;
import net.minecraft.util.*;
import net.minecraft.world.*;
import net.minecraft.world.storage.*;

/** Original bed activation, server player sleeping, waking and placement.
 * The grid provides the block layout and optional monster bounding box. */
public final class SleepOracle {
    static class Grid extends WorldServer {
        Map<BlockPos,IBlockState> cells;
        List<int[]> writes;
        EntityZombie zombie;
        boolean nearby;
        Grid(MinecraftServer h, WorldInfo info) {super(h,new SaveHandlerMP(),info,0,new Profiler());}
        @Override public IBlockState getBlockState(BlockPos p) {
            if(cells==null)return super.getBlockState(p);
            return cells.getOrDefault(p,Blocks.air.getDefaultState());
        }
        @Override public boolean setBlockState(BlockPos p,IBlockState s,int flags) {
            if(cells==null)return super.setBlockState(p,s,flags);
            if(p.getY()<0||p.getY()>=256||getBlockState(p)==s)return false;
            cells.put(new BlockPos(p),s);
            Block b=s.getBlock();writes.add(new int[]{p.getX(),p.getY(),p.getZ(),Block.getIdFromBlock(b)<<4|b.getMetaFromState(s),flags});
            return true;
        }
        @Override public <T extends Entity> List<T> getEntitiesWithinAABB(Class<? extends T> type,AxisAlignedBB box) {
            if(type==EntityMob.class){
                List<T> found=new ArrayList<>();
                if(nearby && box.intersectsWith(zombie.getEntityBoundingBox())) found.add(type.cast(zombie));
                return found;
            }
            return super.getEntitiesWithinAABB(type,box);
        }
        void put(BlockPos p,int packed){cells.put(p,Block.getBlockById(packed>>4).getStateFromMeta(packed&15));}
    }
    static class Player extends PlayerPoseOracle.Player {
        EnumStatus attempt;
        Player(MinecraftServer h,Grid w){super(h,w,1);}
        @Override public EnumStatus trySleep(BlockPos p){return attempt=super.trySleep(p);}
        @Override public void addChatComponentMessage(IChatComponent message) {}
    }
    static List<Double> pos(Player p){return Arrays.asList(p.posX,p.posY,p.posZ);}
    static List<Integer> bed(Player p){BlockPos loc=p.getBedLocation();return loc==null?null:Arrays.asList(loc.getX(),loc.getY(),loc.getZ());}
    public static void main(String[] args)throws Exception {
        Bootstrap.register();File dir=new File(args[0]);dir.mkdirs();
        Constructor<?> ctor=Class.forName("VanillaWorldBridge$Host").getDeclaredConstructor(File.class);ctor.setAccessible(true);
        MinecraftServer host=(MinecraftServer)ctor.newInstance(dir);
        WorldSettings settings=new WorldSettings(189,WorldSettings.GameType.SURVIVAL,false,false,WorldType.DEFAULT);
        Grid w=new Grid(host,new WorldInfo(settings,"beds"));host.worldServers=new WorldServer[]{w,null,null};w.init();
        host.setConfigManager(new ServerConfigurationManager(host){@Override public StatisticsFile getPlayerStatsFile(EntityPlayer p){return new StatisticsFile(host,new File(dir,"stats.json"));}});
        Player p=new Player(host,w);new NetHandlerPlayServer(host,new NetworkManager(EnumPacketDirection.SERVERBOUND),p){@Override public void sendPacket(Packet packet){}};
        w.zombie=new EntityZombie(w);BlockPos head=new BlockPos(15,72,-16);
        for(int x=-1;x<=1;x++)for(int z=-2;z<=0;z++)w.getChunkFromChunkCoords(x,z);
        w.cells=new HashMap<>();w.writes=new ArrayList<>();w.playerEntities.add(p);
        List<Object> nights=new ArrayList<>(),places=new ArrayList<>(),days=new ArrayList<>();
        for(int tm=0;tm<24000;tm++){
            w.setWorldTime(tm);w.setSkylightSubtracted(w.calculateSkylightSubtracted(1));
            days.add(w.isDaytime()?1:0);
        }
        Field sleepTimer=EntityPlayer.class.getDeclaredField("sleepTimer");sleepTimer.setAccessible(true);
        for(int i=0;i<288;i++){
            int dirn=i%4,scenario=i/4%12,mode=i/48;
            long time=scenario==1?6000:15000;
            w.setWorldTime(time);w.setSkylightSubtracted(w.calculateSkylightSubtracted(1));
            w.cells.clear();w.writes.clear();
            for(int x=-2;x<=2;x++)for(int z=-2;z<=2;z++)w.put(head.add(x,-1,z),16);
            BlockPos foot=head.offset(EnumFacing.getHorizontal(dirn).getOpposite());
            int state=26<<4|dirn|8|(mode%2!=0?4:0);
            w.put(head,state);w.put(foot,26<<4|dirn);
            p.setSpawnPoint(null,false);p.attempt=null;p.setHealth(20);sleepTimer.setInt(p,0);
            double x=head.getX()+0.5,y=head.getY(),z=head.getZ()+0.5;
            if(scenario==2)x=head.getX()+3.;if(scenario==3)x=head.getX()+3.00001;
            if(scenario==4)y+=2.;if(scenario==5)y+=2.00001;
            if(scenario==6)z=head.getZ()-3.00001;
            p.setPositionAndRotation(x,y,z,0,0);
            w.nearby=scenario==7||scenario==8;
            w.zombie.setPosition(head.getX()+(scenario==8?8.5:7.5),head.getY(),head.getZ()+0.5);
            BlockPos click=mode%3==1?foot:head;
            if(scenario==9)w.put(head,0);
            if(scenario==10)p.setHealth(0);
            boolean valid=w.getBlockState(click).getBlock()==Blocks.bed;
            if(valid)Blocks.bed.onBlockActivated(w,click,w.getBlockState(click),p,EnumFacing.UP,0,0,0);
            Map<String,Object> row=new LinkedHashMap<>();
            row.put("i",i);row.put("time",time);row.put("start",Arrays.asList(x,y,z));
            row.put("attempt",p.attempt==null?"NONE":p.attempt.name());
            row.put("sleeping",p.isPlayerSleeping());row.put("rest",pos(p));
            row.put("state",w.getBlockState(head).getBlock()==Blocks.bed?Blocks.bed.getMetaFromState(w.getBlockState(head)): -1);
            if(p.isPlayerSleeping()){
                if(scenario==11)w.put(head,0);
                p.wakeUpPlayer(mode%2==0,true,mode%3!=0);
            }
            row.put("wake",pos(p));row.put("spawn",bed(p));row.put("timer",p.getSleepTimer());
            nights.add(row);
        }
        p.setHealth(20);
        int[] states={0,16,31<<4|1,78<<4,9<<4,50<<4|5,53<<4|4,44<<4|8,20<<4,154<<4,175<<4|2};
        Random rng=new Random(189);
        for(int i=0;i<1728;i++){
            w.cells.clear();w.writes.clear();
            int face=i%6;float yaw=(float)((i%24)*22.5-180+(i%3==0?-.001:0));
            p.rotationYaw=yaw;
            List<int[]> cells=new ArrayList<>();
            for(int x=-1;x<=1;x++)for(int z=-1;z<=1;z++)for(int y=-1;y<=1;y++){
                int raw=i%4==0?(y<0?16:0):states[rng.nextInt(states.length)];
                w.put(head.add(x,y,z),raw);cells.add(new int[]{x,y,z,raw});
            }
            ItemStack stack=new ItemStack(Items.bed,3);boolean ok=stack.onItemUse(p,w,head,EnumFacing.getFront(face),.5f,.5f,.5f);
            places.add(Arrays.asList(face,yaw,cells,ok,stack.stackSize,new ArrayList<>(w.writes)));
        }
        List<Object> cycles=new ArrayList<>();
        w.getGameRules().setOrCreateGameRule("doMobSpawning","false");
        w.getGameRules().setOrCreateGameRule("randomTickSpeed","0");
        for(int scenario=0;scenario<5;scenario++){
            w.setWorldTime(15000);w.setSkylightSubtracted(w.calculateSkylightSubtracted(1));
            w.getGameRules().setOrCreateGameRule("doDaylightCycle",scenario==4?"false":"true");
            w.nearby=false;w.cells.clear();p.setHealth(20);p.setSpawnPoint(null,false);sleepTimer.setInt(p,0);
            for(int x=-2;x<=2;x++)for(int z=-2;z<=2;z++)w.put(head.add(x,-1,z),16);
            w.put(head,26<<4|8);w.put(head.north(),26<<4);
            p.setPositionAndRotation(15.5,72,-15.5,0,0);
            Blocks.bed.onBlockActivated(w,head,w.getBlockState(head),p,EnumFacing.UP,0,0,0);
            List<Object> ticks=new ArrayList<>();
            for(int t=0;t<112;t++){
                if(t==18 && scenario==1)p.wakeUpPlayer(false,true,true);
                if(t==18 && scenario==2)w.put(head,0);
                if(t==18 && scenario==3)w.setWorldTime(6000);
                w.tick();
                p.onUpdateEntity();
                int meta=w.getBlockState(head).getBlock()==Blocks.bed?Blocks.bed.getMetaFromState(w.getBlockState(head)):-1;
                ticks.add(Arrays.asList(w.getWorldTime(),p.isPlayerSleeping(),p.getSleepTimer(),bed(p),meta));
            }
            cycles.add(ticks);
        }
        try(Writer out=new OutputStreamWriter(new FileOutputStream(args[1]),"UTF-8")){new Gson().toJson(Arrays.asList(days,nights,places,cycles),out);}
        System.out.println("Original beds: "+days.size()+" day ticks, "+nights.size()+" sleep/wake, "+places.size()+" placements.");
    }
}
