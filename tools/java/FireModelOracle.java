import com.google.gson.*;
import java.io.*;
import java.nio.file.*;
import java.util.*;
import net.minecraft.client.renderer.block.model.*;
import net.minecraft.client.resources.model.ModelRotation;
import net.minecraft.init.Bootstrap;
import net.minecraft.util.*;

/** Sample the installed game's FaceBakery for every placed-fire variant. */
public final class FireModelOracle {
    public static void main(String[] args) throws Exception {
        Bootstrap.register();
        Path root = Paths.get(args[0]);
        FaceBakery bakery = new FaceBakery();
        TorchModelOracle.Sprite sprite = new TorchModelOracle.Sprite();
        Map<ResourceLocation, ModelBlock> models = new HashMap<>();
        try (DirectoryStream<Path> files = Files.newDirectoryStream(root.resolve("models/block"), "*.json")) {
            for (Path path : files) {
                String name = path.getFileName().toString().replace(".json", "");
                models.put(new ResourceLocation("block/" + name), ModelBlock.deserialize(TorchModelOracle.read(path)));
            }
        }
        for (ModelBlock model : models.values()) model.getParentFromMap(models);
        JsonObject variants = new JsonParser().parse(TorchModelOracle.read(root.resolve("blockstates/fire.json")))
            .getAsJsonObject().getAsJsonObject("variants");
        List<Object> rows = new ArrayList<>();
        for (int mask = 0; mask < 16; mask++) {
            for (int upper = 0; upper < 3; upper++) {
                for (boolean alt : new boolean[]{false, true}) {
                    for (boolean flip : new boolean[]{false, true}) {
                        String key = "alt=" + alt + ",east=" + ((mask & 2) != 0) + ",flip=" + flip
                            + ",north=" + ((mask & 1) != 0) + ",south=" + ((mask & 4) != 0)
                            + ",upper=" + upper + ",west=" + ((mask & 8) != 0);
                        JsonObject variant = variants.getAsJsonObject(key);
                        ModelBlock model = models.get(new ResourceLocation("block/" + variant.get("model").getAsString()));
                        ModelRotation rotation = ModelRotation.getModelRotation(0, variant.has("y") ? variant.get("y").getAsInt() : 0);
                        List<Object> faces = new ArrayList<>();
                        for (BlockPart part : model.getElements()) {
                            for (Map.Entry<EnumFacing, BlockPartFace> face : part.mapFaces.entrySet()) {
                                BlockPartFace f = face.getValue();
                                BakedQuad q = bakery.makeBakedQuad(part.positionFrom, part.positionTo, f, sprite,
                                    face.getKey(), rotation, part.partRotation, false, part.shade);
                                int[] v = q.getVertexData();
                                List<Object> vertices = new ArrayList<>();
                                for (int i = 0; i < 4; i++) {
                                    List<Object> point = new ArrayList<>();
                                    for (int offset : new int[]{0, 1, 2, 4, 5}) point.add(Integer.toUnsignedLong(v[7*i+offset]));
                                    vertices.add(point);
                                }
                                faces.add(Arrays.asList(model.resolveTextureName(f.texture).replace("blocks/", ""),
                                    q.getFace().getIndex(), v[3] & 255, vertices));
                            }
                        }
                        int index = mask | upper << 4 | (alt ? 64 : 0) | (flip ? 128 : 0);
                        rows.add(Arrays.asList(index, faces));
                    }
                }
            }
        }
        try (Writer writer = new OutputStreamWriter(new java.util.zip.GZIPOutputStream(new FileOutputStream(args[1])), "UTF-8")) {
            new Gson().toJson(rows, writer);
        }
        System.out.println("Original placed-fire variants: " + rows.size());
    }
}
