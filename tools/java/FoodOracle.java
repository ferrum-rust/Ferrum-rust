// Actual FoodStats and EntityPlayer movement/jump/attack/harvest callbacks.
import com.google.gson.Gson;
import com.mojang.authlib.GameProfile;
import java.lang.reflect.*;
import java.io.ByteArrayOutputStream;
import java.util.*;
import sun.misc.Unsafe;
import net.minecraft.init.*;
import net.minecraft.block.material.Material;
import net.minecraft.block.Block;
import net.minecraft.tileentity.*;
import net.minecraft.block.state.IBlockState;
import net.minecraft.entity.*;
import net.minecraft.entity.player.*;
import net.minecraft.item.*;
import net.minecraft.nbt.*;
import net.minecraft.stats.StatBase;
import net.minecraft.util.*;
import net.minecraft.world.*;
import net.minecraft.world.storage.WorldInfo;

public final class FoodOracle {
    static final class Stop extends RuntimeException {}
    static final class StopInventory extends InventoryPlayer {
        StopInventory(EntityPlayer p){super(p);}
        @Override public void decrementAnimations(){throw new Stop();}
    }
    static class Player extends EntityPlayer {
        boolean eyesWater,water,ladder,sprint;int heals,damages;
        Player(World world){super(world,new GameProfile(new UUID(0,189),"FoodOracle"));}
        @Override public boolean isSpectator(){return false;}
        @Override public void playSound(String sound,float volume,float pitch){}
        @Override public void onDeath(DamageSource source){}
        @Override public boolean isInsideOfMaterial(Material m){return m==Material.water&&eyesWater;}
        @Override public boolean isInWater(){return water;}
        @Override public boolean isOnLadder(){return ladder;}
        @Override public boolean isSprinting(){return sprint;}
        @Override public void addStat(StatBase stat,int amount){}
        @Override public void heal(float amount){heals++;super.heal(amount);}
        @Override public boolean attackEntityFrom(DamageSource source,float amount){damages++;return super.attackEntityFrom(source,amount);}
        void peacefulPrefix(){InventoryPlayer old=inventory;inventory=new StopInventory(this);try{onLivingUpdate();throw new AssertionError("Missing inventory boundary");}catch(Stop stop){}finally{inventory=old;}}
    }
    static final class Target extends Entity {
        boolean succeeds;
        Target(World world,boolean succeeds){super(world);this.succeeds=succeeds;}
        @Override protected void entityInit(){}
        @Override protected void readEntityFromNBT(NBTTagCompound tag){}
        @Override protected void writeEntityToNBT(NBTTagCompound tag){}
        @Override public boolean canAttackWithItem(){return true;}
        @Override public boolean attackEntityFrom(DamageSource source,float amount){return succeeds;}
    }
    static class OracleWorld extends WorldServer {
        GameRules rules;EnumDifficulty difficulty;
        OracleWorld(){super(null,null,null,0,null);}
        @Override public GameRules getGameRules(){return rules;}
        @Override public EnumDifficulty getDifficulty(){return difficulty;}
        @Override public BlockPos getSpawnPoint(){return new BlockPos(0,64,0);}
        @Override public IBlockState getBlockState(BlockPos p){return Blocks.air.getDefaultState();}
        @Override public boolean spawnEntityInWorld(Entity e){return true;}
        @Override public TileEntity getTileEntity(BlockPos p){return null;}
        @Override public boolean setBlockState(BlockPos p,IBlockState s,int flags){return true;}
        @Override public boolean setBlockToAir(BlockPos p){return true;}
        @Override public void setEntityState(Entity e,byte status){}
        @Override public void playSoundAtEntity(Entity e,String sound,float volume,float pitch){}
    }
    static void set(Object o,Class<?> cls,String name,Object value)throws Exception{Field f=cls.getDeclaredField(name);f.setAccessible(true);f.set(o,value);}
    static Player player(int difficulty,boolean regeneration,boolean immune,boolean remote,float health,float maximum)throws Exception{
        Field f=Unsafe.class.getDeclaredField("theUnsafe");f.setAccessible(true);OracleWorld w=(OracleWorld)((Unsafe)f.get(null)).allocateInstance(OracleWorld.class);
        w.difficulty=EnumDifficulty.getDifficultyEnum(difficulty);w.rules=new GameRules();w.rules.setOrCreateGameRule("naturalRegeneration",Boolean.toString(regeneration));set(w,World.class,"rand",new Random(189));set(w,World.class,"isRemote",remote);set(w,World.class,"provider",new WorldProviderSurface());set(w,World.class,"worldInfo",new WorldInfo(new WorldSettings(189L,WorldSettings.GameType.SURVIVAL,false,false,WorldType.DEFAULT),"FoodOracle"));
        Player p=new Player(w);p.capabilities.disableDamage=immune;p.capabilities.isCreativeMode=immune;p.getEntityAttribute(SharedMonsterAttributes.maxHealth).setBaseValue(maximum);p.setHealth(health);return p;
    }
    static NBTTagCompound food(int level,int timer,float saturation,float exhaustion,int presence){NBTTagCompound t=new NBTTagCompound();if(presence==0)t.setInteger("foodLevel",level);else if(presence==2)t.setString("foodLevel","bad type");t.setInteger("foodTickTimer",timer);t.setFloat("foodSaturationLevel",saturation);t.setFloat("foodExhaustionLevel",exhaustion);return t;}
    static long bits(float value){return Integer.toUnsignedLong(Float.floatToRawIntBits(value));}
    static Map<String,Object> snapshot(Player p){NBTTagCompound t=new NBTTagCompound();p.getFoodStats().writeNBT(t);Map<String,Object> out=new LinkedHashMap<String,Object>();out.put("food",t.getInteger("foodLevel"));out.put("timer",t.getInteger("foodTickTimer"));out.put("saturation",bits(t.getFloat("foodSaturationLevel")));out.put("exhaustion",bits(t.getFloat("foodExhaustionLevel")));out.put("previous",p.getFoodStats().getPrevFoodLevel());out.put("health",bits(p.getHealth()));out.put("heals",p.heals);out.put("damages",p.damages);return out;}
    static Map<String,Object> tick(int difficulty,boolean regen,boolean immune,boolean remote,int level,int timer,float saturation,float exhaustion,float health,float maximum,int start,int ticks,int presence)throws Exception{
        Player p=player(difficulty,regen,immune,remote,health,maximum);p.getFoodStats().readNBT(food(level,timer,saturation,exhaustion,presence));
        Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("difficulty",difficulty);row.put("regeneration",regen);row.put("immune",immune);row.put("remote",remote);row.put("maximum",bits(maximum));row.put("start",start);row.put("ticks",ticks);row.put("presence",presence);row.put("nbt",Arrays.asList(level,timer,bits(saturation),bits(exhaustion)));row.put("initial",snapshot(p));
        for(int i=0;i<ticks;i++){p.ticksExisted=start+i;p.hurtResistantTime=Math.max(0,p.hurtResistantTime-1);p.peacefulPrefix();if(!remote)p.getFoodStats().onUpdate(p);}
        row.put("result",snapshot(p));return row;
    }
    static Map<String,Object> action(String action,int flags,double[] delta,float initial,float amount,int level,float saturation)throws Exception{
        boolean immune=(flags&1)!=0,remote=(flags&2)!=0;Player p=player(2,true,immune,remote,20f,20f);p.getFoodStats().readNBT(food(level,0,saturation,initial,0));p.eyesWater=(flags&4)!=0;p.water=(flags&8)!=0;p.ladder=(flags&16)!=0;p.onGround=(flags&32)!=0;p.sprint=(flags&64)!=0;if((flags&128)!=0)p.ridingEntity=new Target(p.worldObj,true);
        if(action.equals("movement"))p.addMovementStat(delta[0],delta[1],delta[2]);
        if(action.equals("jump"))p.jump();
        if(action.equals("attack"))p.attackTargetEntityWithCurrentItem(new Target(p.worldObj,amount!=0));
        if(action.equals("mine"))Blocks.stone.harvestBlock(p.worldObj,p,new BlockPos(0,64,0),Blocks.stone.getDefaultState(),null);
        if(action.equals("player_add"))p.addExhaustion(amount);
        if(action.equals("stats_add"))p.getFoodStats().addExhaustion(amount);
        if(action.equals("eat"))p.getFoodStats().addStats((int)amount,(float)delta[0]);
        Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("action",action);row.put("flags",flags);row.put("delta",delta);row.put("initial",bits(initial));row.put("amount",bits(amount));row.put("level",level);row.put("saturation",bits(saturation));row.put("result",snapshot(p));return row;
    }
    static Map<String,Object> legacy()throws Exception{
        NBTTagCompound playerTag=food(-1,80,-0f,41f,0);playerTag.setShort("Health",(short)20);playerTag.setFloat("HealF",30.5f);NBTTagCompound attribute=new NBTTagCompound();attribute.setString("Name","generic.maxHealth");attribute.setDouble("Base",40.0);NBTTagList attributes=new NBTTagList();attributes.appendTag(attribute);playerTag.setTag("Attributes",attributes);
        Player player=player(2,false,false,false,20f,20f);player.readEntityFromNBT(playerTag);
        NBTTagCompound rules=new NBTTagCompound();rules.setString("naturalRegeneration","false");NBTTagCompound data=new NBTTagCompound();data.setTag("GameRules",rules);data.setTag("Player",playerTag);data.setLong("DayTime",123456L);NBTTagCompound root=new NBTTagCompound();root.setTag("Data",data);ByteArrayOutputStream bytes=new ByteArrayOutputStream();CompressedStreamTools.writeCompressed(root,bytes);StringBuilder hex=new StringBuilder();for(byte b:bytes.toByteArray())hex.append(String.format("%02x",b&255));Map<String,Object> out=new LinkedHashMap<String,Object>();out.put("gzip_hex",hex.toString());out.put("food",snapshot(player));out.put("maximum",bits(player.getMaxHealth()));return out;
    }
    static List<Object> harvest()throws Exception{
        List<Object> out=new ArrayList<Object>();Set<Integer> states=new TreeSet<Integer>();for(Object o:Block.blockRegistry){Block b=(Block)o;for(IBlockState s:b.getBlockState().getValidStates())states.add(Block.getIdFromBlock(b)<<4|b.getMetaFromState(s));}
        for(int raw:states)for(boolean shears:new boolean[]{false,true})for(int flags=0;flags<4;flags++){
            Player p=player(2,true,(flags&1)!=0,(flags&2)!=0,20f,20f);if(shears)p.inventory.mainInventory[0]=new ItemStack(Items.shears);
            Block b=Block.getBlockById(raw>>4);b.harvestBlock(p.worldObj,p,new BlockPos(0,64,0),b.getStateFromMeta(raw&15),null);
            out.add(Arrays.asList(raw,shears,flags,false,snapshot(p)));
        }
        for(int raw:new int[]{176<<4,177<<4|2})for(int flags=0;flags<4;flags++){
            Player p=player(2,true,(flags&1)!=0,(flags&2)!=0,20f,20f);Block b=Block.getBlockById(raw>>4);b.harvestBlock(p.worldObj,p,new BlockPos(0,64,0),b.getStateFromMeta(raw&15),new TileEntityBanner());out.add(Arrays.asList(raw,false,flags,true,snapshot(p)));
        }return out;
    }
    static List<Integer> harvestMask()throws Exception{
        List<Integer> out=new ArrayList<Integer>();Player p=player(2,true,false,false,20f,20f);
        for(int id=0;id<256;id++) {Block b=Block.getBlockById(id);if(b==null)b=Blocks.air;int mask=0,bit=0;for(int item:new int[]{0,270,274,257,278,267,256,359}){p.inventory.mainInventory[0]=item==0?null:new ItemStack(Item.getItemById(item));if(p.canHarvestBlock(b))mask|=1<<bit;bit++;}out.add(mask);}return out;
    }
    public static void main(String[] args)throws Exception{
        Bootstrap.register();List<Object> cases=new ArrayList<Object>();Random random=new Random(189);
        int[] levels={-1,0,1,17,18,20,25};int[] timers={-1,0,78,79,80,Integer.MAX_VALUE};float[] saturations={-1f,-0f,0f,.5f,5f,20f};float[] exhaustion={-1f,0f,4f,Float.intBitsToFloat(Float.floatToRawIntBits(4f)+1),8f,40f,41f};float[] health={0f,1f,10f,19f,20f,30f};
        for(int i=0;i<2048;i++)cases.add(tick(i%4,(i&4)==0,(i&8)!=0,(i&16)!=0,levels[random.nextInt(levels.length)],timers[random.nextInt(timers.length)],saturations[random.nextInt(saturations.length)],exhaustion[random.nextInt(exhaustion.length)],health[random.nextInt(health.length)],(i&32)==0?20f:40f,i%21,i%7==0?160:1,i%31==0?1:i%37==0?2:0));
        for(int difficulty=0;difficulty<4;difficulty++)for(boolean regen:new boolean[]{false,true})for(boolean immune:new boolean[]{false,true})for(int level:new int[]{0,17,18,20})for(float h:new float[]{1f,10f,19f})cases.add(tick(difficulty,regen,immune,false,level,79,0f,4f,h,20f,1,161,0));
        List<Object> actions=new ArrayList<Object>();for(int flags=0;flags<256;flags++)for(double[] d:new double[][]{{0,0,0},{.005,0,0},{.3,.42,-.27},{-.006,-.011,.029},{.000001,3.0,0},{189.125,-.25,-6.75}})actions.add(action("movement",flags,d,4f,0f,20,5f));
        for(String kind:new String[]{"jump","attack","mine","player_add","stats_add","eat"})for(int flags:new int[]{0,1,2,3,64,65,66,67})for(float v:new float[]{0f,.3f,3f,40f,-2f})actions.add(action(kind,flags,new double[]{.6,0,0},39f,v,18,.5f));
        List<Object> foods=new ArrayList<Object>();for(Object object:Item.itemRegistry)if(object instanceof ItemFood){ItemFood item=(ItemFood)object;Field always=ItemFood.class.getDeclaredField("alwaysEdible");always.setAccessible(true);for(int meta=0;meta<16;meta++){ItemStack stack=new ItemStack(item,1,meta);foods.add(Arrays.asList(Item.getIdFromItem(item),meta,item.getHealAmount(stack),bits(item.getSaturationModifier(stack)),always.getBoolean(item)));}}
        Map<String,Object> out=new LinkedHashMap<String,Object>();out.put("harvest",harvest());out.put("harvest_mask",harvestMask());out.put("legacy_import",legacy());out.put("ticks",cases);out.put("actions",actions);out.put("foods",foods);System.out.println(new Gson().toJson(out));
    }
}
