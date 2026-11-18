import com.google.gson.Gson;
import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import net.minecraft.block.Block;
import net.minecraft.block.BlockStairs;
import net.minecraft.block.state.IBlockState;
import net.minecraft.init.*;
import net.minecraft.nbt.CompressedStreamTools;
import net.minecraft.nbt.NBTTagCompound;
import net.minecraft.nbt.NBTTagList;
import net.minecraft.tileentity.*;
import net.minecraft.util.*;
import net.minecraft.world.WorldServer;
import net.minecraft.world.chunk.storage.RegionFile;
import sun.misc.Unsafe;

/**
 * Direct original block collision queries and position-prepared queries.
 * Restores each Block singleton's initial bounds before independent cases.
 */
public final class CollisionShapeOracle {
  static final EnumFacing[] SIDES = {EnumFacing.NORTH, EnumFacing.EAST, EnumFacing.SOUTH,
      EnumFacing.WEST, EnumFacing.DOWN, EnumFacing.UP};
  static class Fixture extends WorldServer {
    BlockPos origin;
    IBlockState center;
    int[] neighbors;
    TileEntity piston;
    Fixture() {
      super(null, null, null, 0, null);
    }
    public IBlockState getBlockState(BlockPos p) {
      if (p.equals(origin))
        return center;
      for (int i = 0; i < SIDES.length; i++)
        if (p.equals(origin.offset(SIDES[i])))
          return state(neighbors[i]);
      return Blocks.air.getDefaultState();
    }
    public TileEntity getTileEntity(BlockPos p) {
      return p.equals(origin) ? piston : null;
    }
    public boolean isBlockNormalCube(BlockPos p, boolean fallback) {
      Block b = getBlockState(p).getBlock();
      return b.getMaterial().isOpaque() && b.isFullCube();
    }
  }
  final Fixture world;
  final TreeSet<Integer> canonical = new TreeSet<Integer>();
  final Map<Block, double[]> pristine = new IdentityHashMap<Block, double[]>();
  final Field[] bounds = new Field[6];
  final List<List<double[]>> palette = new ArrayList<List<double[]>>();
  final Map<String, Integer> paletteIds = new HashMap<String, Integer>();
  final List<int[]> registry = new ArrayList<int[]>(), contexts = new ArrayList<int[]>(),
                    far = new ArrayList<int[]>(), pistons = new ArrayList<int[]>(),
                    carriedBoxes = new ArrayList<int[]>();
  final Set<String> seen = new HashSet<String>();
  final List<int[]> placement = new ArrayList<int[]>(), placementContexts = new ArrayList<int[]>(),
                    placementFar = new ArrayList<int[]>(),
                    placementHistory = new ArrayList<int[]>();
  final List<int[]> singles = new ArrayList<int[]>(), singleContexts = new ArrayList<int[]>(),
                    singleFar = new ArrayList<int[]>();
  // ExtendedBlockStorage reads its 16-bit packed state through this ID map.
  // This differs from getStateFromMeta for flower pots and upper double plants.
  static IBlockState state(int packed) {
    IBlockState state = Block.BLOCK_STATE_IDS.getByValue(packed);
    return state == null ? Blocks.air.getDefaultState() : state;
  }
  // TileEntityPiston.readFromNBT decodes blockId/blockData through this path.
  static IBlockState decodeMeta(int packed) {
    return Block.getBlockById(packed >> 4).getStateFromMeta(packed & 15);
  }
  static int raw(IBlockState state) {
    Block b = state.getBlock();
    return Block.getIdFromBlock(b) << 4 | b.getMetaFromState(state);
  }
  CollisionShapeOracle() throws Exception {
    Field unsafe = Unsafe.class.getDeclaredField("theUnsafe");
    unsafe.setAccessible(true);
    world = (Fixture) ((Unsafe) unsafe.get(null)).allocateInstance(Fixture.class);
    String[] names = {"minX", "minY", "minZ", "maxX", "maxY", "maxZ"};
    for (int i = 0; i < 6; i++) {
      bounds[i] = Block.class.getDeclaredField(names[i]);
      bounds[i].setAccessible(true);
    }
    Blocks.end_portal_frame.setBlockBoundsForItemRender();
    for (Block b : Block.blockRegistry) {
      double[] box = new double[6];
      for (int i = 0; i < 6; i++) box[i] = bounds[i].getDouble(b);
      pristine.put(b, box);
      for (IBlockState s : b.getBlockState().getValidStates()) canonical.add(raw(s));
    }
    if (canonical.size() != 1394)
      throw new IllegalStateException("Unexpected state count " + canonical.size());
  }
  void restore(Block b) throws Exception {
    double[] box = pristine.get(b);
    for (int i = 0; i < 6; i++) bounds[i].setDouble(b, box[i]);
  }
  int query(boolean prepare) throws Exception {
    Block b = world.center.getBlock();
    restore(b);
    if (prepare)
      b.setBlockBoundsBasedOnState(world, world.origin);
    List<AxisAlignedBB> boxes = new ArrayList<AxisAlignedBB>();
    BlockPos p = world.origin;
    AxisAlignedBB sweep = new AxisAlignedBB(
        p.getX() - 2, p.getY() - 2, p.getZ() - 2, p.getX() + 3, p.getY() + 3, p.getZ() + 3);
    b.addCollisionBoxesToList(world, p, world.center, sweep, boxes, null);
    return intern(boxes, p);
  }
  int intern(List<AxisAlignedBB> boxes, BlockPos p) {
    List<double[]> local = new ArrayList<double[]>();
    StringBuilder key = new StringBuilder();
    for (AxisAlignedBB a : boxes) {
      double[] v = {a.minX - p.getX(), a.minY - p.getY(), a.minZ - p.getZ(), a.maxX - p.getX(),
          a.maxY - p.getY(), a.maxZ - p.getZ()};
      local.add(v);
      for (double d : v) key.append(Long.toHexString(Double.doubleToRawLongBits(d))).append(',');
    }
    String k = key.toString();
    Integer id = paletteIds.get(k);
    if (id == null) {
      id = palette.size();
      paletteIds.put(k, id);
      palette.add(local);
    }
    return id;
  }
  int single(boolean prepare) throws Exception {
    Block b = world.center.getBlock();
    restore(b);
    if (prepare)
      b.setBlockBoundsBasedOnState(world, world.origin);
    AxisAlignedBB box = b.getCollisionBoundingBox(world, world.origin, world.center);
    return intern(
        box == null ? Collections.<AxisAlignedBB>emptyList() : Collections.singletonList(box),
        world.origin);
  }
  void context(int center, int[] neighbors) throws Exception {
    String key = center + Arrays.toString(neighbors);
    if (!seen.add(key))
      return;
    world.center = state(center);
    world.neighbors = neighbors;
    world.origin = BlockPos.ORIGIN;
    world.piston = null;
    int[] row = Arrays.copyOf(neighbors, 9);
    System.arraycopy(row, 0, row, 1, 6);
    row[0] = center;
    row[7] = query(false);
    row[8] = query(true);
    contexts.add(row);
    singleContexts.add(new int[] {single(false), single(true)});
  }
  void staticCases() throws Exception {
    for (int s : canonical) {
      world.center = state(s);
      world.origin = BlockPos.ORIGIN;
      world.neighbors = new int[6];
      world.piston = null;
      registry.add(new int[] {s, query(false), query(true)});
      singles.add(new int[] {s, single(false), single(true)});
      for (int[] xyz : new int[][] {
               {16777215, 121, -16777215}, {-29999983, 254, 29999983}, {1000001, 37, -1000001}}) {
        world.origin = new BlockPos(xyz[0], xyz[1], xyz[2]);
        far.add(new int[] {s, xyz[0], xyz[1], xyz[2], query(false), query(true)});
        singleFar.add(new int[] {single(false), single(true)});
      }
    }
  }
  void connectedCases() throws Exception {
    int[] connectors = {85, 113, 188, 189, 190, 191, 192, 139, 101, 102, 160};
    for (int id : connectors) {
      for (int other : canonical)
        for (int side = 0; side < 4; side++) {
          int[] n = new int[6];
          n[side] = other;
          context(id << 4, n);
        }
      for (int other :
          new int[] {1, 20, 44, 53, 79, 85, 86, 95, 101, 102, 107, 113, 139, 160, 166, 188})
        for (int mask = 0; mask < 16; mask++) {
          int[] n = new int[6];
          for (int i = 0; i < 4; i++)
            if ((mask & (1 << i)) != 0)
              n[i] = other << 4;
          context(id << 4, n);
        }
    }
    for (int s : canonical) {
      int id = s >> 4;
      if (id == 54 || id == 146)
        for (int mask = 0; mask < 16; mask++) {
          int[] n = new int[6];
          for (int i = 0; i < 4; i++)
            if ((mask & (1 << i)) != 0)
              n[i] = id << 4 | 2;
          context(s, n);
        }
      if (id == 64 || id == 71 || id >= 193 && id <= 197)
        for (int mate = 0; mate < 12; mate++) {
          int[] n = new int[6];
          n[(s & 8) != 0 ? 4 : 5] = id << 4 | mate;
          context(s, n);
        }
      if (state(s).getBlock() instanceof BlockStairs)
        for (int side = 0; side < 4; side++)
          for (int m = 0; m < 8; m++) {
            int[] n = new int[6];
            n[side] = 67 << 4 | m;
            context(s, n);
            for (int blocker = 0; blocker < 4; blocker++)
              if (blocker != side) {
                int[] pair = n.clone();
                pair[blocker] = 53 << 4 | (s & 7);
                context(s, pair);
              }
          }
    }
  }
  void movingCases() throws Exception {
    for (int carried : canonical)
      for (int face = 0; face < 6; face++) {
        world.origin = BlockPos.ORIGIN;
        world.center = state(36 << 4 | face);
        world.neighbors = new int[6];
        world.piston = null;
        IBlockState carriedState = decodeMeta(carried);
        Block b = carriedState.getBlock();
        restore(b);
        int shape;
        try {
          AxisAlignedBB box = b == Blocks.piston_extension || b == Blocks.air
              ? null
              : b.getCollisionBoundingBox(world, world.origin, carriedState);
          shape = intern(
              box == null ? Collections.<AxisAlignedBB>emptyList() : Collections.singletonList(box),
              world.origin);
        } catch (IllegalArgumentException e) {
          shape = -1;
        }
        carriedBoxes.add(new int[] {carried, face, shape});
      }
    for (int carried : new int[] {0, 16, 12 << 4, 20 << 4, 33 << 4 | 5, 34 << 4 | 5, 36 << 4 | 5,
             44 << 4, 44 << 4 | 8, 53 << 4 | 3, 79 << 4, 88 << 4, 165 << 4})
      for (int face = 0; face < 6; face++)
        for (int extend = 0; extend < 2; extend++)
          for (int quarter = 0; quarter <= 4; quarter++) {
            world.origin = BlockPos.ORIGIN;
            world.neighbors = new int[6];
            world.center = state(36 << 4 | face);
            NBTTagCompound nbt = new NBTTagCompound();
            nbt.setInteger("blockId", carried >> 4);
            nbt.setInteger("blockData", carried & 15);
            nbt.setInteger("facing", face);
            nbt.setFloat("progress", quarter * .25f);
            nbt.setBoolean("extending", extend != 0);
            TileEntityPiston tile = new TileEntityPiston();
            tile.readFromNBT(nbt);
            world.piston = tile;
            restore(state(carried).getBlock());
            int direct = query(false);
            pistons.add(new int[] {carried, face, extend, quarter, direct});
          }
  }
  int placementShape(Block candidate, boolean pristine) throws Exception {
    if (pristine)
      restore(candidate);
    try {
      AxisAlignedBB box =
          candidate.getCollisionBoundingBox(world, world.origin, candidate.getDefaultState());
      return intern(
          box == null ? Collections.<AxisAlignedBB>emptyList() : Collections.singletonList(box),
          world.origin);
    } catch (IllegalArgumentException e) {
      return -1;
    }
  }
  void placementContext(Block candidate, int existing, int[] neighbors) throws Exception {
    world.center = state(existing);
    world.origin = BlockPos.ORIGIN;
    world.neighbors = neighbors;
    world.piston = null;
    int[] row = new int[9];
    row[0] = Block.getIdFromBlock(candidate);
    row[1] = existing;
    System.arraycopy(neighbors, 0, row, 2, 6);
    row[8] = placementShape(candidate, true);
    placementContexts.add(row);
  }
  void placementCases() throws Exception {
    Integer[] states = canonical.toArray(new Integer[0]);
    Random r = new Random(189);
    for (Block candidate : Block.blockRegistry) {
      int id = Block.getIdFromBlock(candidate);
      int[] row = new int[states.length + 2];
      row[0] = id;
      row[1] = raw(candidate.getDefaultState());
      world.origin = BlockPos.ORIGIN;
      world.neighbors = new int[6];
      world.piston = null;
      for (int i = 0; i < states.length; i++) {
        world.center = state(states[i]);
        row[i + 2] = placementShape(candidate, true);
      }
      placement.add(row);
      for (int trial = 0; trial < 128; trial++) {
        int[] neighbors = new int[6];
        for (int i = 0; i < 6; i++) neighbors[i] = states[r.nextInt(states.length)];
        placementContext(candidate, states[r.nextInt(states.length)], neighbors);
      }
      if (id == 64 || id == 71 || id >= 193 && id <= 197) {
        for (int a = 0; a < 16; a++)
          for (int b = 0; b < 16; b++)
            for (int c = 0; c < 16; c++)
              placementContext(
                  candidate, 35 << 4 | a, new int[] {0, 0, 0, 0, 35 << 4 | b, 35 << 4 | c});
      }
      if (id == 139)
        for (int other : states)
          for (int mask : new int[] {1, 5, 10, 15}) {
            int[] n = new int[6];
            for (int i = 0; i < 4; i++)
              if ((mask & (1 << i)) != 0)
                n[i] = other;
            placementContext(candidate, 0, n);
          }
      for (int[] xyz : new int[][] {
               {16777215, 121, -16777215}, {-29999983, 254, 29999983}, {1000001, 37, -1000001}}) {
        world.origin = new BlockPos(xyz[0], xyz[1], xyz[2]);
        world.center = state(0);
        world.neighbors = new int[6];
        placementFar.add(new int[] {id, xyz[0], xyz[1], xyz[2], placementShape(candidate, true)});
      }
      world.origin = BlockPos.ORIGIN;
      world.center = state(0);
      world.neighbors = new int[6];
      int fresh = placementShape(candidate, true);
      for (int s : states)
        if ((s >> 4) == id)
          for (int mask : new int[] {0, 1, 5, 15}) {
            restore(candidate);
            world.center = state(s);
            world.neighbors = new int[6];
            for (int i = 0; i < 4; i++)
              if ((mask & (1 << i)) != 0)
                world.neighbors[i] = s;
            candidate.setBlockBoundsBasedOnState(world, world.origin);
            world.center = state(0);
            world.neighbors = new int[6];
            int warm = placementShape(candidate, false);
            if (warm != fresh)
              placementHistory.add(new int[] {id, s, mask, fresh, warm});
          }
    }
  }
  public static void main(String[] args) throws Exception {
    PrintStream output = System.out;
    System.setOut(System.err);
    Bootstrap.register();
    CollisionShapeOracle oracle = new CollisionShapeOracle();
    oracle.staticCases();
    oracle.connectedCases();
    oracle.movingCases();
    oracle.placementCases();
    Map<String, Object> data = new LinkedHashMap<String, Object>();
    int[] decode = new int[4096];
    for (int i = 0; i < decode.length; i++) {
      try {
        decode[i] = raw(decodeMeta(i));
      } catch (IllegalArgumentException e) {
        decode[i] = -1;
      }
    }
    data.put("shapes", oracle.palette);
    data.put("canonical", oracle.registry);
    data.put("contexts", oracle.contexts);
    data.put("far", oracle.far);
    data.put("pistons", oracle.pistons);
    data.put("carried", oracle.carriedBoxes);
    data.put("decode", decode);
    data.put("placement", oracle.placement);
    data.put("placement_context", oracle.placementContexts);
    data.put("placement_far", oracle.placementFar);
    data.put("placement_history", oracle.placementHistory);
    data.put("single", oracle.singles);
    data.put("single_context", oracle.singleContexts);
    data.put("single_far", oracle.singleFar);
    if (args.length > 0)
      writePistonRegion(new File(args[0]));
    output.println(new Gson().toJson(data));
  }
  static void writePistonRegion(File target) throws Exception {
    NBTTagCompound root = new NBTTagCompound(), level = new NBTTagCompound(),
                   section = new NBTTagCompound();
    byte[] blocks = new byte[4096], meta = new byte[2048];
    int index = 256 + 14 * 16 + 15;
    blocks[index] = 36;
    meta[index / 2] = (byte) (5 << 4);
    section.setByte("Y", (byte) 4);
    section.setByteArray("Blocks", blocks);
    section.setByteArray("Data", meta);
    NBTTagList sections = new NBTTagList();
    sections.appendTag(section);
    level.setTag("Sections", sections);
    NBTTagCompound piston = new NBTTagCompound();
    piston.setInteger("x", -1);
    piston.setInteger("y", 65);
    piston.setInteger("z", -2);
    piston.setInteger("blockId", 35);
    piston.setInteger("blockData", 14);
    piston.setInteger("facing", 5);
    piston.setBoolean("extending", true);
    piston.setFloat("progress", .5f);
    TileEntityPiston original = new TileEntityPiston();
    original.readFromNBT(piston);
    NBTTagCompound saved = new NBTTagCompound();
    original.writeToNBT(saved);
    NBTTagList tiles = new NBTTagList();
    tiles.appendTag(saved);
    level.setTag("TileEntities", tiles);
    root.setTag("Level", level);
    target.getParentFile().mkdirs();
    RegionFile region = new RegionFile(target);
    try (DataOutputStream stream = region.getChunkDataOutputStream(31, 31)) {
      CompressedStreamTools.write(root, stream);
    }
    region.close();
  }
}
