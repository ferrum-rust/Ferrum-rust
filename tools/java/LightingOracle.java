// Verification only: real WorldServer/Chunk lighting and block-change callbacks.
import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import net.minecraft.block.Block;
import net.minecraft.init.Bootstrap;
import net.minecraft.util.BlockPos;
import net.minecraft.world.*;
import net.minecraft.world.chunk.Chunk;
import com.google.gson.GsonBuilder;

public class LightingOracle {
    static Field field(Class<?> c,String name) throws Exception {Field f=c.getDeclaredField(name);f.setAccessible(true);return f;}
    static Object snapshot(WorldServer w,String name,int x,int z) throws Exception {
        Map<String,Object> s=new LinkedHashMap<>();s.put("name",name);
        List<Object> chunks=new ArrayList<>();
        for(int cx=-2;cx<=2;cx++)for(int cz=-2;cz<=2;cz++) {
            Chunk c=w.getChunkFromChunkCoords(cx,cz);Map<String,Object> row=new LinkedHashMap<>();
            row.put("pos",new int[]{cx,cz});row.put("height",c.getHeightMap().clone());
            row.put("minimum",field(Chunk.class,"heightMapMinimum").getInt(c));
            row.put("gaps",((boolean[])field(Chunk.class,"updateSkylightColumns").get(c)).clone());
            row.put("queued_checks",field(Chunk.class,"queuedLightChecks").getInt(c));
            boolean[] sections=new boolean[16];for(int i=0;i<16;i++)sections[i]=c.getBlockStorageArray()[i]!=null;row.put("sections",sections);
            List<int[]> rle=new ArrayList<>();int previous=-1,count=0;
            for(int y=0;y<256;y++)for(int dz=0;dz<16;dz++)for(int dx=0;dx<16;dx++) {
                BlockPos p=new BlockPos(cx*16+dx,y,cz*16+dz);
                int v=(c.getLightFor(EnumSkyBlock.SKY,p)<<4)|c.getLightFor(EnumSkyBlock.BLOCK,p);
                if(previous!=v&&count>0){rle.add(new int[]{count,previous});count=0;}previous=v;count++;
            }
            rle.add(new int[]{count,previous});row.put("light",rle);chunks.add(row);
        }
        s.put("chunks",chunks);List<int[]> queries=new ArrayList<>();
        for(int subtract:new int[]{0,11}) {
            w.setSkylightSubtracted(subtract);
            for(int y:new int[]{-1,0,1,3,4,5,6,7,8,15,16,17,31,32,63,254,255,256})for(int dx=-2;dx<=2;dx++) {
                BlockPos p=new BlockPos(x+dx,y,z);
                queries.add(new int[]{subtract,p.getX(),p.getY(),p.getZ(),w.getLightFor(EnumSkyBlock.SKY,p),w.getLightFor(EnumSkyBlock.BLOCK,p),w.getLightFromNeighborsFor(EnumSkyBlock.SKY,p),w.getLightFromNeighborsFor(EnumSkyBlock.BLOCK,p),w.getLightFromNeighbors(p),w.getLight(p)});
            }
        }
        w.setSkylightSubtracted(0);s.put("queries",queries);return s;
    }
    static void edits(WorldServer w,List<Object> steps,String name,int x,int z,List<int[]> edits) throws Exception {
        for(int[] e:edits){Block b=Block.getBlockById(e[3]>>4);w.setBlockState(new BlockPos(e[0],e[1],e[2]),b.getStateFromMeta(e[3]&15),2);}
        Map<String,Object> step=new LinkedHashMap<>();step.put("edits",edits);step.put("snapshot",snapshot(w,name,x,z));steps.add(step);
    }
    static Object run(File root,int number,boolean noSky,int x,int z) throws Exception {
        WorldServer w=BlockTickOracle.world(new File(root,"lighting_"+number));
        field(WorldProvider.class,"hasNoSky").setBoolean(w.provider,noSky);
        Map<String,Object> row=new LinkedHashMap<>();row.put("no_sky",noSky);row.put("sea_level",w.getSeaLevel());row.put("origin",new int[]{x,z});List<Object> steps=new ArrayList<>();
        edits(w,steps,"initial",x,z,new ArrayList<int[]>());
        List<int[]> roof=new ArrayList<>();for(int dx=-4;dx<=4;dx++)for(int dz=-4;dz<=4;dz++)roof.add(new int[]{x+dx,8,z+dz,1<<4});
        edits(w,steps,"roof",x,z,roof);
        edits(w,steps,"lamps",x,z,Arrays.asList(new int[]{x-2,4,z,89<<4},new int[]{x+2,4,z,169<<4},new int[]{x,4,z,44<<4},new int[]{x+1,4,z,(44<<4)|8},new int[]{x,5,z,20<<4}));
        edits(w,steps,"one_lamp_removed",x,z,Arrays.asList(new int[]{x-2,4,z,0}));
        edits(w,steps,"both_lamps_removed",x,z,Arrays.asList(new int[]{x+2,4,z,0}));
        edits(w,steps,"translucent_roof",x,z,Arrays.asList(new int[]{x,8,z,20<<4},new int[]{x+1,8,z,18<<4},new int[]{x-1,8,z,9<<4}));
        edits(w,steps,"shaft",x,z,Arrays.asList(new int[]{x,8,z,0},new int[]{x,5,z,0},new int[]{x,4,z,0}));
        List<int[]> upper=new ArrayList<>();for(int dx=-1;dx<=1;dx++)for(int dz=-1;dz<=1;dz++)upper.add(new int[]{x+dx,32,z+dz,1<<4});
        upper.add(new int[]{x,17,z,89<<4});edits(w,steps,"new_sections",x,z,upper);
        edits(w,steps,"highest_column",x,z,Arrays.asList(new int[]{x,255,z,1<<4},new int[]{x+1,254,z,89<<4}));
        edits(w,steps,"remove_highest",x,z,Arrays.asList(new int[]{x,255,z,0},new int[]{x+1,254,z,0}));
        Method gaps=Chunk.class.getDeclaredMethod("recheckGaps",boolean.class);gaps.setAccessible(true);
        for(boolean one:new boolean[]{true,false}) {
            List<int[]> ops=new ArrayList<>();for(int cx=-1;cx<=1;cx++)for(int cz=-1;cz<=1;cz++){gaps.invoke(w.getChunkFromChunkCoords(cx,cz),one);ops.add(new int[]{cx,cz,one?1:0});}
            Map<String,Object> step=new LinkedHashMap<>();step.put("gaps",ops);step.put("snapshot",snapshot(w,one?"one_gap":"all_gaps",x,z));steps.add(step);
        }
        for(int cx=-2;cx<=2;cx++)for(int cz=-2;cz<=2;cz++)w.getChunkFromChunkCoords(cx,cz).setTerrainPopulated(false);
        List<int[]> init=new ArrayList<>();
        for(int[] p:new int[][]{{0,0},{-1,0},{1,0},{2,2}}) {
            Chunk c=w.getChunkFromChunkCoords(p[0],p[1]);c.func_150809_p();init.add(new int[]{p[0],p[1],c.isLightPopulated()?1:0});
        }
        Map<String,Object> initialization=new LinkedHashMap<>();initialization.put("initialize",init);initialization.put("snapshot",snapshot(w,"initialize",x,z));steps.add(initialization);
        List<int[]> raw=Arrays.asList(new int[]{3,10,3,89<<4},new int[]{15,18,3,89<<4},new int[]{8,31,15,89<<4});
        for(int[] e:raw){Block b=Block.getBlockById(e[3]>>4);w.getChunkFromChunkCoords(e[0]>>4,e[2]>>4).setBlockState(new BlockPos(e[0],e[1],e[2]),b.getStateFromMeta(e[3]&15));}
        w.getChunkFromChunkCoords(0,0).resetRelightChecks();
        int previous=0;for(int target:new int[]{1,8,512}) {
            for(int n=previous;n<target;n++)w.getChunkFromChunkCoords(0,0).enqueueRelightChecks();
            Map<String,Object> step=new LinkedHashMap<>();if(previous==0){step.put("raw_edits",raw);step.put("reset_checks",true);}
            step.put("enqueue",target-previous);step.put("snapshot",snapshot(w,"queued_"+target,x,z));steps.add(step);previous=target;
        }
        row.put("steps",steps);return row;
    }
    public static void main(String[] args) throws Exception {
        Bootstrap.register();List<Object> cases=new ArrayList<>();int i=0;
        for(boolean noSky:new boolean[]{false,true})for(int[] origin:new int[][]{{8,8},{15,0},{-1,-1},{-16,15}})cases.add(run(new File(args[1]),i++,noSky,origin[0],origin[1]));
        try(Writer w=new OutputStreamWriter(new FileOutputStream(args[0]),"UTF-8")){new GsonBuilder().serializeNulls().create().toJson(cases,w);}
        System.out.println("Original lighting oracle: "+cases.size()+" worlds, 128 checkpoints");System.exit(0);
    }
}
