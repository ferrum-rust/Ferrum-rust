import com.google.gson.Gson;
import java.io.*;
import java.util.*;
import java.util.zip.GZIPOutputStream;
import net.minecraft.block.state.IBlockState;
import net.minecraft.entity.Entity;
import net.minecraft.entity.passive.EntityVillager;
import net.minecraft.init.Bootstrap;
import net.minecraft.util.*;
import net.minecraft.village.*;

/** Original VillageCollection.tick, door scans/merges, center, radius and reputation. */
public final class VillageHomeOracle {
    static class World extends FallingEntityOracle.TestWorld {
        final Map<BlockPos,Integer> map=new HashMap<>();
        final List<EntityVillager> residents=new ArrayList<>();
        World(){super(0);}
        public IBlockState getBlockState(BlockPos p){return FallingEntityOracle.state(map.getOrDefault(p,0));}
        public boolean canSeeSky(BlockPos p){
            for(int y=p.getY();y<=8;y++)if(getBlockState(new BlockPos(p.getX(),y,p.getZ())).getBlock().getLightOpacity()>0)return false;
            return true;
        }
        public <T extends Entity> List<T> getEntitiesWithinAABB(Class<? extends T> cls,AxisAlignedBB b){
            List<T> result=new ArrayList<>();
            for(EntityVillager e:residents)if(cls.isInstance(e)&&e.getEntityBoundingBox().intersectsWith(b))result.add(cls.cast(e));
            return result;
        }
        void door(int x,int y,int z,int type,int meta){
            map.put(new BlockPos(x,y,z),type<<4|meta);map.put(new BlockPos(x,y+1,z),type<<4|8);
        }
        void roof(int x,int y,int z){map.put(new BlockPos(x,y,z),16);}
    }
    static Object record(VillageCollection collection,World world,int[] query) throws Exception {
        List<Object> values=new ArrayList<>();
        for(Village v:collection.getVillageList()){
            List<Object> doors=new ArrayList<>();
            for(VillageDoorInfo d:v.getVillageDoorInfoList()){
                BlockPos p=d.getDoorBlockPos();doors.add(Arrays.asList(p.getX(),p.getY(),p.getZ(),d.getInsideOffsetX(),d.getInsideOffsetZ(),d.getInsidePosY()));
            }
            BlockPos center=v.getCenter();values.add(Arrays.asList(center.getX(),center.getY(),center.getZ(),v.getVillageRadius(),v.getNumVillagers(),v.getReputationForPlayer("Player"),doors));
        }
        return Arrays.asList(values,collection.getVillageList().indexOf(collection.getNearestVillage(new BlockPos(query[0],query[1],query[2]),32)),Long.toString(ItemEntityOracle.seed(world.rand)));
    }
    static Object run(int sample)throws Exception {
        World w=new World();
        int x=(sample%3-1)*90,z=(sample%4-2)*23,n=sample%4;
        Random r=new Random(sample*821L+189);
        for(int i=0;i<10;i++){
            int a=x+r.nextInt(25)-12,b=z+r.nextInt(25)-12,y=1+r.nextInt(3),dir=r.nextInt(4);
            int type=i==9?71:new int[]{64,193,194,195,196,197}[i%6];
            w.door(a,y,b,type,dir);
            for(int j=1;j<=3;j++){
                if(i%4!=3) w.roof(a+(dir==0?j:dir==2?-j:0),y+3,b+(dir==1?j:dir==3?-j:0));
                if(i%4==2) w.roof(a-(dir==0?j:dir==2?-j:0),y+3,b-(dir==1?j:dir==3?-j:0));
            }
        }
        w.door(x+84,2,z,64,0);w.roof(x+85,5,z);
        List<Object> blocks=new ArrayList<>();
        for(Map.Entry<BlockPos,Integer> e:w.map.entrySet()){
            BlockPos p=e.getKey();blocks.add(Arrays.asList(p.getX(),p.getY(),p.getZ(),e.getValue()));
        }
        for(int i=0;i<3;i++){EntityVillager e=new EntityVillager(w);e.setPosition(x+i,1,z);w.residents.add(e);}
        VillageCollection c=new VillageCollection(w);long seed=sample*23872L-189;w.rand.setSeed(seed);
        List<Object> steps=new ArrayList<>();
        int[] query=new int[]{x+40,2,z};
        for(int t=1;t<=1220;t++){
            List<Object> positions=new ArrayList<>(),edits=new ArrayList<>();
            if(t==1||t==2||t==9||(sample%2==0&&t==1103)){
                int px=t==2?x+84:x,pz=z;
                for(int j=0;j<2;j++){positions.add(new int[]{px,2,pz});c.addToVillagerPositionList(new BlockPos(px,2,pz));}
            }
            if(t==35 && sample%3==1){
                BlockPos p=new BlockPos(x+84,2,z);w.map.remove(p);w.map.remove(p.up());
                edits.add(new int[]{p.getX(),p.getY(),p.getZ(),0});edits.add(new int[]{p.getX(),p.getY()+1,p.getZ(),0});
            }
            if(t==22 && !c.getVillageList().isEmpty()) c.getVillageList().get(0).setReputationForPlayer("Player",1);
            if(t==30 && sample%4==1) w.residents.clear();
            c.tick();
            Object check=t<11||t==22||t==30||t==35||t==40||t==1103||t>=1200?record(c,w,query):null;
            if(check!=null||!positions.isEmpty()||!edits.isEmpty())steps.add(Arrays.asList(t,positions,edits,check));
        }
        return Arrays.asList(sample,Long.toString(seed),new int[]{x,2,z},query,blocks,steps);
    }
    public static void main(String[] args)throws Exception{
        Bootstrap.register();List<Object> cases=new ArrayList<>();
        for(int i=0;i<24;i++)cases.add(run(i));
        try(Writer out=new OutputStreamWriter(new GZIPOutputStream(new FileOutputStream(args[0])),"UTF-8")){new Gson().toJson(cases,out);}
        System.out.println("Original VillageCollection 24 sessions / 29,280 ticks");
    }
}
