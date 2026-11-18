import java.util.*;
import com.google.gson.Gson;
import net.minecraft.world.gen.*;

/** Calls the installed original noise classes; contains no replacement math. */
public final class NoiseOracle {
    static List<Map<String,Object>> cases = new ArrayList<Map<String,Object>>();
    static Map<String,Object> row(String kind,long seed) {
        Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("kind",kind);row.put("seed",Long.toString(seed));cases.add(row);return row;
    }
    static void values(Map<String,Object> row,double[] values) {
        List<String> bits=new ArrayList<String>();for(double d:values)bits.add(String.format(Locale.ROOT,"%016x",Double.doubleToRawLongBits(d)));row.put("bits",bits);
    }
    static void improved(long seed,double[] origin,int[] size,double[] scale) {
        Random random=new Random(seed);NoiseGeneratorImproved noise=new NoiseGeneratorImproved(random);
        double[] result=new double[size[0]*size[1]*size[2]];for(int i=0;i<result.length;i++)result[i]=i*0.03125;
        noise.populateNoiseArray(result,origin[0],origin[1],origin[2],size[0],size[1],size[2],scale[0],scale[1],scale[2],0.125);
        Map<String,Object> row=row("improved",seed);row.put("origin",origin);row.put("size",size);row.put("scale",scale);row.put("divisor",0.125);row.put("next_long",Long.toString(random.nextLong()));values(row,result);
    }
    static void octaves(long seed,int[] origin,int[] size,double[] scale,int count) {
        Random random=new Random(seed);NoiseGeneratorOctaves noise=new NoiseGeneratorOctaves(random,count);
        double[] result=noise.generateNoiseOctaves(null,origin[0],origin[1],origin[2],size[0],size[1],size[2],scale[0],scale[1],scale[2]);
        Map<String,Object> row=row("octaves",seed);row.put("origin",origin);row.put("size",size);row.put("scale",scale);row.put("count",count);row.put("next_long",Long.toString(random.nextLong()));values(row,result);
    }
    public static void main(String[] args) {
        long[] seeds={0L,1L,-1L,189L,123456789123456789L,Long.MIN_VALUE,Long.MAX_VALUE};
        for(long seed:seeds) {
            Random random=new Random(seed);Map<String,Object> rr=row("random",seed);List<String> sequence=new ArrayList<String>();
            int[] bounds={1,2,3,7,16,31,256,257,1000,1073741825,Integer.MAX_VALUE};
            for(int i=0;i<64;i++) {sequence.add(Integer.toString(random.nextInt()));sequence.add(Integer.toString(random.nextInt(bounds[i%bounds.length])));sequence.add(Long.toString(random.nextLong()));sequence.add(Integer.toUnsignedString(Float.floatToRawIntBits(random.nextFloat())));sequence.add(Long.toUnsignedString(Double.doubleToRawLongBits(random.nextDouble())));sequence.add(Boolean.toString(random.nextBoolean()));}
            byte[] bytes=new byte[37];random.nextBytes(bytes);rr.put("sequence",sequence);rr.put("bytes",bytes);
            improved(seed,new double[]{-17.5,64.25,101.125},new int[]{5,33,5},new double[]{0.03125,0.0625,0.03125});
            improved(seed,new double[]{-30000000.5,-10.0,16777216.25},new int[]{16,1,16},new double[]{0.0625,1.0,0.0625});
            improved(seed,new double[]{-255.5,-257.75,255.875},new int[]{3,7,4},new double[]{3.5,10.25,0.375});
            octaves(seed,new int[]{-4,0,12},new int[]{5,33,5},new double[]{684.412,684.412,684.412},16);
            octaves(seed,new int[]{30000000,0,-30000000},new int[]{5,1,5},new double[]{200.0,1.0,200.0},16);
            octaves(seed,new int[]{-100,-10,101},new int[]{3,7,4},new double[]{8.555149841308594,4.277574920654297,8.555149841308594},8);
            NoiseGeneratorSimplex simplex=new NoiseGeneratorSimplex(new Random(seed));
            double[][] points={{0.,0.},{-1.,-1.},{-10.5,16.25},{16777215.5,-16777216.25},{30000000.,-30000000.},{0.125,-0.625}};
            double[] result=new double[points.length];for(int i=0;i<points.length;i++)result[i]=simplex.func_151605_a(points[i][0],points[i][1]);
            Map<String,Object> r=row("simplex_point",seed);r.put("points",points);values(r,result);
            result=new double[16*16];simplex.func_151606_a(result,-176.0,320.0,16,16,0.0625,0.0625,0.55);
            r=row("simplex_region",seed);r.put("origin",new double[]{-176.0,320.0});r.put("size",new int[]{16,16});r.put("scale",new double[]{0.0625,0.0625});r.put("amplitude",0.55);values(r,result);
            NoiseGeneratorPerlin perlin=new NoiseGeneratorPerlin(new Random(seed),4);
            result=perlin.func_151600_a(null,-176.0,320.0,16,16,0.0625,0.0625,1.0,0.5);
            r=row("perlin_region",seed);r.put("origin",new double[]{-176.0,320.0});r.put("size",new int[]{16,16});r.put("scale",new double[]{0.0625,0.0625});r.put("lacunarity",1.0);r.put("persistence",0.5);r.put("count",4);values(r,result);
            result=new double[points.length];for(int i=0;i<points.length;i++)result[i]=perlin.func_151601_a(points[i][0],points[i][1]);
            r=row("perlin_point",seed);r.put("points",points);r.put("count",4);values(r,result);
        }
        Map<String,Object> root=new LinkedHashMap<String,Object>();root.put("schema",1);root.put("minecraft","1.8.9");root.put("java",System.getProperty("java.version"));root.put("cases",cases);
        System.out.println(new Gson().toJson(root));
    }
}
