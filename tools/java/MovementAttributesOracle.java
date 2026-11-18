import com.google.gson.Gson;
import com.mojang.authlib.GameProfile;
import io.netty.buffer.Unpooled;
import java.io.PrintStream;
import java.lang.reflect.Method;
import java.util.*;
import net.minecraft.client.Minecraft;
import net.minecraft.client.multiplayer.PlayerControllerMP;
import net.minecraft.client.network.NetHandlerPlayClient;
import net.minecraft.entity.SharedMonsterAttributes;
import net.minecraft.entity.ai.attributes.*;
import net.minecraft.init.Bootstrap;
import net.minecraft.network.PacketBuffer;
import net.minecraft.network.play.server.S20PacketEntityProperties;

/**
 * Actual S20 bytecode + ModifiableAttributeInstance + EntityPlayerSP sprint.
 * The receiver applies the same three public attribute calls as the original
 * NetHandlerPlayClient; rendering and the transport thread are not required.
 */
public final class MovementAttributesOracle {
  static final UUID SPRINT = UUID.fromString("662a6b8d-da3e-4c1c-8813-96ea6097278d");
  static String hex(byte[] bytes) {
    StringBuilder s = new StringBuilder();
    for (byte b : bytes) s.append(String.format("%02x", b & 255));
    return s.toString();
  }
  static Map<String, Object> modifier(AttributeModifier m) {
    Map<String, Object> o = new LinkedHashMap<String, Object>();
    o.put("uuid", m.getID().toString());
    o.put("amount", Long.toHexString(Double.doubleToRawLongBits(m.getAmount())));
    o.put("operation", m.getOperation());
    return o;
  }
  static List<Object> modifiers(Collection<AttributeModifier> values) {
    List<Object> out = new ArrayList<Object>();
    for (AttributeModifier m : values) out.add(modifier(m));
    return out;
  }
  static MovementOracle.Player player() throws Exception {
    Minecraft mc = MovementOracle.minecraft();
    MovementOracle.TestWorld w = new MovementOracle.TestWorld(0, 0, 0);
    NetHandlerPlayClient net =
        new NetHandlerPlayClient(mc, null, null, new GameProfile(new UUID(0, 189), "Attributes"));
    mc.playerController = new PlayerControllerMP(mc, net);
    return new MovementOracle.Player(mc, w, net);
  }
  static Map<String, Object> run(byte[] bytes, MovementOracle.Player p, Boolean sprint)
      throws Exception {
    Map<String, Object> row = new LinkedHashMap<String, Object>();
    row.put("packet", hex(bytes));
    S20PacketEntityProperties packet = new S20PacketEntityProperties();
    try {
      packet.readPacketData(new PacketBuffer(Unpooled.wrappedBuffer(bytes)));
    } catch (Exception e) {
      row.put("read_error", true);
      return row;
    }
    row.put("read_error", false);
    row.put("entity_id", packet.getEntityId());
    List<Object> snapshots = new ArrayList<Object>();
    for (S20PacketEntityProperties.Snapshot s : packet.func_149441_d()) {
      Map<String, Object> o = new LinkedHashMap<String, Object>();
      o.put("name", s.func_151409_a());
      o.put("base", Long.toHexString(Double.doubleToRawLongBits(s.func_151410_b())));
      o.put("modifiers", modifiers(s.func_151408_c()));
      snapshots.add(o);
    }
    row.put("snapshots", snapshots);
    IAttributeInstance target = p.getEntityAttribute(SharedMonsterAttributes.movementSpeed);
    try {
      for (S20PacketEntityProperties.Snapshot s : packet.func_149441_d()) {
        target.setBaseValue(s.func_151410_b());
        target.removeAllModifiers();
        for (AttributeModifier m : s.func_151408_c()) target.applyModifier(m);
      }
    } catch (Exception e) {
      row.put("apply_error", true);
      return row;
    }
    row.put("apply_error", false);
    row.put("value", Long.toHexString(Double.doubleToRawLongBits(target.getAttributeValue())));
    Method ordered =
        ModifiableAttributeInstance.class.getDeclaredMethod("func_180375_b", int.class);
    ordered.setAccessible(true);
    List<Object> ops = new ArrayList<Object>();
    for (int op = 0; op < 3; op++)
      ops.add(modifiers((Collection<AttributeModifier>) ordered.invoke(target, op)));
    row.put("ordered", ops);
    if (sprint != null) {
      p.setSprinting(sprint);
      row.put("sprint", sprint);
      row.put(
          "after_sprint", Long.toHexString(Double.doubleToRawLongBits(target.getAttributeValue())));
    }
    return row;
  }
  static byte[] encode(S20PacketEntityProperties packet) throws Exception {
    PacketBuffer b = new PacketBuffer(Unpooled.buffer());
    packet.writePacketData(b);
    byte[] out = new byte[b.readableBytes()];
    b.readBytes(out);
    return out;
  }
  static byte[] raw(int properties, int count, int operation, boolean duplicate) throws Exception {
    PacketBuffer b = new PacketBuffer(Unpooled.buffer());
    b.writeVarIntToBuffer(189);
    b.writeInt(properties);
    if (properties > 0) {
      b.writeString("generic.movementSpeed");
      b.writeDouble(0.2);
      b.writeVarIntToBuffer(count);
      for (int i = 0; i < count; i++) {
        b.writeUuid(new UUID(0, duplicate ? 1 : i + 1));
        b.writeDouble(0.1);
        b.writeByte(operation);
      }
    }
    byte[] out = new byte[b.readableBytes()];
    b.readBytes(out);
    return out;
  }
  public static void main(String[] args) throws Exception {
    PrintStream output = System.out;
    System.setOut(System.err);
    Bootstrap.register();
    List<Object> cases = new ArrayList<Object>();
    for (int seed = 0; seed < 32; seed++) {
      Random random = new Random(seed);
      ServersideAttributeMap map = new ServersideAttributeMap();
      IAttributeInstance source = map.registerAttribute(SharedMonsterAttributes.movementSpeed);
      MovementOracle.Player p = player();
      List<Object> steps = new ArrayList<Object>();
      for (int step = 0; step < 6; step++) {
        source.setBaseValue(1 + random.nextDouble() * 3);
        source.removeAllModifiers();
        int count = new int[] {0, 3, 9, 13, 28, 5}[step];
        for (int i = 0; i < count; i++) {
          UUID id =
              i == 0 && step % 2 == 0 ? SPRINT : new UUID(random.nextLong(), random.nextLong());
          source.applyModifier(new AttributeModifier(
              id, "oracle", (random.nextDouble() - .5) * .1, random.nextInt(3)));
        }
        steps.add(run(encode(new S20PacketEntityProperties(189, Collections.singleton(source))), p,
            step % 2 == 0));
      }
      cases.add(steps);
    }
    for (byte[] bytes : new byte[][] {raw(-1, 0, 0, false), raw(1, -1, 0, false),
             raw(1, 2, 1, true), raw(1, 1, 3, false), raw(1, 1, -1, false)})
      cases.add(Collections.singletonList(run(bytes, player(), null)));
    output.println(new Gson().toJson(cases));
  }
}
