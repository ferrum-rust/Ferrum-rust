import java.util.*;
import com.google.gson.Gson;
import net.minecraft.init.Bootstrap;
import net.minecraft.world.WorldType;
import net.minecraft.world.biome.*;
import net.minecraft.world.gen.layer.*;
import net.minecraft.block.Block;

/** Original class calls only. Registry metadata and output grids are data. */
public final class BiomeOracle {
    public static void main(String[] args) {
        Bootstrap.register();
        Map<String,Object> root=new LinkedHashMap<String,Object>();
        List<Map<String,Object>> registry=new ArrayList<Map<String,Object>>();
        for(int i=0;i<256;i++) {
            BiomeGenBase b=BiomeGenBase.getBiome(i);if(b==null)continue;
            Map<String,Object> row=new LinkedHashMap<String,Object>();
            row.put("id",i);row.put("name",b.biomeName);row.put("class",b.getBiomeClass().getSimpleName());
            row.put("height",Float.floatToRawIntBits(b.minHeight));row.put("variation",Float.floatToRawIntBits(b.maxHeight));
            row.put("temperature",Float.floatToRawIntBits(b.temperature));row.put("rainfall",Float.floatToRawIntBits(b.rainfall));
            row.put("snowy",b.isSnowyBiome());row.put("mesa",b instanceof BiomeGenMesa);row.put("temp_category",b.getTempCategory().ordinal());
            row.put("top",Block.BLOCK_STATE_IDS.get(b.topBlock));row.put("filler",Block.BLOCK_STATE_IDS.get(b.fillerBlock));registry.add(row);
        }
        root.put("registry",registry);
        List<Map<String,Object>> cases=new ArrayList<Map<String,Object>>();
        long[] seeds={0,1,-1,189,8675309,Long.MIN_VALUE,Long.MAX_VALUE};
        int[][] regions={{0,0,32,32},{-65,-33,64,48},{-513,1023,37,29},{29999968,-30000000,32,32},{-1024,-1024,256,256},{719,-389,128,128}};
        for(long seed:seeds) {
            GenLayer[] layers=GenLayer.initializeAllBiomeGenerators(seed,WorldType.DEFAULT,"");
            for(int layer=0;layer<2;layer++)for(int[] region:regions) {
                IntCache.resetIntCache();int[] grid=layers[layer].getInts(region[0],region[1],region[2],region[3]);
                Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("seed",Long.toString(seed));row.put("layer",layer);row.put("region",region);row.put("values",Arrays.copyOf(grid,region[2]*region[3]));cases.add(row);
            }
        }
        root.put("cases",cases);System.out.println(new Gson().toJson(root));
    }
}
