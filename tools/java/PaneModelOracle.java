import com.google.gson.*;
import java.io.*;
import java.nio.file.*;
import java.util.*;
import net.minecraft.block.Block;
import net.minecraft.block.BlockStainedGlassPane;
import net.minecraft.client.renderer.block.model.*;
import net.minecraft.client.resources.model.ModelRotation;
import net.minecraft.init.Bootstrap;
import net.minecraft.util.*;

/** Fingerprint original baked faces in each connected pane/bars variant. */
public final class PaneModelOracle {
    static long add(long hash, int word) {
        return (hash ^ Integer.toUnsignedLong(word)) * 0x100000001b3L;
    }
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
        List<Object> rows = new ArrayList<>();
        int faces = 0;
        for (int id : new int[]{101, 102, 160}) {
            Block block = Block.getBlockById(id);
            for (int meta = 0; meta < (id == 160 ? 16 : 1); meta++) {
                String name = id == 101 ? "iron_bars" : "glass_pane";
                if (id == 160) name = block.getStateFromMeta(meta).getValue(BlockStainedGlassPane.COLOR).getName() + "_stained_glass_pane";
                JsonObject variants = new JsonParser().parse(TorchModelOracle.read(root.resolve("blockstates/" + name + ".json")))
                    .getAsJsonObject().getAsJsonObject("variants");
                for (int mask = 0; mask < 16; mask++) {
                    String variantKey = "east=" + ((mask & 2) != 0) + ",north=" + ((mask & 1) != 0)
                        + ",south=" + ((mask & 4) != 0) + ",west=" + ((mask & 8) != 0);
                    JsonObject variant = variants.getAsJsonObject(variantKey);
                    ModelBlock model = models.get(new ResourceLocation("block/" + variant.get("model").getAsString()));
                    ModelRotation rotation = ModelRotation.getModelRotation(0, variant.has("y") ? variant.get("y").getAsInt() : 0);
                    long hash = 0xcbf29ce484222325L;
                    int count = 0;
                    for (BlockPart part : model.getElements()) {
                        for (Map.Entry<EnumFacing, BlockPartFace> face : part.mapFaces.entrySet()) {
                            BlockPartFace f = face.getValue();
                            BakedQuad q = bakery.makeBakedQuad(part.positionFrom, part.positionTo, f, sprite,
                                face.getKey(), rotation, part.partRotation, false, part.shade);
                            String texture = model.resolveTextureName(f.texture).replace("blocks/", "");
                            for (char c : texture.toCharArray()) hash = add(hash, c);
                            hash = add(hash, q.getFace().getIndex());
                            hash = add(hash, f.cullFace == null ? 6 : rotation.rotateFace(f.cullFace).getIndex());
                            int[] v = q.getVertexData();
                            hash = add(hash, v[3] & 255);
                            for (int i = 0; i < 4; i++) {
                                for (int offset : new int[]{0, 1, 2, 4, 5}) hash = add(hash, v[7*i+offset]);
                            }
                            count++;
                        }
                    }
                    rows.add(Arrays.asList(id << 4 | meta, mask, count, Long.toUnsignedString(hash)));
                    faces += count;
                }
            }
        }
        try (Writer writer = new OutputStreamWriter(new FileOutputStream(args[1]), "UTF-8")) {
            new Gson().toJson(rows, writer);
        }
        System.out.println("Original panes and bars: " + rows.size() + " variants / " + faces + " faces.");
    }
}
