import java.io.PrintStream;
import java.lang.reflect.Field;
import net.minecraft.init.Bootstrap;
import net.minecraft.item.Item;
import net.minecraft.stats.*;
import net.minecraft.entity.EntityList;

/** Export original registered stat IDs, formatting types and item aliases. */
public final class StatisticsOracle {
    static String id(StatBase stat) {return stat==null?"-":stat.statId;}
    public static void main(String[] args) throws Exception {
        PrintStream out=System.out;System.setOut(System.err);Bootstrap.register();
        Field type=StatBase.class.getDeclaredField("type");type.setAccessible(true);
        for(StatBase s:StatList.generalStats) {
            Object t=type.get(s);
            String format=t==StatBase.timeStatType?"time":t==StatBase.distanceStatType?"distance":t==StatBase.simpleStatType?"number":"decimal";
            out.println("g\t"+s.statId+"\t"+format);
        }
        for(StatCrafting s:StatList.objectMineStats) {
            int n=Item.getIdFromItem(s.func_150959_a());
            out.println("b\t"+n+"\t"+id(StatList.objectCraftStats[n])+"\t"+id(StatList.objectUseStats[n])+"\t"+s.statId);
        }
        for(StatCrafting s:StatList.itemStats) {
            int n=Item.getIdFromItem(s.func_150959_a());
            out.println("i\t"+n+"\t"+id(StatList.objectBreakStats[n])+"\t"+id(StatList.objectCraftStats[n])+"\t"+s.statId);
        }
        for(EntityList.EntityEggInfo egg:EntityList.entityEggs.values()) {
            out.println("m\t"+EntityList.getStringFromID(egg.spawnedID)+"\t"+id(egg.field_151512_d)+"\t"+id(egg.field_151513_e));
        }
    }
}
