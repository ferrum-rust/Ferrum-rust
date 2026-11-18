import java.io.PrintStream;
import java.util.*;
import com.google.gson.Gson;
import net.minecraft.block.Block;
import net.minecraft.block.material.Material;
import net.minecraft.init.Bootstrap;

/** Numeric registry properties from original 1.8.9 classes, not copied lists. */
public final class MaterialOracle {
    public static void main(String[] args) {
        PrintStream output=System.out;System.setOut(System.err);Bootstrap.register();
        List<int[]> rows=new ArrayList<int[]>();
        for(int id=0;id<256;id++) {
            Block b=Block.getBlockById(id);Material m=b.getMaterial();
            int flags=(m.blocksMovement()?1:0)|(m.isSolid()?2:0)|(m.isLiquid()?4:0)|(m==Material.portal?8:0)
                |(b.isNormalCube()?16:0)|(b.isFullBlock()?32:0)|(m==Material.leaves?64:0)|(b.isFullCube()?128:0)
                |(m.isReplaceable()?256:0)|(m.isOpaque()?512:0)|(m.getCanBurn()?1024:0)|(m.blocksLight()?2048:0)|(b.isOpaqueCube()?4096:0)|(b.canProvidePower()?8192:0)|(m==Material.vine?16384:0)|(m==Material.circuits?32768:0);
            rows.add(new int[]{id,Block.getIdFromBlock(b),flags,(Block.getIdFromBlock(b)<<4)|b.getMetaFromState(b.getDefaultState()),b.getLightOpacity(),b.getLightValue(),b.getUseNeighborBrightness()?1:0});
        }
        List<Integer> unknown=new ArrayList<Integer>();for(int id:new int[]{-1,256,4095})unknown.add(Block.getIdFromBlock(Block.getBlockById(id)));
        Map<String,Object> result=new LinkedHashMap<String,Object>();
        result.put("bits",Arrays.asList("blocks_movement","is_solid","is_liquid","is_portal","normal_cube","full_block","leaves","full_cube","replaceable","opaque","can_burn","blocks_light","opaque_cube","can_provide_power","is_vine","is_circuits"));
        result.put("columns",Arrays.asList("requested_id","resolved_id","flags","default_state","light_opacity","light_value","neighbor_brightness"));result.put("rows",rows);result.put("unknown_resolved_ids",unknown);
        output.println(new Gson().toJson(result));
    }
}
