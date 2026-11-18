import com.google.gson.Gson;
import com.mojang.authlib.GameProfile;
import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import net.minecraft.block.*;
import net.minecraft.block.material.Material;
import net.minecraft.block.state.IBlockState;
import net.minecraft.client.Minecraft;
import net.minecraft.client.entity.EntityPlayerSP;
import net.minecraft.client.multiplayer.PlayerControllerMP;
import net.minecraft.client.network.NetHandlerPlayClient;
import net.minecraft.client.settings.*;
import net.minecraft.enchantment.Enchantment;
import net.minecraft.entity.*;
import net.minecraft.init.*;
import net.minecraft.item.ItemStack;
import net.minecraft.potion.*;
import net.minecraft.profiler.Profiler;
import net.minecraft.stats.*;
import net.minecraft.util.*;
import net.minecraft.world.*;
import net.minecraft.world.chunk.*;
import net.minecraft.world.storage.WorldInfo;
import sun.misc.Unsafe;

/**
 * Original EntityPlayerSP/LivingBase/Entity physics. Only rendering, networking,
 * achievements, and entity lists are replaced; all block shapes and water
 * acceleration run the original World/Block bytecode.
 */
public final class MovementOracle {
  static Unsafe unsafe() throws Exception {
    Field f = Unsafe.class.getDeclaredField("theUnsafe");
    f.setAccessible(true);
    return (Unsafe) f.get(null);
  }
  static final class TestWorld extends World {
    final int scenario, ox, oz;
    final Map<Long, Chunk> chunks = new HashMap<Long, Chunk>();
    TestWorld(int scenario, int ox, int oz) {
      super(null,
          new WorldInfo(new WorldSettings(
                            189, WorldSettings.GameType.SURVIVAL, false, false, WorldType.DEFAULT),
              "motion"),
          new WorldProviderSurface(), new Profiler(), true);
      this.scenario = scenario;
      this.ox = ox;
      this.oz = oz;
      provider.registerWorld(this);
      worldInfo.setDifficulty(EnumDifficulty.NORMAL);
    }
    protected IChunkProvider createChunkProvider() {
      return null;
    }
    protected int getRenderDistanceChunks() {
      return 8;
    }
    protected boolean isChunkLoaded(int x, int z, boolean allowEmpty) {
      return scenario != 34 && scenario != 35
          || (x == Math.floorDiv(ox, 16) && z == Math.floorDiv(oz, 16));
    }
    public Chunk getChunkFromChunkCoords(int x, int z) {
      long k = (long) x << 32 ^ (z & 0xffffffffL);
      Chunk c = chunks.get(k);
      if (c == null) {
        c = new Chunk(this, x, z);
        c.setChunkLoaded(true);
        chunks.put(k, c);
      }
      return c;
    }
    public IBlockState getBlockState(BlockPos p) {
      if ((scenario == 34 || scenario == 35)
          && (Math.floorDiv(p.getX() - ox, 16) != 0 || Math.floorDiv(p.getZ() - oz, 16) != 0))
        return Blocks.air.getDefaultState();
      int s = initial(p.getX() - ox, p.getY(), p.getZ() - oz, scenario);
      return Block.getBlockById(s >> 4).getStateFromMeta(s & 15);
    }
    public List<Entity> getEntitiesWithinAABBExcludingEntity(Entity e, AxisAlignedBB b) {
      return Collections.emptyList();
    }
    public boolean isRainingAt(BlockPos p) {
      return false;
    }
  }
  static int initial(int x, int y, int z, int scenario) {
    if (y < 0 || y >= 256)
      return 0;
    if (y == 0) {
      if (scenario == 9 && z < 7)
        return 0;
      return (scenario == 14 ? 79 : scenario == 15 ? 88 : scenario == 16 ? 165 : 1) << 4;
    }
    if (scenario == 4 && y <= 3)
      return 9 << 4;
    if (scenario == 5 && y <= 3)
      return 11 << 4;
    if (scenario == 6 && y == 1)
      return 8 << 4 | Math.floorMod(x, 8);
    if (scenario == 7 && x == 8 && z == 7 && y <= 8)
      return 65 << 4 | 3;
    if (scenario == 7 && z == 6 && y <= 8)
      return 1 << 4;
    if (scenario == 8 && x == 8 && z == 7 && y <= 8)
      return 106 << 4 | 4;
    if (scenario == 8 && z == 6 && y <= 8)
      return 1 << 4;
    if (scenario == 10 && z <= 6 && y == 1)
      return 44 << 4;
    if (scenario == 11 && z == 6 && y == 1)
      return 53 << 4 | 3;
    if (scenario == 12 && z == 6 && y < 5)
      return 1 << 4;
    if (scenario == 13 && y == 3)
      return 1 << 4;
    if (scenario == 17 && z >= 5 && z <= 8 && y <= 3)
      return 30 << 4;
    if (scenario == 30 && y == 1)
      return 8 << 4 | Math.floorMod(x + 2 * z, 8);
    if (scenario == 31 && x <= 7 && y <= 4)
      return 1 << 4;
    if (scenario == 31 && y <= 3)
      return 8 << 4 | 8;
    if (scenario == 32 && z == 6 && y <= 4)
      return 1 << 4;
    if (scenario == 32 && y <= 3)
      return 9 << 4;
    if (scenario == 33 && x == 8 && z == 8 && y <= 2)
      return 1 << 4;
    if (scenario == 34 && y <= 3)
      return 9 << 4;
    if (scenario == 35 && y <= 3)
      return 30 << 4;
    return 0;
  }
  static class Player extends EntityPlayerSP {
    boolean use, jumped, jumpSprint;
    final List<Boolean> flightChanges = new ArrayList<Boolean>();
    Player(Minecraft mc, World world, NetHandlerPlayClient net) {
      super(mc, world, net, new StatFileWriter());
      movementInput = new MovementInputFromOptions(mc.gameSettings);
    }
    protected boolean isCurrentViewEntity() {
      return true;
    }
    public boolean isSpectator() {
      return false;
    }
    public boolean isUsingItem() {
      return use;
    }
    public void sendPlayerAbilities() {
      flightChanges.add(capabilities.isFlying);
    }
    public void addStat(StatBase stat, int amount) {}
    public void jump() {
      jumped = true;
      jumpSprint = isSprinting();
      super.jump();
    }
    int sprintTimer() {
      return sprintToggleTimer;
    }
    int flyTimer() {
      return flyToggleTimer;
    }
  }
  static Minecraft minecraft() throws Exception {
    Minecraft mc = (Minecraft) unsafe().allocateInstance(Minecraft.class);
    mc.gameSettings = (GameSettings) unsafe().allocateInstance(GameSettings.class);
    GameSettings s = mc.gameSettings;
    s.keyBindForward = new KeyBinding("forward", 1, "oracle");
    s.keyBindBack = new KeyBinding("back", 2, "oracle");
    s.keyBindLeft = new KeyBinding("left", 3, "oracle");
    s.keyBindRight = new KeyBinding("right", 4, "oracle");
    s.keyBindJump = new KeyBinding("jump", 5, "oracle");
    s.keyBindSneak = new KeyBinding("sneak", 6, "oracle");
    s.keyBindSprint = new KeyBinding("sprint", 7, "oracle");
    return mc;
  }
  static List<String> bits(double... values) {
    List<String> out = new ArrayList<String>();
    for (double d : values) out.add(Long.toHexString(Double.doubleToRawLongBits(d)));
    return out;
  }
  static void effect(Player p, int id, Integer amplifier) {
    p.removePotionEffect(id);
    Potion.potionTypes[id].removeAttributesModifiersFromEntity(p, p.getAttributeMap(), 0);
    if (amplifier != null) {
      p.addPotionEffect(new PotionEffect(id, 1200, amplifier));
      Potion.potionTypes[id].applyAttributesModifiersToEntity(p, p.getAttributeMap(), amplifier);
    }
  }
  static Integer amplifier(Player p, int id) {
    PotionEffect e = p.getActivePotionEffect(Potion.potionTypes[id]);
    return e == null ? null : e.getAmplifier();
  }
  static int input(int scenario, int tick) {
    if (scenario == 0)
      return tick < 30 ? 1 : 0;
    if (scenario == 1)
      return tick < 30 ? 1 | 64 : 0;
    if (scenario == 2)
      return tick < 30 ? 1 | 8 : 0;
    if (scenario == 3)
      return 1 | 64 | (tick < 35 ? 16 : 0);
    if (scenario == 4 || scenario == 5)
      return tick < 30 ? 1 | 16 : 0;
    if (scenario == 6)
      return tick < 20 ? 0 : 1;
    if (scenario == 7 || scenario == 8)
      return 1 | (tick > 30 ? 32 : 0);
    if (scenario == 9)
      return 1 | 32;
    if (scenario == 13)
      return 1 | 16;
    if (scenario == 18)
      return tick < 25 ? 1 : 0;
    if (scenario == 19)
      return tick < 20 ? 16 : tick < 40 ? 32 : 0;
    if (scenario == 20)
      return tick < 20 ? 1 | 16 : 0;
    if (scenario == 21)
      return tick < 30 ? 1 | 128 : 1 | 64;
    if (scenario == 22)
      return tick < 3 ? 1 : tick < 5 ? 0 : 1;
    if (scenario == 23)
      return 1 | ((tick % 13) < 4 ? 16 : 0);
    if (scenario == 24 || scenario == 26 || scenario == 29)
      return tick == 0 || tick == 6 ? 16 : 0;
    if (scenario == 25)
      return tick == 0 || tick == 7 ? 16 : 0;
    if (scenario == 27 || scenario == 30 || scenario == 31 || scenario == 33)
      return 0;
    if (scenario == 28)
      return tick < 30 ? 1 | 64 | (tick < 10 ? 16 : 0) : 0;
    if (scenario == 32)
      return tick < 30 ? 1 | 16 : 1;
    if (scenario == 39)
      return 1 | 64 | (tick < 35 ? 16 : 0);
    if (scenario == 40 || scenario == 41)
      return 1 | 64;
    if (scenario == 42)
      return 1;
    if (scenario == 43)
      return tick < 30 ? 1 | 16 : 0;
    if (scenario >= 36)
      return 1 | (tick > 20 ? 64 : 0);
    return 1;
  }
  public static void main(String[] args) throws Exception {
    PrintStream output = System.out;
    System.setOut(System.err);
    Bootstrap.register();
    List<Object> rows = new ArrayList<Object>();
    Field jump = EntityLivingBase.class.getDeclaredField("jumpTicks");
    jump.setAccessible(true);
    boolean potions = args.length > 0 && args[0].equals("potions");
    for (int scenario = args.length == 0 ? 0
            : potions                    ? 36
                                         : 24;
        scenario < (args.length == 0 ? 24
                : potions            ? 44
                                     : 36);
        scenario++)
      for (float yaw : new float[] {180f, 213.3f, -45f}) {
        int ox = yaw == -45f ? 1000000 : 0, oz = yaw == -45f ? -1000000 : 0;
        TestWorld world = new TestWorld(scenario == 20 || scenario >= 42 ? 4 : scenario, ox, oz);
        Minecraft mc = minecraft();
        NetHandlerPlayClient net =
            new NetHandlerPlayClient(mc, null, null, new GameProfile(new UUID(0, 189), "Oracle"));
        mc.playerController = new PlayerControllerMP(mc, net);
        Player p = new Player(mc, world, net);
        double y = scenario == 18 || scenario == 19 || scenario == 26 || scenario == 28 ? 20
            : scenario == 4 || scenario == 5 || scenario == 20 || scenario == 27 || scenario == 31
                || scenario == 32 || scenario == 34 || scenario >= 42
            ? 1.1
            : scenario == 16 ? 6
            : scenario == 15 ? 0.875
                             : 1;
        boolean flying = scenario >= 18 && scenario <= 20 || scenario >= 26 && scenario <= 28;
        p.setPosition(ox
                + (scenario == 34 || scenario == 35 ? 15.8
                        : scenario == 33            ? 8.9
                                                    : 8.5),
            y, oz + 8.5);
        p.rotationYaw = yaw;
        p.onGround = !flying && y <= 1;
        p.capabilities.isFlying = flying;
        p.capabilities.allowFlying = scenario >= 24 && scenario <= 28;
        p.capabilities.setFlySpeed(scenario == 28 ? 0.12f : 0.05f);
        if (scenario == 27)
          p.motionY = -0.2;
        if (scenario == 36)
          effect(p, 1, 0);
        if (scenario == 37)
          effect(p, 2, 1);
        if (scenario == 38) {
          effect(p, 1, 2);
          effect(p, 2, 1);
        }
        if (scenario == 39)
          effect(p, 8, 2);
        if (scenario == 40)
          effect(p, 15, 0);
        int depth = scenario == 42 ? 3 : scenario == 43 ? 1 : 0;
        if (depth > 0) {
          p.inventory.armorInventory[0] = new ItemStack(Items.diamond_boots);
          p.inventory.armorInventory[0].addEnchantment(Enchantment.depthStrider, depth);
        }
        Map<String, Object> row = new LinkedHashMap<String, Object>();
        row.put("scenario", scenario);
        row.put("origin", new int[] {ox, oz});
        row.put("yaw", yaw);
        row.put("position", new double[] {p.posX, p.posY, p.posZ});
        row.put("velocity", new double[] {p.motionX, p.motionY, p.motionZ});
        row.put("grounded", p.onGround);
        row.put("flying", flying);
        row.put("allow_flying", p.capabilities.allowFlying);
        row.put("fly_speed", p.capabilities.getFlySpeed());
        List<Object> ticks = new ArrayList<Object>();
        for (int t = 0; t < 70; t++) {
          if (scenario == 41 && t == 20)
            effect(p, 15, 0);
          if (scenario == 40 && t == 30)
            effect(p, 15, null);
          if (t == 40 && scenario >= 36 && scenario <= 38) {
            effect(p, 1, null);
            effect(p, 2, null);
          }
          int flags = input(scenario, t);
          for (int k = 0; k < 7; k++) KeyBinding.setKeyBindState(k + 1, (flags & (1 << k)) != 0);
          p.use = (flags & 128) != 0;
          p.getFoodStats().setFoodLevel(scenario == 1 && t >= 20 ? 6 : 20);
          p.jumped = false;
          p.jumpSprint = false;
          p.flightChanges.clear();
          p.prevPosX = p.posX;
          p.prevPosY = p.posY;
          p.prevPosZ = p.posZ;
          p.handleWaterMovement();
          p.onLivingUpdate();
          AxisAlignedBB b = p.getEntityBoundingBox();
          Map<String, Object> tick = new LinkedHashMap<String, Object>();
          tick.put("input", flags);
          tick.put("can_sprint", p.getFoodStats().getFoodLevel() > 6);
          tick.put("bits",
              bits(p.posX, p.posY, p.posZ, p.motionX, p.motionY, p.motionZ, b.minX, b.minY, b.minZ,
                  b.maxX, b.maxY, b.maxZ));
          tick.put("flags",
              new boolean[] {p.onGround, p.isSprinting(), p.capabilities.isFlying, p.isInWater(),
                  p.isOnLadder(), p.jumped, p.isCollidedHorizontally});
          tick.put("jump_delay", jump.getInt(p));
          tick.put("sprint_timer", p.sprintTimer());
          tick.put("fly_timer", p.flyTimer());
          tick.put("flight_changes", new ArrayList<Boolean>(p.flightChanges));
          tick.put("eyes_water", p.isInsideOfMaterial(Material.water));
          tick.put("jump_sprinting", p.jumpSprint);
          tick.put("effects",
              Arrays.asList(amplifier(p, 1), amplifier(p, 2), amplifier(p, 8), amplifier(p, 15)));
          tick.put("depth_strider", depth);
          tick.put(
              "air_speed", Integer.toUnsignedLong(Float.floatToRawIntBits(p.jumpMovementFactor)));
          ticks.add(tick);
        }
        row.put("ticks", ticks);
        rows.add(row);
      }
    output.println(new Gson().toJson(rows));
  }
}
