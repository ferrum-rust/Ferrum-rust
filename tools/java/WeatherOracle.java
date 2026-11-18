import com.google.gson.Gson;
import java.io.*;
import java.lang.reflect.Field;
import java.util.*;
import java.util.concurrent.atomic.AtomicLong;
import java.util.zip.CRC32;
import net.minecraft.init.Bootstrap;
import net.minecraft.profiler.Profiler;
import net.minecraft.world.*;
import net.minecraft.world.chunk.IChunkProvider;
import net.minecraft.world.storage.*;

/** Exercise the owned 1.8.9 World methods without a client, window or save files. */
public class WeatherOracle {
    static class TestWorld extends World {
        TestWorld(WorldInfo info) {
            super(null, info, new WorldProviderSurface(), new Profiler(), false);
            calculateInitialWeather();
        }
        protected IChunkProvider createChunkProvider() { return null; }
        protected int getRenderDistanceChunks() { return 8; }
        void advance() { updateWeather(); }
    }
    static void word(CRC32 checksum, int value) {
        for (int shift = 0; shift < 32; shift += 8) checksum.update(value >>> shift);
    }
    public static void main(String[] args) throws Exception {
        Bootstrap.register();
        WorldSettings settings = new WorldSettings(189, WorldSettings.GameType.SURVIVAL, false, false, WorldType.DEFAULT);
        Field randomState = Random.class.getDeclaredField("seed");
        randomState.setAccessible(true);
        List<Object> cases = new ArrayList<>();
        for (int i = 0; i < 64; i++) {
            int seed = i * 97531 - 17777;
            int clear = i % 4 == 0 ? i : 0;
            int rain = new int[] {-3, 0, 1, 2, 30, 80, 120, 400}[i % 8];
            int thunder = new int[] {20, 1, 0, 106}[(i / 4) % 4];
            WorldInfo info = new WorldInfo(settings, "weather");
            info.setCleanWeatherTime(clear);
            info.setRainTime(rain);
            info.setThunderTime(thunder);
            info.setRaining((i & 16) != 0);
            info.setThundering((i & 32) != 0);
            TestWorld world = new TestWorld(info);
            world.rand.setSeed(seed);
            CRC32 checksum = new CRC32();
            for (int tick = 0; tick <= 256; tick++) {
                if (tick != 0) world.advance();
                word(checksum, info.getCleanWeatherTime());
                word(checksum, info.getRainTime());
                word(checksum, info.getThunderTime());
                word(checksum, info.isRaining() ? 1 : 0);
                word(checksum, info.isThundering() ? 1 : 0);
                long rng = ((AtomicLong) randomState.get(world.rand)).get();
                word(checksum, (int) rng);
                word(checksum, (int) (rng >>> 32));
                for (float partial : new float[] {0F, 0.5F, 1F}) {
                    word(checksum, Float.floatToIntBits(world.getRainStrength(partial)));
                    word(checksum, Float.floatToIntBits(world.getThunderStrength(partial)));
                }
            }
            cases.add(Arrays.asList(seed, clear, rain, thunder, (i & 16) != 0, (i & 32) != 0, checksum.getValue()));
        }
        try (Writer output = new FileWriter(args[0])) { new Gson().toJson(cases, output); }
    }
}
