import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import java.util.concurrent.atomic.AtomicLong;
import java.util.zip.GZIPOutputStream;
import com.google.gson.Gson;
import com.mojang.authlib.GameProfile;
import net.minecraft.block.Block;
import net.minecraft.entity.Entity;
import net.minecraft.entity.item.*;
import net.minecraft.entity.player.EntityPlayer;
import net.minecraft.init.Bootstrap;
import net.minecraft.inventory.InventoryHelper;
import net.minecraft.item.*;
import net.minecraft.nbt.NBTTagCompound;
import net.minecraft.profiler.Profiler;
import net.minecraft.server.MinecraftServer;
import net.minecraft.stats.StatBase;
import net.minecraft.tileentity.TileEntityChest;
import net.minecraft.util.*;
import net.minecraft.world.*;
import net.minecraft.world.storage.*;

/** Real WorldServer update order, chest spills, player drops and item pickup.
 * Player position is supplied externally, just as it is by the native host.
 * Its tick executes the original section query and original EntityItem callback.
 */
public final class RuntimeItemOracle {
    static Field field(Class<?> c,String n){try{Field f=c.getDeclaredField(n);f.setAccessible(true);return f;}catch(Exception e){throw new RuntimeException(e);}}
    static final Field SEED=field(Random.class,"seed"),RAND=field(Entity.class,"rand"),AGE=field(EntityItem.class,"age"),DELAY=field(EntityItem.class,"delayBeforeCanPickup");
    static String state(Random r)throws Exception{return Long.toString(((AtomicLong)SEED.get(r)).get());}
    static Random entityRandom(Entity e)throws Exception{return (Random)RAND.get(e);}
    static Random math()throws Exception{return (Random)field(Class.forName("java.lang.Math$RandomNumberGeneratorHolder"),"randomNumberGenerator").get(null);}
    static Random spill()throws Exception{return (Random)field(InventoryHelper.class,"RANDOM").get(null);}
    static List<String> bits(double... nums){List<String> out=new ArrayList<>();for(double n:nums)out.add(Long.toHexString(Double.doubleToRawLongBits(n)));return out;}
    static ItemStack stack(int id,int count,int meta,int tag){ItemStack s=new ItemStack(Item.getItemById(id),count,meta);if(tag!=0){NBTTagCompound n=new NBTTagCompound();n.setInteger("variant",tag);s.setTagCompound(n);}return s;}
    static int[] value(ItemStack s){return s==null?null:new int[]{Item.getIdFromItem(s.getItem()),s.stackSize,s.getMetadata(),s.hasTagCompound()?s.getTagCompound().getInteger("variant"):0};}
    static final class Player extends EntityPlayer {
        List<Object> events=new ArrayList<>();
        Player(WorldServer w){super(w,new GameProfile(new UUID(0,189),"Player"));}
        public boolean isSpectator(){return false;}
        public void onUpdate(){
            if(xpCooldown>0)--xpCooldown;
            for(Entity e:worldObj.getEntitiesWithinAABBExcludingEntity(this,getEntityBoundingBox().expand(1,.5,1)))if(!e.isDead)e.onCollideWithPlayer(this);
        }
        public void addStat(StatBase s,int amount){events.add(Arrays.asList("stat",s.statId));}
        public void onItemPickup(Entity e,int count){events.add(Arrays.asList(e instanceof EntityXPOrb?"xpPickup":"pickup",e instanceof EntityXPOrb?e.getEntityId():((Fixture)worldObj).itemIds.get(e),count));}
    }
    static final class Fixture extends WorldServer {
        final IdentityHashMap<Entity,Integer> itemIds=new IdentityHashMap<>(),fallingIds=new IdentityHashMap<>();
        Fixture(MinecraftServer host){super(host,new SaveHandlerMP(),new WorldInfo(new WorldSettings(189,WorldSettings.GameType.SURVIVAL,false,false,WorldType.FLAT).setWorldName("3;7;1;"),"native item oracle"),0,new Profiler());}
        public boolean spawnEntityInWorld(Entity e){
            if(!super.spawnEntityInWorld(e))return false;
            try{if(e instanceof EntityItem){int id=itemIds.size();itemIds.put(e,id);entityRandom(e).setSeed(917+id*31);}
                if(e instanceof EntityFallingBlock){int id=fallingIds.size();fallingIds.put(e,id);entityRandom(e).setSeed(189+id*17);}
                if(e instanceof EntityXPOrb)entityRandom(e).setSeed(991L+41L*e.getEntityId());
            }catch(Exception ex){throw new RuntimeException(ex);}return true;
        }
        public void playSoundAtEntity(Entity entity,String name,float volume,float pitch){
            if(entity instanceof Player)((Player)entity).events.add(Arrays.asList("sound",name,Float.floatToRawIntBits(volume),Float.floatToRawIntBits(pitch)));
        }
    }
    static List<Object> frame(Fixture w,Player p,int t)throws Exception{
        List<Object> entities=new ArrayList<>(),slots=new ArrayList<>();
        for(Entity e:w.loadedEntityList){
            if(e instanceof EntityItem){EntityItem i=(EntityItem)e;entities.add(Arrays.asList("i",w.itemIds.get(e),value(i.getEntityItem()),AGE.getInt(i),DELAY.getInt(i),i.ticksExisted,i.isDead,bits(i.posX,i.posY,i.posZ,i.motionX,i.motionY,i.motionZ),state(entityRandom(i)),i.chunkCoordX,i.chunkCoordY,i.chunkCoordZ,i.getThrower()));}
            if(e instanceof EntityFallingBlock)entities.add(Arrays.asList("f",w.fallingIds.get(e),e.ticksExisted));
            if(e instanceof EntityXPOrb){EntityXPOrb o=(EntityXPOrb)e;entities.add(Arrays.asList("xp",o.getEntityId(),o.getXpValue(),o.xpOrbAge,o.isDead,bits(o.posX,o.posY,o.posZ,o.motionX,o.motionY,o.motionZ),state(entityRandom(o)),o.chunkCoordX,o.chunkCoordY,o.chunkCoordZ));}
            if(e instanceof Player)entities.add(Arrays.asList("p"));
        }
        for(int n=0;n<36;n++){ItemStack s=p.inventory.mainInventory[n];if(s!=null)slots.add(Arrays.asList(n,value(s)));}
        List<Object> events=new ArrayList<>(p.events);p.events.clear();
        return Arrays.asList(t,entities,slots,events,state(math()),state(spill()),state(entityRandom(p)),state(w.rand),Arrays.asList(p.experienceTotal,p.experienceLevel,Float.floatToRawIntBits(p.experience),p.xpCooldown,p.getScore()));
    }
    static void item(Fixture w,double x,int id,int count,int delay){EntityItem e=new EntityItem(w,x,1.,8.5,stack(id,count,0,0));e.setPickupDelay(delay);w.spawnEntityInWorld(e);}
    static Map<String,Object> run(MinecraftServer host,int mode,double x)throws Exception{
        Fixture w=new Fixture(host);host.worldServers=new WorldServer[]{w,null,null};w.init();
        for(int cx=-3;cx<=3;cx++)for(int cz=-3;cz<=3;cz++)w.theChunkProviderServer.loadChunk(cx,cz);
        int firstId=x<0?-49:x>15.?57:3;
        field(Entity.class,"nextEntityID").setInt(null,firstId);
        Player p=new Player(w);p.setPosition(x,1.,8.5);p.rotationYaw=(float)(x*13.7);p.rotationPitch=12.375f;
        entityRandom(p).setSeed(521);math().setSeed(3189);spill().setSeed(491);w.rand.setSeed(0);w.setTotalWorldTime(0);
        if(mode==0||mode==2||mode==9||mode==11||mode==13)w.spawnEntityInWorld(p);
        if(mode<=3||mode==9){
            if(mode==2){for(int n=0;n<36;n++)p.inventory.mainInventory[n]=stack(4,64,0,0);p.inventory.mainInventory[5]=stack(264,62,0,0);}
            item(w,x+.15,264,5,mode==0?0:1);item(w,x-.12,264,3,mode==9?23:0);
            item(w,x+.26,17,1,mode==9?15:0);
            if(mode==3)p.setPosition(x+6.,1.,8.5);
        }
        if(mode>=10){
            if(mode==12)p.setPosition(x+6.,1.,8.5);
            w.spawnEntityInWorld(new EntityXPOrb(w,x+.15,1.,8.5,5));
            item(w,x-.12,17,1,0);
            w.spawnEntityInWorld(new EntityXPOrb(w,x-.21,1.,8.5,mode==13?1237:3));
            item(w,x+.16,264,1,0);
            w.spawnEntityInWorld(new EntityXPOrb(w,x+.27,1.,8.5,17));
        }
        if(!w.playerEntities.contains(p))w.spawnEntityInWorld(p);
        if(mode==4){
            BlockPos at=new BlockPos(x,1,8);
            BlockTickOracle.set(w,new int[]{at.getX(),1,8,54<<4|2});TileEntityChest tile=(TileEntityChest)w.getTileEntity(at);
            tile.setInventorySlotContents(0,stack(264,64,0,7));tile.setInventorySlotContents(3,stack(276,1,919,0));tile.setInventorySlotContents(26,stack(35,31,8,0));
            w.setBlockToAir(at);p.setPosition(x+6.,1.,8.5);
        }
        if(mode==5){BlockTickOracle.set(w,new int[]{(int)Math.floor(x),1,8,44<<4});BlockTickOracle.set(w,new int[]{(int)Math.floor(x),6,8,12<<4|1});p.setPosition(x+6.,1.,8.5);}
        if(mode==6){
            p.inventory.setItemStack(stack(35,9,2,6));
            net.minecraft.inventory.ContainerPlayer c=(net.minecraft.inventory.ContainerPlayer)p.inventoryContainer;
            c.craftMatrix.setInventorySlotContents(0,stack(264,4,0,0));c.craftMatrix.setInventorySlotContents(3,stack(276,1,919,0));
            c.onContainerClosed(p);
        }
        if(mode==7||mode==8){p.inventory.mainInventory[0]=stack(264,8,0,7);p.dropOneItem(mode==8);}
        List<Object> frames=new ArrayList<>();frames.add(frame(w,p,0));
        for(int t=1;t<=64;t++){
            if(t==30&&(mode==3||mode==4||mode==5||mode==12))p.setPosition(x,1.,8.5);
            w.setTotalWorldTime(t);w.tickUpdates(false);w.updateEntities();frames.add(frame(w,p,t));
        }
        Map<String,Object> row=new LinkedHashMap<>();row.put("mode",mode);row.put("x",x);row.put("frames",frames);return row;
    }
    public static void main(String[] args)throws Exception{
        Bootstrap.register();File dir=new File(args[1]);dir.mkdirs();Constructor<?> c=Class.forName("VanillaWorldBridge$Host").getDeclaredConstructor(File.class);c.setAccessible(true);MinecraftServer host=(MinecraftServer)c.newInstance(dir);
        List<Object> rows=new ArrayList<>();for(double x:new double[]{8.5,15.975,-.025})for(int m=0;m<14;m++)rows.add(run(host,m,x));
        try(Writer out=new OutputStreamWriter(new GZIPOutputStream(new FileOutputStream(args[0])),"UTF-8")){new Gson().toJson(rows,out);}
        System.out.println("Original WorldServer mixed item phases: "+rows.size()+" scenarios, "+(rows.size()*65)+" checkpoints");
    }
}
