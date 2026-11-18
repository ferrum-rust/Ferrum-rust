import com.google.gson.Gson;
import java.io.*;
import java.util.*;
import java.util.zip.GZIPOutputStream;
import net.minecraft.block.Block;
import net.minecraft.block.state.IBlockState;
import net.minecraft.entity.Entity;
import net.minecraft.init.Bootstrap;
import net.minecraft.nbt.NBTTagCompound;
import net.minecraft.pathfinding.*;
import net.minecraft.util.BlockPos;
import net.minecraft.world.pathfinder.WalkNodeProcessor;

/** Original WalkNodeProcessor/PathFinder, including blocked and partial routes. */
public final class GroundPathOracle {
    static final class World extends FallingEntityOracle.TestWorld {
        int layout,ox,oz,probe=-1;
        World() { super(0); }
        public IBlockState getBlockState(BlockPos p) {
            if(probe>=0) return FallingEntityOracle.state(p.getX()==0 && p.getY()==1 && p.getZ()==0?probe:0);
            int x=p.getX()-ox,y=p.getY(),z=p.getZ()-oz;
            int raw=0;
            if(y==0) raw=16;
            if(layout==1 && x==2 && Math.abs(z)<4 && y>=1 && y<=3)raw=16;
            if(layout==2 && x>=2 && y==1)raw=16;
            if(layout==3 && x<-1 && y>=0 && y<=3)raw=16;
            if(layout==4 && x==2 && Math.abs(z)<=6 && y==1)raw=85<<4;
            if(layout==5 && x==2 && Math.abs(z)<=6 && y==1)raw=z==1?107<<4|4:113<<4;
            if(layout==6 && x>=1 && x<=3 && y==1)raw=9<<4;
            if(layout==7 && Math.abs(x)<4 && Math.abs(z)<7 && y==1)raw=10<<4;
            if(layout==8 && x==2 && Math.abs(z)<4 && y>=1 && y<=3)raw=(z==0&&y<3)?64<<4|(y==2?8:0):16;
            if(layout==9 && x==2 && Math.abs(z)<4 && y>=1 && y<=3)raw=(z==0&&y<3)?71<<4|(y==2?8:4):16;
            if(layout==10 && x==2 && Math.abs(z)<4 && y==1)raw=96<<4|Math.floorMod(z,8);
            if(layout==11 && x>=-4 && x<=4 && z==0 && y==1)raw=66<<4;
            if(layout==12 && x==2 && Math.abs(z)<5 && y==1)raw=78<<4|Math.floorMod(z,8);
            if(layout==13 && x>=2 && x<6 && y==0)raw=0;
            if(layout==14 && x<=2 && y>=1 && y<=5)raw=16;
            if(layout==15 && y>=1&&y<=2&&Math.floorMod(x*37+z*11,13)<3)raw=16;
            if(layout==16 && Math.abs(x)<3 && Math.abs(z)<3 && y<=3)raw=8<<4;
            if(layout==17 && Math.abs(z)<5 && y==3)raw=16;
            return FallingEntityOracle.state(raw);
        }
    }
    static final class Body extends Entity {
        boolean wet,lava;
        int limit;
        Body(World w,float width,float height,double x,double y,double z,int limit,boolean wet,boolean lava) {
            super(w);setSize(width,height);setPosition(x,y,z);this.wet=wet;this.lava=lava;this.limit=limit;
        }
        protected void entityInit(){}
        protected void readEntityFromNBT(NBTTagCompound n){}
        protected void writeEntityToNBT(NBTTagCompound n){}
        public int getMaxFallHeight(){return limit;}
        public boolean isInWater(){return wet;}
        public boolean isInLava(){return lava;}
    }
    static Object path(int i) {
        Random random=new Random(i*7319L-189);
        World w=new World();w.layout=i%18;
        w.ox=new int[]{0,-24,32768,16777217}[(i/18)%4];w.oz=(i/72)%2==0?7:-31;
        int type=random.nextInt(4);
        float width=new float[]{.6f,.9f,1.4f,.3f}[type],height=new float[]{1.8f,1.3f,2.9f,.9f}[type];
        double x=w.ox+random.nextInt(5)-3+random.nextDouble(),z=w.oz+random.nextInt(7)-3+random.nextDouble();
        double y=w.layout==14?6.:1.+(i%11==0?.6:0.);
        boolean wet=w.layout==16||i%31==0,lava=w.layout==7&&(i&1)==0;
        int limit=i%7;
        Body body=new Body(w,width,height,x,y,z,limit,wet,lava);
        double tx=w.ox+random.nextInt(17)-7+random.nextDouble(),tz=w.oz+random.nextInt(17)-8+random.nextDouble(),ty=i%5+1.;
        Body target=new Body(w,width,height,tx,ty,tz,limit,false,false);
        WalkNodeProcessor processor=new WalkNodeProcessor();
        int flags=random.nextInt(16);
        processor.setEnterDoors((flags&1)!=0);processor.setBreakDoors((flags&2)!=0);processor.setAvoidsWater((flags&4)!=0);processor.setCanSwim((flags&8)!=0);
        float distance=new float[]{4,8,16,24}[i%4];
        boolean block=i%2==0;
        PathFinder finder=new PathFinder(processor);
        PathEntity path=block?finder.createEntityPathTo(w,body,new BlockPos(tx,ty,tz),distance):finder.createEntityPathTo(w,body,target,distance);
        List<Object> points=null;
        if(path!=null){points=new ArrayList<>();for(int j=0;j<path.getCurrentPathLength();j++){PathPoint p=path.getPathPointFromIndex(j);points.add(new int[]{p.xCoord,p.yCoord,p.zCoord});}}
        return Arrays.asList(w.layout,w.ox,w.oz,type,new double[]{body.posX,body.posY,body.posZ},new double[]{tx,ty,tz},wet,lava,limit,flags,distance,block,points);
    }
    public static void main(String[]args)throws Exception {
        Bootstrap.register();Map<String,Object> output=new LinkedHashMap<>();
        World w=new World();List<Object> pass=new ArrayList<>();
        TreeSet<Integer> states=new TreeSet<>();
        for(Object b:Block.blockRegistry){Block block=(Block)b;for(IBlockState state:block.getBlockState().getValidStates())states.add(FallingEntityOracle.state(state));}
        BlockPos p=new BlockPos(0,1,0);
        for(int raw:states){w.probe=raw;pass.add(Arrays.asList(raw,w.getBlockState(p).getBlock().isPassable(w,p)));}
        List<Object> cases=new ArrayList<>();for(int i=0;i<1800;i++)cases.add(path(i));
        output.put("passable",pass);output.put("paths",cases);
        try(Writer writer=new OutputStreamWriter(new GZIPOutputStream(new FileOutputStream(args[0])),"UTF-8")){new Gson().toJson(output,writer);}
        System.out.println("Original ground pathfinder 1800 routes / "+pass.size()+" raw block passability states");
    }
}
