import com.google.gson.Gson;
import java.io.PrintStream;
import java.util.*;
import net.minecraft.block.Block;
import net.minecraft.block.state.IBlockState;
import net.minecraft.init.Bootstrap;
import net.minecraft.util.*;

/** Exercise the exact bounds calls on the ordinary crosshair and RenderGlobal paths. */
public final class SelectionBoundsOracle {
    static double[] coords(AxisAlignedBB b) {
        return new double[] { b.minX, b.minY, b.minZ, b.maxX, b.maxY, b.maxZ };
    }
    static int[] pos(BlockPos p) { return new int[]{p.getX(), p.getY(), p.getZ()}; }
    static Object hit(MovingObjectPosition h) {
        return h == null ? null : Arrays.asList(h.typeOfHit.ordinal(), pos(h.getBlockPos()), h.sideHit.getIndex(),
            new double[]{h.hitVec.xCoord,h.hitVec.yCoord,h.hitVec.zCoord});
    }
    final CollisionShapeOracle original = new CollisionShapeOracle();
    final CollisionShapeOracle.Fixture world = original.world;
    final List<Object> outlines = new ArrayList<>(), small = new ArrayList<>();
    SelectionBoundsOracle() throws Exception {}
    void state(int raw, BlockPos p, int[] sides) throws Exception {
        world.origin = p;
        world.neighbors = sides;
        world.center = CollisionShapeOracle.state(raw);
        original.restore(world.center.getBlock());
    }
    void outline(int raw, BlockPos p, int[] sides) throws Exception {
        state(raw, p, sides);
        Block b = world.center.getBlock();
        if (!b.canCollideCheck(world.center, false)) return;
        // Torch and stair picking first mutates the Block singleton's bounds.
        Vec3 start = new Vec3(p.getX()+.5, p.getY()+.25, p.getZ()+2);
        b.collisionRayTrace(world,p,start,start.addVector(0,0,-3));
        b.setBlockBoundsBasedOnState(world,p);
        outlines.add(Arrays.asList(raw,pos(p),sides,coords(b.getSelectedBoundingBox(world,p))));
    }
    void narrow(int raw, BlockPos p) throws Exception {
        int[] sides = {16,0,0,0,0,0}; // Walking / picking from the south towards a stone backing.
        state(raw, p, sides);
        Block b = world.center.getBlock();
        b.setBlockBoundsBasedOnState(world,p);
        List<AxisAlignedBB> list = new ArrayList<>();
        b.addCollisionBoxesToList(world,p,world.center,
            new AxisAlignedBB(p.add(-2,-2,-2),p.add(3,3,3)),list,null);
        List<Object> collision = new ArrayList<>(), probes = new ArrayList<>();
        for (AxisAlignedBB box : list) collision.add(coords(box));
        for (double x : new double[]{.01,.15,.3999,.4,.5,.6,.6001,.85,.99})
            for (double y : new double[]{.01,.25,.5,.5999,.6,.6001,.79,.9}) {
                Vec3 from = new Vec3(p.getX()+x,p.getY()+y,p.getZ()+2.5);
                Vec3 to = from.addVector(0,0,-4);
                probes.add(Arrays.asList(x,y,hit(world.rayTraceBlocks(from,to,false,false,true))));
            }
        small.add(Arrays.asList(raw,pos(p),sides,collision,probes));
    }
    public static void main(String[] args) throws Exception {
        PrintStream out = System.out;
        System.setOut(System.err);
        Bootstrap.register();
        SelectionBoundsOracle oracle = new SelectionBoundsOracle();
        BlockPos p = new BlockPos(-5,64,9), far = new BlockPos(16777213,78,-16777211);
        Set<Integer> narrow = new HashSet<>(Arrays.asList(6,26,27,28,31,32,37,38,39,40,50,55,59,65,66,69,70,72,75,76,77,83,92,93,94,96,104,105,106,111,115,127,131,132,140,141,142,143,144,147,148,149,150,157,171));
        for (int raw : oracle.original.canonical) {
            oracle.outline(raw,p,new int[6]);
            oracle.outline(raw,far,new int[6]);
            if (narrow.contains(raw>>4)) oracle.narrow(raw,p);
        }
        for (int id : new int[]{54,64,71,85,101,102,106,113,139,146,160,188,193})
            for (int mask = 0; mask < 64; mask++) {
                int[] sides = new int[6];
                for (int i=0;i<6;i++) if ((mask & (1<<i)) != 0)
                    sides[i] = id==54 || id==146 || id==64 || id==71 || id==193 ? (id<<4 | (i==5?9:2)) : 16;
                oracle.outline(id<<4 | (id==54 || id==146 ? 2:0),p,sides);
            }
        out.println(new Gson().toJson(Arrays.asList(oracle.outlines,oracle.small)));
    }
}
