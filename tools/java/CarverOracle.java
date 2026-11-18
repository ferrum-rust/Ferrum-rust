import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import com.google.gson.Gson;
import net.minecraft.block.Block;
import net.minecraft.block.state.IBlockState;
import net.minecraft.init.Bootstrap;
import net.minecraft.util.MathHelper;
import net.minecraft.world.chunk.ChunkPrimer;
import net.minecraft.world.gen.MapGenCaves;
import net.minecraft.world.gen.MapGenRavine;

/** Differential fixture generator invoking the user's actual cave/ravine bytecode.
 * Synthetic primers deliberately have no grass/mycelium: no World instance is
 * needed by the original carvers until biome top-block restoration is requested.
 * Only MapGenBase's seed/neighbor dispatch is replicated in this harness.
 */
public final class CarverOracle {
    static final class Caves extends MapGenCaves {
        void run(long seed,int cx,int cz,ChunkPrimer primer) {
            rand.setSeed(seed);long a=rand.nextLong(),b=rand.nextLong();
            for(int x=cx-range;x<=cx+range;x++) for(int z=cz-range;z<=cz+range;z++) {
                rand.setSeed((long)x*a ^ (long)z*b ^ seed);
                recursiveGenerate(null,x,z,cx,cz,primer);
            }
        }
    }
    static final class Ravines extends MapGenRavine {
        void run(long seed,int cx,int cz,ChunkPrimer primer) {
            rand.setSeed(seed);long a=rand.nextLong(),b=rand.nextLong();
            for(int x=cx-range;x<=cx+range;x++) for(int z=cz-range;z<=cz+range;z++) {
                rand.setSeed((long)x*a ^ (long)z*b ^ seed);
                recursiveGenerate(null,x,z,cx,cz,primer);
            }
        }
    }
    static int input(int x,int y,int z,int pattern) {
        if(y==0) return 7<<4;
        if(pattern==1) {
            if(y%19==0) return (12<<4)|1;
            if(y%17==0) return 13<<4;
            if(y%13==0) return (159<<4)|14;
        }
        if(pattern==2 && y>=58 && y<=66) return 9<<4;
        return 1<<4;
    }
    static long byteHash(long hash,int value) {return (hash^(value&255))*0x100000001b3L;}
    public static void main(String[] args)throws Exception {
        PrintStream output=System.out;System.setOut(System.err);Bootstrap.register();
        List<Map<String,Object>> rows=new ArrayList<Map<String,Object>>();
        long[] seeds={0L,1L,-1L,123456789012345L,Long.MIN_VALUE};
        int[][] coords={{0,0},{1,-1},{-1,1},{-8,-7},{7,8},{16,-17},{-31,32},{113,-211}};
        for(long seed:seeds) for(int[] c:coords) for(int pattern=0;pattern<3;pattern++) for(int which=0;which<2;which++) {
            ChunkPrimer primer=new ChunkPrimer();
            for(int y=0;y<256;y++)for(int z=0;z<16;z++)for(int x=0;x<16;x++) {
                int s=input(x,y,z,pattern);primer.setBlockState(x,y,z,Block.getBlockById(s>>4).getStateFromMeta(s&15));
            }
            if(which==0)new Caves().run(seed,c[0],c[1],primer);else new Ravines().run(seed,c[0],c[1],primer);
            long hash=0xcbf29ce484222325L;int changed=0,air=0,lava=0;
            for(int y=0;y<256;y++)for(int z=0;z<16;z++)for(int x=0;x<16;x++) {
                IBlockState state=primer.getBlockState(x,y,z);
                int packed=Block.getIdFromBlock(state.getBlock())<<4|state.getBlock().getMetaFromState(state);
                hash=byteHash(byteHash(hash,packed),packed>>8);
                if(packed!=input(x,y,z,pattern))changed++;
                if(packed==0)air++;if((packed>>4)==10||(packed>>4)==11)lava++;
            }
            Map<String,Object> row=new LinkedHashMap<String,Object>();
            row.put("kind",which==0?"caves":"ravines");row.put("seed",Long.toString(seed));row.put("chunk",c);
            row.put("pattern",pattern);row.put("hash",Long.toUnsignedString(hash,16));
            row.put("changed",changed);row.put("air",air);row.put("lava",lava);rows.add(row);
        }
        Field field=MathHelper.class.getDeclaredField("SIN_TABLE");field.setAccessible(true);
        float[] table=(float[])field.get(null);long hash=0xcbf29ce484222325L;
        for(float value:table){int bits=Float.floatToRawIntBits(value);for(int i=0;i<4;i++)hash=byteHash(hash,bits>>>(8*i));}
        List<List<String>> angles=new ArrayList<List<String>>();
        for(float angle:new float[]{0f,-0f,0.1f,-0.1f,(float)Math.PI,-(float)Math.PI,1234567.5f,-1234567.5f,Float.MAX_VALUE,Float.NaN}) {
            angles.add(Arrays.asList(Integer.toUnsignedString(Float.floatToRawIntBits(angle)),Integer.toUnsignedString(Float.floatToRawIntBits(MathHelper.sin(angle))),Integer.toUnsignedString(Float.floatToRawIntBits(MathHelper.cos(angle)))));
        }
        Map<String,Object> result=new LinkedHashMap<String,Object>();result.put("sine_table_hash",Long.toUnsignedString(hash,16));result.put("angles",angles);result.put("cases",rows);
        output.println(new Gson().toJson(result));
    }
}
