import com.google.gson.Gson;
import java.io.*;
import java.util.*;
import net.minecraft.block.Block;
import net.minecraft.init.Bootstrap;
import net.minecraft.util.EnumWorldBlockLayer;

/** Original registry layers; CUTOUT disables mipmaps in EntityRenderer. */
public final class CutoutLayerOracle {
    public static void main(String[] args) throws Exception {
        Bootstrap.register();
        List<Integer> ids = new ArrayList<Integer>();
        for (Block block : Block.blockRegistry) {
            if (block.getBlockLayer() == EnumWorldBlockLayer.CUTOUT) {
                ids.add(Block.getIdFromBlock(block));
            }
        }
        Collections.sort(ids);
        try (Writer out = new OutputStreamWriter(new FileOutputStream(args[0]), "UTF-8")) {
            new Gson().toJson(ids, out);
        }
        System.out.println("Original CUTOUT blocks: " + ids);
    }
}
