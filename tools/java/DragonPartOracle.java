import java.io.PrintStream;
import java.util.*;
import com.google.gson.Gson;
import net.minecraft.entity.Entity;
import net.minecraft.entity.boss.EntityDragon;
import net.minecraft.init.Bootstrap;

/** Sample all seven attackable child entities while the client crosses the yaw seam and climbs. */
public final class DragonPartOracle{
    public static void main(String[] args)throws Exception{
        PrintStream out=System.out;System.setOut(System.err);Bootstrap.register();
        MovementOracle.TestWorld world=new MovementOracle.TestWorld(0,0,0);EntityDragon dragon=new EntityDragon(world);
        dragon.setLocationAndAngles(1,65,2,163,0);
        List<Object> rows=new ArrayList<>();
        for(int i=0;i<72;i++){
            if(i%4==0){dragon.setPositionAndRotation2(1+i*0.27,65+Math.sin(i*0.11)*3,2+i*0.14,(float)((170+i*5)%360-180),0,3,i%8==0);}
            double[] before={dragon.posY,dragon.rotationYaw};dragon.onLivingUpdate();
            double[] after={dragon.posX,dragon.posY,dragon.posZ,dragon.rotationYaw};
            List<double[]> parts=new ArrayList<>();
            for(Entity e:dragon.getParts())parts.add(new double[]{e.posX,e.posY,e.posZ,e.width,e.height,e.getEntityId()-dragon.getEntityId()});
            rows.add(Arrays.asList(before,after,parts));
        }
        out.println(new Gson().toJson(rows));
    }
}
