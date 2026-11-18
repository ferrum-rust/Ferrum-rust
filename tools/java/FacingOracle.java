import java.io.*;
import java.util.*;
import com.google.gson.Gson;
import net.minecraft.block.Block;
import net.minecraft.block.state.IBlockState;
import net.minecraft.init.Bootstrap;
import net.minecraft.profiler.Profiler;
import net.minecraft.util.BlockPos;
import net.minecraft.world.*;
import net.minecraft.world.chunk.IChunkProvider;
import net.minecraft.world.storage.WorldInfo;
public final class FacingOracle {
    static int state(IBlockState s){return Block.getIdFromBlock(s.getBlock())<<4|s.getBlock().getMetaFromState(s);}
    static IBlockState block(int state){return Block.getBlockById(state>>4).getStateFromMeta(state&15);}
    static class TestWorld extends World {
        final Map<BlockPos,IBlockState> blocks=new HashMap<BlockPos,IBlockState>();final List<int[]> reads=new ArrayList<int[]>(),writes=new ArrayList<int[]>();
        TestWorld(){super(null,new WorldInfo(new WorldSettings(0,WorldSettings.GameType.SURVIVAL,false,false,WorldType.DEFAULT),"facing"),new WorldProviderSurface(),new Profiler(),false);}
        protected IChunkProvider createChunkProvider(){return null;}
        protected int getRenderDistanceChunks(){return 8;}
        public IBlockState getBlockState(BlockPos p){reads.add(new int[]{p.getX(),p.getY(),p.getZ()});IBlockState s=blocks.get(p);return s==null?block(0):s;}
        public boolean setBlockState(BlockPos p,IBlockState s,int flags){blocks.put(new BlockPos(p),s);writes.add(new int[]{p.getX(),p.getY(),p.getZ(),state(s),flags});return true;}
    }
    public static void main(String[]args){PrintStream output=System.out;System.setOut(System.err);Bootstrap.register();List<Object> cases=new ArrayList<Object>();Random random=new Random(189);
        for(int id:new int[]{61,62,23,158,54,146})for(int meta=0;meta<16;meta++){
            int state=state(block(id<<4|meta));if(state!=(id<<4|meta))continue;
            for(int variant=0;variant<128;variant++){
                TestWorld w=new TestWorld();List<int[]> blocks=new ArrayList<int[]>();
                for(int x=-2;x<=2;x++)for(int z=-2;z<=2;z++){
                    int[] values={0,16,288,864+2,864+3,864+4,864+5,2336+2,2336+3};int value=values[random.nextInt(values.length)];
                    if(x==0&&z==0)value=state;w.blocks.put(new BlockPos(x,64,z),block(value));blocks.add(new int[]{x,64,z,value});
                }
                block(state).getBlock().onBlockAdded(w,new BlockPos(0,64,0),block(state));
                Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("state",state);row.put("blocks",blocks);row.put("reads",w.reads);row.put("writes",w.writes);cases.add(row);
            }
        }
        Map<String,Object> result=new LinkedHashMap<String,Object>();result.put("cases",cases);output.println(new Gson().toJson(result));
    }
}
