import java.io.*;
import java.util.*;
import com.google.gson.Gson;
import net.minecraft.init.*;
import net.minecraft.block.*;
import net.minecraft.block.state.IBlockState;
import net.minecraft.entity.Entity;
import net.minecraft.profiler.Profiler;
import net.minecraft.world.*;
import net.minecraft.world.storage.WorldInfo;
import net.minecraft.world.chunk.IChunkProvider;
import net.minecraft.util.BlockPos;

/** Original graph/callback bytecode. Recursive World notifications are explicit
 * output, allowing integration to replay them in original scheduler order. */
public final class RailOracle {
    static final Comparator<BlockPos> ORDER=(a,b)->{int c=Integer.compare(a.getX(),b.getX());if(c==0)c=Integer.compare(a.getY(),b.getY());return c==0?Integer.compare(a.getZ(),b.getZ()):c;};
    static long hash(long h,int n){for(int i=0;i<4;i++)h=((h^((n>>>(i*8))&255))*0x100000001b3L);return h;}
    static int raw(IBlockState s){return Block.getIdFromBlock(s.getBlock())<<4|s.getBlock().getMetaFromState(s);}
    static IBlockState state(int s){return Block.getBlockById(s>>4).getStateFromMeta(s&15);}
    static final class TestWorld extends World {
        final TreeMap<BlockPos,IBlockState> blocks=new TreeMap<BlockPos,IBlockState>(ORDER);final Set<BlockPos> power=new HashSet<BlockPos>();final List<int[]> writes=new ArrayList<int[]>(),notifications=new ArrayList<int[]>(),powers=new ArrayList<int[]>();long readHash=0xcbf29ce484222325L;int reads,drops;
        TestWorld(){super(null,new WorldInfo(new WorldSettings(189L,WorldSettings.GameType.SURVIVAL,false,false,WorldType.DEFAULT),"oracle"),new WorldProviderSurface(),new Profiler(),false);provider.registerWorld(this);rand.setSeed(189);}
        protected IChunkProvider createChunkProvider(){return null;}
        protected int getRenderDistanceChunks(){return 8;}
        public IBlockState getBlockState(BlockPos p){reads++;readHash=hash(hash(hash(readHash,p.getX()),p.getY()),p.getZ());IBlockState s=blocks.get(p);return s==null?(p.getY()==10?Blocks.stone:Blocks.air).getDefaultState():s;}
        public boolean setBlockState(BlockPos p,IBlockState s,int flags){writes.add(new int[]{p.getX(),p.getY(),p.getZ(),raw(s),flags});blocks.put(new BlockPos(p),s);return true;}
        public boolean isBlockPowered(BlockPos p){powers.add(new int[]{p.getX(),p.getY(),p.getZ()});return power.contains(p);}
        public void notifyNeighborsOfStateChange(BlockPos p,Block b){notifications.add(new int[]{p.getX(),p.getY(),p.getZ(),Block.getIdFromBlock(b)});}
        public boolean spawnEntityInWorld(Entity e){drops++;return true;}
    }
    static Map<String,Object> execute(TestWorld world,String kind,int center,boolean force,boolean sourcePower){
            List<int[]> initial=new ArrayList<int[]>(),power=new ArrayList<int[]>();for(Map.Entry<BlockPos,IBlockState> e:world.blocks.entrySet()){BlockPos q=e.getKey();initial.add(new int[]{q.getX(),q.getY(),q.getZ(),raw(e.getValue())});}for(BlockPos q:new TreeSet<BlockPos>(ORDER){{addAll(world.power);}})power.add(new int[]{q.getX(),q.getY(),q.getZ()});
            int result=center;BlockPos p=new BlockPos(0,11,0);BlockRailBase block=(BlockRailBase)Block.getBlockById(center>>4);
            if(kind.equals("shape"))result=raw(block.new Rail(world,p,state(center)).func_180364_a(world.isBlockPowered(p),force).getBlockState());
            else if(kind.equals("added"))block.onBlockAdded(world,p,state(center));
            else if(kind.equals("neighbor"))block.onNeighborBlockChange(world,p,state(center),sourcePower?Blocks.redstone_block:Blocks.rail);
            else block.breakBlock(world,p,state(center));
            Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("kind",kind);row.put("center",center);row.put("force",force);row.put("source_power",sourcePower);row.put("initial",initial);row.put("power",power);row.put("result",result);row.put("reads",world.reads);row.put("read_hash",Long.toUnsignedString(world.readHash));row.put("writes",world.writes);row.put("notifications",world.notifications);row.put("power_queries",world.powers);row.put("drops",world.drops);return row;
    }
    public static void main(String[]args)throws Exception{
        PrintStream out=System.out;System.setOut(System.err);Bootstrap.register();Random r=new Random(189);List<Object> rows=new ArrayList<Object>();
        int[] rails={66,27,28,157};int[] support={0,16,44<<4,44<<4|8,53<<4,53<<4|4,78<<4|6,78<<4|7,154<<4};
        for(int i=0;i<8192;i++){
            String kind=new String[]{"shape","added","neighbor","removed"}[i%4];int id=kind.equals("shape")?rails[(i/4)%4]:i<4096?66:kind.equals("added")?new int[]{27,157}[(i/4)%2]:rails[(i/4)%4];int center=id<<4|r.nextInt(id==66?10:6)|(id!=66&&r.nextBoolean()?8:0);boolean force=r.nextBoolean(),sourcePower=r.nextBoolean();TestWorld world=new TestWorld();
            for(int x=-2;x<=2;x++)for(int y=10;y<=12;y++)for(int z=-2;z<=2;z++)if(r.nextInt(4)==0){int rail=rails[r.nextInt(4)];int s=rail<<4|r.nextInt(rail==66?10:6)|(rail!=66&&r.nextBoolean()?8:0);world.blocks.put(new BlockPos(x,y,z),state(s));}
            BlockPos p=new BlockPos(0,11,0);world.blocks.put(p,state(center));world.blocks.put(p.down(),state(support[r.nextInt(support.length)]));
            for(int x=-2;x<=2;x++)for(int y=10;y<=12;y++)for(int z=-2;z<=2;z++)if(r.nextBoolean())world.power.add(new BlockPos(x,y,z));
            rows.add(execute(world,kind,center,force,sourcePower));
        }
        for(int id:new int[]{27,157})for(int shape=0;shape<6;shape++)for(int distance=1;distance<=10;distance++){
            TestWorld world=new TestWorld();int[] d=new int[][]{{0,0,1},{1,0,0},{1,1,0},{-1,1,0},{0,1,-1},{0,1,1}}[shape];
            for(int k=0;k<=10;k++){BlockPos q=new BlockPos(d[0]*k,11+d[1]*k,d[2]*k);world.blocks.put(q,state(id<<4|shape|(k>0?8:0)));world.blocks.put(q.down(),Blocks.stone.getDefaultState());if(k==distance)world.power.add(q);}
            rows.add(execute(world,"neighbor",id<<4|shape,false,false));
        }
        out.println(new Gson().toJson(rows));
    }
}
