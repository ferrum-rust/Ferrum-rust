import java.io.*;
import java.util.*;
import com.google.gson.Gson;
import net.minecraft.block.Block;
import net.minecraft.block.state.IBlockState;
import net.minecraft.entity.Entity;
import net.minecraft.entity.monster.EntityGuardian;
import net.minecraft.init.Bootstrap;
import net.minecraft.nbt.NBTTagCompound;
import net.minecraft.profiler.Profiler;
import net.minecraft.util.BlockPos;
import net.minecraft.world.*;
import net.minecraft.world.biome.*;
import net.minecraft.world.storage.WorldInfo;
import net.minecraft.world.chunk.IChunkProvider;
import net.minecraft.world.gen.structure.*;

/** Original full monument template execution and NBT-reload state transitions. */
public final class MonumentPopulationOracle {
    static final Comparator<BlockPos> ORDER=(a,b)->{int c=Integer.compare(a.getX(),b.getX());if(c!=0)return c;c=Integer.compare(a.getY(),b.getY());return c!=0?c:Integer.compare(a.getZ(),b.getZ());};
    static final class TestWorld extends World {
        final TreeMap<BlockPos,IBlockState> blocks=new TreeMap<BlockPos,IBlockState>(ORDER);
        final List<int[]> elders=new ArrayList<int[]>();final int pattern;
        TestWorld(long seed,int pattern){super(null,new WorldInfo(new WorldSettings(seed,WorldSettings.GameType.SURVIVAL,true,false,WorldType.DEFAULT),"oracle"),new WorldProviderSurface(),new Profiler(),false);provider.registerWorld(this);this.pattern=pattern;setSeaLevel(pattern==2?70:63);}
        protected IChunkProvider createChunkProvider(){return null;}
        protected int getRenderDistanceChunks(){return 8;}
        public IBlockState getBlockState(BlockPos p){IBlockState s=blocks.get(p);if(s!=null)return s;int y=p.getY();int floor=pattern==0?35:31+Math.floorMod(p.getX()*3+p.getZ()*7,13);int id=y<0||y>255?0:y==0?7:y<floor?1:y<getSeaLevel()?9:0;return Block.getBlockById(id).getDefaultState();}
        public boolean setBlockState(BlockPos p,IBlockState s,int flags){if(p.getY()<0||p.getY()>255)return false;blocks.put(new BlockPos(p),s);return true;}
        public DifficultyInstance getDifficultyForLocation(BlockPos p){return new DifficultyInstance(EnumDifficulty.NORMAL,0,0,0);}
        public boolean spawnEntityInWorld(Entity e){if(!(e instanceof EntityGuardian)||!((EntityGuardian)e).isElder())throw new AssertionError(e);EntityGuardian g=(EntityGuardian)e;if(g.getHealth()!=g.getMaxHealth())throw new AssertionError("health");elders.add(new int[]{(int)Math.floor(e.posX),(int)Math.floor(e.posY),(int)Math.floor(e.posZ)});return true;}
        long hash(){long h=-3750763034362895579L;for(Map.Entry<BlockPos,IBlockState> e:blocks.entrySet()){BlockPos p=e.getKey();IBlockState s=e.getValue();for(int n:new int[]{p.getX(),p.getY(),p.getZ(),Block.getIdFromBlock(s.getBlock())<<4|s.getBlock().getMetaFromState(s)})h=MonumentOracle.hash(h,n);}return h;}
    }
    static final class Generator extends StructureOceanMonument {
        StructureStart start(TestWorld world,int x,int z){worldObj=world;return getStructureStart(x,z);}
    }
    public static void main(String[] args)throws Exception {
        PrintStream output=System.out;System.setOut(System.err);Bootstrap.register();StructureOceanMonumentPieces.registerOceanMonumentPieces();
        List<Object> cases=new ArrayList<Object>();
        for(long seed:new long[]{0,1,-1,189,123456789012345L,Long.MIN_VALUE,Long.MAX_VALUE})for(int variant=0;variant<3;variant++){
            int cx=variant==0?0:variant==1?-17:28,cz=variant==0?0:variant==1?-39:33;TestWorld world=new TestWorld(seed,variant);StructureStart start=new Generator().start(world,cx,cz);long randomSeed=seed^189L;Random random=new Random(randomSeed);
            Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("seed",Long.toString(seed));row.put("chunk",new int[]{cx,cz});row.put("pattern",variant);row.put("sea",world.getSeaLevel());row.put("randomSeed",Long.toString(randomSeed));
            List<int[]> chunks=new ArrayList<int[]>();for(int x=cx-3;x<=cx+3;x++)for(int z=cz-3;z<=cz+3;z++)if(start.getBoundingBox().intersectsWith(x*16+8,z*16+8,x*16+23,z*16+23))chunks.add(new int[]{x,z});if(variant==1)Collections.reverse(chunks);chunks.add(chunks.getFirst());
            List<Object> steps=new ArrayList<Object>();for(int i=0;i<chunks.size();i++){
                boolean reload=variant!=0&&i==chunks.size()/2;if(reload){NBTTagCompound saved=start.writeStructureComponentsToNBT(cx,cz);StructureOceanMonument.StartMonument loaded=new StructureOceanMonument.StartMonument();loaded.readStructureComponentsFromNBT(world,saved);start=loaded;}
                int[] c=chunks.get(i);ChunkCoordIntPair pos=new ChunkCoordIntPair(c[0],c[1]);StructureBoundingBox clip=new StructureBoundingBox(c[0]*16+8,0,c[1]*16+8,c[0]*16+23,255,c[1]*16+23);boolean generated=start.func_175788_a(pos);
                if(generated){start.generateStructure(world,random,clip);start.func_175787_b(pos);}
                Map<String,Object> step=new LinkedHashMap<String,Object>();step.put("chunk",c);step.put("reload",reload);step.put("generated",generated);step.put("count",world.blocks.size());step.put("hash",Long.toUnsignedString(world.hash()));step.put("elders",new ArrayList<int[]>(world.elders));step.put("next",Long.toString(random.nextLong()));steps.add(step);
            }row.put("steps",steps);cases.add(row);
        }output.println(new Gson().toJson(cases));
    }
}
