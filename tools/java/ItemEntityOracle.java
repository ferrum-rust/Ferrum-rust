import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import java.util.concurrent.atomic.AtomicLong;
import java.util.zip.GZIPOutputStream;
import com.google.gson.Gson;
import com.mojang.authlib.GameProfile;
import net.minecraft.entity.*;
import net.minecraft.entity.item.EntityItem;
import net.minecraft.entity.player.EntityPlayer;
import net.minecraft.init.Bootstrap;
import net.minecraft.item.*;
import net.minecraft.nbt.*;
import net.minecraft.stats.StatBase;
import net.minecraft.util.*;

/** Direct original ItemStack/InventoryPlayer/EntityItem merge and pickup calls. */
public final class ItemEntityOracle {
    static Field field(Class<?> c,String n)throws Exception{Field f=c.getDeclaredField(n);f.setAccessible(true);return f;}
    static final class World extends FallingEntityOracle.TestWorld {World(int n){super(n);}}
    static final class Player extends EntityPlayer {
        final List<Object> calls=new ArrayList<>();
        Player(World w){super(w,new GameProfile(new UUID(0,189),"Player"));}
        public boolean isSpectator(){return false;}
        public void addStat(StatBase s,int amount){calls.add(Arrays.asList("stat",s.statId,amount));}
        public void onItemPickup(Entity e,int count){calls.add(Arrays.asList("pickup",count));}
    }
    static final class ItemEntity extends EntityItem {
        ItemEntity(World w,ItemStack s,long seed){super(w,8.5,8,8.5,s);rand.setSeed(seed);}
        long randomState()throws Exception{return ((AtomicLong)field(Random.class,"seed").get(rand)).get();}
    }
    static ItemStack stack(int id,int count,int meta,int tag){
        ItemStack s=new ItemStack(Item.getItemById(id),count,meta);
        if(tag!=0){NBTTagCompound n=new NBTTagCompound();n.setInteger("variant",tag);s.setTagCompound(n);}return s;
    }
    static int[] stack(ItemStack s){return s==null?null:new int[]{Item.getIdFromItem(s.getItem()),s.stackSize,s.getMetadata(),s.hasTagCompound()?s.getTagCompound().getInteger("variant"):0};}
    static List<Object> slots(Player p){List<Object> out=new ArrayList<>();for(ItemStack s:p.inventory.mainInventory)out.add(stack(s));return out;}
    static List<Object> state(ItemEntity e)throws Exception{return Arrays.asList(stack(e.getEntityItem()),field(EntityItem.class,"age").getInt(e),field(EntityItem.class,"delayBeforeCanPickup").getInt(e),e.isDead,Long.toString(e.randomState()));}
    static void age(EntityItem e,int age,int delay)throws Exception{field(EntityItem.class,"age").setInt(e,age);field(EntityItem.class,"delayBeforeCanPickup").setInt(e,delay);}
    static Map<String,Object> merge(World w,Random r,int id,int mode)throws Exception{
        long seed=r.nextLong();int meta=r.nextInt(4);int ac=1+r.nextInt(64),bc=1+r.nextInt(64);
        int ai=r.nextInt(3),bi=(mode&1)==0?ai:r.nextInt(3);
        ItemEntity a=new ItemEntity(w,stack(id,ac,meta,ai),seed),b=new ItemEntity(w,stack(mode%16==0?1:id,bc,mode%3==0?meta+1:meta,bi),seed+1);
        age(a,mode%17==0?-32768:r.nextInt(5000),mode%19==0?32767:r.nextInt(40));age(b,r.nextInt(5000),mode%23==0?32767:r.nextInt(40));
        if(mode%29==0)b.setDead();
        Map<String,Object> row=new LinkedHashMap<>();row.put("before",Arrays.asList(state(a),state(b)));
        Method m=EntityItem.class.getDeclaredMethod("combineItems",EntityItem.class);m.setAccessible(true);row.put("merged",m.invoke(a,b));row.put("after",Arrays.asList(state(a),state(b)));return row;
    }
    static Map<String,Object> pickup(World w,Random r,int id,int mode)throws Exception{
        int meta=mode%3;int tag=mode%4==0?1:0;long seed=r.nextLong();
        Player p=new Player(w);p.capabilities.isCreativeMode=(mode&8)!=0;
        int pattern=mode%6;
        if(pattern!=0)for(int i=0;i<36;i++)p.inventory.mainInventory[i]=stack(4,64,0,0);
        if(pattern==1||pattern==2)p.inventory.mainInventory[5]=stack(id,Math.max(1,Item.getItemById(id).getItemStackLimit()-2),meta,tag);
        if(pattern==2||pattern==3)p.inventory.mainInventory[17]=null;
        if(pattern==4)p.inventory.mainInventory[7]=stack(id,1,meta+1,tag);
        if(pattern==5)p.inventory.mainInventory[2]=stack(id,1,meta,tag==0?1:0);
        ItemEntity e=new ItemEntity(w,stack(id,1+r.nextInt(16),meta,tag),seed);
        int owner=mode%7;String name=owner==1||owner==3?"Other":owner==2?"Player":null;
        if(name!=null)e.setOwner(name);age(e,owner==3?5800:mode%2==0?5799:100,mode%9==0?10:0);
        Map<String,Object> row=new LinkedHashMap<>();row.put("owner",name==null?"":name);row.put("creative",p.capabilities.isCreativeMode);row.put("before",state(e));row.put("slots",slots(p));
        w.events.clear();p.calls.clear();e.onCollideWithPlayer(p);row.put("after",state(e));row.put("result",slots(p));row.put("sounds",new ArrayList<>(w.events));row.put("calls",p.calls);return row;
    }
    static long seed(Random r)throws Exception{return ((AtomicLong)field(Random.class,"seed").get(r)).get();}
    public static void main(String[]args)throws Exception{
        Bootstrap.register();World world=new World(0);Random r=new Random(189);Map<String,Object> result=new LinkedHashMap<>();
        List<int[]> facts=new ArrayList<>();for(Item i:Item.itemRegistry)facts.add(new int[]{Item.getIdFromItem(i),i.getItemStackLimit(),i.getHasSubtypes()?1:0,i.getMaxDamage()});result.put("facts",facts);
        List<Object> constructors=new ArrayList<>();Random math=(Random)field(Class.forName("java.lang.Math$RandomNumberGeneratorHolder"),"randomNumberGenerator").get(null);
        for(int n=0;n<256;n++){long raw=r.nextLong();math.setSeed(raw);ItemEntity e=new ItemEntity(world,stack(1,1,0,0),0);constructors.add(Arrays.asList(Long.toString(raw),Integer.toUnsignedLong(Float.floatToRawIntBits(e.hoverStart)),Integer.toUnsignedLong(Float.floatToRawIntBits(e.rotationYaw)),FallingEntityOracle.bits(e.motionX,e.motionY,e.motionZ),Long.toString(seed(math))));}result.put("constructors",constructors);
        List<Object> merges=new ArrayList<>(),pickups=new ArrayList<>();
        for(int id:new int[]{1,5,17,35,264,276,332,334,369,373,403})for(int mode=0;mode<256;mode++){merges.add(merge(world,r,id,mode));pickups.add(pickup(world,r,id,mode));}
        result.put("merges",merges);result.put("pickups",pickups);
        try(Writer out=new OutputStreamWriter(new GZIPOutputStream(new FileOutputStream(args[0])),"UTF-8")){new Gson().toJson(result,out);}
        System.out.println("Original items "+facts.size()+", constructors "+constructors.size()+", merges "+merges.size()+", pickups "+pickups.size());
    }
}
