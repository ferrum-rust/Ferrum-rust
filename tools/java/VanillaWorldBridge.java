import java.io.*;
import java.net.Proxy;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.util.*;
import net.minecraft.block.Block;
import net.minecraft.init.Bootstrap;
import net.minecraft.profiler.Profiler;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.management.ServerConfigurationManager;
import net.minecraft.util.BlockPos;
import net.minecraft.world.*;
import net.minecraft.world.chunk.Chunk;
import net.minecraft.world.chunk.storage.AnvilSaveHandler;
import net.minecraft.world.chunk.storage.ExtendedBlockStorage;
import net.minecraft.world.gen.ChunkProviderGenerate;
import net.minecraft.world.storage.WorldInfo;

/** Thin host for the user's original 1.8.9 bytecode, not a rewritten generator.
 * No network endpoint, server run loop, login, EULA file or renderer is started.
 * stdout is a line protocol; vanilla logs are kept on stderr.
 */
public final class VanillaWorldBridge {
    private static final PrintStream PROTOCOL = System.out;
    private final Host host;
    private final WorldServer world;
    private final AnvilSaveHandler save;
    private final File directory;

    private static final class Host extends MinecraftServer {
        private final File root;
        Host(File root) {
            // This constructor deliberately leaves NetworkSystem null.
            super(Proxy.NO_PROXY, new File(root, "bridge-usercache.json"));
            this.root = root;
            ServerConfigurationManager manager = new ServerConfigurationManager(this) {};
            manager.setViewDistance(8);
            setConfigManager(manager);
            setBuildLimit(256);
            setCanSpawnAnimals(true);
            setCanSpawnNPCs(true);
        }
        void initializeSpawnChunks() { initialWorldChunkLoad(); }
        protected boolean startServer() { throw new UnsupportedOperationException("No server loop in generation bridge"); }
        public boolean canStructuresSpawn() { return true; }
        public WorldSettings.GameType getGameType() { return WorldSettings.GameType.SURVIVAL; }
        public EnumDifficulty getDifficulty() { return EnumDifficulty.NORMAL; }
        public boolean isHardcore() { return false; }
        public int getOpPermissionLevel() { return 0; }
        public boolean shouldBroadcastRconToOps() { return false; }
        public boolean shouldBroadcastConsoleToOps() { return false; }
        public boolean isDedicatedServer() { return false; }
        public boolean shouldUseNativeTransport() { return false; }
        public boolean isCommandBlockEnabled() { return false; }
        public String shareToLAN(WorldSettings.GameType type, boolean commands) { throw new UnsupportedOperationException("No networking"); }
        public File getDataDirectory() { return root; }
    }

    VanillaWorldBridge(File directory, long seed) throws Exception {
        this.directory = directory.getCanonicalFile();
        if (!directory.isDirectory() && !directory.mkdirs()) throw new IOException("Cannot create world directory");
        this.host = new Host(directory);
        this.save = new AnvilSaveHandler(directory.getParentFile(), directory.getName(), true);
        WorldInfo info = save.loadWorldInfo();
        WorldSettings settings;
        boolean fresh = info == null;
        if (fresh) {
            settings = new WorldSettings(seed, WorldSettings.GameType.SURVIVAL, true, false, WorldType.DEFAULT);
            info = new WorldInfo(settings, "Ferrum vanilla 1.8.9 " + seed);
        } else {
            if (info.getSeed() != seed) throw new IOException("Cached world's seed does not match requested seed");
            if (info.getTerrainType() != WorldType.DEFAULT) throw new IOException("Only original DEFAULT terrain is supported by this bridge");
            settings = new WorldSettings(info);
        }
        world = new WorldServer(host, save, info, 0, new Profiler());
        host.worldServers = new WorldServer[] {world, null, null};
        world.init();
        // Calls original biome spawn search and original canCoordinateBeSpawn.
        world.initialize(settings);
        // Exactly the original server's 25*25 spawn preload and ordering. Run
        // this on fresh worlds; reloads retain the persisted population state.
        if (fresh) host.initializeSpawnChunks();
        persist();
    }

    private void persist() throws Exception {
        world.saveAllChunks(true, null);
        world.saveChunkData();
        save.flush();
    }

    private static int coordinate(String value) {
        int n = Integer.parseInt(value);
        if (n < -1874998 || n > 1874998) throw new IllegalArgumentException("Chunk coordinate outside bridge bounds");
        return n;
    }

