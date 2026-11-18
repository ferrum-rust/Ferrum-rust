import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import java.nio.file.*;
import com.google.gson.Gson;
import net.minecraft.block.Block;
import net.minecraft.block.state.IBlockState;
import net.minecraft.entity.EnumCreatureType;
import net.minecraft.entity.Entity;
import net.minecraft.entity.passive.EntityVillager;
import net.minecraft.inventory.IInventory;
import net.minecraft.item.*;
import net.minecraft.nbt.*;
import net.minecraft.tileentity.*;
import net.minecraft.init.Bootstrap;
import net.minecraft.profiler.Profiler;
import net.minecraft.server.MinecraftServer;
import net.minecraft.util.*;
import net.minecraft.world.*;
import net.minecraft.world.biome.BiomeGenBase;
import net.minecraft.world.chunk.*;
import net.minecraft.world.gen.*;
import net.minecraft.world.storage.*;

/** Actual WorldServer/ChunkProviderServer/ChunkProviderGenerate, with a small
 * deterministic population callback specifically forcing nested block reads. */
public final class RuntimeOracle {
    static List<Object> events;
    static List<Object> watchedWrites;
    static class OracleWorld extends WorldServer {
        OracleWorld(MinecraftServer server,ISaveHandler save,WorldInfo info,int dimension,Profiler profiler){super(server,save,info,dimension,profiler);}
        public boolean setBlockState(BlockPos p,IBlockState state,int flags){
            if(watchedWrites!=null&&p.equals(new BlockPos(-1384,76,1215))){List<String> stack=new ArrayList<String>();for(StackTraceElement e:Thread.currentThread().getStackTrace())if(e.getClassName().startsWith("net.minecraft"))stack.add(e.toString());watchedWrites.add(Arrays.asList(Block.getIdFromBlock(state.getBlock())<<4|state.getBlock().getMetaFromState(state),flags,stack));}
            return super.setBlockState(p,state,flags);
        }
    }
    static long randomState(Random random) {
        try {Field f=Random.class.getDeclaredField("seed");f.setAccessible(true);return ((java.util.concurrent.atomic.AtomicLong)f.get(random)).get();}
        catch(Exception e){throw new RuntimeException(e);}
    }
    static class TracedRandom extends Random {
        List<Object> trace=new ArrayList<Object>();
        public synchronized void setSeed(long seed){super.setSeed(seed);if(trace!=null)trace.add(Arrays.asList("seed",Long.toString(seed)));}
        protected int next(int bits){int value=super.next(bits);if(trace!=null){List<String> callers=new ArrayList<String>();for(StackTraceElement e:Thread.currentThread().getStackTrace())if(e.getClassName().startsWith("net.minecraft"))callers.add(e.getClassName()+"."+e.getMethodName()+":"+e.getLineNumber());trace.add(Arrays.asList(bits,value,Long.toString(randomState(this)),callers));}return value;}
    }
    static void event(String name,int x,int z){events.add(Arrays.asList(name,x,z));}
    static class Generator implements IChunkProvider {
        final WorldServer world;final ChunkProviderGenerate terrain;final Random random;final long seed;final boolean full;
        Generator(WorldServer w,long seed,boolean full)throws Exception {world=w;this.seed=seed;this.full=full;terrain=new ChunkProviderGenerate(w,seed,full,"");Field f=ChunkProviderGenerate.class.getDeclaredField("rand");f.setAccessible(true);random=(Random)f.get(terrain);}
        public boolean chunkExists(int x,int z){return true;}
        public Chunk provideChunk(int x,int z){event("generate",x,z);return terrain.provideChunk(x,z);}
        public Chunk provideChunk(BlockPos p){return provideChunk(p.getX()>>4,p.getZ()>>4);}
        public void populate(IChunkProvider p,int x,int z){
            event("populate",x,z);if(full){terrain.populate(p,x,z);return;}random.setSeed(seed);long a=random.nextLong()/2*2+1,b=random.nextLong()/2*2+1;random.setSeed((long)x*a+(long)z*b^seed);
            event("before:"+random.nextInt(1000)+":"+world.rand.nextInt(17),x,z);
            if(x==0&&z==0)world.getBlockState(new BlockPos(32,70,0));
            if(x==1&&z==0){world.getBlockState(new BlockPos(48,70,0));world.getBlockState(new BlockPos(48,70,16));}
            world.setBlockState(new BlockPos(x*16+8,80,z*16+8),Block.getBlockById(89).getDefaultState(),2);
            event("after:"+random.nextInt(1000)+":"+world.rand.nextInt(17),x,z);
        }
        public boolean populateChunk(IChunkProvider p,Chunk c,int x,int z){return full&&terrain.populateChunk(p,c,x,z);}
        public void recreateStructures(Chunk c,int x,int z){}
        public boolean saveChunks(boolean all,IProgressUpdate p){return true;}
        public boolean unloadQueuedChunks(){return false;}
        public boolean canSave(){return true;}
        public String makeString(){return "Runtime callback oracle";}
        public List<BiomeGenBase.SpawnListEntry> getPossibleCreatures(EnumCreatureType t,BlockPos p){return Collections.emptyList();}
        public BlockPos getStrongholdGen(World w,String n,BlockPos p){return null;}
        public int getLoadedChunkCount(){return 0;}
        public void saveExtraData(){}
    }
    static long byteHash(long h,int b){return (h^(b&255))*0x100000001b3L;}
    static List<Object> slots(IInventory inv){
        List<Object> out=new ArrayList<Object>();for(int i=0;i<inv.getSizeInventory();i++){
            ItemStack s=inv.getStackInSlot(i);if(s==null){out.add(null);continue;}
            Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("id",Item.getIdFromItem(s.getItem()));row.put("meta",s.getMetadata());row.put("count",s.stackSize);
            List<int[]> enchants=new ArrayList<int[]>();if(s.hasTagCompound()){NBTTagList list=s.getTagCompound().getTagList("StoredEnchantments",10);for(int j=0;j<list.tagCount();j++){NBTTagCompound e=list.getCompoundTagAt(j);enchants.add(new int[]{e.getShort("id"),e.getShort("lvl")});}}
            row.put("enchants",enchants);out.add(row);
        }return out;
    }
    static String[] hashes(Chunk c){long blocks=0xcbf29ce484222325L,light=blocks;
        for(int y=0;y<256;y++)for(int z=0;z<16;z++)for(int x=0;x<16;x++){
            BlockPos p=new BlockPos(c.xPosition*16+x,y,c.zPosition*16+z);IBlockState s=c.getBlockState(p);int value=Block.getIdFromBlock(s.getBlock())<<4|s.getBlock().getMetaFromState(s);
            blocks=byteHash(byteHash(blocks,value),value>>8);light=byteHash(byteHash(light,c.getLightFor(EnumSkyBlock.SKY,p)),c.getLightFor(EnumSkyBlock.BLOCK,p));
        }return new String[]{Long.toUnsignedString(blocks,16),Long.toUnsignedString(light,16)};
    }
    public static void main(String[]args)throws Exception {
        PrintStream output=System.out;System.setOut(System.err);Bootstrap.register();File dir=new File(args[0]);dir.mkdirs();Constructor<?> con=Class.forName("VanillaWorldBridge$Host").getDeclaredConstructor(File.class);con.setAccessible(true);MinecraftServer host=(MinecraftServer)con.newInstance(dir);List<Object> cases=new ArrayList<Object>();
        boolean full=args.length>1&&args[1].equals("full");
        boolean custom=args.length>2;int originX=custom?Integer.parseInt(args[3]):0,originZ=custom?Integer.parseInt(args[4]):0,order=custom?Integer.parseInt(args[5]):0,width=custom?Integer.parseInt(args[6]):0;
        for(long seed:custom?new long[]{Long.parseLong(args[2])}:new long[]{0,-1,189,Long.MIN_VALUE})for(int reverse=custom?order:0;reverse<(custom?order+1:2);reverse++){
            watchedWrites=args.length>7&&args[7].equals("writes")?new ArrayList<Object>():null;
            WorldServer world=new OracleWorld(host,new SaveHandlerMP(),new WorldInfo(new WorldSettings(seed,WorldSettings.GameType.SURVIVAL,false,false,WorldType.DEFAULT),"runtime-oracle"),0,new Profiler());host.worldServers=new WorldServer[]{world,null,null};world.init();world.rand.setSeed(1234);events=new ArrayList<Object>();
            if(args.length>7&&args[7].equals("random")){Field randomField=World.class.getDeclaredField("rand");randomField.setAccessible(true);TracedRandom trace=new TracedRandom();trace.setSeed(1234);randomField.set(world,trace);}
            Generator generator=new Generator(world,seed,full);ChunkProviderServer provider=new ChunkProviderServer(world,null,generator);world.theChunkProviderServer=provider;Field f=World.class.getDeclaredField("chunkProvider");f.setAccessible(true);f.set(world,provider);
            List<int[]> operations=new ArrayList<int[]>(Arrays.asList(new int[][]{{0,0},{0,1},{1,0},{2,1},{1,1}}));if(custom){operations.clear();for(int x=originX-1;x<originX-1+width;x++)for(int z=originZ-1;z<originZ-1+width;z++)operations.add(new int[]{x,z});}if(reverse==1)Collections.reverse(operations);if(reverse==2)Collections.shuffle(operations,new Random(189));
            String prefix=seed+"_"+originX+"_"+originZ+"_"+reverse;
            List<Object> checkpoints=new ArrayList<Object>();for(int[]p:operations){provider.provideChunk(p[0],p[1]);checkpoints.add(Arrays.asList(p[0],p[1],Long.toString(randomState(generator.random)),Long.toString(randomState(world.rand))));if(world.rand instanceof TracedRandom)((TracedRandom)world.rand).trace.add(Arrays.asList("checkpoint",p[0],p[1]));}
            // These nonloading queries must not add chunks to either provider.
            List<Integer> queries=Arrays.asList(world.getHeight(new BlockPos(1600,0,1600)).getY(),world.getLightFor(EnumSkyBlock.SKY,new BlockPos(1600,70,1600)),world.getLightFor(EnumSkyBlock.BLOCK,new BlockPos(1600,70,1600)),world.getBiomeGenForCoords(new BlockPos(1600,70,1600)).biomeID);
            List<Object> chunks=new ArrayList<Object>();for(Chunk c:provider.func_152380_a()){String[] h=hashes(c);chunks.add(Arrays.asList(c.xPosition,c.zPosition,c.isTerrainPopulated(),c.isLightPopulated(),h[0],h[1]));
                if(full){byte[] raw=new byte[131072];int index=0;for(int y=0;y<256;y++)for(int z=0;z<16;z++)for(int x=0;x<16;x++){IBlockState s=c.getBlockState(new BlockPos(c.xPosition*16+x,y,c.zPosition*16+z));int v=Block.getIdFromBlock(s.getBlock())<<4|s.getBlock().getMetaFromState(s);raw[index++]=(byte)v;raw[index++]=(byte)(v>>8);}Path folder=dir.toPath().resolve("blocks");Files.createDirectories(folder);Files.write(folder.resolve(prefix+"_"+c.xPosition+"_"+c.zPosition+".bin"),raw);}
            }
            List<Object> villagers=new ArrayList<Object>(),containers=new ArrayList<Object>(),spawners=new ArrayList<Object>();
            for(Entity e:world.loadedEntityList)if(e instanceof EntityVillager)villagers.add(Arrays.asList((int)Math.floor(e.posX),(int)Math.floor(e.posY),(int)Math.floor(e.posZ),((EntityVillager)e).getProfession()));
            for(Chunk c:provider.func_152380_a())for(Map.Entry<BlockPos,TileEntity> entry:c.getTileEntityMap().entrySet()){
                BlockPos p=entry.getKey();TileEntity t=entry.getValue();int[] pos={p.getX(),p.getY(),p.getZ()};
                if(t instanceof TileEntityChest||t instanceof TileEntityDispenser)containers.add(Arrays.asList(pos,Block.getIdFromBlock(world.getBlockState(p).getBlock()),slots((IInventory)t)));
                if(t instanceof TileEntityMobSpawner){NBTTagCompound n=new NBTTagCompound();t.writeToNBT(n);spawners.add(Arrays.asList(pos,n.getString("EntityId")));}
            }
            Map<String,Object> row=new LinkedHashMap<String,Object>();if(watchedWrites!=null)row.put("watched_writes",watchedWrites);row.put("villagers",villagers);row.put("containers",containers);row.put("spawners",spawners);row.put("seed",Long.toString(seed));row.put("raw_prefix",prefix);row.put("operations",operations);row.put("checkpoints",checkpoints);if(world.rand instanceof TracedRandom)row.put("random_trace",((TracedRandom)world.rand).trace);row.put("events",events);row.put("chunks",chunks);row.put("queries",queries);row.put("provider_next_long",Long.toString(generator.random.nextLong()));row.put("world_next_long",Long.toString(world.rand.nextLong()));cases.add(row);
        }
        Map<String,Object> result=new LinkedHashMap<String,Object>();result.put("cases",cases);output.println(new Gson().toJson(result));
    }
}
