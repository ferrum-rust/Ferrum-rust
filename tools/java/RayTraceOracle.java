import com.google.gson.Gson;
import io.netty.buffer.Unpooled;
import java.io.PrintStream;
import java.lang.reflect.Field;
import java.util.*;
import net.minecraft.block.*;
import net.minecraft.block.state.IBlockState;
import net.minecraft.init.*;
import net.minecraft.network.PacketBuffer;
import net.minecraft.network.play.client.C08PacketPlayerBlockPlacement;
import net.minecraft.util.*;
import sun.misc.Unsafe;

/**
 * Original Block.collisionRayTrace and World.rayTraceBlocks. Inputs use the
 * actual packed section state map and include all eight flag combinations.
 */
public final class RayTraceOracle {
  static class Grid extends CollisionShapeOracle.Fixture {
    Map<BlockPos, IBlockState> cells;
    public IBlockState getBlockState(BlockPos p) {
      return cells.getOrDefault(p, Blocks.air.getDefaultState());
    }
  }
  final CollisionShapeOracle source;
  final Grid world;
  final List<double[]> boxes = new ArrayList<double[]>(), rays = new ArrayList<double[]>();
  final Map<String, Integer> boxIds = new HashMap<String, Integer>(),
                             hitIds = new HashMap<String, Integer>();
  final List<Object> hits = new ArrayList<Object>();
  final List<String> c08 = new ArrayList<String>();
  final List<Object> cases = new ArrayList<Object>(), scenes = new ArrayList<Object>();
  final List<int[]> registry = new ArrayList<int[]>(), contexts = new ArrayList<int[]>();
  final BlockPos p = new BlockPos(0, 64, 0);
  RayTraceOracle() throws Exception {
    source = new CollisionShapeOracle();
    Field f = Unsafe.class.getDeclaredField("theUnsafe");
    f.setAccessible(true);
    world = (Grid) ((Unsafe) f.get(null)).allocateInstance(Grid.class);
    world.cells = new HashMap<BlockPos, IBlockState>();
    world.origin = p;
    for (int axis = 0; axis < 3; axis++)
      for (int sign : new int[] {-1, 1})
        for (double across : new double[] {0.0, .0625, .125, .25, .5, .75, .875, .9375, 1.}) {
          double[] r = {across, across, across, 1 - across, 1 - across, 1 - across};
          r[axis] = sign < 0 ? -1.25 : 2.25;
          r[axis + 3] = sign < 0 ? 2.25 : -1.25;
          rays.add(r);
        }
    for (double[] r : new double[][] {{-.5, -.5, -.5, 1.5, 1.5, 1.5}, {2, 2, 2, 0, 0, 0},
             {.5, .5, .5, .5, .5, .5}, {.5, .5, .5, .6, .6, .6}, {.5, .5, .5, 1.5, .5, .5},
             {-.0001, .5, .5, .0001, .5, .5}, {-1, .25, .25, 0, .25, .25},
             {.5, 2, .50001, .500001, -1, .49999}})
      rays.add(r);
  }
  void set(BlockPos q, int raw) {
    world.cells.put(q, CollisionShapeOracle.state(raw));
  }
  int box(Block b) {
    double[] a = {b.getBlockBoundsMinX(), b.getBlockBoundsMinY(), b.getBlockBoundsMinZ(),
        b.getBlockBoundsMaxX(), b.getBlockBoundsMaxY(), b.getBlockBoundsMaxZ()};
    String key = Arrays.toString(a);
    Integer id = boxIds.get(key);
    if (id == null) {
      id = boxes.size();
      boxIds.put(key, id);
      boxes.add(a);
    }
    return id;
  }
  int hit(MovingObjectPosition result) {
    if (result == null)
      return -1;
    BlockPos q = result.getBlockPos();
    Vec3 v = result.hitVec;
    List<Object> record = Arrays.<Object>asList(result.typeOfHit.ordinal(), q.getX(), q.getY(),
        q.getZ(), result.sideHit.getIndex(), bits(v.xCoord), bits(v.yCoord), bits(v.zCoord));
    String key = record.toString();
    Integer id = hitIds.get(key);
    if (id == null) {
      id = hits.size();
      hitIds.put(key, id);
      hits.add(record);
      c08.add(
          result.typeOfHit == MovingObjectPosition.MovingObjectType.BLOCK ? packet(result) : null);
    }
    return id;
  }
  static String packet(MovingObjectPosition hit) {
    try {
      BlockPos q = hit.getBlockPos();
      Vec3 v = hit.hitVec;
      PacketBuffer b = new PacketBuffer(Unpooled.buffer());
      new C08PacketPlayerBlockPlacement(q, hit.sideHit.getIndex(), null,
          (float) (v.xCoord - q.getX()), (float) (v.yCoord - q.getY()),
          (float) (v.zCoord - q.getZ()))
          .writePacketData(b);
      StringBuilder hex = new StringBuilder();
      while (b.readableBytes() > 0) hex.append(String.format("%02x", b.readUnsignedByte()));
      return hex.toString();
    } catch (Exception e) {
      throw new IllegalStateException(e);
    }
  }
  static String bits(double x) {
    return Long.toHexString(Double.doubleToRawLongBits(x));
  }
  int[] worldHits(Vec3 a, Vec3 b) {
    int[] out = new int[8];
    for (int f = 0; f < 8; f++)
      out[f] = hit(world.rayTraceBlocks(a, b, (f & 1) != 0, (f & 2) != 0, (f & 4) != 0));
    return out;
  }
  void shape(int raw, int[] neighbors, boolean baseline) throws Exception {
    world.cells.clear();
    set(p, raw);
    for (int i = 0; i < 6; i++) set(p.offset(CollisionShapeOracle.SIDES[i]), neighbors[i]);
    IBlockState state = world.getBlockState(p);
    Block b = state.getBlock();
    source.restore(b);
    int flags = (b.canCollideCheck(state, false) ? 1 : 0) | (b.canCollideCheck(state, true) ? 2 : 0)
        | (b.getCollisionBoundingBox(world, p, state) != null ? 4 : 0);
    b.collisionRayTrace(world, p, new Vec3(-1, 64.5, .5), new Vec3(2, 64.5, .5));
    int selected = box(b);
    if (baseline) {
      registry.add(new int[] {raw, selected, flags});
    } else {
      int[] row = new int[8];
      row[0] = raw;
      System.arraycopy(neighbors, 0, row, 1, 6);
      row[7] = selected;
      contexts.add(row);
    }
  }
  void canonicalCases() throws Exception {
    for (int raw : source.canonical) {
      shape(raw, new int[6], true);
      world.cells.clear();
      set(p, raw);
      List<int[]> results = new ArrayList<int[]>();
      Block b = world.getBlockState(p).getBlock();
      for (double[] r : rays) {
        Vec3 a = new Vec3(r[0], 64 + r[1], r[2]), end = new Vec3(r[3], 64 + r[4], r[5]);
        int[] row = new int[9];
        row[0] = hit(b.collisionRayTrace(world, p, a, end));
        System.arraycopy(worldHits(a, end), 0, row, 1, 8);
        results.add(row);
      }
      cases.add(Arrays.asList(raw, results));
    }
    source.connectedCases();
    for (int[] c : source.contexts) shape(c[0], Arrays.copyOfRange(c, 1, 7), false);
    for (int meta = 0; meta < 16; meta++)
      for (int up : source.canonical) shape(106 << 4 | meta, new int[] {0, 0, 0, 0, 0, up}, false);
  }
  void scene(int ox, int oz, int seed, int count) {
    Random r = new Random(seed);
    world.cells.clear();
    int[] kinds = {0, 0, 0, 0, 16, 20 << 4, 8 << 4, 8 << 4 | 3, 11 << 4, 30 << 4, 31 << 4 | 1,
        37 << 4, 44 << 4, 44 << 4 | 8, 50 << 4 | 1, 53 << 4 | 2, 54 << 4 | 3, 64 << 4 | 1,
        65 << 4 | 3, 66 << 4 | 3, 78 << 4 | 4, 85 << 4, 92 << 4 | 3, 102 << 4, 106 << 4 | 4,
        139 << 4, 145 << 4 | 1, 171 << 4};
    List<int[]> cells = new ArrayList<int[]>();
    for (int x = -4; x <= 4; x++)
      for (int z = -4; z <= 4; z++)
        for (int y = 63; y <= 66; y++) {
          int state = kinds[r.nextInt(kinds.length)];
          if (state != 0) {
            set(new BlockPos(ox + x, y, oz + z), state);
            cells.add(new int[] {ox + x, y, oz + z, state});
          }
        }
    List<Object> traces = new ArrayList<Object>();
    for (int i = 0; i < count + 8; i++) {
      double[] v = {ox + r.nextDouble() * 14 - 7, 62 + r.nextDouble() * 6,
          oz + r.nextDouble() * 14 - 7, ox + r.nextDouble() * 14 - 7, 62 + r.nextDouble() * 6,
          oz + r.nextDouble() * 14 - 7};
      if (i < count && i % 4 == 0) {
        v[0] = Math.floor(v[0]);
        v[2] = Math.floor(v[2]);
        v[3] = v[0] + 5;
        v[5] = v[2] + 5;
      }
      if (i >= count) {
        v = new double[] {ox + .5, 70.5, oz + .5, ox + 199 + (i - count) + .5, 70.5, oz + .5};
      }
      List<String> input = new ArrayList<String>();
      for (double d : v) input.add(bits(d));
      traces.add(
          Arrays.asList(input, worldHits(new Vec3(v[0], v[1], v[2]), new Vec3(v[3], v[4], v[5]))));
    }
    Map<String, Object> result = new LinkedHashMap<String, Object>();
    result.put("cells", cells);
    result.put("rays", traces);
    scenes.add(result);
  }
  public static void main(String[] args) throws Exception {
    PrintStream out = System.out;
    System.setOut(System.err);
    Bootstrap.register();
    RayTraceOracle o = new RayTraceOracle();
    o.canonicalCases();
    o.scene(0, 0, 189, 512);
    o.scene(-37, -91, -1, 512);
    o.scene(1000000, -1000000, 47, 512);
    o.scene(16777215, -16777215, 11, 512);
    Map<String, Object> result = new LinkedHashMap<String, Object>();
    result.put("boxes", o.boxes);
    result.put("registry", o.registry);
    result.put("contexts", o.contexts);
    result.put("inputs", o.rays);
    result.put("hits", o.hits);
    result.put("canonical", o.cases);
    result.put("scenes", o.scenes);
    result.put("c08", o.c08);
    out.println(new Gson().toJson(result));
  }
}
