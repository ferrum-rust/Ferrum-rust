import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import com.google.gson.Gson;
import net.minecraft.init.Bootstrap;
import net.minecraft.nbt.*;
import net.minecraft.profiler.Profiler;
import net.minecraft.server.MinecraftServer;
import net.minecraft.world.*;
import net.minecraft.world.storage.*;
import net.minecraft.world.gen.structure.*;

/** Actual MapGenStructure discovery/map traversal + serialized NBT reload. */
public final class StructureOrderOracle {
    static Field live;
    static List<String> liveKeys(MapGenStructure g)throws Exception {List<String> out=new ArrayList<String>();for(Object key:((Map<?,?>)live.get(g)).keySet())out.add(key.toString());return out;}
    static List<String> featureKeys(World w,MapGenStructure g){MapGenStructureData d=(MapGenStructureData)w.loadItemData(MapGenStructureData.class,g.getStructureName());return new ArrayList<String>(d.getTagCompound().getKeySet());}
    static void reload(World w,MapGenStructure g)throws Exception {
        MapGenStructureData data=(MapGenStructureData)w.loadItemData(MapGenStructureData.class,g.getStructureName());NBTTagCompound n=new NBTTagCompound();data.writeToNBT(n);ByteArrayOutputStream bytes=new ByteArrayOutputStream();CompressedStreamTools.write(n,new DataOutputStream(bytes));NBTTagCompound read=CompressedStreamTools.read(new DataInputStream(new ByteArrayInputStream(bytes.toByteArray())));MapGenStructureData restored=new MapGenStructureData(g.getStructureName());restored.readFromNBT(read);w.setItemData(g.getStructureName(),restored);
    }
    public static void main(String[] args)throws Exception {
        PrintStream output=System.out;System.setOut(System.err);Bootstrap.register();live=MapGenStructure.class.getDeclaredField("structureMap");live.setAccessible(true);File root=new File(args[0]);root.mkdirs();Constructor<?> constructor=Class.forName("VanillaWorldBridge$Host").getDeclaredConstructor(File.class);constructor.setAccessible(true);MinecraftServer host=(MinecraftServer)constructor.newInstance(root);List<Object> cases=new ArrayList<Object>();
        for(long seed:new long[]{0,-1,189,Long.MIN_VALUE}) {
            WorldServer w=new WorldServer(host,new SaveHandlerMP(),new WorldInfo(new WorldSettings(seed,WorldSettings.GameType.SURVIVAL,true,false,WorldType.DEFAULT),"structure-order"),0,new Profiler());host.worldServers=new WorldServer[]{w,null,null};w.init();w.rand.setSeed(1234);
            MapGenStructure[] maps={new MapGenMineshaft(),new MapGenVillage(),new MapGenStronghold(),new MapGenScatteredFeature(),new StructureOceanMonument()};List<Object> steps=new ArrayList<Object>();
            List<int[]> positions=new ArrayList<int[]>(Arrays.asList(new int[][]{{0,0},{-20,-20},{-40,-40},{20,20},{40,40},{80,-80},{-80,80},{120,120},{-120,-120},{0,0},{16,16},{-16,-16},{64,0},{0,64},{64,64},{-64,0},{0,-64},{-64,-64},{0,0}}));
            for(int i=0;i<positions.size();i++) {
                boolean restart=i==9||i==18||i==22;
                if(restart){for(MapGenStructure g:maps)reload(w,g);maps=new MapGenStructure[]{new MapGenMineshaft(),new MapGenVillage(),new MapGenStronghold(),new MapGenScatteredFeature(),new StructureOceanMonument()};}
                List<Object> stages=new ArrayList<Object>();
                for(MapGenStructure g:maps){g.generate(null,w,positions.get(i)[0],positions.get(i)[1],null);Map<String,Object> stage=new LinkedHashMap<String,Object>();stage.put("live",liveKeys(g));stage.put("features",featureKeys(w,g));stage.put("world_next_long",Long.toString(w.rand.nextLong()));stages.add(stage);}
                if(i==0){Field coords=MapGenStronghold.class.getDeclaredField("structureCoords");coords.setAccessible(true);for(ChunkCoordIntPair p:(ChunkCoordIntPair[])coords.get(maps[2]))positions.add(new int[]{p.chunkXPos,p.chunkZPos});positions.add(new int[]{0,0});}
                Map<String,Object> step=new LinkedHashMap<String,Object>();step.put("chunk",positions.get(i));step.put("restart",restart);step.put("stages",stages);steps.add(step);
            }
            Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("seed",Long.toString(seed));row.put("steps",steps);cases.add(row);
        }
        Map<String,Object> result=new LinkedHashMap<String,Object>();result.put("cases",cases);result.put("runtime",System.getProperty("java.version"));output.println(new Gson().toJson(result));
    }
}
