import java.io.*;
import java.util.*;
import com.google.gson.Gson;
import net.minecraft.block.Block;
import net.minecraft.block.material.Material;
import net.minecraft.block.state.IBlockState;
import net.minecraft.entity.Entity;
import net.minecraft.entity.monster.EntityWitch;
import net.minecraft.init.Bootstrap;
import net.minecraft.inventory.IInventory;
import net.minecraft.item.Item;
import net.minecraft.item.ItemStack;
import net.minecraft.nbt.*;
import net.minecraft.profiler.Profiler;
import net.minecraft.tileentity.*;
import net.minecraft.util.BlockPos;
import net.minecraft.world.*;
import net.minecraft.world.biome.*;
import net.minecraft.world.storage.WorldInfo;
import net.minecraft.world.chunk.IChunkProvider;
import net.minecraft.world.gen.structure.*;

/** Actual original structure methods, isolated from unrelated world callbacks. */
public final class ScatteredOracle {
    static final class TestWorld extends World {
        final Map<BlockPos,IBlockState> blocks=new HashMap<BlockPos,IBlockState>();
        final Map<BlockPos,TileEntity> tiles=new HashMap<BlockPos,TileEntity>();
        final List<int[]> witches=new ArrayList<int[]>();
        final int pattern,biome;final WorldChunkManager manager;
        TestWorld(long seed,int pattern,int biome){super(null,new WorldInfo(new WorldSettings(seed,WorldSettings.GameType.SURVIVAL,true,false,WorldType.DEFAULT),"oracle"),new WorldProviderSurface(),new Profiler(),false);provider.registerWorld(this);this.pattern=pattern;this.biome=biome;manager=new WorldChunkManagerHell(BiomeGenBase.getBiome(biome),0.5f);}
        protected IChunkProvider createChunkProvider(){return null;}
        protected int getRenderDistanceChunks(){return 8;}
        public WorldChunkManager getWorldChunkManager(){return manager;}
        public BiomeGenBase getBiomeGenForCoords(BlockPos p){return BiomeGenBase.getBiome(biome);}
        public IBlockState getBlockState(BlockPos p){
            IBlockState s=blocks.get(p);if(s!=null)return s;
            int y=p.getY(),h=pattern==0?63:60+Math.floorMod(p.getX()*3+p.getZ()*7,9);
            int id=y<0||y>255?0:y==0?7:y<h-3?1:y<h?3:y==h?2:pattern==2&&y<66?9:0;
            return Block.getBlockById(id).getDefaultState();
        }
        public boolean setBlockState(BlockPos p,IBlockState s,int flags){
            if(p.getY()<0||p.getY()>255)return false;BlockPos key=new BlockPos(p);blocks.put(key,s);
            int id=Block.getIdFromBlock(s.getBlock());TileEntity tile=tiles.get(key);
            if(id==54){if(!(tile instanceof TileEntityChest))tiles.put(key,new TileEntityChest());}
            else if(id==23){if(!(tile instanceof TileEntityDispenser))tiles.put(key,new TileEntityDispenser());}
            else tiles.remove(key);
            return true;
        }
        public BlockPos getTopSolidOrLiquidBlock(BlockPos p){for(int y=256;y>=0;y--){Material m=getBlockState(new BlockPos(p.getX(),y-1,p.getZ())).getBlock().getMaterial();if(m.blocksMovement()&&m!=Material.leaves)return new BlockPos(p.getX(),y,p.getZ());}return new BlockPos(p.getX(),-1,p.getZ());}
        public TileEntity getTileEntity(BlockPos p){return tiles.get(p);}
        public DifficultyInstance getDifficultyForLocation(BlockPos p){return new DifficultyInstance(EnumDifficulty.NORMAL,0,0,0);}
        public boolean spawnEntityInWorld(Entity e){if(!(e instanceof EntityWitch))throw new AssertionError(e);witches.add(new int[]{(int)Math.floor(e.posX),(int)Math.floor(e.posY),(int)Math.floor(e.posZ)});return true;}
    }
    static final class Candidates extends MapGenScatteredFeature {
        StructureStart candidate(TestWorld world,long seed,int x,int z){
            worldObj=world;rand.setSeed(seed);long a=rand.nextLong(),b=rand.nextLong();rand.setSeed((long)x*a^(long)z*b^seed);rand.nextInt();
            return canSpawnStructureAtCoords(x,z)?getStructureStart(x,z):null;
        }
    }
    static int[] bbox(StructureBoundingBox b){return new int[]{b.minX,b.minY,b.minZ,b.maxX,b.maxY,b.maxZ};}
    static Map<String,Object> nbt(StructureComponent p){
        NBTTagCompound n=p.createStructureBaseNBT();Map<String,Object> r=new LinkedHashMap<String,Object>();r.put("id",n.getString("id"));r.put("BB",n.getIntArray("BB"));
        for(String key:new String[]{"O","GD","Width","Height","Depth","HPos"})r.put(key,n.getInteger(key));
        for(String key:new String[]{"Witch","placedMainChest","placedHiddenChest","placedTrap1","placedTrap2","hasPlacedChest0","hasPlacedChest1","hasPlacedChest2","hasPlacedChest3"})if(n.hasKey(key))r.put(key,n.getBoolean(key));
        return r;
    }
    static List<Object> slots(IInventory inv){
        List<Object> out=new ArrayList<Object>();for(int i=0;i<inv.getSizeInventory();i++){
            ItemStack s=inv.getStackInSlot(i);if(s==null){out.add(null);continue;}
            Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("id",Item.getIdFromItem(s.getItem()));row.put("meta",s.getMetadata());row.put("count",s.stackSize);
            List<int[]> enchants=new ArrayList<int[]>();if(s.hasTagCompound()){NBTTagList list=s.getTagCompound().getTagList("StoredEnchantments",10);for(int j=0;j<list.tagCount();j++){NBTTagCompound e=list.getCompoundTagAt(j);enchants.add(new int[]{e.getShort("id"),e.getShort("lvl")});}}
            row.put("enchants",enchants);out.add(row);
        }return out;
    }
    public static void main(String[] args)throws Exception {
        PrintStream output=System.out;System.setOut(System.err);Bootstrap.register();ComponentScatteredFeaturePieces.registerScatteredFeaturePieces();
        List<Map<String,Object>> candidates=new ArrayList<Map<String,Object>>();Candidates map=new Candidates();
        for(long seed:new long[]{0,1,-1,123456789012345L,Long.MIN_VALUE})for(int biome:new int[]{2,17,21,22,6,1,130,134}){
            TestWorld world=new TestWorld(seed,0,biome);
            for(int x=-80;x<=80;x++)for(int z=-80;z<=80;z++){
                StructureStart start=map.candidate(world,seed,x,z);if(start==null)continue;
                Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("seed",Long.toString(seed));row.put("biome",biome);row.put("chunk",new int[]{x,z});row.put("bounds",bbox(start.getBoundingBox()));row.put("piece",nbt(start.getComponents().getFirst()));candidates.add(row);
            }
        }
        List<Map<String,Object>> cases=new ArrayList<Map<String,Object>>();String[] ids={"TeDP","TeJP","TeSH"};int[][] sizes={{21,15,21},{12,10,15},{7,7,9}};
        for(int kind=0;kind<3;kind++)for(int orientation=0;orientation<4;orientation++)for(int variant=0;variant<6;variant++){
            long seed=-189L+orientation*17+variant*31;TestWorld world=new TestWorld(seed,variant%3,kind==0?2:kind==1?21:6);int[] size=sizes[kind];
            int sx=orientation%2==0?size[0]:size[2],sz=orientation%2==0?size[2]:size[0];
            NBTTagCompound n=new NBTTagCompound();n.setString("id",ids[kind]);n.setIntArray("BB",new int[]{-3,64,-5,-3+sx-1,64+size[1]-1,-5+sz-1});n.setInteger("O",orientation);n.setInteger("GD",0);n.setInteger("Width",size[0]);n.setInteger("Height",size[1]);n.setInteger("Depth",size[2]);n.setInteger("HPos",-1);
            StructureComponent piece=MapGenStructureIO.getStructureComponent(n,world);Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("before",nbt(piece));row.put("pattern",variant%3);row.put("seed",Long.toString(seed));
            List<int[]> order=new ArrayList<int[]>();for(int x=-8;x<=16;x+=8)for(int z=-8;z<=16;z+=8)order.add(new int[]{x,z});if(variant%2==1)Collections.reverse(order);
            List<int[]> clips=new ArrayList<int[]>();Random random=new Random(seed);
            for(int[] pos:order){StructureBoundingBox clip=new StructureBoundingBox(pos[0],0,pos[1],pos[0]+7,255,pos[1]+7);if(!piece.getBoundingBox().intersectsWith(clip))continue;clips.add(bbox(clip));if(!piece.addComponentParts(world,random,clip))throw new AssertionError("unexpected rejection");}
            row.put("clips",clips);row.put("after",nbt(piece));row.put("next_long",Long.toString(random.nextLong()));
            List<int[]> blocks=new ArrayList<int[]>();for(Map.Entry<BlockPos,IBlockState> e:world.blocks.entrySet()){BlockPos p=e.getKey();IBlockState s=e.getValue();blocks.add(new int[]{p.getX(),p.getY(),p.getZ(),Block.getIdFromBlock(s.getBlock())<<4|s.getBlock().getMetaFromState(s)});}Collections.sort(blocks,(a,b)->{for(int i=0;i<3;i++){int c=Integer.compare(a[i],b[i]);if(c!=0)return c;}return 0;});row.put("blocks",blocks);
            List<Map<String,Object>> containers=new ArrayList<Map<String,Object>>();for(Map.Entry<BlockPos,TileEntity> e:world.tiles.entrySet()){Map<String,Object> c=new LinkedHashMap<String,Object>();BlockPos p=e.getKey();c.put("position",new int[]{p.getX(),p.getY(),p.getZ()});c.put("block",e.getValue() instanceof TileEntityDispenser?23:54);c.put("slots",slots((IInventory)e.getValue()));containers.add(c);}row.put("containers",containers);row.put("witches",world.witches);cases.add(row);
        }
        Map<String,Object> result=new LinkedHashMap<String,Object>();result.put("candidates",candidates);result.put("cases",cases);output.println(new Gson().toJson(result));
    }
}
