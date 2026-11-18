import com.google.gson.Gson;
import java.io.*;
import java.lang.reflect.Field;
import java.util.*;
import net.minecraft.block.*;
import net.minecraft.block.state.IBlockState;
import net.minecraft.entity.player.EntityPlayer;
import net.minecraft.init.*;
import net.minecraft.util.BlockPos;
import net.minecraft.world.WorldServer;
import sun.misc.Unsafe;

/** Drive the original bed search while controlling only its block input. */
public final class BedSpawnOracle {
    static class Grid extends WorldServer {
        Map<BlockPos,IBlockState> cells;
        Grid() { super(null,null,null,0,null); }
        @Override public IBlockState getBlockState(BlockPos p) {
            return cells.getOrDefault(p, Blocks.air.getDefaultState());
        }
        void put(BlockPos p, int packed) {
            cells.put(p, Block.getBlockById(packed >> 4).getStateFromMeta(packed & 15));
        }
    }
    static int[] xyz(BlockPos p) {
        return p == null ? null : new int[]{p.getX(),p.getY(),p.getZ()};
    }
    public static void main(String[] args) throws Exception {
        Bootstrap.register();
        Field f = Unsafe.class.getDeclaredField("theUnsafe");
        f.setAccessible(true);
        Grid w = (Grid)((Unsafe)f.get(null)).allocateInstance(Grid.class);
        w.cells = new HashMap<>();
        Set<Integer> states = new TreeSet<>();
        for (Block b : Block.blockRegistry)
            for (IBlockState s : b.getBlockState().getValidStates())
                states.add(Block.getIdFromBlock(b) << 4 | b.getMetaFromState(s));
        BlockPos p = new BlockPos(-16,72,15);
        List<int[]> forced = new ArrayList<>();
        for (int state : states) {
            w.cells.clear();
            w.put(p, state);
            int flags = EntityPlayer.getBedSpawnLocation(w,p,true) != null ? 1 : 0;
            w.cells.clear();
            w.put(p.up(), state);
            if (EntityPlayer.getBedSpawnLocation(w,p,true) != null) flags |= 2;
            forced.add(new int[]{state, flags});
        }
        List<Object> layouts = new ArrayList<>();
        int[] floor = {0,16,32,44<<4,44<<4|8,53<<4|4,53<<4,78<<4|6,78<<4|7,154<<4,20<<4,9<<4};
        int[] space = {0,0,0,0,16,20<<4,9<<4,11<<4,50<<4|5,70<<4,31<<4|1};
        Random random = new Random(189);
        for(int i=0;i<512;i++) {
            int meta = (i & 3) | ((i / 4 % 3 == 0) ? 0 : (i / 4 % 3 == 1 ? 8 : 12));
            int[] cells = new int[75];
            w.cells.clear();
            int j = 0;
            for(int y=-1;y<=1;y++) for(int x=-2;x<=2;x++) for(int z=-2;z<=2;z++) {
                int[] ids = y == -1 ? floor : space;
                int state = ids[random.nextInt(ids.length)];
                cells[j++] = state;
                w.put(p.add(x,y,z),state);
            }
            w.put(p,26<<4|meta);
            List<int[]> exits = new ArrayList<>();
            for(int skip=0;skip<5;skip++) exits.add(xyz(BlockBed.getSafeExitLocation(w,p,skip)));
            layouts.add(Arrays.asList(meta,cells,exits,xyz(EntityPlayer.getBedSpawnLocation(w,p,false))));
        }
        try (Writer out = new OutputStreamWriter(new FileOutputStream(args[0]),"UTF-8")) {
            new Gson().toJson(Arrays.asList(forced,layouts),out);
        }
        System.out.println("Original bed spawn: "+forced.size()+" states, "+layouts.size()+" layouts.");
    }
}
