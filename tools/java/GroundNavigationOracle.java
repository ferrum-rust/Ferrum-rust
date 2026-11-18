import com.google.gson.Gson;
import java.io.*;
import java.util.*;
import java.util.zip.GZIPOutputStream;
import net.minecraft.entity.EntityLiving;
import net.minecraft.entity.ai.EntityMoveHelper;
import net.minecraft.init.Bootstrap;
import net.minecraft.pathfinding.*;
import net.minecraft.util.*;

/** Runs the original PathNavigateGround shortcut, stuck and move-helper code
 * with controlled body positions so navigation can be checked independently. */
public final class GroundNavigationOracle {
    static class Body extends EntityLiving {
        boolean wet,lava;
        Body(GroundPathOracle.World w,float width,float height){super(w);setSize(width,height);}
        public boolean isInWater(){return wet;}
        public boolean isInLava(){return lava;}
    }
    static final class Nav extends PathNavigateGround {
        Nav(Body e, GroundPathOracle.World w){super(e,w);}
        boolean direct(Vec3 from,Vec3 to,int x,int y,int z){return isDirectPathBetweenPoints(from,to,x,y,z);}
    }
    static PathEntity path(int[][] entries){
        PathPoint[] points=new PathPoint[entries.length];
        for(int i=0;i<entries.length;i++)points[i]=new PathPoint(entries[i][0],entries[i][1],entries[i][2]);
        return new PathEntity(points);
    }
    static Object run(int i)throws Exception {
        GroundPathOracle.World w=new GroundPathOracle.World();w.layout=i%18;w.ox=-3;w.oz=7;
        int type=i%4;
        float width=new float[]{.6f,.9f,1.4f,.3f}[type],height=new float[]{1.8f,1.3f,2.9f,.9f}[type];
        Body e=new Body(w,width,height);Nav nav=new Nav(e,w);
        int pattern=i%8;boolean swim=(i&1)==0;nav.setCanSwim(swim);
        float requirement=i%7==0?2.5f:1f;nav.setHeightRequirement(requirement);
        int[][] points=new int[14][3];for(int j=0;j<points.length;j++)points[j]=new int[]{w.ox+j/2,1+(pattern==6&&j>=8?1:0),w.oz+(j+1)/2};
        double offset=(int)(width+1f)*.5;
        List<Object> frames=new ArrayList<>();
        int ticks=pattern==4?112:28;
        for(int t=1;t<=ticks;t++){
            int j=pattern==4?0:Math.min(points.length-1,(t-1)/3);
            double x=points[j][0]+offset+.07,z=points[j][2]+offset+.09,y=points[j][1]+(pattern==1?.75:pattern==3?1.2:0);
            e.setPosition(x,y,z);e.onGround=pattern==1?t%4==0:pattern!=3;
            e.wet=w.layout==16;e.lava=w.layout==7;
            boolean install=t==1||(i%3==0 && t==16);
            Boolean accepted=install?nav.setPath(path(points),.6):null;
            FallingEntityOracle.field(EntityMoveHelper.class,"update").setBoolean(e.getMoveHelper(),false);
            nav.onUpdateNavigation();
            PathEntity p=nav.getPath();EntityMoveHelper m=e.getMoveHelper();
            frames.add(Arrays.asList(new double[]{x,y,z},e.onGround,e.wet,e.lava,install,accepted,
                p==null?null:new int[]{p.getCurrentPathIndex(),p.getCurrentPathLength()},m.isUpdating()?FallingEntityOracle.bits(m.getX(),m.getY(),m.getZ(),m.getSpeed()):null));
        }
        List<Object> directs=new ArrayList<>();Random r=new Random(i*67819L+189);
        for(int k=0;k<72;k++){
            Vec3 a=new Vec3(w.ox+r.nextInt(13)-6+(k%3==0?0:.5),1+r.nextInt(3),w.oz+r.nextInt(13)-6+(k%4==0?0:.5));
            Vec3 b=new Vec3(k%5==0?a.xCoord:w.ox+r.nextInt(15)-7+.5,a.yCoord+k%3,k%6==0?a.zCoord:w.oz+r.nextInt(15)-7+.5);
            int sx=(int)Math.ceil(width),sy=(int)height+1;
            directs.add(Arrays.asList(new double[]{a.xCoord,a.yCoord,a.zCoord},new double[]{b.xCoord,b.yCoord,b.zCoord},nav.direct(a,b,sx,sy,sx)));
        }
        return Arrays.asList(w.layout,w.ox,w.oz,type,swim,requirement,points,frames,directs);
    }
    public static void main(String[]args)throws Exception {
        Bootstrap.register();List<Object> cases=new ArrayList<>();for(int i=0;i<144;i++)cases.add(run(i));
        try(Writer out=new OutputStreamWriter(new GZIPOutputStream(new FileOutputStream(args[0])),"UTF-8")){new Gson().toJson(cases,out);}
        System.out.println("Original ground navigator 144 traces / 10,368 direct-path checks");
    }
}
