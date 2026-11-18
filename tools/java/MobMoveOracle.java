import com.google.gson.Gson;
import java.io.*;
import java.util.*;
import java.util.zip.GZIPOutputStream;
import net.minecraft.entity.*;
import net.minecraft.entity.ai.*;
import net.minecraft.init.Bootstrap;
import net.minecraft.util.MathHelper;
public final class MobMoveOracle {
    static long bits(float f){return Integer.toUnsignedLong(Float.floatToRawIntBits(f));}
    static Object scenario(int i)throws Exception{
        GroundPathOracle.World w=new GroundPathOracle.World();w.layout=i%18;
        GroundNavigationOracle.Body e=new GroundNavigationOracle.Body(w,1.4f,2.9f);
        Random r=new Random(i*1378941L-189);
        double attribute=new double[]{0.,0.1,0.23,0.25,0.5,4.,32.}[i%7];e.getEntityAttribute(SharedMonsterAttributes.movementSpeed).setBaseValue(attribute);
        e.rotationYaw=(r.nextFloat()-.5f)*1440;float initial=e.rotationYaw;
        List<Object> frames=new ArrayList<>();
        for(int t=1;t<=32;t++){
            double x=r.nextDouble()*30-15,y=t%4==0?1.5001:1.1,z=r.nextDouble()*28-14;
            double dx=t%5==0?0:r.nextDouble()*6-3,dz=t%7==0?0:r.nextDouble()*5-2.5;
            double dy=t%3==0?0:t%3==1?1.0:-.5;
            if(t%11==0){dx=0;dz=0;dy=0;}
            boolean command=t%9!=0;
            double speed=new double[]{0.,.6,1.,1.2,1.6,4.}[t%6];
            e.setPosition(x,y,z);
            double[] target={x+dx,Math.floor(y+.5)+dy,z+dz};
            EntityMoveHelper move=e.getMoveHelper();
            if(command)move.setMoveTo(target[0],target[1],target[2],speed);
            move.onUpdateMoveHelper();
            boolean jumping=FallingEntityOracle.field(EntityJumpHelper.class,"isJumping").getBoolean(e.getJumpHelper());
            e.getJumpHelper().doJump();
            frames.add(Arrays.asList(new double[]{x,y,z},command,target,speed,bits(e.rotationYaw),bits(e.getAIMoveSpeed()),bits(e.moveForward),jumping));
        }
        return Arrays.asList(attribute,bits(initial),frames);
    }
    public static void main(String[]args)throws Exception{
        Bootstrap.register();Map<String,Object> output=new LinkedHashMap<>();List<Object> cases=new ArrayList<>(),angles=new ArrayList<>();
        for(int i=0;i<128;i++)cases.add(scenario(i));
        Random r=new Random(189);for(int i=0;i<4096;i++){
            double x=i%13==0?-0.:r.nextDouble()*8-4,y=i%11==0?0.:r.nextDouble()*8-4;
            angles.add(FallingEntityOracle.bits(y,x,MathHelper.atan2(y,x)));
        }
        output.put("cases",cases);output.put("angles",angles);
        List<Object> tables=new ArrayList<>();for(String name:new String[]{"field_181164_e","field_181165_f"}){
            double[] values=(double[])FallingEntityOracle.field(MathHelper.class,name).get(null);tables.add(FallingEntityOracle.bits(values));
        }
        output.put("tables",tables);
        try(Writer writer=new OutputStreamWriter(new GZIPOutputStream(new FileOutputStream(args[0])),"UTF-8")){new Gson().toJson(output,writer);}
        System.out.println("Original EntityMoveHelper 4096 movement updates and 4096 MathHelper.atan2 bit patterns");
    }
}
