import com.google.gson.Gson;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import java.io.*;
import java.nio.file.*;
import java.util.*;
import net.minecraft.block.Block;
import net.minecraft.block.state.IBlockState;
import net.minecraft.client.renderer.BlockModelShapes;
import net.minecraft.client.renderer.block.model.*;
import net.minecraft.client.resources.model.ModelResourceLocation;
import net.minecraft.client.resources.model.ModelRotation;
import net.minecraft.init.Bootstrap;
import net.minecraft.util.*;

/** Export vanilla's metadata-to-resource mapping and reference faces for directional circuit models. */
public final class CircuitModelOracle {
    static void write(Object data, String filename) throws IOException {
        try (Writer writer = new OutputStreamWriter(new FileOutputStream(filename), "UTF-8")) {
            new Gson().toJson(data, writer);
        }
    }
    public static void main(String[] args) throws Exception {
        Bootstrap.register();
        Map<IBlockState,ModelResourceLocation> locations = new BlockModelShapes(null).getBlockStateMapper().putAllStateModelLocations();
        List<Object> registry = new ArrayList<>();
        for (Block block : Block.blockRegistry) {
            int id = Block.getIdFromBlock(block);
            for (int meta = 0; meta < 16; meta++) {
                IBlockState s;
                try { s = block.getStateFromMeta(meta); } catch (RuntimeException bad) { continue; }
                if (s == null || block.getMetaFromState(s) != meta) continue;
                ModelResourceLocation loc = locations.get(s);
                if (loc != null) registry.add(Arrays.asList(id << 4 | meta,loc.getResourcePath(),loc.getVariant()));
            }
        }
        write(registry,args[1]);
        Path root=Paths.get(args[0]);
        FaceBakery bakery=new FaceBakery();
        TorchModelOracle.Sprite sprite=new TorchModelOracle.Sprite();
        Map<ResourceLocation,ModelBlock> models=new HashMap<>();
        try (DirectoryStream<Path> files=Files.newDirectoryStream(root.resolve("models/block"),"*.json")) {
            for (Path path:files) models.put(new ResourceLocation("block/"+path.getFileName().toString().replace(".json","")), ModelBlock.deserialize(TorchModelOracle.read(path)));
        }
        for (ModelBlock model:models.values()) model.getParentFromMap(models);
        List<Object> samples=new ArrayList<>();
        String[] names = args.length > 3 ? Arrays.copyOfRange(args, 3, args.length) : new String[]{"lever","piston","sticky_piston","piston_head","redstone_wire","unpowered_repeater","powered_repeater","unpowered_comparator","powered_comparator","stone_monster_egg","cobblestone_monster_egg","stone_brick_monster_egg","mossy_brick_monster_egg","cracked_brick_monster_egg","chiseled_brick_monster_egg"};
        for(String name:names) {
            JsonObject variants=new JsonParser().parse(TorchModelOracle.read(root.resolve("blockstates/"+name+".json"))).getAsJsonObject().getAsJsonObject("variants");
            for(Map.Entry<String,com.google.gson.JsonElement> entry:variants.entrySet()){
                JsonObject v=entry.getValue().getAsJsonObject();
                ModelRotation rotation=ModelRotation.getModelRotation(v.has("x")?v.get("x").getAsInt():0,v.has("y")?v.get("y").getAsInt():0);
                ModelBlock model=models.get(new ResourceLocation("block/"+v.get("model").getAsString()));
                List<Object> faces=new ArrayList<>();
                for(BlockPart part:model.getElements()) for(Map.Entry<EnumFacing,BlockPartFace> f:part.mapFaces.entrySet()) {
                    BlockPartFace info=f.getValue();
                    BakedQuad q=bakery.makeBakedQuad(part.positionFrom,part.positionTo,info,sprite,f.getKey(),rotation,part.partRotation,v.has("uvlock") && v.get("uvlock").getAsBoolean(),part.shade);
                    int[] data=q.getVertexData();
                    List<Object> verts=new ArrayList<>();
                    for(int i=0;i<4;i++) verts.add(new float[]{Float.intBitsToFloat(data[7*i]),Float.intBitsToFloat(data[7*i+1]),Float.intBitsToFloat(data[7*i+2]),Float.intBitsToFloat(data[7*i+4]),Float.intBitsToFloat(data[7*i+5])});
                    faces.add(Arrays.asList(model.resolveTextureName(info.texture).replace("blocks/",""),q.getFace().getIndex(),info.cullFace==null?6:rotation.rotateFace(info.cullFace).getIndex(),data[3]&255,info.tintIndex,verts));
                }
                samples.add(Arrays.asList(name,entry.getKey(),faces));
            }
        }
        write(samples,args[2]);
        System.out.println(registry.size()+" block states; "+samples.size()+" original circuit variants");
    }
}
