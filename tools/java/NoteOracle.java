import com.google.gson.Gson;
import com.mojang.authlib.GameProfile;
import java.io.PrintStream;
import java.util.*;
import net.minecraft.block.Block;
import net.minecraft.block.state.IBlockState;
import net.minecraft.client.Minecraft;
import net.minecraft.client.network.NetHandlerPlayClient;
import net.minecraft.init.Blocks;
import net.minecraft.init.Bootstrap;
import net.minecraft.nbt.NBTTagCompound;
import net.minecraft.tileentity.*;
import net.minecraft.util.*;
import net.minecraft.world.World;
import net.minecraft.entity.player.EntityPlayer;

/** Bytecode traces from note-block tuning and original World.isBlockPowered. */
public final class NoteOracle {
    static class Grid extends SpecialClickOracle.W {
        Map<BlockPos,Integer> grid=new HashMap<BlockPos,Integer>();
        TileEntityComparator comparator=new TileEntityComparator();
        boolean dirty;
        Grid(boolean remote)throws Exception {
            super(remote); at=new BlockPos(3,67,-12);
            tile=new TileEntityNote(); tile.setWorldObj(this);tile.setPos(at);comparator.setOutputSignal(13);
        }
        int state(BlockPos p){Integer s=grid.get(p);return s==null?0:s;}
        public TileEntity getTileEntity(BlockPos p){
            int id=state(p)>>4;return id==25?tile:(id==149||id==150?comparator:null);
        }
        public void markChunkDirty(BlockPos p,TileEntity te){dirty=true;}
        public void updateComparatorOutputLevel(BlockPos p,Block b){}
    }
    public static void main(String[] args)throws Exception{
        PrintStream out=System.out;System.setOut(System.err);Bootstrap.register();
        Minecraft mc=MovementOracle.minecraft();
        List<Object> turns=new ArrayList<Object>(), masks=new ArrayList<Object>(), power=new ArrayList<Object>();
        for(boolean remote:new boolean[]{false,true}){
            Grid w=new Grid(remote);
            NetHandlerPlayClient net=new NetHandlerPlayClient(mc,null,null,new GameProfile(new UUID(0,189),"Note"));
            MovementOracle.Player player=new MovementOracle.Player(mc,w,net);
            for(int n=0;n<25;n++) for(int under:new int[]{1,5,12,20,3}) for(boolean blocked:new boolean[]{false,true}){
                w.clear();w.grid.clear();w.grid.put(w.at,25<<4);w.grid.put(w.at.down(),under<<4);
                if(blocked)w.grid.put(w.at.up(),20<<4);
                w.tile.note=(byte)n;w.dirty=false;
                boolean accepted=Blocks.noteblock.onBlockActivated(w,w.at,w.getBlockState(w.at),player,EnumFacing.UP,0.5f,1f,0.5f);
                NBTTagCompound saved=new NBTTagCompound();w.tile.writeToNBT(saved);
                TileEntityNote copy=new TileEntityNote();copy.readFromNBT(saved);
                turns.add(Arrays.asList(remote,n,under,blocked,accepted,(int)copy.note,w.dirty,w.events.toArray(),w.particles.toArray()));
            }
        }
        Grid w=new Grid(false);
        int[] weak=new int[4096], strong=new int[4096];
        for(int state=0;state<4096;state++){
            IBlockState s=Block.BLOCK_STATE_IDS.getByValue(state);if(s==null)continue;
            w.grid.clear();w.grid.put(w.at,state);Block b=s.getBlock();
            for(EnumFacing face:EnumFacing.values()){
                if(b.getWeakPower(w,w.at,s,face)>0)weak[state]|=1<<face.getIndex();
                if(b.getStrongPower(w,w.at,s,face)>0)strong[state]|=1<<face.getIndex();
            }
            if(weak[state]!=0||strong[state]!=0)masks.add(Arrays.asList(state,weak[state],strong[state]));
        }
        for(int raw=0;raw<4096;raw++)if((weak[raw]|strong[raw])!=0)for(int face=0;face<6;face++)for(int scenario=0;scenario<9;scenario++){
            EnumFacing side=EnumFacing.getFront(face);
            w.grid.clear();w.grid.put(w.at,25<<4);
            BlockPos near=w.at.offset(side);
            if(scenario==0)w.grid.put(near,raw);
            else if(scenario<=6){w.grid.put(near,16);w.grid.put(near.offset(EnumFacing.getFront(scenario-1)),raw);}
            else {
                w.grid.put(near,raw);
                for(EnumFacing extra:EnumFacing.Plane.HORIZONTAL)if((extra.getIndex()+face)%2==(scenario-7)){
                    BlockPos wire=near.offset(extra);if(!wire.equals(w.at))w.grid.put(wire,55<<4|9);
                }
            }
            w.grid.put(w.at,25<<4);w.tile.note=7;w.tile.previousRedstoneState=false;
            w.clear();boolean powered=w.isBlockPowered(w.at);
            for(int i=0;i<2;i++)Blocks.noteblock.onNeighborBlockChange(w,w.at,w.getBlockState(w.at),Blocks.redstone_block);
            int plays=w.events.size()/2;
            boolean latched=w.tile.previousRedstoneState;
            w.grid.clear();w.grid.put(w.at,25<<4);Blocks.noteblock.onNeighborBlockChange(w,w.at,w.getBlockState(w.at),Blocks.air);
            power.add(Arrays.asList(raw,face,scenario,powered,plays,latched,w.tile.previousRedstoneState));
        }
        Map<String,Object> data=new LinkedHashMap<String,Object>();data.put("turns",turns);data.put("masks",masks);data.put("power",power);
        out.println(new Gson().toJson(data));
    }
}
