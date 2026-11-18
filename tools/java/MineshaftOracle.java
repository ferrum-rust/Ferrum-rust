import java.io.*;
import java.util.*;
import com.google.gson.Gson;
import net.minecraft.block.Block;
import net.minecraft.block.state.IBlockState;
import net.minecraft.entity.Entity;
import net.minecraft.entity.item.EntityMinecartChest;
import net.minecraft.enchantment.Enchantment;
import net.minecraft.enchantment.EnchantmentData;
import net.minecraft.enchantment.EnchantmentHelper;
import net.minecraft.init.Bootstrap;
import net.minecraft.init.Items;
import net.minecraft.item.ItemStack;
import net.minecraft.item.Item;
import net.minecraft.tileentity.TileEntity;
import net.minecraft.tileentity.TileEntityMobSpawner;
import net.minecraft.nbt.*;
import net.minecraft.profiler.Profiler;
import net.minecraft.util.BlockPos;
import net.minecraft.world.*;
import net.minecraft.world.storage.WorldInfo;
import net.minecraft.world.chunk.IChunkProvider;
import net.minecraft.world.gen.structure.*;

/** Actual 1.8.9 mineshaft pieces run in a synthetic world, never a server. */
public final class MineshaftOracle {
    static final class SpawnMap extends MapGenMineshaft {
        StructureMineshaftStart candidate(long seed,int x,int z){
            rand.setSeed(seed);long a=rand.nextLong(),b=rand.nextLong();rand.setSeed((long)x*a^(long)z*b^seed);rand.nextInt();
            return canSpawnStructureAtCoords(x,z)?new StructureMineshaftStart(new FlatWorld(seed),rand,x,z):null;
        }
    }
    static final class FlatWorld extends World {
        final Map<BlockPos,IBlockState> blocks=new HashMap<BlockPos,IBlockState>();
        final Map<BlockPos,TileEntity> tiles=new HashMap<BlockPos,TileEntity>();
        final List<EntityMinecartChest> carts=new ArrayList<EntityMinecartChest>();
        int pattern;
        FlatWorld(long seed){super(null,new WorldInfo(new WorldSettings(seed,WorldSettings.GameType.SURVIVAL,false,false,WorldType.DEFAULT),"Oracle"),new WorldProviderSurface(),new Profiler(),false);}
        protected IChunkProvider createChunkProvider(){return null;}
        protected int getRenderDistanceChunks(){return 8;}
        public IBlockState getBlockState(BlockPos p){
            IBlockState state=blocks.get(p);if(state!=null)return state;
            int id=p.getY()==0?7:p.getY()<80&&p.getY()>0?1:0;
            if(pattern==1&&p.getY()>=20&&p.getY()<=60&&Math.floorMod(p.getX(),13)<4&&Math.floorMod(p.getZ(),17)<8)id=0;
            if(pattern==2&&p.getY()>0&&p.getY()<80&&Math.floorMod(p.getX(),31)==0&&Math.floorMod(p.getZ(),29)<5)id=9;
            return Block.getBlockById(id).getDefaultState();
        }
        public boolean setBlockState(BlockPos p,IBlockState state,int flags){
            p=new BlockPos(p.getX(),p.getY(),p.getZ());blocks.put(p,state);
            if(Block.getIdFromBlock(state.getBlock())==52){TileEntityMobSpawner tile=new TileEntityMobSpawner();tile.setPos(p);tile.setWorldObj(this);tiles.put(p,tile);}else tiles.remove(p);
            return true;
        }
        public TileEntity getTileEntity(BlockPos p){return tiles.get(p);}
        public boolean spawnEntityInWorld(Entity e){carts.add((EntityMinecartChest)e);return true;}
    }
    static Map<String,Object> cart(EntityMinecartChest cart){
        Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("position",new double[]{cart.posX,cart.posY,cart.posZ});List<Object> slots=new ArrayList<Object>();
        for(int i=0;i<cart.getSizeInventory();i++){
            ItemStack stack=cart.getStackInSlot(i);if(stack==null){slots.add(null);continue;}
            Map<String,Object> item=new LinkedHashMap<String,Object>();item.put("id",Item.getIdFromItem(stack.getItem()));item.put("meta",stack.getMetadata());item.put("count",stack.stackSize);
            List<int[]> enchants=new ArrayList<int[]>();if(stack.hasTagCompound()){NBTTagList list=stack.getTagCompound().getTagList("StoredEnchantments",10);for(int j=0;j<list.tagCount();j++){NBTTagCompound e=list.getCompoundTagAt(j);enchants.add(new int[]{e.getShort("id"),e.getShort("lvl")});}}
            item.put("enchants",enchants);slots.add(item);
        }row.put("slots",slots);return row;
    }
    static long hash(FlatWorld world,StructureBoundingBox clip){
        long hash=0xcbf29ce484222325L;
        for(int y=0;y<80;y++)for(int z=clip.minZ;z<=clip.maxZ;z++)for(int x=clip.minX;x<=clip.maxX;x++){
            IBlockState state=world.getBlockState(new BlockPos(x,y,z));int packed=Block.getIdFromBlock(state.getBlock())<<4|state.getBlock().getMetaFromState(state);
            hash^=packed;hash*=0x100000001b3L;
        }return hash;
    }
    static List<Integer> bbox(StructureBoundingBox b){return Arrays.asList(b.minX,b.minY,b.minZ,b.maxX,b.maxY,b.maxZ);}
    static Map<String,Object> piece(StructureComponent p){
        NBTTagCompound tag=p.createStructureBaseNBT();Map<String,Object> row=new LinkedHashMap<String,Object>();
        row.put("id",tag.getString("id"));row.put("bounds",bbox(p.getBoundingBox()));row.put("direction",tag.getInteger("O"));row.put("depth",tag.getInteger("GD"));
        for(String key:new String[]{"hr","sc","hps","tf"})if(tag.hasKey(key))row.put(key,tag.getBoolean(key));
        for(String key:new String[]{"Num","D"})if(tag.hasKey(key))row.put(key,tag.getInteger(key));
        if(tag.hasKey("Entrances")){List<int[]> entries=new ArrayList<int[]>();NBTTagList list=tag.getTagList("Entrances",11);for(int i=0;i<list.tagCount();i++)entries.add(list.getIntArrayAt(i));row.put("Entrances",entries);}
        return row;
    }
    public static void main(String[] args)throws Exception{
        PrintStream output=System.out;System.setOut(System.err);Bootstrap.register();StructureMineshaftPieces.registerStructurePieces();
        List<Map<String,Object>> rows=new ArrayList<Map<String,Object>>();
        for(long seed:new long[]{0,1,-1,123456789012345L,Long.MIN_VALUE})for(int[] c:new int[][]{{0,0},{-13,29},{113,-211}}){
            Random random=new Random(seed);StructureMineshaftStart start=new StructureMineshaftStart(new FlatWorld(seed),random,c[0],c[1]);
            Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("seed",Long.toString(seed));row.put("chunk",c);row.put("bounds",bbox(start.getBoundingBox()));
            List<Map<String,Object>> pieces=new ArrayList<Map<String,Object>>();for(StructureComponent p:start.getComponents())pieces.add(piece(p));
            row.put("pieces",pieces);row.put("next_long",Long.toString(random.nextLong()));rows.add(row);
        }
        List<Map<String,Object>> enchantments=new ArrayList<Map<String,Object>>();
        for(Enchantment e:Enchantment.enchantmentsBookList){
            Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("id",e.effectId);row.put("weight",e.getWeight());
            List<int[]> ranges=new ArrayList<int[]>();for(int i=e.getMinLevel();i<=e.getMaxLevel();i++)ranges.add(new int[]{e.getMinEnchantability(i),e.getMaxEnchantability(i)});row.put("ranges",ranges);
            List<Integer> incompatible=new ArrayList<Integer>();for(Enchantment other:Enchantment.enchantmentsBookList)if(!e.canApplyTogether(other))incompatible.add(other.effectId);row.put("incompatible",incompatible);enchantments.add(row);
        }
        List<Map<String,Object>> books=new ArrayList<Map<String,Object>>();
        for(long seed=-50;seed<=50;seed++){
            Random random=new Random(seed);List<EnchantmentData> list=EnchantmentHelper.buildEnchantmentList(random,new ItemStack(Items.book),30);
            List<int[]> enchants=new ArrayList<int[]>();if(list!=null)for(EnchantmentData e:list)enchants.add(new int[]{e.enchantmentobj.effectId,e.enchantmentLevel});
            Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("seed",Long.toString(seed));row.put("enchants",enchants);row.put("next_long",Long.toString(random.nextLong()));books.add(row);
        }
        List<Integer> fullBlocks=new ArrayList<Integer>();for(int id=0;id<198;id++)if(Block.getBlockById(id).isFullBlock())fullBlocks.add(id);
        List<Map<String,Object>> populations=new ArrayList<Map<String,Object>>();
        for(long seed:new long[]{0,1,-1,123456789012345L,Long.MIN_VALUE})for(int pattern=0;pattern<3;pattern++){
            int cx=seed==0?0:-13,cz=seed==0?0:29;FlatWorld world=new FlatWorld(seed);world.pattern=pattern;
            StructureMineshaftStart start=new StructureMineshaftStart(world,new Random(seed),cx,cz);StructureBoundingBox bounds=start.getBoundingBox();
            Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("seed",Long.toString(seed));row.put("chunk",new int[]{cx,cz});row.put("pattern",pattern);
            List<Map<String,Object>> clips=new ArrayList<Map<String,Object>>();
            for(int x=(bounds.minX-8)>>4;x<=((bounds.maxX-8)>>4);x++)for(int z=(bounds.minZ-8)>>4;z<=((bounds.maxZ-8)>>4);z++){
                long popseed=seed^(long)x*341873128712L^(long)z*132897987541L;Random random=new Random(popseed);
                StructureBoundingBox clip=new StructureBoundingBox((x<<4)+8,(z<<4)+8,(x<<4)+23,(z<<4)+23);
                int before=world.carts.size();start.generateStructure(world,random,clip);
                Map<String,Object> c=new LinkedHashMap<String,Object>();c.put("chunk",new int[]{x,z});c.put("hash",Long.toUnsignedString(hash(world,clip)));c.put("next_long",Long.toString(random.nextLong()));
                List<Map<String,Object>> carts=new ArrayList<Map<String,Object>>();for(int i=before;i<world.carts.size();i++)carts.add(cart(world.carts.get(i)));c.put("carts",carts);clips.add(c);
            }
            row.put("clips",clips);
            List<Map<String,Object>> pieces=new ArrayList<Map<String,Object>>();for(StructureComponent p:start.getComponents())pieces.add(piece(p));row.put("pieces",pieces);
            List<int[]> spawners=new ArrayList<int[]>();for(Map.Entry<BlockPos,TileEntity> e:world.tiles.entrySet()){NBTTagCompound tag=new NBTTagCompound();e.getValue().writeToNBT(tag);if(!"CaveSpider".equals(tag.getString("EntityId")))throw new AssertionError(tag);BlockPos p=e.getKey();spawners.add(new int[]{p.getX(),p.getY(),p.getZ()});}row.put("spawners",spawners);
            populations.add(row);
        }
        List<Map<String,Object>> candidates=new ArrayList<Map<String,Object>>();SpawnMap map=new SpawnMap();
        for(long seed:new long[]{0,1,-1,123456789012345L,Long.MIN_VALUE})for(int x=-80;x<=80;x++)for(int z=-80;z<=80;z++){
            StructureMineshaftStart start=map.candidate(seed,x,z);if(start==null)continue;
            Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("seed",Long.toString(seed));row.put("chunk",new int[]{x,z});row.put("bounds",bbox(start.getBoundingBox()));row.put("count",start.getComponents().size());candidates.add(row);
        }
        Map<String,Object> result=new LinkedHashMap<String,Object>();result.put("cases",rows);result.put("enchantments",enchantments);result.put("books",books);result.put("full_blocks",fullBlocks);result.put("populations",populations);result.put("candidates",candidates);output.println(new Gson().toJson(result));
    }
}
