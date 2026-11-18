import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import com.google.gson.Gson;
import net.minecraft.block.Block;
import net.minecraft.init.*;
import net.minecraft.profiler.Profiler;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.management.*;
import net.minecraft.entity.Entity;
import net.minecraft.entity.player.*;
import net.minecraft.stats.StatisticsFile;
import net.minecraft.util.AxisAlignedBB;
import com.mojang.authlib.GameProfile;
import net.minecraft.util.BlockPos;
import net.minecraft.world.*;
import net.minecraft.world.chunk.Chunk;
import net.minecraft.world.gen.ChunkProviderServer;
import net.minecraft.world.storage.*;

/** A fresh original WorldServer chooses its spawn. Only the subsequent 625
 * startup load requests are captured without generating that second region. */
public final class WorldSpawnOracle {
    static final class ProbeWorld extends WorldServer {
        final int mode;
        final List<Object> probes = new ArrayList<>();
        ItemInWorldManager spawning;
        long playerSeed;
        boolean seeded;
        List<Integer> top;
        List<Integer> collisions;
        ProbeWorld(MinecraftServer host, WorldInfo info, int mode) {
            super(host, new SaveHandlerMP(), info, 0, new Profiler());
            this.mode = mode;
        }
        @Override public Block getGroundAboveSeaLevel(BlockPos p) {
            Block result = mode == 0 ? super.getGroundAboveSeaLevel(p) : Blocks.stone;
            probes.add(Arrays.asList(p.getX(), p.getZ(), Block.getIdFromBlock(result)));
            return result;
        }
        @Override public BlockPos getSpawnPoint(){
            if(spawning!=null && spawning.thisPlayerMP!=null && !seeded){
                entityRandom(spawning.thisPlayerMP).setSeed(playerSeed);
                seeded=true;
            }
            return super.getSpawnPoint();
        }
        @Override public BlockPos getTopSolidOrLiquidBlock(BlockPos p){
            BlockPos result=super.getTopSolidOrLiquidBlock(p);
            if(seeded)top=Arrays.asList(p.getX(),p.getY(),p.getZ(),result.getY());
            return result;
        }
        @Override public List<AxisAlignedBB> getCollidingBoundingBoxes(Entity e,AxisAlignedBB box){
            List<AxisAlignedBB> result=super.getCollidingBoundingBoxes(e,box);
            if(seeded && e==spawning.thisPlayerMP)collisions.add(result.size());
            return result;
        }
    }
    static Random entityRandom(Entity entity){
        try {Field f=Entity.class.getDeclaredField("rand");f.setAccessible(true);return(Random)f.get(entity);}
        catch(Exception ex){throw new RuntimeException(ex);}
    }
    static List<Object> players(ProbeWorld world,MinecraftServer host,File dir,BlockPos spawn)throws Exception {
        host.setConfigManager(new ServerConfigurationManager(host) {
            @Override public StatisticsFile getPlayerStatsFile(EntityPlayer player){return new StatisticsFile(host,new File(dir,"spawn-probe-stats.json"));}
        });
        Field sky=WorldProvider.class.getDeclaredField("hasNoSky");sky.setAccessible(true);
        List<Object> rows=new ArrayList<>();int counter=0;
        for(double distance:new double[]{0.75,1,2.5,5.2,9.75,1000})for(int mode=0;mode<3;mode++)for(int r=0;r<32;r++){
            world.getWorldBorder().setCenter(spawn.getX(),spawn.getZ());
            world.getWorldBorder().setTransition(distance*2);
            world.getWorldInfo().setGameType(mode==1?WorldSettings.GameType.ADVENTURE:WorldSettings.GameType.SURVIVAL);
            sky.setBoolean(world.provider,mode==2);
            world.spawning=new ItemInWorldManager(world);world.seeded=false;world.playerSeed=189L+r*741937L;world.top=null;world.collisions=new ArrayList<>();
            EntityPlayerMP player=new EntityPlayerMP(host,world,new GameProfile(new UUID(0,++counter),"SpawnOracle"),world.spawning);
            if(!world.seeded)throw new AssertionError("Player Random boundary missed");
            Map<String,Object> row=new LinkedHashMap<>();row.put("seed",Long.toString(world.playerSeed));row.put("mode",mode);row.put("distance",distance);
            row.put("top",world.top==null?Collections.emptyList():world.top);row.put("collisions",world.collisions);
            row.put("position",Arrays.asList(player.posX,player.posY,player.posZ));row.put("rng",Long.toString(RuntimeOracle.randomState(entityRandom(player))));
            rows.add(row);world.seeded=false;world.spawning=null;
        }
        return rows;
    }
    static List<Object> chunkHashes(ChunkProviderServer provider) {
        List<Object> chunks = new ArrayList<>();
        for(Chunk c:provider.func_152380_a()){
            String[] h = RuntimeOracle.hashes(c);
            chunks.add(Arrays.asList(c.xPosition,c.zPosition,c.isTerrainPopulated(),c.isLightPopulated(),h[0],h[1]));
        }
        return chunks;
    }
    public static void main(String[] args) throws Exception {
        Bootstrap.register();
        File dir = new File(args[0]); dir.mkdirs();
        long seed = Long.parseLong(args[1]); int mode = Integer.parseInt(args[2]);
        Constructor<?> constructor = Class.forName("VanillaWorldBridge$Host").getDeclaredConstructor(File.class);
        constructor.setAccessible(true);
        MinecraftServer host = (MinecraftServer)constructor.newInstance(dir);
        WorldSettings settings = new WorldSettings(seed, WorldSettings.GameType.SURVIVAL, true, false, WorldType.DEFAULT);
        ProbeWorld world = new ProbeWorld(host, new WorldInfo(settings, "world-spawn"), mode);
        host.worldServers = new WorldServer[]{world, null, null}; world.init();
        world.rand.setSeed(1234);
        RuntimeOracle.events = new ArrayList<>();
        RuntimeOracle.Generator generator = new RuntimeOracle.Generator(world, seed, true);
        ChunkProviderServer provider = new ChunkProviderServer(world, null, generator);
        world.theChunkProviderServer = provider;
        Field f = World.class.getDeclaredField("chunkProvider"); f.setAccessible(true); f.set(world, provider);
        if(mode == 2) world.provider.getWorldChunkManager().getBiomesToSpawnIn().clear();
        Method choose = WorldServer.class.getDeclaredMethod("createSpawnPosition", WorldSettings.class);
        choose.setAccessible(true); choose.invoke(world, settings);
        BlockPos spawn = world.getSpawnPoint();
        Map<String,Object> row = new LinkedHashMap<>();
        row.put("seed", Long.toString(seed));row.put("mode", mode);
        row.put("spawn", Arrays.asList(spawn.getX(), spawn.getY(), spawn.getZ()));
        row.put("probes", world.probes);
        row.put("chunks",chunkHashes(provider));
        row.put("provider_rand", Long.toString(RuntimeOracle.randomState(generator.random)));
        row.put("world_rand", Long.toString(RuntimeOracle.randomState(world.rand)));
        List<Object> order = new ArrayList<>();
        world.theChunkProviderServer = new ChunkProviderServer(world,null,generator) {
            @Override public Chunk loadChunk(int x,int z){order.add(Arrays.asList(x,z)); return null;}
        };
        Method warmup = MinecraftServer.class.getDeclaredMethod("initialWorldChunkLoad");
        warmup.setAccessible(true); warmup.invoke(host);
        row.put("warmup", order);
        if(args.length > 4 && args[4].equals("full-region")) {
            world.theChunkProviderServer = provider;
            warmup.invoke(host);
            row.put("region", chunkHashes(provider));
            row.put("region_provider_rand", Long.toString(RuntimeOracle.randomState(generator.random)));
            row.put("region_world_rand", Long.toString(RuntimeOracle.randomState(world.rand)));
        }
        if(args.length > 4 && args[4].equals("players")) {
            world.theChunkProviderServer=provider;warmup.invoke(host);
            row.put("players",players(world,host,dir,spawn));
        }
        try(Writer out = new OutputStreamWriter(new FileOutputStream(args[3]),"UTF-8")) {new Gson().toJson(row,out);}
    }
}
