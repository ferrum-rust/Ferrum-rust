import java.io.*;
import java.util.*;
import com.google.gson.Gson;
import net.minecraft.block.Block;
import net.minecraft.block.material.Material;
import net.minecraft.block.state.IBlockState;
import net.minecraft.entity.Entity;
import net.minecraft.entity.passive.EntityVillager;
import net.minecraft.init.Bootstrap;
import net.minecraft.item.Item;
import net.minecraft.item.ItemStack;
import net.minecraft.nbt.*;
import net.minecraft.profiler.Profiler;
import net.minecraft.tileentity.*;
import net.minecraft.util.BlockPos;
import net.minecraft.world.*;
import net.minecraft.world.storage.WorldInfo;
import net.minecraft.world.chunk.IChunkProvider;
import net.minecraft.world.gen.structure.*;

/** Executes original piece methods. Synthetic terrain isolates structure
 * writes from unrelated scheduled block ticks; no game/server is launched. */
public final class VillagePopulationOracle {
    static final class TestWorld extends World {
        final Map<BlockPos,IBlockState> blocks=new HashMap<BlockPos,IBlockState>();
        final Map<BlockPos,TileEntity> tiles=new HashMap<BlockPos,TileEntity>();
        final List<int[]> villagers=new ArrayList<int[]>(); final int pattern;
        TestWorld(int pattern){super(null,new WorldInfo(new WorldSettings(0,WorldSettings.GameType.SURVIVAL,true,false,WorldType.DEFAULT),"oracle"),new WorldProviderSurface(),new Profiler(),false);provider.registerWorld(this);this.pattern=pattern;}
        protected IChunkProvider createChunkProvider(){return null;}
        protected int getRenderDistanceChunks(){return 8;}
        int height(int x,int z){return pattern==0?63:60+Math.floorMod(x*3+z*7,9);}
        IBlockState base(BlockPos p){int h=height(p.getX(),p.getZ()),y=p.getY();int id=y<0||y>255?0:y==0?7:y<h-3?1:y<h?3:y==h?2:pattern==2&&y<66?9:0;return Block.getBlockById(id).getDefaultState();}
        public IBlockState getBlockState(BlockPos p){IBlockState s=blocks.get(p);return s==null?base(p):s;}
        public boolean setBlockState(BlockPos p,IBlockState s,int flags){if(p.getY()<0||p.getY()>255)return false;BlockPos key=new BlockPos(p);blocks.put(key,s);if(Block.getIdFromBlock(s.getBlock())==54){if(!tiles.containsKey(key))tiles.put(key,new TileEntityChest());}else tiles.remove(key);return true;}
        public BlockPos getTopSolidOrLiquidBlock(BlockPos p){for(int y=256;y>=0;y--){Material m=getBlockState(new BlockPos(p.getX(),y-1,p.getZ())).getBlock().getMaterial();if(m.blocksMovement()&&m!=Material.leaves)return new BlockPos(p.getX(),y,p.getZ());}return new BlockPos(p.getX(),-1,p.getZ());}
        public TileEntity getTileEntity(BlockPos p){return tiles.get(p);}
        public void notifyNeighborsOfStateChange(BlockPos p,Block block){}
        public DifficultyInstance getDifficultyForLocation(BlockPos p){return new DifficultyInstance(EnumDifficulty.NORMAL,0,0,0);}
        public boolean spawnEntityInWorld(Entity e){if(e instanceof EntityVillager)villagers.add(new int[]{(int)Math.floor(e.posX),(int)Math.floor(e.posY),(int)Math.floor(e.posZ),((EntityVillager)e).getProfession()});return true;}
    }
    static int state(IBlockState s){return Block.getIdFromBlock(s.getBlock())<<4|s.getBlock().getMetaFromState(s);}
    public static void main(String[] args)throws Exception {
        PrintStream output=System.out;System.setOut(System.err);Bootstrap.register();StructureVillagePieces.registerVillagePieces();
        String[] ids={"ViStart","ViL","ViSH","ViST","ViBH","ViSmH","ViPH","ViDF","ViF","ViS","ViTRH","ViSR"};
        int[][] size={{6,15,6},{3,4,2},{5,6,5},{5,12,9},{9,9,6},{4,6,5},{9,7,11},{13,4,9},{7,4,9},{10,6,7},{9,7,12},{3,3,21}};
        List<Map<String,Object>> cases=new ArrayList<Map<String,Object>>();
        for(int kind=0;kind<ids.length;kind++)for(int orientation=0;orientation<4;orientation++)for(int variant=0;variant<6;variant++) {
            TestWorld world=new TestWorld(variant%3);int[] dim=size[kind];int sx=(orientation%2==0?dim[0]:dim[2]),sz=(orientation%2==0?dim[2]:dim[0]);
            NBTTagCompound n=new NBTTagCompound();n.setString("id",ids[kind]);n.setIntArray("BB",new int[]{-3,64,-5,-3+sx-1,64+dim[1]-1,-5+sz-1});n.setInteger("O",orientation);n.setInteger("GD",2);n.setInteger("HPos",-1);n.setInteger("VCount",0);n.setBoolean("Desert",variant>=3);n.setBoolean("Terrace",variant%2==1);n.setBoolean("C",variant%2==1);n.setInteger("T",variant%3);n.setInteger("CA",59);n.setInteger("CB",141);n.setInteger("CC",142);n.setInteger("CD",59);n.setInteger("Length",21);n.setBoolean("Chest",false);
            Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("before",VillageOracle.nbt(n));row.put("pattern",variant%3);
            StructureComponent piece=MapGenStructureIO.getStructureComponent(n,world);Random random=new Random(-189L+variant*23+orientation);row.put("seed",Long.toString(-189L+variant*23+orientation));
            // Different first intersections exercise persistent HPos and crop RNG
            // consumption outside the current clip, followed by remaining quadrants.
            List<int[]> clips=new ArrayList<int[]>();int[][] order=variant%2==0?new int[][]{{-8,-8},{0,-8},{-8,0},{0,0},{-8,8},{0,8},{8,-8},{8,0},{8,8},{16,-8},{16,0},{16,8}}:new int[][]{{0,0},{-8,-8},{-8,0},{0,-8},{8,0},{8,-8},{0,8},{-8,8},{8,8},{16,8},{16,0},{16,-8}};
            for(int[] pos:order){StructureBoundingBox clip=new StructureBoundingBox(pos[0],0,pos[1],pos[0]+7,255,pos[1]+7);if(piece.getBoundingBox().intersectsWith(clip)){clips.add(new int[]{clip.minX,clip.minY,clip.minZ,clip.maxX,clip.maxY,clip.maxZ});piece.addComponentParts(world,random,clip);}}
            row.put("clips",clips);row.put("after",VillageOracle.nbt(piece.createStructureBaseNBT()));row.put("next_long",Long.toString(random.nextLong()));
            List<int[]> blocks=new ArrayList<int[]>();for(Map.Entry<BlockPos,IBlockState> e:world.blocks.entrySet()){BlockPos p=e.getKey();blocks.add(new int[]{p.getX(),p.getY(),p.getZ(),state(e.getValue())});}Collections.sort(blocks,(a,b)->{for(int i=0;i<3;i++){int c=Integer.compare(a[i],b[i]);if(c!=0)return c;}return 0;});row.put("blocks",blocks);row.put("villagers",world.villagers);
            List<Object> chests=new ArrayList<Object>();for(Map.Entry<BlockPos,TileEntity> e:world.tiles.entrySet()){TileEntityChest chest=(TileEntityChest)e.getValue();List<int[]> items=new ArrayList<int[]>();for(int slot=0;slot<chest.getSizeInventory();slot++){ItemStack stack=chest.getStackInSlot(slot);if(stack!=null)items.add(new int[]{slot,Item.getIdFromItem(stack.getItem()),stack.getMetadata(),stack.stackSize});}Map<String,Object> c=new LinkedHashMap<String,Object>();c.put("pos",new int[]{e.getKey().getX(),e.getKey().getY(),e.getKey().getZ()});c.put("items",items);chests.add(c);}row.put("chests",chests);cases.add(row);
        }
        List<int[]> properties=new ArrayList<int[]>();for(Object obj:Block.blockRegistry){Block b=(Block)obj;int flags=(b.isFullBlock()?1:0)|(b.isNormalCube()?2:0)|(b.getMaterial().blocksMovement()?4:0)|(b.getMaterial()==Material.leaves?8:0);properties.add(new int[]{Block.getIdFromBlock(b),flags,state(b.getDefaultState())});}
        Map<String,Object> out=new LinkedHashMap<String,Object>();out.put("properties",properties);out.put("cases",cases);output.println(new Gson().toJson(out));
    }
}
