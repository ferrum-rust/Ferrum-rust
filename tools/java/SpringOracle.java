// Verification only: run original WorldGenLiquids with real WorldServer callbacks.
import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import net.minecraft.block.Block;
import net.minecraft.block.BlockFalling;
import net.minecraft.init.Bootstrap;
import net.minecraft.entity.Entity;
import net.minecraft.entity.item.EntityItem;
import net.minecraft.item.Item;
import net.minecraft.util.BlockPos;
import net.minecraft.world.WorldServer;
import net.minecraft.world.NextTickListEntry;
import net.minecraft.world.gen.feature.WorldGenLiquids;
import com.google.gson.GsonBuilder;

public class SpringOracle {
    static List<int[]> setup;
    static void set(WorldServer w,int x,int y,int z,int state) {
        setup.add(new int[]{x,y,z,state});
        Block b=Block.getBlockById(state>>4);
        w.setBlockState(new BlockPos(x,y,z),b.getStateFromMeta(state&15),2);
    }
    static List<Object> lightHashes(WorldServer w) {
        List<Object> rows=new ArrayList<>();
        for(int cx=-1;cx<=1;cx++)for(int cz=-1;cz<=1;cz++) {
            long hash=0xcbf29ce484222325L;
            for(int y=0;y<256;y++)for(int z=0;z<16;z++)for(int x=0;x<16;x++) {
                BlockPos p=new BlockPos(cx*16+x,y,cz*16+z);
                int light=w.getLightFor(net.minecraft.world.EnumSkyBlock.SKY,p)*16+w.getLightFor(net.minecraft.world.EnumSkyBlock.BLOCK,p);
                hash=(hash^light)*0x100000001b3L;
            }
            rows.add(Arrays.asList(cx,cz,Long.toUnsignedString(hash)));
        }
        return rows;
    }
    static List<int[]> states(WorldServer w,int x,int z) {
        List<int[]> out=new ArrayList<int[]>();
        for(int y=1;y<=32;y++)for(int dz=-12;dz<=12;dz++)for(int dx=-12;dx<=12;dx++) {
            int state=BlockTickOracle.state(w,x+dx,y,z+dz);
            if(state!=0)out.add(new int[]{x+dx,y,z+dz,state});
        }
        return out;
    }
    static Map<String,Object> run(File root,int index,int fluid,int mode,int direction,int y,long seed) throws Exception {
        WorldServer w=BlockTickOracle.world(new File(root,"case-"+index));
        setup=new ArrayList<>();
        int x=index%2==0?8:-1,z=index%2==0?8:0;
        int[][] dirs={{0,-1},{1,0},{0,1},{-1,0}};
        set(w,x,y+1,z,16);set(w,x,y-1,z,16);
        for(int d=0;d<4;d++)if(d!=direction)set(w,x+dirs[d][0],y,z+dirs[d][1],16);
        if(mode==1)set(w,x,y+1,z,0);
        if(mode==2)set(w,x,y-1,z,0);
        if(mode==3)set(w,x,y,z,3<<4);
        if(mode==4)set(w,x+dirs[(direction+1)%4][0],y,z+dirs[(direction+1)%4][1],0);
        if(mode==5)set(w,x,y,z,16);
        if(mode==6)set(w,x+dirs[direction][0]*2,1,z+dirs[direction][1]*2,(fluid==8?11:9)<<4);
        if(mode==7){for(int dx=-9;dx<=9;dx++)for(int dz=-9;dz<=9;dz++)set(w,x+dx,y-1,z+dz,16);}
        if(mode>=8&&mode<=12) {
            for(int dx=-9;dx<=9;dx++)for(int dz=-9;dz<=9;dz++)set(w,x+dx,1,z+dz,16);
            int sand=mode==9?13<<4:(12<<4)|(mode==10?1:0);
            set(w,x+dirs[direction][0],y+1,z+dirs[direction][1],sand);
            if(mode==11)set(w,x+dirs[direction][0],y+2,z+dirs[direction][1],13<<4);
            if(mode==12)for(int dx=-9;dx<=9;dx++)for(int dz=-9;dz<=9;dz++)set(w,x+dx,1,z+dz,0);
        }
        if(mode>=13) {
            for(int dx=-9;dx<=9;dx++)for(int dz=-9;dz<=9;dz++)set(w,x+dx,1,z+dz,2<<4);
            int plant=new int[]{(31<<4)|1,(59<<4)|7,175<<4,83<<4,39<<4,(106<<4)|4,78<<4,(127<<4)|8}[mode-13];
            int px=x+dirs[direction][0]*2,pz=z+dirs[direction][1]*2;
            set(w,px,2,pz,plant);if(mode==15)set(w,px,3,pz,(175<<4)|8);
            // A second unsupported plant beside the falling stream receives
            // an actual neighbor callback, not only direct water displacement.
            int qx=x+dirs[direction][0]+dirs[(direction+1)%4][0],qz=z+dirs[direction][1]+dirs[(direction+1)%4][1];
            set(w,qx,5,qz,plant);if(mode==15)set(w,qx,6,qz,(175<<4)|8);
        }
        // Isolate the generation callback from pre-fixture placement schedules.
        for(String name:new String[]{"pendingTickListEntriesTreeSet","pendingTickListEntriesHashSet"}){Field f=WorldServer.class.getDeclaredField(name);f.setAccessible(true);((Set<?>)f.get(w)).clear();}
        Map<String,Object> row=new LinkedHashMap<String,Object>();
        row.put("fluid",fluid);row.put("mode",mode);row.put("direction",direction);row.put("seed",Long.toString(seed));row.put("position",new int[]{x,y,z});
        row.put("initial",states(w,x,z));row.put("fall_instantly",mode>=8);BlockFalling.fallInstantly=mode>=8;
        row.put("setup",setup);
        w.rand.setSeed(seed^0x123456789abcdefL);Random population=new Random(seed);
        boolean result=new WorldGenLiquids(Block.getBlockById(fluid)).generate(w,population,new BlockPos(x,y,z));
        BlockFalling.fallInstantly=false;
        row.put("success",result);row.put("final",states(w,x,z));
        if(seed==0)row.put("light_hashes",lightHashes(w));
        List<Object> drops=new ArrayList<>();for(Entity e:w.loadedEntityList)if(e instanceof EntityItem){EntityItem item=(EntityItem)e;drops.add(Arrays.asList(Item.getIdFromItem(item.getEntityItem().getItem()),item.getEntityItem().getMetadata(),item.getEntityItem().stackSize,e.posX,e.posY,e.posZ));}row.put("items",drops);
        row.put("population_next",Long.toString(population.nextLong()));row.put("world_next",Long.toString(w.rand.nextLong()));
        Field field=WorldServer.class.getDeclaredField("pendingTickListEntriesTreeSet");field.setAccessible(true);
        List<Object> pending=new ArrayList<Object>();
        for(Object item:(Set<?>)field.get(w)){NextTickListEntry e=(NextTickListEntry)item;pending.add(Arrays.asList(e.scheduledTime,e.position.getX(),e.position.getY(),e.position.getZ(),Block.getIdFromBlock(e.getBlock())));}
        row.put("pending",pending);return row;
    }
    public static void main(String[] args)throws Exception {
        Bootstrap.register();List<Object> rows=new ArrayList<Object>();int i=0;
        for(int fluid:new int[]{8,10})for(int mode=0;mode<21;mode++)for(int direction=0;direction<4;direction++)for(long seed:new long[]{0,-1,189}) {
            rows.add(run(new File(args[1]),i++,fluid,mode,direction,mode==7?4:12,seed));
        }
        File output=new File(args[0]);output.getParentFile().mkdirs();
        try(Writer writer=new OutputStreamWriter(new FileOutputStream(output),"UTF-8")){new GsonBuilder().create().toJson(rows,writer);}
        System.out.println("Original real-world spring oracle: "+rows.size()+" cases -> "+output);System.exit(0);
    }
}
