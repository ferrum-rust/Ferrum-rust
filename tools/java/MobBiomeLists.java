import com.google.gson.Gson;
import java.lang.reflect.Field;
import java.nio.file.*;
import java.util.*;
import net.minecraft.entity.*;
import net.minecraft.init.Bootstrap;
import net.minecraft.util.WeightedRandom;
import net.minecraft.world.biome.BiomeGenBase;

/** Export the registry used by the installed game's natural-spawn picker. */
public final class MobBiomeLists {
    public static void main(String[] args) throws Exception {
        Bootstrap.register();
        Map<Integer, List<Object>> result = new TreeMap<>();
        Field weight = WeightedRandom.Item.class.getDeclaredField("itemWeight");
        weight.setAccessible(true);
        for (BiomeGenBase biome : BiomeGenBase.getBiomeGenArray()) {
            if (biome == null) continue;
            List<Object> groups = new ArrayList<>();
            for (EnumCreatureType type : new EnumCreatureType[]{EnumCreatureType.MONSTER, EnumCreatureType.CREATURE, EnumCreatureType.AMBIENT, EnumCreatureType.WATER_CREATURE}) {
                List<Object> entries = new ArrayList<>();
                for (BiomeGenBase.SpawnListEntry e : biome.getSpawnableList(type)) {
                    int id = 0;
                    while (id < 256 && EntityList.getClassFromID(id) != e.entityClass) id++;
                    if (id == 256) throw new IllegalStateException(e.entityClass.toString());
                    entries.add(new int[]{id, weight.getInt(e), e.minGroupCount, e.maxGroupCount});
                }
                groups.add(entries);
            }
            result.put(biome.biomeID, groups);
        }
        Files.write(Paths.get(args[0]), new Gson().toJson(result).getBytes("UTF-8"));
    }
}
