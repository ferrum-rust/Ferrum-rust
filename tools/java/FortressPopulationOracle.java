import java.io.*;
import java.util.*;
import com.google.gson.Gson;
import net.minecraft.block.Block;
import net.minecraft.block.state.IBlockState;
import net.minecraft.init.Bootstrap;
import net.minecraft.inventory.IInventory;
import net.minecraft.nbt.*;
import net.minecraft.profiler.Profiler;
import net.minecraft.tileentity.*;
import net.minecraft.util.BlockPos;
import net.minecraft.world.*;
import net.minecraft.world.storage.WorldInfo;
import net.minecraft.world.chunk.IChunkProvider;
import net.minecraft.world.gen.structure.*;

/** Template oracle; callback arguments retained separately from block writes. */
public final class FortressPopulationOracle {
    static final class TestWorld extends World {
        final Map<BlockPos,IBlockState> blocks=new HashMap<BlockPos,IBlockState>();final Map<BlockPos,TileEntity> tiles=new HashMap<BlockPos,TileEntity>();final List<int[]> forced=new ArrayList<int[]>();final int pattern;
        TestWorld(int pattern){super(null,new WorldInfo(new WorldSettings(0,WorldSettings.GameType.SURVIVAL,true,false,WorldType.DEFAULT),"oracle"),new WorldProviderHell(),new Profiler(),false);provider.registerWorld(this);this.pattern=pattern;}
        protected IChunkProvider createChunkProvider(){return null;}
        protected int getRenderDistanceChunks(){return 8;}
        public IBlockState getBlockState(BlockPos p){IBlockState s=blocks.get(p);if(s!=null)return s;int floor=pattern==0?63:28+Math.floorMod(p.getX()*3+p.getZ()*7,11);int id=p.getY()<0||p.getY()>255?0:p.getY()==0?7:p.getY()<floor?87:pattern==2&&p.getY()<45?11:0;return Block.getBlockById(id).getDefaultState();}
        public boolean setBlockState(BlockPos p,IBlockState s,int flags){if(p.getY()<0||p.getY()>255)return false;BlockPos key=new BlockPos(p);blocks.put(key,s);int id=Block.getIdFromBlock(s.getBlock());TileEntity old=tiles.get(key);if(id==54){if(!(old instanceof TileEntityChest))tiles.put(key,new TileEntityChest());}else if(id==52){if(!(old instanceof TileEntityMobSpawner))tiles.put(key,new TileEntityMobSpawner());}else tiles.remove(key);return true;}
        public TileEntity getTileEntity(BlockPos p){return tiles.get(p);}
        public void forceBlockUpdateTick(Block block,BlockPos p,Random random){if(Block.getIdFromBlock(block)!=10||Block.getIdFromBlock(getBlockState(p).getBlock())!=10)throw new AssertionError();forced.add(new int[]{p.getX(),p.getY(),p.getZ()});}
    }
    public static void main(String[] args)throws Exception{
        PrintStream output=System.out;System.setOut(System.err);Bootstrap.register();StructureNetherBridgePieces.registerNetherFortressPieces();
        String[] ids={"NeBS","NeBCr","NeRC","NeSR","NeMT","NeCE","NeSC","NeSCSC","NeSCRT","NeSCLT","NeCCS","NeCTB","NeCSR","NeBEF","NeStart"};
        int[][] sizes={{5,10,19},{19,10,19},{7,9,7},{7,11,7},{7,8,9},{13,14,13},{5,7,5},{5,7,5},{5,7,5},{5,7,5},{5,14,10},{9,7,9},{13,14,13},{5,10,8},{19,10,19}};List<Object> cases=new ArrayList<Object>();
        for(int k=0;k<ids.length;k++)for(int orientation=0;orientation<4;orientation++)for(int variant=0;variant<3;variant++){
            TestWorld world=new TestWorld(variant);int[] size=sizes[k];int sx=orientation%2==0?size[0]:size[2],sz=orientation%2==0?size[2]:size[0];NBTTagCompound n=new NBTTagCompound();n.setString("id",ids[k]);n.setIntArray("BB",new int[]{-3,40,-5,-3+sx-1,40+size[1]-1,-5+sz-1});n.setInteger("O",orientation);n.setInteger("GD",5);n.setInteger("Seed",-789+variant*197+orientation*837);n.setBoolean("Chest",variant!=1);
            StructureComponent piece=MapGenStructureIO.getStructureComponent(n,world);Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("before",FortressOracle.nbt(piece));row.put("pattern",variant);long seed=-189L+orientation*17+variant*31+k*47;row.put("seed",Long.toString(seed));Random random=new Random(seed);
            List<StructureBoundingBox> clips=new ArrayList<StructureBoundingBox>();for(int x=-8;x<=16;x+=8)for(int z=-8;z<=16;z+=8){StructureBoundingBox b=new StructureBoundingBox(x,0,z,x+7,255,z+7);if(piece.getBoundingBox().intersectsWith(b))clips.add(b);}if(variant==1)Collections.reverse(clips);clips.add(clips.getFirst());List<int[]> clipRows=new ArrayList<int[]>();for(StructureBoundingBox clip:clips){piece.addComponentParts(world,random,clip);clipRows.add(StrongholdOracle.bbox(clip));}row.put("clips",clipRows);row.put("after",FortressOracle.nbt(piece));row.put("next",Long.toString(random.nextLong()));
            List<int[]> blocks=new ArrayList<int[]>();for(Map.Entry<BlockPos,IBlockState> e:world.blocks.entrySet()){BlockPos p=e.getKey();IBlockState s=e.getValue();blocks.add(new int[]{p.getX(),p.getY(),p.getZ(),Block.getIdFromBlock(s.getBlock())<<4|s.getBlock().getMetaFromState(s)});}Collections.sort(blocks,(a,b)->{for(int i=0;i<3;i++){int c=Integer.compare(a[i],b[i]);if(c!=0)return c;}return 0;});row.put("blocks",blocks);
            List<Object> chests=new ArrayList<Object>(),spawners=new ArrayList<Object>();for(Map.Entry<BlockPos,TileEntity> e:world.tiles.entrySet()){BlockPos p=e.getKey();int[] pos={p.getX(),p.getY(),p.getZ()};if(e.getValue()instanceof IInventory)chests.add(Arrays.asList(pos,ScatteredOracle.slots((IInventory)e.getValue())));else{NBTTagCompound tag=new NBTTagCompound();e.getValue().writeToNBT(tag);spawners.add(Arrays.asList(pos,tag.getString("EntityId")));}}row.put("chests",chests);row.put("spawners",spawners);row.put("forced",world.forced);cases.add(row);
        }output.println(new Gson().toJson(cases));
    }
}
