import com.google.gson.*;
import java.io.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import net.minecraft.block.Block;
import net.minecraft.block.state.IBlockState;
import net.minecraft.init.Bootstrap;
import net.minecraft.world.*;
import net.minecraft.world.biome.*;
import net.minecraft.world.chunk.*;
import net.minecraft.world.gen.*;
import net.minecraft.world.storage.WorldInfo;

/** Export canonical metadata and the actual debug-grid order, and hash provideChunk results. */
public final class WorldTypesOracle {
    private static int packed(IBlockState state) {
        Block block = state.getBlock();
        return (Block.getIdFromBlock(block) << 4) | block.getMetaFromState(state);
    }
    private static void write(Path file,Object value) throws Exception {
        Files.write(file,(new Gson().toJson(value)+"\n").getBytes(StandardCharsets.UTF_8));
    }
    private static void registry(Path dir) throws Exception {
        Map<String,Object> rows = new TreeMap<>();
        try (OutputStream out = Files.newOutputStream(dir.resolve("world_debug_states.bin"))) {
            for (Block block : Block.blockRegistry) {
                int id = Block.getIdFromBlock(block);
                List<Integer> metadata = new ArrayList<>();
                for (int i=0; i<16; i++) {
                    try { metadata.add(block.getMetaFromState(block.getStateFromMeta(i))); }
                    catch (RuntimeException e) { metadata.add(-1); }
                }
                rows.put(Block.blockRegistry.getNameForObject(block).getResourcePath(),Arrays.asList(id,metadata));
                for (IBlockState state : block.getBlockState().getValidStates()) {
                    int value = packed(state); out.write(value); out.write(value >>> 8);
                }
            }
        }
        write(dir.resolve("world_flat_blocks.json"),rows);
    }
    private static Map<String,Object> sample(String type, String options, int x, int z) throws Exception {
        long seed = 189;
        WorldType kind = WorldType.parseWorldType(type);
        boolean flat = kind == WorldType.FLAT, debug = kind == WorldType.DEBUG_WORLD;
        WorldChunkManager manager = flat || debug ? new WorldChunkManagerHell(
            BiomeGenBase.getBiomeFromBiomeList(flat ? FlatGeneratorInfo.createFlatGeneratorFromString(options).getBiome() : 1,BiomeGenBase.ocean), 0.5f)
            : new WorldChunkManager(seed,kind,options);
        World w = TerrainOracle.world(seed,manager);
        TerrainOracle.set(w,World.class,"worldAccesses",Collections.emptyList());
        TerrainOracle.set(w,World.class,"worldInfo",new WorldInfo(new WorldSettings(seed,WorldSettings.GameType.SURVIVAL,false,false,kind),"Oracle"));
        IChunkProvider provider = flat ? new ChunkProviderFlat(w,seed,false,options) : debug ? new ChunkProviderDebug(w)
            : new ChunkProviderGenerate(w,seed,false,options);
        Chunk chunk = provider.provideChunk(x,z);
        byte[] blocks = new byte[65536*2];
        int offset=0;
        for (int y=0; y<256; y++) for (int k=0; k<16; k++) for (int j=0; j<16; j++) {
            int p = packed(chunk.getBlockState(new net.minecraft.util.BlockPos(x*16+j,y,z*16+k)));
            blocks[offset++] = (byte)p; blocks[offset++] = (byte)(p >>> 8);
        }
        Map<String,Object> row = new LinkedHashMap<>();
        row.put("type",type);row.put("options",options);row.put("pos",Arrays.asList(x,z));
        row.put("blocks",TerrainOracle.fnv(blocks));row.put("biomes",TerrainOracle.fnv(chunk.getBiomeArray()));
        return row;
    }
    public static void main(String[] args) throws Exception {
        Bootstrap.register();
        Path out = Paths.get(args[0]);
        registry(out);
        List<Object> rows = new ArrayList<>();
        JsonObject presets = new JsonParser().parse(new String(Files.readAllBytes(Paths.get(args[1])),StandardCharsets.UTF_8)).getAsJsonObject();
        for (JsonElement preset : presets.getAsJsonArray("flat")) {
            rows.add(sample("flat",preset.getAsJsonObject().get("settings").getAsString(),0,0));
        }
        for (String options : new String[]{
            "", "2;7,4x35:14,3:2;37;village", "3;minecraft:bedrock,2*minecraft:air,wool:8,leaves:13;162;",
            "3;257*minecraft:dirt,4*minecraft:grass;1;", "3;no_such_block;1;", "3;air;245;"
        }) rows.add(sample("flat",options,-2,3));
        for (String type : new String[]{"amplified","largeBiomes","debug_all_block_states"}) {
            for (int x : new int[]{0,4,-9}) rows.add(sample(type,"",x,x+1));
        }
        for (JsonElement preset : presets.getAsJsonArray("customized")) {
            rows.add(sample("customized",preset.getAsJsonObject().get("settings").getAsString(),1,-3));
        }
        rows.add(sample("customized","{\"fixedBiome\":10,\"biomeSize\":1,\"riverSize\":2,\"seaLevel\":93,\"useCaves\":false,\"useRavines\":false}",-1,2));
        rows.add(sample("customized","{\"fixedBiome\":2,\"seaLevel\":97,\"useLavaOceans\":true}",5,-7));
        write(out.resolve("world-terrain-hashes.json"),rows);
    }
}
