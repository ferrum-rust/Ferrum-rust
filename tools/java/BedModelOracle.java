import com.google.gson.*;
import java.io.*;
import java.nio.file.*;
import java.util.*;
import net.minecraft.block.*;
import net.minecraft.block.state.IBlockState;
import net.minecraft.client.renderer.block.model.*;
import net.minecraft.client.resources.model.ModelRotation;
import net.minecraft.init.*;
import net.minecraft.util.EnumFacing;

public final class BedModelOracle {
    public static void main(String[] args)throws Exception{
        Bootstrap.register();Path root=Paths.get(args[0]);
        JsonParser parser=new JsonParser();FaceBakery bakery=new FaceBakery();
        TorchModelOracle.Sprite sprite=new TorchModelOracle.Sprite();
        JsonObject variants=parser.parse(TorchModelOracle.read(root.resolve("blockstates/bed.json"))).getAsJsonObject().getAsJsonObject("variants");
        List<Object> rows=new ArrayList<>();int faces=0;
        for(int meta:new int[]{0,1,2,3,8,9,10,11,12,13,14,15}){
            IBlockState state=Blocks.bed.getStateFromMeta(meta);
            String key="facing="+state.getValue(BlockBed.FACING).getName()+",part="+state.getValue(BlockBed.PART).getName();
            JsonObject variant=variants.getAsJsonObject(key);
            String raw=TorchModelOracle.read(root.resolve("models/block/"+variant.get("model").getAsString()+".json"));
            ModelBlock model=ModelBlock.deserialize(raw);
            JsonObject textures=parser.parse(raw).getAsJsonObject().getAsJsonObject("textures");
            ModelRotation rotation=ModelRotation.getModelRotation(0,variant.has("y")?variant.get("y").getAsInt():0);
            List<Object> quads=new ArrayList<>();
            for(BlockPart part:model.getElements())for(Map.Entry<EnumFacing,BlockPartFace> face:part.mapFaces.entrySet()){
                BlockPartFace f=face.getValue();BakedQuad q=bakery.makeBakedQuad(part.positionFrom,part.positionTo,f,sprite,face.getKey(),rotation,part.partRotation,false,part.shade);
                List<Object> points=new ArrayList<>();int[] data=q.getVertexData();
                for(int n=0;n<4;n++){
                    List<Long> v=new ArrayList<>();
                    for(int offset:new int[]{0,1,2,4,5})v.add(Integer.toUnsignedLong(data[n*7+offset]));
                    points.add(v);
                }
                quads.add(Arrays.asList(textures.get(f.texture.substring(1)).getAsString(),q.getFace().getIndex(),points,Integer.toUnsignedLong(data[3])));faces++;
            }
            rows.add(Arrays.asList((26<<4)|meta,quads));
        }
        try(Writer out=new OutputStreamWriter(new FileOutputStream(args[1]),"UTF-8")){new Gson().toJson(rows,out);}
        System.out.println("Original bed models: "+rows.size()+" states / "+faces+" faces.");
    }
}
