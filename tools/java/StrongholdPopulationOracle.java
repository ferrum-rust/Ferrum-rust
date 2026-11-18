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

/** Original bytecode placement, with world callbacks isolated for exact writes. */
public final class StrongholdPopulationOracle {
    static final class TestWorld extends World {
        final Map<BlockPos,IBlockState> blocks=new HashMap<BlockPos,IBlockState>();
        final Map<BlockPos,TileEntity> tiles=new HashMap<BlockPos,TileEntity>();
        final int pattern;
        TestWorld(int pattern){super(null,new WorldInfo(new WorldSettings(0,WorldSettings.GameType.SURVIVAL,true,false,WorldType.DEFAULT),"oracle"),new WorldProviderSurface(),new Profiler(),false);provider.registerWorld(this);this.pattern=pattern;}
        protected IChunkProvider createChunkProvider(){return null;}
        protected int getRenderDistanceChunks(){return 8;}
        public IBlockState getBlockState(BlockPos p){IBlockState s=blocks.get(p);if(s!=null)return s;int id=p.getY()<0||p.getY()>63?0:1;if(pattern==1&&Math.floorMod(p.getX()*3+p.getY()*2+p.getZ()*7,17)<4)id=0;if(pattern==2&&p.equals(new BlockPos(-4,39,-5)))id=9;return Block.getBlockById(id).getDefaultState();}
        public boolean setBlockState(BlockPos p,IBlockState s,int flags){if(p.getY()<0||p.getY()>255)return false;BlockPos key=new BlockPos(p);blocks.put(key,s);int id=Block.getIdFromBlock(s.getBlock());TileEntity old=tiles.get(key);if(id==54){if(!(old instanceof TileEntityChest))tiles.put(key,new TileEntityChest());}else if(id==52){if(!(old instanceof TileEntityMobSpawner))tiles.put(key,new TileEntityMobSpawner());}else tiles.remove(key);return true;}
        public TileEntity getTileEntity(BlockPos p){return tiles.get(p);}
    }
    public static void main(String[] args)throws Exception {
        PrintStream output=System.out;System.setOut(System.err);Bootstrap.register();StructureStrongholdPieces.registerStrongholdPieces();
        String[] ids={"SHS","SHPH","SHLT","SHRT","SHRC","SHSSD","SHSD","SH5C","SHCC","SHLi","SHPR","SHStart","SHFC"};
        int[][] sizes={{5,5,7},{9,5,11},{5,5,5},{5,5,5},{11,7,11},{5,11,8},{5,11,5},{10,9,11},{5,5,7},{14,11,15},{11,8,16},{5,11,5},{5,5,3}};
        List<Object> cases=new ArrayList<Object>();String[] doors={"OPENING","WOOD_DOOR","GRATES","IRON_DOOR"};
        for(int k=0;k<ids.length;k++)for(int orientation=0;orientation<4;orientation++)for(int variant=0;variant<4;variant++){
            TestWorld world=new TestWorld(variant%3);int[] size=sizes[k].clone();if(k==9&&variant%2==0)size[1]=6;int sx=orientation%2==0?size[0]:size[2],sz=orientation%2==0?size[2]:size[0];
            NBTTagCompound n=new NBTTagCompound();n.setString("id",ids[k]);n.setIntArray("BB",new int[]{-3,40,-5,-3+sx-1,40+size[1]-1,-5+sz-1});n.setInteger("O",orientation);n.setInteger("GD",5);n.setString("EntryDoor",doors[variant]);n.setInteger("Type",variant==3?4:variant);n.setInteger("Steps",3);n.setBoolean("Tall",size[1]>6);n.setBoolean("Source",k==11);n.setBoolean("Left",variant%2==0);n.setBoolean("Right",variant>1);n.setBoolean("leftLow",variant%2==0);n.setBoolean("leftHigh",variant>1);n.setBoolean("rightLow",variant!=1);n.setBoolean("rightHigh",variant!=2);
            StructureComponent piece=MapGenStructureIO.getStructureComponent(n,world);Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("before",StrongholdOracle.nbt(piece));row.put("pattern",variant%3);long seed=-189L+orientation*17+variant*31+k*47;row.put("seed",Long.toString(seed));Random random=new Random(seed);
            List<StructureBoundingBox> clips=new ArrayList<StructureBoundingBox>();for(int x=-8;x<=16;x+=8)for(int z=-8;z<=16;z+=8){StructureBoundingBox b=new StructureBoundingBox(x,0,z,x+7,255,z+7);if(piece.getBoundingBox().intersectsWith(b))clips.add(b);}if(variant%2==1)Collections.reverse(clips);clips.add(clips.getFirst());
            List<Object> clipRows=new ArrayList<Object>();for(StructureBoundingBox clip:clips){boolean ok=piece.addComponentParts(world,random,clip);clipRows.add(Arrays.asList(StrongholdOracle.bbox(clip),ok));}row.put("clips",clipRows);row.put("after",StrongholdOracle.nbt(piece));row.put("next",Long.toString(random.nextLong()));
            List<int[]> blocks=new ArrayList<int[]>();for(Map.Entry<BlockPos,IBlockState> e:world.blocks.entrySet()){BlockPos p=e.getKey();IBlockState s=e.getValue();blocks.add(new int[]{p.getX(),p.getY(),p.getZ(),Block.getIdFromBlock(s.getBlock())<<4|s.getBlock().getMetaFromState(s)});}Collections.sort(blocks,(a,b)->{for(int i=0;i<3;i++){int c=Integer.compare(a[i],b[i]);if(c!=0)return c;}return 0;});row.put("blocks",blocks);
            List<Object> chests=new ArrayList<Object>(),spawners=new ArrayList<Object>();for(Map.Entry<BlockPos,TileEntity> e:world.tiles.entrySet()){BlockPos p=e.getKey();int[] pos={p.getX(),p.getY(),p.getZ()};if(e.getValue()instanceof IInventory)chests.add(Arrays.asList(pos,ScatteredOracle.slots((IInventory)e.getValue())));else {NBTTagCompound tag=new NBTTagCompound();e.getValue().writeToNBT(tag);spawners.add(Arrays.asList(pos,tag.getString("EntityId")));}}
            row.put("chests",chests);row.put("spawners",spawners);cases.add(row);
        }output.println(new Gson().toJson(cases));
    }
}
