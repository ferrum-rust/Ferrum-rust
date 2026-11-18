import java.io.*;
import java.nio.file.*;
import java.nio.charset.StandardCharsets;
import java.util.*;
import com.google.gson.*;
import net.minecraft.block.Block;
import net.minecraft.block.BlockTorch;
import net.minecraft.client.renderer.block.model.*;
import net.minecraft.client.renderer.texture.TextureAtlasSprite;
import net.minecraft.client.resources.model.ModelRotation;
import net.minecraft.init.Bootstrap;
import net.minecraft.util.EnumFacing;

/** Bake the owned original model JSON with 1.8.9 FaceBakery. The sprite maps
 * UVs to one normalized texture-array layer, matching the Vulkan atlas layout. */
public final class TorchModelOracle {
    static final class Sprite extends TextureAtlasSprite {
        Sprite(){super("oracle");}
        public float getInterpolatedU(double texel){return (float)texel/16f;}
        public float getInterpolatedV(double texel){return (float)texel/16f;}
    }
    static String read(Path p)throws IOException{return new String(Files.readAllBytes(p),StandardCharsets.UTF_8);}
    public static void main(String[] args)throws Exception {
        Bootstrap.register();Path root=Paths.get(args[0]);JsonParser parser=new JsonParser();
        FaceBakery bakery=new FaceBakery();Sprite sprite=new Sprite();List<Object> rows=new ArrayList<>();
        for(int id:new int[]{50,75,76}) {
            String name=id==50?"torch":id==75?"unlit_redstone_torch":"redstone_torch";
            JsonObject variants=parser.parse(read(root.resolve("blockstates/"+name+".json"))).getAsJsonObject().getAsJsonObject("variants");
            Block block=Block.getBlockById(id);
            for(int meta=1;meta<=5;meta++){
                String facing=block.getStateFromMeta(meta).getValue(BlockTorch.FACING).getName();
                JsonObject variant=variants.getAsJsonObject("facing="+facing);
                JsonObject child=parser.parse(read(root.resolve("models/block/"+variant.get("model").getAsString()+".json"))).getAsJsonObject();
                ModelBlock base=ModelBlock.deserialize(read(root.resolve("models/"+child.get("parent").getAsString()+".json")));
                ModelRotation rotation=ModelRotation.getModelRotation(0,variant.has("y")?variant.get("y").getAsInt():0);
                List<Object> quads=new ArrayList<>();
                for(BlockPart part:base.getElements())for(Map.Entry<EnumFacing,BlockPartFace> face:part.mapFaces.entrySet()){
                    BakedQuad q=bakery.makeBakedQuad(part.positionFrom,part.positionTo,face.getValue(),sprite,face.getKey(),rotation,part.partRotation,false,part.shade);
                    List<Object> vertices=new ArrayList<>();int[] data=q.getVertexData();
                    for(int i=0;i<4;i++){
                        List<Long> vertex=new ArrayList<>();for(int offset:new int[]{0,1,2,4,5})vertex.add(Integer.toUnsignedLong(data[i*7+offset]));
                        vertices.add(vertex);
                    }
                    quads.add(Arrays.asList(face.getKey().getIndex(),q.getFace().getIndex(),vertices,Integer.toUnsignedLong(data[3])));
                }
                rows.add(Arrays.asList(id<<4|meta,child.getAsJsonObject("textures").get("torch").getAsString(),quads));
            }
        }
        try(Writer out=new OutputStreamWriter(new FileOutputStream(args[1]),"UTF-8")){new Gson().toJson(rows,out);}
        System.out.println("Original torch models: 15 states, 90 quads, 360 vertices");
    }
}
