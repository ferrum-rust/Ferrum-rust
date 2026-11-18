import com.google.gson.Gson;
import io.netty.buffer.Unpooled;
import java.io.PrintStream;
import java.lang.reflect.Method;
import java.util.*;
import net.minecraft.entity.EntityLivingBase;
import net.minecraft.init.Bootstrap;
import net.minecraft.nbt.NBTTagCompound;
import net.minecraft.network.PacketBuffer;
import net.minecraft.network.play.server.*;
import net.minecraft.potion.PotionEffect;

/** Actual S1D/S1E serialization and EntityPlayerSP effect-map ticks. */
public final class PlayerEffectPacketOracle {
  static byte[] bytes(PacketBuffer b) {
    byte[] out = new byte[b.readableBytes()];
    b.readBytes(out);
    return out;
  }
  static Map<String, Object> effect(PotionEffect e) {
    Map<String, Object> o = new LinkedHashMap<String, Object>();
    o.put("id", e.getPotionID());
    o.put("duration", e.getDuration());
    o.put("amplifier", e.getAmplifier());
    o.put("ambient", e.getIsAmbient());
    o.put("show_particles", e.getIsShowParticles());
    return o;
  }
  static List<Object> state(MovementOracle.Player p) {
    List<Object> out = new ArrayList<Object>();
    for (PotionEffect e : p.getActivePotionEffects()) out.add(effect(e));
    return out;
  }
  static void record(
      List<Object> steps, MovementOracle.Player p, String action, PotionEffect added) {
    Map<String, Object> o = new LinkedHashMap<String, Object>();
    o.put("action", action);
    if (added != null)
      o.put("effect", effect(added));
    o.put("effects", state(p));
    o.put("health", p.getHealth());
    NBTTagCompound food = new NBTTagCompound();
    p.getFoodStats().writeNBT(food);
    o.put("exhaustion", food.getFloat("foodExhaustionLevel"));
    steps.add(o);
  }
  public static void main(String[] args) throws Exception {
    PrintStream output = System.out;
    System.setOut(System.err);
    Bootstrap.register();
    Map<String, Object> result = new LinkedHashMap<String, Object>();
    List<Object> packets = new ArrayList<Object>();
    for (int id : new int[] {1, 2, 8, 15, 23})
      for (int amp : new int[] {-1, 0, 1, 127, 128, 255})
        for (int duration : new int[] {-1, 0, 1, 32767, 32768, 1000000})
          for (boolean show : new boolean[] {false, true}) {
            PotionEffect e = new PotionEffect(id, duration, amp, true, show);
            S1DPacketEntityEffect add = new S1DPacketEntityEffect(189, e);
            PacketBuffer b = new PacketBuffer(Unpooled.buffer());
            add.writePacketData(b);
            byte[] raw = bytes(b);
            S1DPacketEntityEffect read = new S1DPacketEntityEffect();
            read.readPacketData(new PacketBuffer(Unpooled.wrappedBuffer(raw)));
            Map<String, Object> row = new LinkedHashMap<String, Object>();
            row.put("add", MovementAttributesOracle.hex(raw));
            row.put("entity_id", read.getEntityId());
            row.put("effect_id", read.getEffectId());
            row.put("amplifier", read.getAmplifier());
            row.put("duration", read.getDuration());
            row.put("show_particles", read.func_179707_f());
            row.put("duration_max", read.func_149429_c());
            S1EPacketRemoveEntityEffect remove = new S1EPacketRemoveEntityEffect(189, e);
            b = new PacketBuffer(Unpooled.buffer());
            remove.writePacketData(b);
            row.put("remove", MovementAttributesOracle.hex(bytes(b)));
            packets.add(row);
          }
    result.put("packets", packets);
    List<Object> sequences = new ArrayList<Object>();
    Method tick = EntityLivingBase.class.getDeclaredMethod("updatePotionEffects");
    tick.setAccessible(true);
    for (int id : new int[] {1, 2, 8, 15, 10, 19, 17, 23}) {
      MovementOracle.Player p = MovementAttributesOracle.player();
      p.setHealth(10);
      List<Object> steps = new ArrayList<Object>();
      PotionEffect e = new PotionEffect(id, 2, 0, false, true);
      p.addPotionEffect(e);
      record
      (steps, p, "add", new PotionEffect(e));
      for (int t = 0; t < 5; t++) {
        tick.invoke(p);
        record
        (steps, p, "tick", null);
      }
      e = new PotionEffect(id, 4, 1, false, false);
      p.addPotionEffect(e);
      record
      (steps, p, "add", new PotionEffect(e));
      for (int t = 0; t < 6; t++) {
        tick.invoke(p);
        record
        (steps, p, "tick", null);
      }
      e = new PotionEffect(id, 50, 0, false, true);
      p.addPotionEffect(e);
      record
      (steps, p, "add", new PotionEffect(e));
      p.removePotionEffectClient(id);
      Map<String, Object> row = new LinkedHashMap<String, Object>();
      row.put("id", id);
      record
      (steps, p, "remove", null);
      row.put("steps", steps);
      sequences.add(row);
    }
    result.put("sequences", sequences);
    output.println(new Gson().toJson(result));
  }
}
