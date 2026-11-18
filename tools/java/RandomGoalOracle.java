import com.google.gson.Gson;
import java.io.*;
import java.util.*;
import java.util.zip.GZIPOutputStream;
import net.minecraft.entity.EntityCreature;
import net.minecraft.entity.ai.RandomPositionGenerator;
import net.minecraft.init.Bootstrap;
import net.minecraft.util.*;

/** Executes all three RandomPositionGenerator entry points with real homes
 * and a weight callback that records each candidate in evaluation order. */
public final class RandomGoalOracle {
    static final class Creature extends EntityCreature {
        List<int[]> probes=new ArrayList<>(); int pattern;
        Creature(FallingEntityOracle.TestWorld w){super(w);}
        public float getBlockPathWeight(BlockPos p) {
            probes.add(new int[]{p.getX(),p.getY(),p.getZ()});
            if(pattern==0)return 0;
            if(pattern==1)return -100000;
            if(pattern==2)return (p.getY()%3)==0?10f:-.125f;
            return Math.floorMod(p.getX()*31+p.getZ()*71+p.getY()*7,17)*.125f;
        }
    }
    static Object run(int n){
        FallingEntityOracle.TestWorld w=new FallingEntityOracle.TestWorld(0);
        Creature c=new Creature(w);Random inputs=new Random(-189L+n*17617L);
        long seed=inputs.nextLong();c.getRNG().setSeed(seed);c.pattern=n%4;
        double x=inputs.nextDouble()*80-40,y=inputs.nextDouble()*80-10,z=inputs.nextDouble()*80-40;
        c.setPosition(x,y,z);
        BlockPos center=new BlockPos(x+inputs.nextInt(13)-6,y+inputs.nextInt(7)-3,z+inputs.nextInt(13)-6);
        int radius=new int[]{-1,0,1,3,8,16,35}[n%7];
        if(radius>=0)c.setHomePosAndDistance(center,radius);
        int horiz=new int[]{0,1,2,5,10,16}[n%6],vertical=n%8;
        Vec3 to=new Vec3(x+inputs.nextDouble()*16-8,y+inputs.nextDouble()*12-6,z+inputs.nextDouble()*16-8);
        Vec3 v=n%3==0?RandomPositionGenerator.findRandomTarget(c,horiz,vertical):n%3==1?RandomPositionGenerator.findRandomTargetBlockTowards(c,horiz,vertical,to):RandomPositionGenerator.findRandomTargetBlockAwayFrom(c,horiz,vertical,to);
        try{return Arrays.asList(n,Long.toString(seed),new double[]{x,y,z},new int[]{center.getX(),center.getY(),center.getZ(),radius},horiz,vertical,new double[]{to.xCoord,to.yCoord,to.zCoord},c.probes,v==null?null:new int[]{(int)v.xCoord,(int)v.yCoord,(int)v.zCoord},Long.toString(ItemEntityOracle.seed(c.getRNG())));}catch(Exception e){throw new RuntimeException(e);}
    }
    public static void main(String[]args)throws Exception{
        Bootstrap.register();List<Object> rows=new ArrayList<>();for(int i=0;i<4096;i++)rows.add(run(i));
        try(Writer writer=new OutputStreamWriter(new GZIPOutputStream(new FileOutputStream(args[0])),"UTF-8")){new Gson().toJson(rows,writer);}
        System.out.println("Original random AI destinations 4096 seeded home/direction/weight cases");
    }
}
