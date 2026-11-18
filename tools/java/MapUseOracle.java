import com.google.gson.Gson;
import java.lang.reflect.Field;
import java.util.*;
import sun.misc.Unsafe;
import net.minecraft.init.*;
import net.minecraft.block.*;
import net.minecraft.block.state.IBlockState;
import net.minecraft.item.*;
import net.minecraft.nbt.NBTTagCompound;
import net.minecraft.util.*;
import net.minecraft.world.*;
import net.minecraft.world.chunk.Chunk;
import net.minecraft.world.storage.*;

/** Original map center, item allocation, pixel shading and player icons. */
public final class MapUseOracle {
    static int top(int scenario,int x,int z) {
        return scenario==0 ? 64 : scenario==1 ? 64+Math.floorMod(Math.floorDiv(x,8)+Math.floorDiv(z,8),5) : 64;
    }
    static IBlockState state(int scenario,int x,int y,int z) {
        int top=top(scenario,x,z);
        if(y>top || y<0)return Blocks.air.getDefaultState();
        if(scenario==2 && y>=60)return Blocks.water.getDefaultState();
        if(y==top)return (scenario==1 && Math.floorMod(x,16)<4 ? Blocks.sand : Blocks.grass).getDefaultState();
        return Blocks.stone.getDefaultState();
    }
    static class MapChunk extends Chunk {
        final int scenario;
        MapChunk(World w,int x,int z,int scenario){super(w,x,z);this.scenario=scenario;}
        @Override public boolean isEmpty(){return false;}
        @Override public int getHeightValue(int x,int z){return top(scenario,xPosition*16+x,zPosition*16+z)+1;}
        @Override public IBlockState getBlockState(BlockPos p){return state(scenario,xPosition*16+(p.getX()&15),p.getY(),zPosition*16+(p.getZ()&15));}
        @Override public Block getBlock(int x,int y,int z){return state(scenario,xPosition*16+(x&15),y,zPosition*16+(z&15)).getBlock();}
    }
    static class MapWorld extends FoodOracle.OracleWorld {
        int scenario,nextId;Map<String,WorldSavedData> data;
        @Override public int getUniqueDataId(String name){return nextId++;}
        @Override public void setItemData(String name,WorldSavedData value){data.put(name,value);}
        @Override public WorldSavedData loadItemData(Class<? extends WorldSavedData> cls,String name){return data.get(name);}
        @Override public Chunk getChunkFromChunkCoords(int x,int z){return new MapChunk(this,x,z,scenario);}
        @Override public Chunk getChunkFromBlockCoords(BlockPos p){return getChunkFromChunkCoords(p.getX()>>4,p.getZ()>>4);}
    }
    static MapWorld world(int scenario,boolean nether)throws Exception {
        Field f=Unsafe.class.getDeclaredField("theUnsafe");f.setAccessible(true);
        MapWorld w=(MapWorld)((Unsafe)f.get(null)).allocateInstance(MapWorld.class);
        w.scenario=scenario;w.data=new HashMap<>();w.rules=new GameRules();w.difficulty=EnumDifficulty.NORMAL;
        FoodOracle.set(w,World.class,"rand",new Random(189));
        WorldProvider provider=nether ? new WorldProviderHell() : new WorldProviderSurface();
        FoodOracle.set(provider,WorldProvider.class,"hasNoSky",nether);
        FoodOracle.set(provider,WorldProvider.class,"dimensionId",nether?-1:0);
        FoodOracle.set(w,World.class,"provider",provider);
        WorldInfo info=new WorldInfo(new WorldSettings(189,WorldSettings.GameType.SURVIVAL,false,false,WorldType.DEFAULT),"MapOracle");
        info.setWorldTime(6000);FoodOracle.set(w,World.class,"worldInfo",info);
        return w;
    }
    static Map<String,Object> run(int scenario,boolean nether,double x,double z)throws Exception {
        MapWorld w=world(scenario,nether);FoodOracle.Player p=new FoodOracle.Player(w);
        p.setLocationAndAngles(x,72,z,37.25f,0);p.dimension=nether?-1:0;
        ItemStack held=new ItemStack(Items.map,1);held=Items.map.onItemRightClick(held,w,p);p.inventory.mainInventory[0]=held;
        MapData data=Items.filled_map.getMapData(held,w);List<String> hashes=new ArrayList<>();
        for(int tick=0;tick<16;tick++){
            Items.filled_map.onUpdate(held,w,p,0,true);long hash=0xcbf29ce484222325L;
            for(byte b:data.colors)hash=(hash^(b&255))*0x100000001b3L;
            hashes.add(Long.toUnsignedString(hash,16));
        }
        List<int[]> icons=new ArrayList<>();for(Vec4b icon:data.mapDecorations.values())
            icons.add(new int[]{icon.func_176110_a(),icon.func_176112_b(),icon.func_176113_c(),icon.func_176111_d()});
        Map<String,Object> row=new LinkedHashMap<>();row.put("scenario",scenario);row.put("nether",nether);
        row.put("position",new double[]{x,72,z});row.put("center",new int[]{data.xCenter,data.zCenter});
        row.put("scale",data.scale);row.put("id",held.getMetadata());row.put("item",Item.getIdFromItem(held.getItem()));
        row.put("hashes",hashes);row.put("icons",icons);return row;
    }
    public static void main(String[] args)throws Exception {
        Bootstrap.register();int[] colors=new int[4096];
        for(int id=0;id<256;id++)for(int meta=0;meta<16;meta++){
            Block b=Block.getBlockById(id);IBlockState s;
            try{s=b.getStateFromMeta(meta);}catch(Exception e){s=b.getDefaultState();}
            colors[id*16+meta]=b.getMapColor(s).colorIndex;
        }
        List<Object> cases=new ArrayList<>();for(int scenario=0;scenario<3;scenario++)
            for(double[] p:new double[][]{{0.5,0.5},{-64.1,129.9}})cases.add(run(scenario,false,p[0],p[1]));
        cases.add(run(0,true,0.5,0.5));
        Map<String,Object> out=new LinkedHashMap<>();out.put("map_colors",colors);out.put("cases",cases);
        System.out.println(new Gson().toJson(out));
    }
}
