import com.google.gson.Gson;
import java.io.*;
import java.util.*;
import java.util.zip.GZIPOutputStream;
import net.minecraft.block.state.IBlockState;
import net.minecraft.entity.Entity;
import net.minecraft.entity.monster.EntityIronGolem;
import net.minecraft.init.Bootstrap;
import net.minecraft.nbt.*;
import net.minecraft.util.BlockPos;
import net.minecraft.village.Village;

/** Exercises Village.tick's rare 1/7000 golem branch without replacing it. */
public final class VillageGolemOracle {
    static final class World extends FallingEntityOracle.TestWorld {
        final int layout;
        final List<Object> births=new ArrayList<>();
        World(int layout){super(0);this.layout=layout;}
        public IBlockState getBlockState(BlockPos p) {
            int x=p.getX(),y=p.getY(),z=p.getZ();
            if(x>=-3&&x<=3&&z>=-1&&z<=1&&(y==1||y==2))return FallingEntityOracle.state(64<<4|(y==1?0:8));
            if(y==0&&layout%5!=4)return FallingEntityOracle.state((layout%5==2?44:1)<<4|(layout%5==2?8:0));
            if(layout%5==1&&x<0&&y>=1&&y<=4)return FallingEntityOracle.state(16);
            return FallingEntityOracle.state(0);
        }
        public boolean spawnEntityInWorld(Entity e){
            if(e instanceof EntityIronGolem)births.add(Arrays.asList(e.posX,e.posY,e.posZ));
            return true;
        }
    }
    static long eligible(long start){
        Random random=new Random();
        for(long seed=start;;seed++){random.setSeed(seed);random.nextInt(50);if(random.nextInt(7000)==0)return seed;}
    }
    static Object run(int i)throws Exception {
        int layout=i%5,pop=i%11==0?9:10+(i%20),golems=i%13==0?4:0,limit=i%7==0?20:21;
        long seed=i%6==0?i:eligible(i*219907L);
        World w=new World(layout);
        NBTTagCompound tag=new NBTTagCompound();tag.setInteger("PopSize",pop);tag.setInteger("Golems",golems);tag.setInteger("Radius",32);tag.setInteger("CY",1);
        NBTTagList doors=new NBTTagList();int count=0;
        for(int x=-3;x<=3;x++)for(int z=-1;z<=1;z++)if(count++<limit){
            NBTTagCompound door=new NBTTagCompound();door.setInteger("X",x);door.setInteger("Y",1);door.setInteger("Z",z);door.setInteger("IDX",2);door.setInteger("TS",1);doors.appendTag(door);
        }
        tag.setTag("Doors",doors);
        Village v=new Village(w);v.readVillageDataFromNBT(tag);w.rand.setSeed(seed);v.tick(1);
        return Arrays.asList(layout,pop,golems,limit,Long.toString(seed),w.births,Long.toString(ItemEntityOracle.seed(w.rand)));
    }
    public static void main(String[]args)throws Exception {
        Bootstrap.register();List<Object> rows=new ArrayList<>();for(int i=0;i<160;i++)rows.add(run(i));
        try(Writer out=new OutputStreamWriter(new GZIPOutputStream(new FileOutputStream(args[0])),"UTF-8")){new Gson().toJson(rows,out);}
        System.out.println("Original Village.tick 160 golem eligibility/placement cases");
    }
}
