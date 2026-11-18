import com.google.gson.Gson;
import java.lang.reflect.Field;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.*;
import net.minecraft.client.model.*;

/** Read the closed lid, latch and body directly from the installed single and large models. */
public final class ChestModelOracle {
    public static void main(String[] args) throws Exception {
        Field quads = ModelBox.class.getDeclaredField("quadList");
        quads.setAccessible(true);
        List<Object> models = new ArrayList<>();
        for (ModelChest chest : new ModelChest[]{new ModelChest(), new ModelLargeChest()}) {
            List<Object> vertices = new ArrayList<>();
            for (ModelRenderer part : new ModelRenderer[]{chest.chestLid, chest.chestKnob, chest.chestBelow}) {
                for (TexturedQuad face : (TexturedQuad[])quads.get(part.cubeList.get(0))) {
                    for (PositionTextureVertex v : face.vertexPositions) {
                        vertices.add(new double[]{
                            (v.vector3D.xCoord + part.rotationPointX) / 16.,
                            1. - (v.vector3D.yCoord + part.rotationPointY) / 16.,
                            1. - (v.vector3D.zCoord + part.rotationPointZ) / 16.,
                            v.texturePositionX, v.texturePositionY
                        });
                    }
                }
            }
            models.add(vertices);
        }
        Files.write(Paths.get(args[0]), new Gson().toJson(models).getBytes("UTF-8"));
    }
}
