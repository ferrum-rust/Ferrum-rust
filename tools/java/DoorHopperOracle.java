import com.google.common.base.Predicate;
import com.google.gson.Gson;
import java.io.PrintStream;
import java.lang.reflect.Field;
import java.util.*;
import net.minecraft.block.Block;
import net.minecraft.entity.Entity;
import net.minecraft.entity.player.EntityPlayer;
import net.minecraft.init.Bootstrap;
import net.minecraft.inventory.IInventory;
import net.minecraft.item.*;
import net.minecraft.nbt.NBTTagCompound;
import net.minecraft.tileentity.*;
import net.minecraft.util.*;
import net.minecraft.world.GameRules;
import sun.misc.Unsafe;

/** Invoke the original ItemDoor.onItemUse and TileEntityHopper.update on small reproducible scenes. */
public final class DoorHopperOracle {
    static class Grid extends HandPlacementOracle.Grid {
        Map<BlockPos, TileEntity> tiles;
        GameRules rules;
        boolean powered;
        public boolean isBlockPowered(BlockPos p) { return powered; }
        public GameRules getGameRules() { return rules; }
        public void markBlockRangeForRenderUpdate(BlockPos a, BlockPos b) {}
        public void playAuxSFXAtEntity(EntityPlayer player, int event, BlockPos at, int data) {}
        public TileEntity getTileEntity(BlockPos p) { return tiles.get(p); }
        public void notifyNeighborsOfStateChange(BlockPos p, Block block) {}
        public void markChunkDirty(BlockPos p, TileEntity tile) {}
        public void updateComparatorOutputLevel(BlockPos p, Block b) {}
        public List<Entity> getEntitiesInAABBexcluding(Entity e, AxisAlignedBB box, Predicate<? super Entity> filter) { return Collections.emptyList(); }
        public <T extends Entity> List<T> getEntitiesWithinAABB(Class<? extends T> type, AxisAlignedBB box, Predicate<? super T> filter) {
            List<T> matches = new ArrayList<>();
            for (Entity e : entities) if (!e.isDead && type.isInstance(e) && box.intersectsWith(e.getEntityBoundingBox())) matches.add(type.cast(e));
            return matches;
        }
        void tile(BlockPos p, int raw, TileEntity tile) {
            set(p, raw); tiles.put(p, tile); tile.setWorldObj(this); tile.setPos(p);
        }
    }
    static int count(IInventory c) {
        int total = 0;
        for (int i = 0; i < c.getSizeInventory(); i++) if (c.getStackInSlot(i) != null) total += c.getStackInSlot(i).stackSize;
        return total;
    }
    public static void main(String[] args) throws Exception {
        PrintStream out = System.out;
        System.setOut(System.err);
        Bootstrap.register();
        Field field = Unsafe.class.getDeclaredField("theUnsafe");
        field.setAccessible(true);
        Grid w = (Grid)((Unsafe)field.get(null)).allocateInstance(Grid.class);
        Field rng = net.minecraft.world.World.class.getDeclaredField("rand");
        rng.setAccessible(true); rng.set(w, new Random(189));
        w.cells = new HashMap<>(); w.writes = new ArrayList<>(); w.entities = new ArrayList<>(); w.tiles = new HashMap<>();
        BlockPos p = new BlockPos(8, 2, 8);
        MovementOracle.Player player = MovementAttributesOracle.player();
        player.capabilities.allowEdit = true;
        List<Object> doors = new ArrayList<>(), hopper = new ArrayList<>(), poweredDoors = new ArrayList<>();
        for (int id : new int[]{324,330,427,428,429,430,431}) for(int yaw : new int[]{0,45,135,-135,-45}) for(int pattern=0;pattern<8;pattern++) {
            int block = id == 324 ? 64 : id == 330 ? 71 : id-234;
            w.cells.clear(); w.set(p.down(), 16);
            BlockPos clicked = p.down(); EnumFacing side = EnumFacing.UP;
            if (pattern == 1) w.set(p.west(),16);
            if (pattern == 2) w.set(p.east().up(),16);
            if (pattern == 3) w.set(p.east(),block<<4);
            if (pattern == 4) w.set(p.up(),16);
            if (pattern == 5) side = EnumFacing.NORTH;
            if (pattern == 6) { w.set(p,31<<4|1); clicked = p; }
            if (pattern == 7) w.set(p.down(),0);
            ItemStack stack = new ItemStack(Item.getItemById(id),3);
            player.rotationYaw = yaw;
            boolean placed = stack.onItemUse(player,w,clicked,side,.5f,1f,.5f);
            doors.add(new int[]{id,yaw,pattern,placed?1:0,stack.stackSize,CollisionShapeOracle.raw(w.getBlockState(p)),CollisionShapeOracle.raw(w.getBlockState(p.up()))});
        }
        w.rules = new GameRules();
        w.rules.setOrCreateGameRule("doTileDrops","false");
        for (int id : new int[]{324,330}) {
            w.cells.clear();
            w.set(p.down(),16);
            w.powered = true;
            player.rotationYaw = 0;
            ItemStack door = new ItemStack(Item.getItemById(id));
            door.onItemUse(player,w,p.down(),EnumFacing.UP,.5f,1f,.5f);
            Block block = w.getBlockState(p).getBlock();
            for (int step=0; step<6; step++) {
                if (step==0) {
                    block.onNeighborBlockChange(w,p.up(),w.getBlockState(p.up()),block);
                    block.onNeighborBlockChange(w,p,w.getBlockState(p),block);
                } else if (step==1 || step==4) {
                    block.onBlockActivated(w,p.up(),w.getBlockState(p.up()),player,EnumFacing.NORTH,.5f,.25f,0f);
                } else if (step==2 || step==3) {
                    w.powered = step==2;
                    block.onNeighborBlockChange(w,p.up(),w.getBlockState(p.up()),Block.getBlockById(152));
                } else {
                    w.set(p.down(),0);
                    block.onNeighborBlockChange(w,p,w.getBlockState(p),Block.getBlockById(1));
                }
                poweredDoors.add(new int[]{id,step,CollisionShapeOracle.raw(w.getBlockState(p)),CollisionShapeOracle.raw(w.getBlockState(p.up()))});
            }
        }
        w.cells.clear();
        TileEntityHopper h = new TileEntityHopper();
        TileEntityChest top = new TileEntityChest(), bottom = new TileEntityChest();
        w.tile(p,154<<4,h); w.tile(p.up(),54<<4|2,top); w.tile(p.down(),54<<4|2,bottom);
        top.setInventorySlotContents(0,new ItemStack(Item.getItemById(264),7));
        for(int tick=0;tick<76;tick++) {
            if (tick==14 || tick==32) { w.set(p,154<<4|(tick==14?8:0)); h.updateContainingBlockInfo(); }
            h.update();
            NBTTagCompound nbt = new NBTTagCompound(); h.writeToNBT(nbt);
            hopper.add(new int[]{count(top),count(h),count(bottom),nbt.getInteger("TransferCooldown")});
        }
        out.println(new Gson().toJson(Arrays.asList(doors,hopper,poweredDoors)));
    }
}
