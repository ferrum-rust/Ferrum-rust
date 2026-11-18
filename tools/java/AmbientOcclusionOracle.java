// Execute the original BlockModelRenderer ambient-occlusion component on an
// immutable IBlockAccess. No graphics context, replacement renderer or formulas.
import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import com.google.gson.Gson;
import net.minecraft.block.*;
import net.minecraft.block.state.IBlockState;
import net.minecraft.client.renderer.*;
import net.minecraft.init.*;
import net.minecraft.tileentity.TileEntity;
import net.minecraft.util.*;
import net.minecraft.world.*;
import net.minecraft.world.biome.BiomeGenBase;

public class AmbientOcclusionOracle {
    static Field field(Class<?> c,String n)throws Exception {Field f=c.getDeclaredField(n);f.setAccessible(true);return f;}
    static long bits(float f){return Integer.toUnsignedLong(Float.floatToRawIntBits(f));}
    static int index(BlockPos p){return (p.getY()-62)*25+(p.getZ()+2)*5+p.getX()+2;}
    static class Grid implements IBlockAccess {
        int[] states,light;
        public IBlockState getBlockState(BlockPos p){int s=states[index(p)];return Block.getBlockById(s>>4).getStateFromMeta(s&15);}
        public int getCombinedLight(BlockPos p,int minimum){int n=light[index(p)];return (n>>4)<<20|Math.max(n&15,minimum)<<4;}
        public TileEntity getTileEntity(BlockPos p){return null;}
        public boolean isAirBlock(BlockPos p){return getBlockState(p).getBlock()==Blocks.air;}
        public BiomeGenBase getBiomeGenForCoords(BlockPos p){return BiomeGenBase.plains;}
        public boolean extendedLevelsInChunkCache(){return false;}
        public int getStrongPower(BlockPos p,EnumFacing f){return 0;}
        public WorldType getWorldType(){return WorldType.DEFAULT;}
    }
    public static void main(String[] args)throws Exception {
        Bootstrap.register();Map<String,Object> result=new LinkedHashMap<>();List<Object> facts=new ArrayList<>(),neighbor=new ArrayList<>(),translation=new ArrayList<>(),vertices=new ArrayList<>();
        for(int id=0;id<256;id++){Block b=Block.getBlockById(id);facts.add(Arrays.asList(b.isTranslucent()?1:0,bits(b.getAmbientOcclusionLightValue()),b.isOpaqueCube()?1:0,b instanceof BlockSlab?1:0));}
        Class<?> translationType=Class.forName("net.minecraft.client.renderer.BlockModelRenderer$VertexTranslations");Method translationMethod=translationType.getDeclaredMethod("getVertexTranslations",EnumFacing.class);translationMethod.setAccessible(true);
        for(EnumFacing face:EnumFacing.values()) {
            Object info=BlockModelRenderer.EnumNeighborInfo.getNeighbourInfo(face);Class<?> type=info.getClass();
            List<Integer> directions=new ArrayList<>();for(EnumFacing f:(EnumFacing[])field(type,"field_178276_g").get(info))directions.add(f.getIndex());
            List<Object> weights=new ArrayList<>();for(String name:new String[]{"field_178286_j","field_178287_k","field_178284_l","field_178285_m"}){List<Integer> values=new ArrayList<>();for(Object o:(Object[])field(type,name).get(info))values.add(field(o.getClass(),"field_178229_m").getInt(o));weights.add(values);}
            neighbor.add(Arrays.asList(directions,field(type,"field_178289_i").getBoolean(info),weights));
            Object t=translationMethod.invoke(null,face);List<Integer> trans=new ArrayList<>();for(String name:new String[]{"field_178191_g","field_178200_h","field_178201_i","field_178198_j"})trans.add(field(translationType,name).getInt(t));translation.add(trans);
            List<Object> verts=new ArrayList<>();for(int i=0;i<4;i++){EnumFaceDirection.VertexInformation v=EnumFaceDirection.getFacing(face).getVertexInformation(i);verts.add(Arrays.asList(v.xIndex,v.yIndex,v.zIndex));}vertices.add(verts);
        }
        result.put("facts",facts);result.put("neighbors",neighbor);result.put("translations",translation);result.put("vertices",vertices);
        Class<?> type=Class.forName("net.minecraft.client.renderer.BlockModelRenderer$AmbientOcclusionFace");Constructor<?> constructor=type.getDeclaredConstructor(BlockModelRenderer.class);constructor.setAccessible(true);Object ao=constructor.newInstance(new BlockModelRenderer());
        Method update=type.getDeclaredMethod("updateVertexBrightness",IBlockAccess.class,Block.class,BlockPos.class,EnumFacing.class,float[].class,BitSet.class);update.setAccessible(true);
        List<Integer> palette=new ArrayList<>();for(Block b:Block.blockRegistry)for(IBlockState state:b.getBlockState().getValidStates())palette.add(Block.getIdFromBlock(b)<<4|b.getMetaFromState(state));
        float[][] bounds={{0,1,0,1,0,1},{0,.5f,0,1,0,1},{.125f,.875f,.25f,.75f,.375f,.625f}};
        List<Object> boundBits=new ArrayList<>();for(float[] b:bounds){List<Long> row=new ArrayList<>();for(float f:b)row.add(bits(f));boundBits.add(row);}result.put("bounds",boundBits);
        Random rng=new Random(189);List<Object> worlds=new ArrayList<>();int count=0;
        for(int id=0;id<256;id++) {
            if(id>=8&&id<=11)continue; // Liquids have their own renderer.
            Grid grid=new Grid();grid.states=new int[125];grid.light=new int[125];
            for(int i=0;i<125;i++){grid.states[i]=palette.get(rng.nextInt(palette.size()));grid.light[i]=rng.nextInt(5)==0?0:rng.nextInt(256);}
            grid.states[62]=id<<4;List<Object> cases=new ArrayList<>();
            for(boolean fancy:new boolean[]{false,true})for(EnumFacing face:EnumFacing.values())for(int mask=0;mask<4;mask++)for(int b=0;b<bounds.length;b++) {
                Blocks.leaves.setGraphicsLevel(fancy);Blocks.leaves2.setGraphicsLevel(fancy);
                float[] size=new float[12];for(int i=0;i<6;i++){size[i]=bounds[b][i];size[i+6]=1-size[i];}
                BitSet flags=new BitSet();flags.set(0,(mask&1)!=0);flags.set(1,(mask&2)!=0);
                update.invoke(ao,grid,Block.getBlockById(id),new BlockPos(0,64,0),face,size,flags);
                List<Long> colors=new ArrayList<>();for(float f:(float[])field(type,"vertexColorMultiplier").get(ao))colors.add(bits(f));
                cases.add(Arrays.asList(fancy,face.getIndex(),mask,b,colors,((int[])field(type,"vertexBrightness").get(ao)).clone()));count++;
            }
            worlds.add(Arrays.asList(id,grid.states,grid.light,cases));
        }
        result.put("worlds",worlds);
        try(Writer writer=new OutputStreamWriter(new FileOutputStream(args[0]),"UTF-8")){new Gson().toJson(result,writer);}
        System.out.println("Original AO: "+count+" faces, "+palette.size()+" canonical neighbor states");
    }
}
