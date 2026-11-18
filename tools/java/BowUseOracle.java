import com.google.gson.Gson;
import java.lang.reflect.Field;
import java.util.*;
import sun.misc.Unsafe;
import net.minecraft.enchantment.Enchantment;
import com.mojang.authlib.GameProfile;
import net.minecraft.client.entity.AbstractClientPlayer;
import net.minecraft.entity.Entity;
import net.minecraft.entity.projectile.EntityArrow;
import net.minecraft.init.*;
import net.minecraft.item.*;
import net.minecraft.stats.StatBase;
import net.minecraft.world.*;
import net.minecraft.world.storage.WorldInfo;

/** Exercises the installed ItemBow bytecode with a bounded world callback adapter. */
public final class BowUseOracle {
    static final class Player extends FoodOracle.Player {
        int used;
        Player(World world) { super(world); }
        @Override public void renderBrokenItemStack(ItemStack stack) {}
        @Override public void addStat(StatBase stat, int count) {
            if (stat.statId.equals("stat.useItem.minecraft.bow")) used += count;
        }
    }
    static final class OracleWorld extends FoodOracle.OracleWorld {
        EntityArrow arrow;
        int sounds;
        @Override public boolean spawnEntityInWorld(Entity entity) {
            if (entity instanceof EntityArrow) arrow = (EntityArrow) entity;
            return true;
        }
        @Override public void playSoundAtEntity(Entity entity, String sound, float volume, float pitch) {
            if (sound.equals("random.bow")) sounds++;
        }
    }
    static Player player(boolean creative) throws Exception {
        Field unsafe = Unsafe.class.getDeclaredField("theUnsafe");
        unsafe.setAccessible(true);
        OracleWorld world = (OracleWorld)((Unsafe)unsafe.get(null)).allocateInstance(OracleWorld.class);
        world.rules = new GameRules();
        world.difficulty = EnumDifficulty.NORMAL;
        FoodOracle.set(world, World.class, "rand", new Random(189L));
        FoodOracle.set(world, World.class, "provider", new WorldProviderSurface());
        FoodOracle.set(world, World.class, "worldInfo", new WorldInfo(
            new WorldSettings(189L, WorldSettings.GameType.SURVIVAL, false, false, WorldType.DEFAULT),
            "BowUseOracle"));
        Player player = new Player(world);
        player.capabilities.isCreativeMode = creative;
        return player;
    }
    static Object field(Object instance, Class<?> type, String key) throws Exception {
        Field field = type.getDeclaredField(key);
        field.setAccessible(true);
        return field.get(instance);
    }
    static Object run(boolean creative, int ammo, int charge, int[] enchantments,
                      int damage, boolean removeAmmo) throws Exception {
        Player player = player(creative);
        ItemStack bow = new ItemStack(Items.bow, 1, damage);
        int[] keys = {48, 49, 50, 51};
        for (int i = 0; i < keys.length; i++) {
            if (enchantments[i] != 0) bow.addEnchantment(Enchantment.getEnchantmentById(keys[i]), enchantments[i]);
        }
        player.inventory.mainInventory[0] = bow;
        if (ammo > 0) player.inventory.mainInventory[1] = new ItemStack(Items.arrow, ammo);
        bow.useItemRightClick(player.worldObj, player);
        boolean started = player.isUsingItem();
        AbstractClientPlayer view = new AbstractClientPlayer(player.worldObj, new GameProfile(new UUID(0,190),"BowView")) {
            @Override public boolean isSpectator() { return false; }
        };
        if (started) view.setItemInUse(bow, 72000-charge);
        float fov = view.getFovModifier();
        if (removeAmmo) player.inventory.mainInventory[1] = null;
        if (started) {
            player.setItemInUse(bow, 72000 - charge);
            // EntityPlayer.setItemInUse preserves its count for an identical stack.
            FoodOracle.set(player, net.minecraft.entity.player.EntityPlayer.class, "itemInUseCount", 72000-charge);
            player.stopUsingItem();
        }
        OracleWorld world = (OracleWorld)player.worldObj;
        EntityArrow arrow = world.arrow;
        Map<String,Object> out = new LinkedHashMap<>();
        out.put("creative",creative); out.put("ammo",ammo); out.put("charge",charge);
        out.put("enchants",enchantments); out.put("initial_damage",damage); out.put("remove_ammo",removeAmmo);
        out.put("started",started); out.put("fired",arrow != null);
        out.put("fov", fov);
        out.put("remaining_ammo",player.inventory.mainInventory[1] == null ? 0 : player.inventory.mainInventory[1].stackSize);
        out.put("bow_count",bow.stackSize); out.put("bow_damage",bow.getItemDamage());
        out.put("used_stat",player.used); out.put("sounds",world.sounds);
        if (arrow != null) {
            out.put("damage",arrow.getDamage()); out.put("critical",arrow.getIsCritical());
            out.put("punch",field(arrow,EntityArrow.class,"knockbackStrength"));
            out.put("fire",field(arrow,Entity.class,"fire")); out.put("pickup",arrow.canBePickedUp);
        }
        return out;
    }
    public static void main(String[] args) throws Exception {
        Bootstrap.register();
        List<Object> cases = new ArrayList<>();
        for (boolean creative : new boolean[]{false,true}) {
            for (int ammo : new int[]{0,1,3}) {
                for (int charge : new int[]{0,1,2,3,5,10,19,20,100}) {
                    for (int[] enchants : new int[][]{{0,0,0,0},{3,2,1,0},{5,1,1,1}}) {
                        cases.add(run(creative,ammo,charge,enchants,0,false));
                    }
                }
            }
        }
        for (int infinity : new int[]{0,1}) {
            cases.add(run(false,1,20,new int[]{0,0,0,infinity},384,false));
            cases.add(run(false,1,20,new int[]{0,0,0,infinity},0,true));
        }
        System.out.println(new Gson().toJson(cases));
    }
}
