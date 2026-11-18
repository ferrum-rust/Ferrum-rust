import com.google.gson.Gson;
import java.io.PrintStream;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.util.*;
import net.minecraft.block.*;
import net.minecraft.block.material.Material;
import net.minecraft.block.state.IBlockState;
import net.minecraft.entity.Entity;
import net.minecraft.entity.player.EntityPlayer;
import net.minecraft.init.*;
import net.minecraft.item.*;
import net.minecraft.util.*;
import net.minecraft.world.World;
import sun.misc.Unsafe;

/**
 * Test World.canBlockBePlaced's material/support stage separately from the
 * collision-box oracle. A bit set means the original method accepted a cell.
 */
public final class HandPlacementOracle {
  static class Grid extends CollisionShapeOracle.Fixture {
    Map<BlockPos, IBlockState> cells;
    int light;
    List<Entity> entities;
    List<BlockPos> writes;
    public IBlockState getBlockState(BlockPos p) {
      return cells.getOrDefault(p, Blocks.air.getDefaultState());
    }
    public boolean isAirBlock(BlockPos p) {
      return getBlockState(p).getBlock().getMaterial() == Material.air;
    }
    public int getLight(BlockPos p) {
      return light;
    }
    public boolean setBlockState(BlockPos p, IBlockState s, int flags) {
      if (p.getY() < 0 || p.getY() >= 256 || getBlockState(p) == s)
        return false;
      cells.put(new BlockPos(p), s);
      writes.add(new BlockPos(p));
      return true;
    }
    public void playSoundEffect(
        double x, double y, double z, String sound, float volume, float pitch) {}
    public boolean isBlockPowered(BlockPos p) {
      return false;
    }
    public void scheduleUpdate(BlockPos p, Block b, int delay) {}
    public List<Entity> getEntitiesWithinAABBExcludingEntity(Entity excluded, AxisAlignedBB box) {
      List<Entity> result = new ArrayList<Entity>();
      for (Entity entity : entities)
        if (entity != excluded && entity.getEntityBoundingBox().intersectsWith(box))
          result.add(entity);
      return result;
    }
    void set(BlockPos p, int packed) {
      cells.put(p, CollisionShapeOracle.state(packed));
    }
  }
  final Grid world;
  final int[] states;
  final BlockPos pos = new BlockPos(0, 64, 0);
  final Method encouragement;
  final List<Object> rows = new ArrayList<Object>(), randomRows = new ArrayList<Object>();
  final List<int[]> registry = new ArrayList<int[]>(), replaceable = new ArrayList<int[]>();
  final List<int[]> placed = new ArrayList<int[]>();
  final List<Object> clicks = new ArrayList<Object>();
  final List<Object> sounds = new ArrayList<Object>();
  final MovementOracle.Player player;
  HandPlacementOracle() throws Exception {
    Field f = Unsafe.class.getDeclaredField("theUnsafe");
    f.setAccessible(true);
    world = (Grid) ((Unsafe) f.get(null)).allocateInstance(Grid.class);
    world.cells = new HashMap<BlockPos, IBlockState>();
    world.entities = new ArrayList<Entity>();
    world.writes = new ArrayList<BlockPos>();
    player = MovementAttributesOracle.player();
    TreeSet<Integer> canonical = new TreeSet<Integer>();
    for (Block b : Block.blockRegistry)
      for (IBlockState state : b.getBlockState().getValidStates())
        canonical.add(CollisionShapeOracle.raw(state));
    states = new int[canonical.size()];
    int i = 0;
    for (int state : canonical) states[i++] = state;
    encouragement = BlockFire.class.getDeclaredMethod("getEncouragement", Block.class);
    encouragement.setAccessible(true);
  }
  void placementStates(Block block, Item item) {
    int id = Block.getIdFromBlock(block);
    int[] row = new int[1 + 16 * 6 * 4 * 3 * 2];
    row[0] = id;
    int index = 1;
    ItemStack stack = new ItemStack(item);
    for (int meta = 0; meta < 16; meta++)
      for (int side = 0; side < 6; side++)
        for (int yaw = 0; yaw < 4; yaw++)
          for (int vertical = 0; vertical < 3; vertical++)
            for (int upper = 0; upper < 2; upper++) {
              world.cells.clear();
              world.writes.clear();
              world.light = 15;
              world.origin = pos;
              world.neighbors = new int[6];
              for (EnumFacing face : EnumFacing.values()) world.set(pos.offset(face), 16);
              player.setPosition(.5, vertical == 0 ? 64 : vertical == 1 ? 66 : 61, .5);
              player.rotationYaw = yaw * 90f;
              try {
                IBlockState initial = block.onBlockPlaced(world, pos, EnumFacing.getFront(side),
                    .25f, upper == 0 ? .5f : .75f, .75f, item.getMetadata(meta), player);
                world.cells.put(pos, initial);
                world.writes.clear();
                block.onBlockPlacedBy(world, pos, initial, player, stack);
                int state = CollisionShapeOracle.raw(world.getBlockState(pos));
                int extra = world.writes.contains(pos.up())
                    ? CollisionShapeOracle.raw(world.getBlockState(pos.up())) + 1
                    : 0;
                row[index++] = state | (extra << 12);
              } catch (IllegalArgumentException e) {
                row[index++] = -1;
              }
            }
    placed.add(row);
  }
  void clickCases(Block block, Item item, Random r) {
    for (int i = 0; i < 36; i++) {
      int id = Block.getIdFromBlock(block), side = i % 6, yaw = r.nextInt(4), meta = r.nextInt(16),
          vertical = r.nextInt(3);
      float hitY = i % 2 == 0 ? .5f : .75f;
      BlockPos target = pos.offset(EnumFacing.getFront(side));
      world.cells.clear();
      world.writes.clear();
      world.light = i % 2 == 0 ? 0 : 15;
      world.origin = target;
      world.neighbors = new int[6];
      // Fill the complete local neighborhood before choosing the clicked cell.
      for (EnumFacing face : EnumFacing.values())
        world.set(target.offset(face), r.nextBoolean() ? 16 : 0);
      world.set(pos, 16);
      int existing = i % 4 == 0 ? 31 << 4 | 1 : i % 4 == 1 ? 9 << 4 : i % 4 == 2 ? 0 : 78 << 4 | 2;
      world.set(target, existing);
      if (i % 7 == 0)
        world.set(pos, 78 << 4);
      if (i % 11 == 0)
        world.set(pos, 31 << 4 | 1);
      if (id == 44 || id == 126 || id == 182) {
        if (side == 0 || side == 1)
          world.set(pos, id << 4 | (meta & 7) | (side == 0 ? 8 : 0));
        else if (side == 2)
          world.set(target, id << 4 | (meta & 7));
      }
      if ((id == 54 || id == 146) && i % 3 == 0) {
        world.set(target.east(), id << 4 | 2);
        if (i % 2 == 0)
          world.set(target.east().north(), id << 4 | 2);
      }
      BlockPos newPos =
          world.getBlockState(pos).getBlock().isReplaceable(world, pos) ? pos : target;
      player.setPosition(i % 5 == 0 ? newPos.getX() + .5 : newPos.getX() + 3.5,
          vertical == 0       ? 64
              : vertical == 1 ? 66
                              : 61,
          newPos.getZ() + .5);
      player.rotationYaw = yaw * 90f;
      world.entities.clear();
      world.entities.add(player);
      Map<String, Object> sample = new LinkedHashMap<String, Object>();
      List<int[]> before = new ArrayList<int[]>();
      for (Map.Entry<BlockPos, IBlockState> e : world.cells.entrySet()) {
        BlockPos q = e.getKey();
        before.add(
            new int[] {q.getX(), q.getY(), q.getZ(), CollisionShapeOracle.raw(e.getValue())});
      }
      before.sort((a, b) -> a[0] != b[0] ? a[0] - b[0] : a[1] != b[1] ? a[1] - b[1] : a[2] - b[2]);
      sample.put("cells", before);
      sample.put("id", id);
      sample.put("meta", meta);
      sample.put("side", side);
      sample.put("hit_y", hitY);
      sample.put("yaw", player.rotationYaw);
      sample.put("player", new double[] {player.posX, player.posY, player.posZ});
      sample.put("light", world.light);
      ItemStack stack = new ItemStack(item, 2, meta);
      boolean accepted = false;
      boolean error = false;
      try {
        accepted =
            item.onItemUse(stack, player, world, pos, EnumFacing.getFront(side), .25f, hitY, .75f);
      } catch (IllegalArgumentException e) {
        error = true;
      }
      sample.put("accepted", accepted);
      sample.put("error", error);
      sample.put("count", stack.stackSize);
      List<int[]> writes = new ArrayList<int[]>();
      for (BlockPos q : new LinkedHashSet<BlockPos>(world.writes))
        writes.add(new int[] {
            q.getX(), q.getY(), q.getZ(), CollisionShapeOracle.raw(world.getBlockState(q))});
      sample.put("writes", writes);
      clicks.add(sample);
    }
  }
  boolean allowed(Block b, EnumFacing face) {
    return world.canBlockBePlaced(b, pos, true, face, null, null);
  }
  void fixed(Block b, int face, int pattern) {
    byte[] bits = new byte[(states.length + 7) / 8];
    EnumFacing side = EnumFacing.getFront(face);
    for (int i = 0; i < states.length; i++) {
      world.cells.clear();
      world.light = 15;
      if (pattern == 0)
        world.set(pos.down(), states[i]);
      if (pattern == 1)
        world.set(pos.offset(side.getOpposite()), states[i]);
      if (pattern == 2) {
        world.set(pos, states[i]);
        world.set(pos.down(), 16);
      }
      if (pattern == 3) {
        world.set(pos.down(), states[i]);
        world.light = 0;
      }
      if (allowed(b, side))
        bits[i / 8] |= 1 << (i & 7);
    }
    rows.add(Arrays.asList(
        Block.getIdFromBlock(b), face, pattern, Base64.getEncoder().encodeToString(bits)));
  }
  void run() throws Exception {
    Method use = ItemBlock.class.getMethod("onItemUse", ItemStack.class, EntityPlayer.class,
        World.class, BlockPos.class, EnumFacing.class, float.class, float.class, float.class);
    Random random = new Random(189);
    for (Block b : Block.blockRegistry) {
      int id = Block.getIdFromBlock(b);
      Item item = Item.getItemFromBlock(b);
      int[] row = new int[20];
      row[0] = id;
      row[1] = item == null ? -1 : Item.getIdFromItem(item);
      row[2] = item instanceof ItemBlock
              && item.getClass()
                      .getMethod(use.getName(), use.getParameterTypes())
                      .getDeclaringClass()
                  == ItemBlock.class
          ? 1
          : 0;
      row[3] = (int) encouragement.invoke(Blocks.fire, b);
      for (int m = 0; m < 16; m++) row[4 + m] = item == null ? 0 : item.getMetadata(m);
      registry.add(row);
      sounds.add(Arrays.asList(id, b.stepSound.getPlaceSound(),
          (b.stepSound.getVolume() + 1f) * .5f, b.stepSound.getFrequency() * .8f));
      if (row[2] == 1 || id == 44 || id == 126 || id == 182 || id == 78) {
        placementStates(b, item);
        clickCases(b, item, random);
      }
      fixed(b, 1, 0);
      fixed(b, 1, 2);
      fixed(b, 1, 3);
      for (int side = 0; side < 6; side++) fixed(b, side, 1);
      for (int i = 0; i < 96; i++) {
        int[] input = new int[14];
        input[0] = id;
        input[1] = random.nextInt(6);
        input[2] = random.nextInt(16);
        world.cells.clear();
        world.light = input[2];
        input[3] = i % 3 == 0 ? 0 : states[random.nextInt(states.length)];
        world.set(pos, input[3]);
        for (int j = 0; j < 6; j++) {
          input[4 + j] = states[random.nextInt(states.length)];
          world.set(pos.offset(CollisionShapeOracle.SIDES[j]), input[4 + j]);
        }
        for (int j = 0; j < 4; j++) {
          input[10 + j] = states[random.nextInt(states.length)];
          world.set(pos.offset(CollisionShapeOracle.SIDES[j]).down(), input[10 + j]);
        }
        randomRows.add(Arrays.asList(input, allowed(b, EnumFacing.getFront(input[1]))));
      }
    }
    for (int s : states) {
      world.cells.clear();
      world.set(pos, s);
      int first = world.getBlockState(pos).getBlock().isReplaceable(world, pos) ? 1 : 0;
      replaceable.add(new int[] {s, first});
    }
  }
  public static void main(String[] args) throws Exception {
    PrintStream output = System.out;
    System.setOut(System.err);
    Bootstrap.register();
    HandPlacementOracle o = new HandPlacementOracle();
    o.run();
    Map<String, Object> result = new LinkedHashMap<String, Object>();
    result.put("states", o.states);
    result.put("registry", o.registry);
    result.put("replaceable", o.replaceable);
    result.put("bits", o.rows);
    result.put("random", o.randomRows);
    result.put("placed", o.placed);
    result.put("clicks", o.clicks);
    result.put("sounds", o.sounds);
    output.println(new Gson().toJson(result));
  }
}
