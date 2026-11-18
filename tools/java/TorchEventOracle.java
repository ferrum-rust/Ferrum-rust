// Original BlockTorch onAdded/onNeighbor callbacks, all canonical support states.
import java.io.*;
import java.util.*;
import com.google.gson.Gson;
import net.minecraft.block.Block;
import net.minecraft.block.state.IBlockState;
import net.minecraft.init.*;
import net.minecraft.util.BlockPos;

public class TorchEventOracle {
    static Object run(PlantEventOracle.Fixture w,int meta,int support,int mask,int y,boolean added,boolean removed) {
        int state=(50<<4)|meta;BlockPos p=new BlockPos(0,y,0);w.blocks=new HashMap<>();w.writes=new ArrayList<>();w.light=15;w.sky=true;
        w.blocks.put(p,PlantEventOracle.state(removed?0:state));w.blocks.put(p.down(),PlantEventOracle.state(support));
        BlockPos[] sides={p.north(),p.east(),p.south(),p.west()};for(int i=0;i<4;i++)if((mask&(1<<i))!=0)w.blocks.put(sides[i],PlantEventOracle.state(support));
        w.rand.setSeed(189);IBlockState s=PlantEventOracle.state(state);
        if(added)s.getBlock().onBlockAdded(w,p,s);else s.getBlock().onNeighborBlockChange(w,p,s,Blocks.stone);
        Map<String,Object> row=new LinkedHashMap<>();row.put("state",state);row.put("support",support);row.put("mask",mask);row.put("y",y);row.put("added",added);row.put("removed",removed);row.put("writes",w.writes);row.put("world_next",Long.toString(w.rand.nextLong()));return row;
    }
    public static void main(String[] args)throws Exception {
        Bootstrap.register();PlantEventOracle.Fixture w=PlantEventOracle.fixture();TreeSet<Integer> supports=new TreeSet<>();
        for(Block b:Block.blockRegistry)for(IBlockState s:b.getBlockState().getValidStates())supports.add(PlantEventOracle.raw(s));
        List<Object> rows=new ArrayList<>();
        for(int support:supports)for(int meta=1;meta<=5;meta++)for(boolean added:new boolean[]{false,true})rows.add(run(w,meta,support,15,64,added,false));
        for(int meta=1;meta<=5;meta++)for(boolean added:new boolean[]{false,true}) {
            for(int y:new int[]{-1,0,255,256})for(boolean removed:new boolean[]{false,true})rows.add(run(w,meta,0,0,y,added,removed));
            for(int mask=0;mask<16;mask++)rows.add(run(w,meta,16,mask,64,added,false));
        }
        try(Writer out=new OutputStreamWriter(new FileOutputStream(args[0]),"UTF-8")){new Gson().toJson(rows,out);}
        System.out.println("Original torch callbacks: "+supports.size()+" support states, "+rows.size()+" cases");
    }
}
