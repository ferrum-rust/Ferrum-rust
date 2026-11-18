import java.io.*;
import java.util.*;
import java.lang.reflect.Field;
import com.google.gson.Gson;
import net.minecraft.init.Bootstrap;
import net.minecraft.block.state.IBlockState;
import net.minecraft.init.Blocks;
import net.minecraft.tileentity.TileEntity;
import net.minecraft.util.*;
import net.minecraft.world.*;
import net.minecraft.world.biome.*;

/** Original color routines with deterministic synthetic colormaps, no asset redistribution. */
public final class BiomeColorOracle {
    static final List<BiomeGenBase> biomes=new ArrayList<BiomeGenBase>();
    static final class Access implements IBlockAccess {
        int biome;
        Access(int id){biome=id;}
        public TileEntity getTileEntity(BlockPos p){return null;}
        public int getCombinedLight(BlockPos p,int light){return 0;}
        public IBlockState getBlockState(BlockPos p){return Blocks.grass.getDefaultState();}
        public boolean isAirBlock(BlockPos p){return false;}
        public boolean extendedLevelsInChunkCache(){return false;}
        public BiomeGenBase getBiomeGenForCoords(BlockPos p){return biome>=0?BiomeGenBase.getBiome(biome):biomes.get(Math.floorMod(p.getX()*7+p.getZ()*13,biomes.size()));}
        public int getStrongPower(BlockPos p,EnumFacing facing){return 0;}
        public WorldType getWorldType(){return WorldType.DEFAULT;}
    }
    public static void main(String[] args)throws Exception{
        PrintStream output=System.out;System.setOut(System.err);Bootstrap.register();
        int[] grass=new int[65536],foliage=new int[65536];
        for(int y=0;y<256;y++)for(int x=0;x<256;x++){
            grass[y*256+x]=((x*7+y*3)&255)<<16|((x*11+y*5)&255)<<8|((x*13+y*17)&255);
            foliage[y*256+x]=((x*3+y*19)&255)<<16|((x*5+y*23)&255)<<8|((x*29+y*7)&255);
        }
        ColorizerGrass.setGrassBiomeColorizer(grass);ColorizerFoliage.setFoliageBiomeColorizer(foliage);
        List<int[]> delegates=new ArrayList<int[]>();Field base=BiomeGenMutated.class.getDeclaredField("baseBiome");base.setAccessible(true);
        for(BiomeGenBase b:BiomeGenBase.getBiomeGenArray())if(b!=null){biomes.add(b);if(b instanceof BiomeGenMutated)delegates.add(new int[]{b.biomeID,((BiomeGenBase)base.get(b)).biomeID});}
        List<Map<String,Object>> rows=new ArrayList<Map<String,Object>>();
        for(int index=-1;index<biomes.size();index++)for(int[] xz:new int[][]{{0,0},{15,15},{16,-1},{-16,16},{-1234,9123},{29999983,-29999982},{32767,-32768}})for(int y:new int[]{0,64,65,80,128,255}){
            int id=index<0?-1:biomes.get(index).biomeID;Access access=new Access(id);BlockPos p=new BlockPos(xz[0],y,xz[1]);
            BiomeGenBase b=access.getBiomeGenForCoords(p);Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("biome",id);row.put("position",new int[]{xz[0],y,xz[1]});
            row.put("single",new int[]{b.getGrassColorAtPos(p),b.getFoliageColorAtPos(p),b.waterColorMultiplier});
            row.put("average",new int[]{BiomeColorHelper.getGrassColorAtPos(access,p),BiomeColorHelper.getFoliageColorAtPos(access,p),BiomeColorHelper.getWaterColorAtPos(access,p)});rows.add(row);
        }
        Map<String,Object> result=new LinkedHashMap<String,Object>();result.put("delegates",delegates);result.put("cases",rows);output.println(new Gson().toJson(result));
    }
}
