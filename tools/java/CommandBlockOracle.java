import com.google.gson.Gson;
import java.lang.reflect.Field;
import java.util.*;
import sun.misc.Unsafe;
import net.minecraft.block.Block;
import net.minecraft.block.state.IBlockState;
import net.minecraft.init.*;
import net.minecraft.item.ItemStack;
import net.minecraft.nbt.NBTTagCompound;
import net.minecraft.tileentity.*;
import net.minecraft.util.BlockPos;
import net.minecraft.world.*;

/** Actual BlockCommandBlock neighbor/scheduled callbacks and Control NBT. */
public final class CommandBlockOracle {
    static final BlockPos POS=new BlockPos(8,64,8);
    static class World extends FoodOracle.OracleWorld {
        IBlockState state;TileEntityCommandBlock tile;boolean power;int tick,executions;
        List<Integer> pending;
        @Override public boolean isBlockPowered(BlockPos p){return power;}
        @Override public IBlockState getBlockState(BlockPos p){return p.equals(POS)?state:Blocks.air.getDefaultState();}
        @Override public TileEntity getTileEntity(BlockPos p){return p.equals(POS)?tile:null;}
        @Override public boolean setBlockState(BlockPos p,IBlockState s,int flags){state=s;return true;}
        @Override public void scheduleUpdate(BlockPos p,Block b,int delay){if(pending.isEmpty())pending.add(tick+delay);}
        @Override public void updateComparatorOutputLevel(BlockPos p,Block b){executions++;}
        @Override public void markChunkDirty(BlockPos p,TileEntity tile){}
    }
    static World world()throws Exception{
        Field f=Unsafe.class.getDeclaredField("theUnsafe");f.setAccessible(true);
        World w=(World)((Unsafe)f.get(null)).allocateInstance(World.class);
        w.rules=new GameRules();w.difficulty=EnumDifficulty.NORMAL;w.pending=new ArrayList<>();
        w.state=Blocks.command_block.getDefaultState();w.tile=new TileEntityCommandBlock();w.tile.setWorldObj(w);w.tile.setPos(POS);
        return w;
    }
    public static void main(String[] args)throws Exception {
        Bootstrap.register();Map<String,Object> output=new LinkedHashMap<>();List<Object> cases=new ArrayList<>();
        for(int mask=0;mask<32;mask++){
            World w=world();List<Object> steps=new ArrayList<>();
            for(int tick=0;tick<7;tick++){
                w.tick=tick;w.power=tick<5&&(mask&(1<<tick))!=0;
                if(!w.pending.isEmpty()&&w.pending.get(0)<=tick){w.pending.clear();Blocks.command_block.updateTick(w,POS,w.state,new Random(189));}
                Blocks.command_block.onNeighborBlockChange(w,POS,w.state,Blocks.redstone_block);
                steps.add(Arrays.asList(w.power,Blocks.command_block.getMetaFromState(w.state),new ArrayList<>(w.pending),w.executions));
            }
            cases.add(steps);
        }
        output.put("edges",cases);
        World w=world();NBTTagCompound old=new NBTTagCompound();old.setString("Command","old");old.setInteger("SuccessCount",7);w.tile.getCommandBlockLogic().readDataFromNBT(old);
        w.tile.getCommandBlockLogic().setCommand("say hello");NBTTagCompound saved=new NBTTagCompound();w.tile.getCommandBlockLogic().writeDataToNBT(saved);
        output.put("save",Arrays.asList(saved.getString("Command"),saved.getInteger("SuccessCount"),saved.getString("CustomName"),saved.getBoolean("TrackOutput")));
        List<Boolean> permissions=new ArrayList<>();for(int i=0;i<5;i++)permissions.add(w.tile.getCommandBlockLogic().canCommandSenderUseCommand(i,""));output.put("permissions",permissions);
        w.rules.setOrCreateGameRule("sendCommandFeedback","false");ItemStack stack=new ItemStack(Blocks.command_block);stack.setStackDisplayName("Controller");
        Blocks.command_block.onBlockPlacedBy(w,POS,w.state,null,stack);NBTTagCompound placed=new NBTTagCompound();w.tile.getCommandBlockLogic().writeDataToNBT(placed);
        output.put("placement",Arrays.asList(placed.getString("CustomName"),placed.getBoolean("TrackOutput")));
        System.out.println(new Gson().toJson(output));
    }
}
