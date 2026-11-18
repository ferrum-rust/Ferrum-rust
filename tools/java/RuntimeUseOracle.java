import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import java.util.zip.GZIPOutputStream;
import com.google.gson.Gson;
import com.mojang.authlib.GameProfile;
import net.minecraft.block.Block;
import net.minecraft.block.state.IBlockState;
import net.minecraft.entity.Entity;
import net.minecraft.entity.player.*;
import net.minecraft.init.Bootstrap;
import net.minecraft.item.*;
import net.minecraft.network.*;
import net.minecraft.network.play.client.C08PacketPlayerBlockPlacement;
import net.minecraft.network.play.server.S23PacketBlockChange;
import net.minecraft.profiler.Profiler;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.management.ItemInWorldManager;
import net.minecraft.stats.StatBase;
import net.minecraft.stats.StatList;
import net.minecraft.util.*;
import net.minecraft.world.*;
import net.minecraft.world.storage.*;

/** Real C08 -> NetHandlerPlayServer -> ItemInWorldManager -> WorldServer. */
public final class RuntimeUseOracle {
    static Field field(Class<?> c,String n)throws Exception{Field f=c.getDeclaredField(n);f.setAccessible(true);return f;}
    static int state(IBlockState s){return Block.getIdFromBlock(s.getBlock())<<4|s.getBlock().getMetaFromState(s);}
    static int[] at(BlockPos p,int value){return new int[]{p.getX(),p.getY(),p.getZ(),value};}
    static class Fixture extends WorldServer {
        final List<int[]> fx=new ArrayList<>();
        final List<Object> sounds=new ArrayList<>();
        Fixture(MinecraftServer h){super(h,new SaveHandlerMP(),new WorldInfo(new WorldSettings(189,WorldSettings.GameType.SURVIVAL,false,false,WorldType.FLAT).setWorldName("3;7;1;"),"native block use"),0,new Profiler());}
        public void playAuxSFXAtEntity(EntityPlayer p,int id,BlockPos at,int value){fx.add(new int[]{at.getX(),at.getY(),at.getZ(),id});}
        public void playSoundEffect(double x,double y,double z,String name,float volume,float pitch){sounds.add(Arrays.asList(x,y,z,name,volume,pitch));}
    }
    static class Player extends EntityPlayerMP {
        Player(MinecraftServer h,Fixture w){super(h,w,new GameProfile(new UUID(0,189),"Player"),new ItemInWorldManager(w));}
        final List<String> stats=new ArrayList<>();
        public void addStat(StatBase s,int n){if(stats!=null&&s!=null)stats.add(s.statId);}
    }
    static final class Handler extends NetHandlerPlayServer {
        final List<int[]> corrections=new ArrayList<>();
        Handler(MinecraftServer h,Player p){super(h,new NetworkManager(EnumPacketDirection.SERVERBOUND),p);}
        public void sendPacket(Packet packet){if(packet instanceof S23PacketBlockChange){S23PacketBlockChange v=(S23PacketBlockChange)packet;corrections.add(at(v.getBlockPosition(),state(v.getBlockState())));}}
    }
    static Map<String,Object> run(MinecraftServer h,int mode,int side,int variant)throws Exception{
        Fixture w=new Fixture(h);h.worldServers=new WorldServer[]{w,null,null};w.init();for(int x=-1;x<=1;x++)for(int z=-1;z<=1;z++)w.theChunkProviderServer.loadChunk(x,z);
        Player p=new Player(h,w);Handler handler=new Handler(h,p);boolean creative=(variant&1)!=0,sneak=(variant&2)!=0;
        p.theItemInWorldManager.setGameType(creative?WorldSettings.GameType.CREATIVE:WorldSettings.GameType.SURVIVAL);
        int y=mode==10?255:mode==11?254:4;BlockPos clicked=new BlockPos(8,y,8);
        List<int[]> edits=new ArrayList<>();edits.add(at(clicked.down(),1<<4));int clickedState=1<<4,item=1,meta=0;
        switch(mode){
        case 1:clickedState=44<<4|3;item=44;meta=3;break;
        case 2:clickedState=78<<4|variant;item=78;break;
        case 3:clickedState=64<<4|variant;edits.add(at(clicked.up(),64<<4|8));break;
        case 4:clickedState=96<<4|variant;break;
        case 5:clickedState=107<<4|variant;break;
        case 6:clickedState=71<<4|variant;edits.add(at(clicked.up(),71<<4|8));break;
        case 7:clickedState=54<<4|2;edits.add(at(clicked.east(),54<<4|2));break;
        case 8:item=53;break;
        case 9:item=17;meta=2;break;
        case 12:item=50;break;
        case 13:item=35;meta=8;break;
        case 14:item=54;break;
        case 15:item=78;break;
        case 16:item=0;break;
        case 17:clickedState=167<<4|variant;break;
        case 18:clickedState=193<<4|8;edits.add(at(clicked.down(),193<<4|variant));break;
        case 19:item=44;meta=3;edits.add(at(clicked.offset(EnumFacing.getFront(side)),44<<4|3));break;
        case 20:clickedState=58<<4;break;
        case 21:clickedState=53<<4|variant;break;
        case 22:case 23:
            item=355;
            int wy=mode==22?y:y-1;
            if(mode==23)clickedState=0;
            for(EnumFacing f:new EnumFacing[]{EnumFacing.NORTH,EnumFacing.SOUTH,EnumFacing.WEST,EnumFacing.EAST})edits.add(at(new BlockPos(8,wy,8).offset(f),1<<4));
            break;
        case 24:clickedState=73<<4;break;
        case 25:clickedState=46<<4;break;
        }
        edits.add(at(clicked,clickedState));for(int[] e:edits){BlockPos at=new BlockPos(e[0],e[1],e[2]);Block b=Block.getBlockById(e[3]>>4);w.setBlockState(at,b.getStateFromMeta(e[3]&15),2);}
        double dx=variant==4?8.:variant==5?Math.nextDown(8.):variant==6?8.0000000001:variant==7?1.:3.;
        p.setPosition(8.5+dx,y+.5,8.5);p.rotationYaw=variant*70.25f;p.setSneaking(sneak);w.spawnEntityInWorld(p);
        ItemStack held=item==0?null:new ItemStack(Item.getItemById(item),3,meta);p.inventory.mainInventory[0]=held;
        Map<String,Object> row=new LinkedHashMap<>();row.put("mode",mode);row.put("side",side);row.put("variant",variant);row.put("edits",edits);row.put("clicked",at(clicked,state(w.getBlockState(clicked))));row.put("pos",new double[]{p.posX,p.posY,p.posZ});row.put("yaw",p.rotationYaw);row.put("item",new int[]{item,meta});row.put("creative",creative);row.put("sneak",sneak);
        Map<String,Integer> before=new TreeMap<>();
        for(int a=6;a<=10;a++)for(int b=Math.max(0,y-2);b<=Math.min(y+2,255);b++)for(int c=6;c<=10;c++)before.put(a+","+b+","+c,state(w.getBlockState(new BlockPos(a,b,c))));
        handler.corrections.clear();w.fx.clear();w.sounds.clear();p.stats.clear();
        handler.processPlayerBlockPlacement(new C08PacketPlayerBlockPlacement(clicked,side,held,.5f,(float)(variant%5)/4f,.5f));
        List<int[]> delta=new ArrayList<>();for(Map.Entry<String,Integer> pair:before.entrySet()){String[] v=pair.getKey().split(",");BlockPos at=new BlockPos(Integer.parseInt(v[0]),Integer.parseInt(v[1]),Integer.parseInt(v[2]));int s=state(w.getBlockState(at));if(s!=pair.getValue())delta.add(at(at,s));}
        row.put("changed",delta);row.put("corrected",handler.corrections);row.put("aux",w.fx);row.put("count",p.inventory.getCurrentItem()==null?0:p.inventory.getCurrentItem().stackSize);row.put("window",p.openContainer.windowId);row.put("sounds",w.sounds);row.put("stats",p.stats);return row;
    }
    public static void main(String[] args)throws Exception{
        Bootstrap.register();File dir=new File(args[1]);dir.mkdirs();Constructor<?> c=Class.forName("VanillaWorldBridge$Host").getDeclaredConstructor(File.class);c.setAccessible(true);MinecraftServer host=(MinecraftServer)c.newInstance(dir);field(MinecraftServer.class,"serverThread").set(host,Thread.currentThread());
        Map<String,Object> corpus=new LinkedHashMap<>();List<Integer> overrides=new ArrayList<>();
        for(int i=0;i<256;i++){Block b=Block.getBlockById(i);if(b!=null&&b.getClass().getMethod("onBlockActivated",World.class,BlockPos.class,IBlockState.class,EntityPlayer.class,EnumFacing.class,float.class,float.class,float.class).getDeclaringClass()!=Block.class)overrides.add(i);}
        List<Object> sounds=new ArrayList<>(),stats=new ArrayList<>();for(int i=0;i<256;i++){Block b=Block.getBlockById(i);sounds.add(Arrays.asList(b.stepSound.getPlaceSound(),(b.stepSound.getVolume()+1f)/2f,b.stepSound.getFrequency()*.8f));StatBase stat=StatList.objectUseStats[i];stats.add(stat==null?"":stat.statId);}
        corpus.put("sounds",sounds);corpus.put("stats",stats);corpus.put("overrides",overrides);List<Object> rows=new ArrayList<>();for(int mode=0;mode<26;mode++)for(int side=0;side<6;side++)for(int v=0;v<8;v++)rows.add(run(host,mode,side,v));
        corpus.put("cases",rows);try(Writer out=new OutputStreamWriter(new GZIPOutputStream(new FileOutputStream(args[0])),"UTF-8")){new Gson().toJson(corpus,out);}System.out.println("Original server C08 interactions: "+rows.size());
    }
}
