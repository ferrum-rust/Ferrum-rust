import com.google.gson.*;
import java.nio.file.*;
import java.util.*;
import net.minecraft.client.renderer.block.model.*;
import net.minecraft.client.resources.model.*;
import net.minecraft.init.Bootstrap;
import net.minecraft.util.*;

/** Original weighted model selection and FaceBakery UV corners, including side overlays. */
public final class GrassModelOracle {
    public static void main(String[] args) throws Exception {
        Bootstrap.register();
        Path root = Paths.get(args[0]);
        Map<ResourceLocation, ModelBlock> models = new HashMap<>();
        for (String name : new String[]{"grass_normal", "grass", "grass_snowed", "cube_bottom_top", "cube"}) {
            Path path = root.resolve("models/block/" + name + ".json");
            models.put(new ResourceLocation("block/" + name), ModelBlock.deserialize(TorchModelOracle.read(path)));
        }
        for (ModelBlock model : models.values()) model.getParentFromMap(models);
        JsonObject variants = new JsonParser().parse(TorchModelOracle.read(root.resolve("blockstates/grass.json")))
                .getAsJsonObject().getAsJsonObject("variants");
        JsonArray normal = variants.getAsJsonArray("snowy=false");
        List<IBakedModel> ordered = new ArrayList<>();
        WeightedBakedModel.Builder weighted = new WeightedBakedModel.Builder();
        FaceBakery bakery = new FaceBakery();
        TorchModelOracle.Sprite sprite = new TorchModelOracle.Sprite();
        List<Object> faces = new ArrayList<>();
        for (int i = 0; i <= normal.size(); i++) {
            JsonObject variant = i == normal.size() ? variants.getAsJsonObject("snowy=true") : normal.get(i).getAsJsonObject();
            ModelBlock model = models.get(new ResourceLocation("block/" + variant.get("model").getAsString()));
            int y = variant.has("y") ? variant.get("y").getAsInt() : 0;
            ModelRotation rotation = ModelRotation.getModelRotation(0, y);
            SimpleBakedModel.Builder builder = new SimpleBakedModel.Builder(model).setTexture(sprite);
            for (BlockPart part : model.getElements()) {
                for (Map.Entry<EnumFacing, BlockPartFace> entry : part.mapFaces.entrySet()) {
                    BlockPartFace face = entry.getValue();
                    BakedQuad quad = bakery.makeBakedQuad(part.positionFrom, part.positionTo, face, sprite,
                            entry.getKey(), rotation, part.partRotation, false, part.shade);
                    EnumFacing side = quad.getFace();
                    builder.addFaceQuad(side, quad);
                    int[] data = quad.getVertexData();
                    int axis = side.getAxis() == EnumFacing.Axis.X ? 0 : side.getAxis() == EnumFacing.Axis.Y ? 1 : 2;
                    int[][] corners = new int[4][2];
                    for (int v = 0; v < 4; v++) {
                        int dx = Math.round(Float.intBitsToFloat(data[7*v + (axis+1)%3]));
                        int dy = Math.round(Float.intBitsToFloat(data[7*v + (axis+2)%3]));
                        int corner = dy == 0 ? dx : 3-dx;
                        corners[corner][0] = Math.round(Float.intBitsToFloat(data[7*v + 4]));
                        corners[corner][1] = Math.round(Float.intBitsToFloat(data[7*v + 5]));
                    }
                    faces.add(Arrays.asList(i, axis, side.getAxisDirection() == EnumFacing.AxisDirection.POSITIVE, corners));
                }
            }
            if (i < normal.size()) {
                IBakedModel baked = builder.makeBakedModel();
                ordered.add(baked);
                weighted.add(baked, 1);
            }
        }
        WeightedBakedModel original = weighted.build();
        List<int[]> positions = new ArrayList<>();
        for (int x : new int[]{-29999999,-17,-1,0,4,5,6,7,8,29999999}) {
            for (int z : new int[]{-17,-1,4,5,6,7,8,17}) {
                BlockPos p = new BlockPos(x,77,z);
                int index = ordered.indexOf(original.getAlternativeModel(MathHelper.getPositionRandom(p)));
                positions.add(new int[]{x,77,z,index});
            }
        }
        Map<String,Object> result = new LinkedHashMap<>();
        result.put("faces", faces);
        result.put("positions", positions);
        Files.write(Paths.get(args[1]), new Gson().toJson(result).getBytes("UTF-8"));
    }
}
