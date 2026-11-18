import java.io.*;
import java.util.*;
import com.google.gson.Gson;
import net.minecraft.block.*;
import net.minecraft.entity.Entity;
import net.minecraft.entity.item.EntityFallingBlock;
import net.minecraft.init.*;
import net.minecraft.util.*;

/** Original BlockFalling's deferred-entity and instant/unloaded branches. */
public final class FallingBlockOracle {
    static class Fixture extends FallingEntityOracle.TestWorld {
        final int mode,support,sourceY;
        Fixture(int mode,int support,int sourceY){super(0);this.mode=mode;this.support=support;this.sourceY=sourceY;}
        protected boolean isChunkLoaded(int x,int z,boolean allowEmpty){return (mode&1)==0||x!=-2||z!=-2;}
        public net.minecraft.block.state.IBlockState getBlockState(BlockPos p){
            if(p.getY()<0||p.getY()>=256)return Blocks.air.getDefaultState();
            net.minecraft.block.state.IBlockState edit=edits.get(p);if(edit!=null)return edit;
            int s=0;
            if(support==0&&p.getY()==0)s=1<<4;
            if(support==1&&p.getY()==10)s=1<<4;
            if(support==2&&p.getY()==sourceY-1)s=1<<4;
            if(support==3&&p.getY()==sourceY-1)s=9<<4;
            if(support==3&&p.getY()==sourceY-3)s=1<<4;
            if(support==4&&p.getY()==sourceY-1)s=11<<4;
            if(support==4&&p.getY()==sourceY-3)s=1<<4;
            if(support==5&&p.getY()==sourceY-2)s=31<<4|1;
            return FallingEntityOracle.state(s);
        }
        public boolean setBlockState(BlockPos p,net.minecraft.block.state.IBlockState s,int flags){
            if(p.getY()<0||p.getY()>=256)return false;
            net.minecraft.block.state.IBlockState old=getBlockState(p);
            if(s==old)return false;
            events.add(Arrays.asList("set",p.getX(),p.getY(),p.getZ(),FallingEntityOracle.state(s)));
            edits.put(new BlockPos(p.getX(),p.getY(),p.getZ()),s);
            if(old.getBlock()!=s.getBlock())s.getBlock().onBlockAdded(this,p,s);
            if((flags&1)!=0)notifyNeighborsOfStateChange(p,old.getBlock());
            return true;
        }
        // Exercise the same liquid/falling callbacks configured on the Rust probe.
        // Other block families have separate original-WorldServer callback suites.
        public void notifyBlockOfStateChange(BlockPos p,Block old){
            Block target=getBlockState(p).getBlock();
            if(target instanceof BlockLiquid||target instanceof BlockFalling)super.notifyBlockOfStateChange(p,old);
        }
        public boolean spawnEntityInWorld(Entity e){
            EntityFallingBlock f=(EntityFallingBlock)e;
            try{events.add(Arrays.asList("spawn",FallingEntityOracle.bits(f.posX,f.posY,f.posZ),FallingEntityOracle.state(f.getBlock()),FallingEntityOracle.field(EntityFallingBlock.class,"hurtEntities").getBoolean(f)));}
            catch(Exception ex){throw new RuntimeException(ex);}return true;
        }
    }
    public static void main(String[] args)throws Exception{
        Bootstrap.register();List<Object> rows=new ArrayList<>();
        for(int state:new int[]{12<<4,12<<4|1,13<<4,145<<4,145<<4|5,145<<4|10,145<<4|11})for(int y:new int[]{0,1,2,12,64,255})for(int mode=0;mode<4;mode++)for(int support=0;support<6;support++){
            Fixture w=new Fixture(mode,support,y);BlockPos p=new BlockPos(8,y,8);w.edits.put(p,FallingEntityOracle.state(state));BlockFalling.fallInstantly=(mode&2)!=0;
            Block block=Block.getBlockById(state>>4);block.updateTick(w,p,w.getBlockState(p),new Random(189));
            Map<String,Object> row=new LinkedHashMap<>();row.put("state",state);row.put("y",y);row.put("mode",mode);row.put("support",support);row.put("events",w.events);rows.add(row);
        }
        BlockFalling.fallInstantly=false;
        try(Writer out=new OutputStreamWriter(new FileOutputStream(args[0]),"UTF-8")){new Gson().toJson(rows,out);}
        System.out.println("Original falling-block triggers: "+rows.size()+" cases");
        Set<Integer> canonical=new TreeSet<>();
        for(Block block:Block.blockRegistry)for(net.minecraft.block.state.IBlockState s:block.getBlockState().getValidStates())canonical.add(FallingEntityOracle.state(s));
        List<Object> placements=new ArrayList<>();
        Fixture w=new Fixture(0,0,64);BlockPos at=new BlockPos(8,64,8);
        for(int target:canonical){w.edits.put(at,FallingEntityOracle.state(target));placements.add(Arrays.asList(target,
            w.canBlockBePlaced(Blocks.sand,at,true,EnumFacing.UP,null,null),
            w.canBlockBePlaced(Blocks.gravel,at,true,EnumFacing.UP,null,null),
            w.canBlockBePlaced(Blocks.anvil,at,true,EnumFacing.UP,null,null)));}
        try(Writer out=new OutputStreamWriter(new FileOutputStream(args[1]),"UTF-8")){new Gson().toJson(placements,out);}
        System.out.println("Original canBlockBePlaced: "+placements.size()+" targets / 3 gravity blocks");
    }
}
