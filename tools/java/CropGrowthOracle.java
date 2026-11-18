import java.io.*;
import java.util.*;
import com.google.gson.Gson;
import net.minecraft.block.Block;
import net.minecraft.block.state.IBlockState;
import net.minecraft.init.*;
import net.minecraft.item.*;
import net.minecraft.util.BlockPos;

/** Runs the installed original's actual randomTick and ItemDye.applyBonemeal callbacks. */
public class CropGrowthOracle {
    static final BlockPos P = new BlockPos(8,64,8);
    static class Farm extends PlantEventOracle.Fixture {
        int growthLight;
        boolean rain;
        @Override public int getLightFromNeighbors(BlockPos p){ return growthLight; }
        @Override public boolean isRainingAt(BlockPos p){ return rain; }
    }
    static void put(Farm w, int x, int y, int z, int id, int meta){
        w.blocks.put(P.add(x,y,z), PlantEventOracle.state((id<<4)|meta));
    }
    static List<int[]> cells(Map<BlockPos,IBlockState> blocks){
        List<int[]> result=new ArrayList<>();
        for(Map.Entry<BlockPos,IBlockState> cell:blocks.entrySet()){
            BlockPos p=cell.getKey();
            result.add(new int[]{p.getX(),p.getY(),p.getZ(),PlantEventOracle.raw(cell.getValue())});
        }
        result.sort((a,b)->a[0]!=b[0]?a[0]-b[0]:a[1]!=b[1]?a[1]-b[1]:a[2]-b[2]);
        return result;
    }
    static Map<String,Object> run(Farm w,int id,int age,int setup,int seed,boolean meal){
        w.blocks=new HashMap<>(); w.writes=new ArrayList<>(); w.light=15; w.growthLight=15; w.sky=true; w.rain=false;
        int floor=60, moisture=7;
        if(id==39||id==40){floor=setup==3?1:setup==1?110:3; moisture=0; w.light=setup==1||setup==2?15:8;}
        if(id==115){floor=88;moisture=0;}
        if(id==83||id==81){floor=12;moisture=0;}
        for(int x=-6;x<=6;x++)for(int z=-6;z<=6;z++)
            put(w,x,-1,z,floor,id==39||id==40?0:floor==60?(setup==0?0:moisture):0);
        put(w,0,0,0,id,age);
        if(id==60){put(w,0,-1,0,3,0);if(setup==1)put(w,4,1,4,9,0);if(setup==2)w.rain=true;if(setup==3)put(w,0,1,0,59,0);}
        if(id==83)put(w,1,-1,0,9,0);
        if(id==81||id==83){if(setup==1)put(w,0,-1,0,id,0);if(setup==2){put(w,0,-1,0,id,0);put(w,0,-2,0,id,0);}if(setup==3)put(w,0,1,0,1,0);}
        if(id==127){BlockPos dir=P.offset(net.minecraft.util.EnumFacing.getHorizontal(age&3));w.blocks.put(dir,PlantEventOracle.state(17<<4|3));}
        if(id==39||id==40){
            if(setup==2)for(int x=-6;x<=6;x++)for(int z=-6;z<=6;z++)put(w,x,-1,z,3,2);
            if(setup==4)put(w,0,2,0,1,0);
            if(setup==5)for(int i=1;i<=4;i++)put(w,i,0,0,id,0);
            if(setup==6){w.light=13;w.growthLight=13;}
        } else if(id!=60&&id!=81&&id!=83&&id!=127){
            if(setup==2){put(w,0,0,1,id,0);put(w,1,0,1,id,0);}
            if(setup==3)w.growthLight=8;
            if(setup==4)w.growthLight=9;
            if(setup==5&&(id==104||id==105))put(w,0,0,1,id==104?86:103,0);
        }
        Map<String,Object> row=new LinkedHashMap<>();
        row.put("initial",cells(w.blocks));row.put("light",w.light);row.put("growth_light",w.growthLight);row.put("rain",w.rain);row.put("seed",seed);row.put("meal",meal);
        w.rand.setSeed(seed);
        boolean accepted=false;
        if(meal){accepted=ItemDye.applyBonemeal(new ItemStack(Items.dye,2,15),w,P);}
        else {IBlockState s=w.getBlockState(P);s.getBlock().randomTick(w,P,s,w.rand);}
        row.put("accepted",accepted);row.put("writes",w.writes);row.put("next",Long.toString(w.rand.nextLong()));
        return row;
    }
    public static void main(String[] args)throws Exception{
        Bootstrap.register();
        java.lang.reflect.Field f=sun.misc.Unsafe.class.getDeclaredField("theUnsafe");f.setAccessible(true);
        Farm w=(Farm)((sun.misc.Unsafe)f.get(null)).allocateInstance(Farm.class);
        net.minecraft.world.World base=PlantEventOracle.fixture();
        for(String name:new String[]{"rand","worldInfo","worldAccesses"}){
            java.lang.reflect.Field field=net.minecraft.world.World.class.getDeclaredField(name);field.setAccessible(true);field.set(w,field.get(base));
        }
        List<Object> rows=new ArrayList<>();
        for(int id:new int[]{39,40,59,60,81,83,104,105,115,127,141,142}){
            int[] ages=id==39||id==40?new int[]{0}:id==127?new int[]{0,5,10}:id==115?new int[]{0,2,3}:id==81||id==83?new int[]{0,14,15}:new int[]{0,6,7};
            for(int age:ages) for(int setup=0;setup<(id==39||id==40?7:6);setup++){
                for(int seed:new int[]{0,2,18,25,56,60,73,4096,4098})rows.add(run(w,id,age,setup,seed,false));
                for(int seed:new int[]{0,4096})rows.add(run(w,id,age,setup,seed,true));
            }
        }
        List<Integer> tickIds=new ArrayList<>();
        for(int i=0;i<=197;i++){Block b=Block.getBlockById(i);if(b!=null&&b.getTickRandomly())tickIds.add(i);}
        try(Writer file=new FileWriter(args[0])){new Gson().toJson(Arrays.asList(tickIds,rows),file);}
    }
}