    private String load(String args) throws Exception {
        TreeSet<Long> wanted = new TreeSet<Long>(new Comparator<Long>() {
            public int compare(Long a, Long b) {
                int x = Integer.compare((int)(a >> 32), (int)(b >> 32));
                return x != 0 ? x : Integer.compare(a.intValue(), b.intValue());
            }
        });
        List<int[]> requested = new ArrayList<int[]>();
        if (!args.trim().isEmpty()) for (String item : args.trim().split(" +")) {
            String[] parts = item.split(",", -1);
            if (parts.length != 2) throw new IllegalArgumentException("LOAD expects x,z pairs");
            int x = coordinate(parts[0]), z = coordinate(parts[1]);
            requested.add(new int[] {x,z});
            if (requested.size() > 4096) throw new IllegalArgumentException("Too many requested chunks");
            // Original populateChunk waits for a southeast 2*2 neighborhood.
            // Include the northwest decorators which can write into the target.
            for (int dx=-1; dx<=1; ++dx) for (int dz=-1; dz<=1; ++dz)
                wanted.add(((long)(x+dx) << 32) | ((z+dz) & 0xffffffffL));
        }
        for (long p : wanted) world.theChunkProviderServer.loadChunk((int)(p>>32), (int)p);
        for (int[] p : requested) if (!world.theChunkProviderServer.provideChunk(p[0],p[1]).isTerrainPopulated())
            throw new IOException("Original provider did not populate requested chunk " + p[0] + "," + p[1]);
        persist();
        StringBuilder result = new StringBuilder("{\"chunks\":[");
        boolean first = true;
        for (int[] p : requested) {
            if (!first) result.append(','); first=false;
            Chunk c = world.theChunkProviderServer.provideChunk(p[0],p[1]);
            result.append("{\"x\":").append(p[0]).append(",\"z\":").append(p[1]);
            result.append(",\"sha256\":\"").append(hash(c)).append("\",\"populated\":true}");
        }
        result.append("]}");
        // saveAllChunks queues non-spawn columns for unloading. Let vanilla
        // unload them after the files are flushed, bounding a long session.
        int previous;
        do { previous=world.theChunkProviderServer.getLoadedChunkCount(); world.theChunkProviderServer.unloadQueuedChunks(); }
        while (world.theChunkProviderServer.getLoadedChunkCount() < previous);
        save.flush();
        return result.toString();
    }

    private static String hash(Chunk chunk) throws Exception {
        MessageDigest digest = MessageDigest.getInstance("SHA-256");
        ExtendedBlockStorage[] sections = chunk.getBlockStorageArray();
        for (int s=0;s<16;s++) {
            char[] blocks = sections[s] == null ? new char[4096] : sections[s].getData();
            for (char state : blocks) {digest.update((byte)state);digest.update((byte)(state>>8));}
        }
        digest.update(chunk.getBiomeArray());
        StringBuilder hex = new StringBuilder();
        for (byte b : digest.digest()) hex.append(String.format(Locale.ROOT,"%02x",b&255));
        return hex.toString();
    }

    private String info() {
        BlockPos spawn = world.getSpawnPoint();
        return "{\"version\":1,\"minecraft\":\"1.8.9\",\"seed\":\""+world.getSeed()+"\",\"spawn\":["+spawn.getX()+","+spawn.getY()+","+spawn.getZ()+"],\"loaded\":"+world.theChunkProviderServer.getLoadedChunkCount()+"}";
    }

    private String base(String args) throws Exception {
        String[] p=args.trim().split(",");
        if(p.length!=2)throw new IllegalArgumentException("BASE expects x,z");
        int x=coordinate(p[0]),z=coordinate(p[1]);
        // Independent direct original generator instance, before decoration.
        ChunkProviderGenerate generator=new ChunkProviderGenerate(world,world.getSeed(),true,world.getWorldInfo().getGeneratorOptions());
        Chunk c=generator.provideChunk(x,z);
        return "{\"x\":"+x+",\"z\":"+z+",\"base_sha256\":\""+hash(c)+"\"}";
    }

    public static void main(String[] args) {
        try {
            if (args.length!=2) throw new IllegalArgumentException("Usage: VanillaWorldBridge WORLD_DIRECTORY SIGNED_LONG_SEED");
            System.setOut(System.err);
            Bootstrap.register();
            VanillaWorldBridge bridge=new VanillaWorldBridge(new File(args[0]),Long.parseLong(args[1]));
            PROTOCOL.println("READY "+bridge.info());PROTOCOL.flush();
            BufferedReader input=new BufferedReader(new InputStreamReader(System.in,StandardCharsets.UTF_8));
            String line;
            while((line=input.readLine())!=null) {
                if(line.length()>131072)throw new IOException("Command line too large");
                if(line.equals("QUIT"))break;
                if(line.equals("INFO"))PROTOCOL.println("OK "+bridge.info());
                else if(line.startsWith("LOAD "))PROTOCOL.println("OK "+bridge.load(line.substring(5)));
                else if(line.startsWith("BASE "))PROTOCOL.println("OK "+bridge.base(line.substring(5)));
                else throw new IOException("Unknown bridge command");
                PROTOCOL.flush();
            }
            bridge.persist();
            System.exit(0); // Vanilla's file-IO thread is intentionally non-daemon.
        }catch(Throwable error){error.printStackTrace(System.err);PROTOCOL.println("ERROR "+error.toString().replace('\n',' '));PROTOCOL.flush();System.exit(1);}
    }
}
