import com.google.gson.Gson;
import java.lang.reflect.Field;
import java.util.*;
import net.minecraft.block.*;
import net.minecraft.block.material.Material;
import net.minecraft.block.state.*;
import net.minecraft.init.*;
import net.minecraft.item.*;
import net.minecraft.tileentity.*;
import net.minecraft.util.*;
import net.minecraft.world.border.WorldBorder;
import sun.misc.Unsafe;

/** Capture mobility and the vanilla push planner without running a game loop. */
public class RedstoneCircuitOracle {
    static class Grid extends HandPlacementOracle.Grid {
        WorldBorder border;
        TileEntityComparator comparator;
        List<int[]> timers;
        public WorldBorder getWorldBorder() { return border; }
        public int getHeight() { return 256; }
        public TileEntity getTileEntity(BlockPos p){return p.equals(origin)?comparator:null;}
        public <T extends net.minecraft.entity.Entity> List<T> getEntitiesWithinAABB(Class<? extends T> type,AxisAlignedBB bounds,com.google.common.base.Predicate<? super T> filter){return Collections.emptyList();}
        public void notifyBlockOfStateChange(BlockPos p,Block b){}
        public void notifyNeighborsOfStateChange(BlockPos p,Block b){}
        public void notifyNeighborsOfStateExcept(BlockPos p,Block b,EnumFacing f){}
        public boolean isBlockTickPending(BlockPos p,Block b){return false;}
        public void updateBlockTick(BlockPos p,Block b,int delay,int priority){timers.add(new int[]{Block.getIdFromBlock(b),delay,priority});}
        public void scheduleUpdate(BlockPos p,Block b,int delay){updateBlockTick(p,b,delay,0);}
    }
    public static void main(String[] args) throws Exception {
        Bootstrap.register();
        Field f=Unsafe.class.getDeclaredField("theUnsafe"); f.setAccessible(true);
        Grid w=(Grid)((Unsafe)f.get(null)).allocateInstance(Grid.class);
        w.border=new WorldBorder(); w.cells=new HashMap<>(); w.writes=new ArrayList<>(); w.entities=new ArrayList<>();
        BlockPos base=new BlockPos(8,64,8);
        List<Integer> mobility=new ArrayList<>();
        for(int id=0;id<256;id++) {
            Block b=Block.getBlockById(id);
            mobility.add(b==null ? 2 : (b.getMobilityFlag()==1 ? 1 :
                (b==Blocks.obsidian || b.getBlockHardness(w,base)==-1 || b.getMobilityFlag()==2 || b instanceof ITileEntityProvider ? 2 : 0)));
        }
        List<Object> plans=new ArrayList<>();
        Random r=new Random(189);
        int[] ids={0,0,0,0,0,1,1,20,29,33,55,69,95,165,165,165,165,49,54,176};
        for(int sample=0;sample<320;sample++) {
            int fdir=sample%6; EnumFacing face=EnumFacing.getFront(fdir);
            boolean extend=(sample/6)%2==0; w.cells.clear(); w.set(base,(29<<4)|fdir);
            List<int[]> cells=new ArrayList<>();
            for(int j=0;j<22;j++) {
                BlockPos p=base.offset(face,1+r.nextInt(5)).add(r.nextInt(3)-1,r.nextInt(3)-1,r.nextInt(3)-1);
                if(p.equals(base))continue;
                int id=ids[r.nextInt(ids.length)];
                int meta=id==29||id==33 ? r.nextInt(6)+(r.nextBoolean()?8:0) : id==54 ? 2 : 0;
                w.set(p,id*16+meta); cells.add(new int[]{p.getX(),p.getY(),p.getZ(),CollisionShapeOracle.raw(w.getBlockState(p))});
            }
            BlockPistonStructureHelper helper=new BlockPistonStructureHelper(w,base,face,extend);
            boolean can=helper.canMove();
            List<int[]> move=coords(helper.getBlocksToMove()),destroy=coords(helper.getBlocksToDestroy());
            plans.add(Arrays.asList(fdir,extend,cells,can,move,destroy));
        }
        w.origin=base; w.timers=new ArrayList<>();
        MovementOracle.Player player=MovementAttributesOracle.player();
        player.capabilities.allowEdit=true;
        List<Object> devices=new ArrayList<>(), placement=new ArrayList<>();
        for(int i=0;i<256;i++){
            int id=new int[]{93,94,149,150}[i%4];
            int meta=r.nextInt(16), fdir=new int[]{3,4,2,5}[meta&3];
            EnumFacing face=EnumFacing.getFront(fdir);
            int[] inputs={0,152<<4,(55<<4)+r.nextInt(16),(94<<4)+r.nextInt(4)};
            int front=inputs[r.nextInt(4)], side=inputs[r.nextInt(4)];
            w.cells.clear(); w.timers.clear(); w.set(base,(id<<4)|meta);w.set(base.down(),16);
            w.set(base.offset(face),front);w.set(base.offset(face.rotateY()),side);
            w.comparator=new TileEntityComparator();int prior=r.nextInt(16);w.comparator.setOutputSignal(prior);
            IBlockState state=w.getBlockState(base); Block b=state.getBlock();
            b.onNeighborBlockChange(w,base,state,Blocks.lever);
            List<int[]> scheduled=new ArrayList<>(w.timers);
            b.updateTick(w,base,state,r);
            int after=CollisionShapeOracle.raw(w.getBlockState(base)), signal=w.comparator.getOutputSignal();
            w.getBlockState(base).getBlock().onBlockActivated(w,base,w.getBlockState(base),player,EnumFacing.UP,.5f,.5f,.5f);
            devices.add(Arrays.asList((id<<4)|meta,front,side,prior,scheduled,after,signal,
                CollisionShapeOracle.raw(w.getBlockState(base)),w.comparator.getOutputSignal()));
        }
        for(int id:new int[]{331,356,404,69,29,33}) for(int yaw=0;yaw<4;yaw++) for(int face=0;face<6;face++){
            w.cells.clear();w.writes.clear();w.timers.clear();w.set(base,16);
            BlockPos at=base.offset(EnumFacing.getFront(face)); if(face!=0)w.set(at.down(),16);
            player.rotationYaw=yaw*90;player.setPosition(11.5,64.,11.5);
            ItemStack stack=new ItemStack(Item.getItemById(id),2);
            boolean ok=stack.getItem().onItemUse(stack,player,w,base,EnumFacing.getFront(face),.5f,.5f,.5f);
            placement.add(Arrays.asList(id,yaw,face,ok,stack.stackSize,CollisionShapeOracle.raw(w.getBlockState(at))));
        }
        System.out.println(new Gson().toJson(Arrays.asList(mobility,plans,devices,placement)));
    }
    static List<int[]> coords(List<BlockPos> v) {
        List<int[]> out=new ArrayList<>();
        for(BlockPos p:v)out.add(new int[]{p.getX(),p.getY(),p.getZ()});return out;
    }
}
