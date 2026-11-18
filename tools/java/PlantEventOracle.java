// Verification only: original vegetation callbacks with observable world writes.
// Neighbor recursion and item generation have separate real-world/drop oracles.
import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import sun.misc.Unsafe;
import com.google.gson.Gson;
import net.minecraft.block.Block;
import net.minecraft.block.state.IBlockState;
import net.minecraft.init.Bootstrap;
import net.minecraft.init.Blocks;
import net.minecraft.util.BlockPos;
import net.minecraft.world.*;
import net.minecraft.world.storage.WorldInfo;

public class PlantEventOracle {
    static int raw(IBlockState s){return (Block.getIdFromBlock(s.getBlock())<<4)|s.getBlock().getMetaFromState(s);}
    static IBlockState state(int raw){return Block.getBlockById(raw>>4).getStateFromMeta(raw&15);}
    static class Fixture extends WorldServer {
        Map<BlockPos,IBlockState> blocks;List<int[]> writes;int light;boolean sky;
        Fixture(){super(null,null,null,0,null);}
        @Override public IBlockState getBlockState(BlockPos p){IBlockState s=blocks.get(p);return s==null?Blocks.air.getDefaultState():s;}
        @Override public boolean isAirBlock(BlockPos p){return getBlockState(p).getBlock()==Blocks.air;}
        @Override public int getLight(BlockPos p){return light;}
        @Override public boolean canSeeSky(BlockPos p){return sky;}
        @Override public boolean isBlockNormalCube(BlockPos p,boolean fallback){
            if(p.getY()<0||p.getY()>=256||p.getX()<-30000000||p.getX()>=30000000||p.getZ()<-30000000||p.getZ()>=30000000)return fallback;
            Block b=getBlockState(p).getBlock();return b.getMaterial().isOpaque()&&b.isFullCube();
        }
        @Override public boolean setBlockState(BlockPos p,IBlockState s,int flags){
            if(p.getY()<0||p.getY()>=256)return false;
            writes.add(new int[]{p.getX(),p.getY(),p.getZ(),raw(s),flags});blocks.put(new BlockPos(p),s);return true;
        }
    }
    static void field(Object obj,Class<?> c,String name,Object value)throws Exception{Field f=c.getDeclaredField(name);f.setAccessible(true);f.set(obj,value);}
    static Fixture fixture()throws Exception{
        Field f=Unsafe.class.getDeclaredField("theUnsafe");f.setAccessible(true);Fixture w=(Fixture)((Unsafe)f.get(null)).allocateInstance(Fixture.class);
        field(w,World.class,"rand",new Random(0));field(w,World.class,"worldInfo",new WorldInfo(new WorldSettings(189,WorldSettings.GameType.SURVIVAL,false,false,WorldType.DEFAULT),"plant oracle"));
        field(w,World.class,"worldAccesses",new ArrayList<>());w.getGameRules().setOrCreateGameRule("doTileDrops","false");return w;
    }
    static Object run(Fixture w,int raw,int below,int above,int mask,int light,boolean sky,int y)throws Exception{
        BlockPos p=new BlockPos(0,y,0);w.blocks=new HashMap<>();w.writes=new ArrayList<>();w.light=light;w.sky=sky;
        w.blocks.put(p,state(raw));w.blocks.put(p.down(),state(below));w.blocks.put(p.up(),state(above));
        BlockPos[] sides={p.north(),p.east(),p.south(),p.west()};for(int i=0;i<4;i++)if((mask&(1<<i))!=0){w.blocks.put(sides[i],state(17<<4|3));w.blocks.put(sides[i].down(),state(9<<4));}
        w.rand.setSeed(189);state(raw).getBlock().onNeighborBlockChange(w,p,state(raw),Blocks.stone);
        Map<String,Object> row=new LinkedHashMap<>();row.put("state",raw);row.put("below",below);row.put("above",above);row.put("mask",mask);row.put("light",light);row.put("sky",sky);row.put("y",y);row.put("writes",w.writes);row.put("world_next",Long.toString(w.rand.nextLong()));return row;
    }
    public static void main(String[] args)throws Exception {
        Bootstrap.register();Fixture w=fixture();List<Object> rows=new ArrayList<>();TreeSet<Integer> states=new TreeSet<>();
        for(int id:new int[]{6,31,32,37,38,39,40,59,60,78,81,83,104,105,106,111,115,127,141,142,175})for(IBlockState state:Block.getBlockById(id).getBlockState().getValidStates())states.add(raw(state));
        int[] below={0,16,32,48,50,192,193,2816,2544,1408,960,288,2576,320,128,144,145,1264,2784,1248,1253,1254,1255,704,688,1328,2800};
        for(int state:states){
            for(int floor:below)for(int light:new int[]{7,8,12,13})for(boolean sky:new boolean[]{false,true})rows.add(run(w,state,floor,0,0,light,sky,64));
            for(int mask=0;mask<16;mask++)for(int top:new int[]{0,16,2800,2808,1696,1711})rows.add(run(w,state,48,top,mask,15,true,64));
            for(int y:new int[]{-1,0,255,256})rows.add(run(w,state,144,2808,15,15,true,y));
        }
        try(Writer out=new OutputStreamWriter(new FileOutputStream(args[0]),"UTF-8")){new Gson().toJson(rows,out);}
        System.out.println("Original plant callbacks: "+states.size()+" canonical states, "+rows.size()+" cases");
    }
}
