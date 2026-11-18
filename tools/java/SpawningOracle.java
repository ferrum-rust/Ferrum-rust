// Direct original-bytecode world-generation animal spawning oracle.
import com.google.gson.Gson;
import java.lang.reflect.*;
import java.util.*;
import sun.misc.Unsafe;
import net.minecraft.init.*;
import net.minecraft.block.*;
import net.minecraft.block.state.IBlockState;
import net.minecraft.entity.*;
import net.minecraft.entity.passive.*;
import net.minecraft.util.BlockPos;
import net.minecraft.world.*;
import net.minecraft.world.biome.*;
import net.minecraft.world.border.WorldBorder;
import net.minecraft.world.storage.WorldInfo;

public final class SpawningOracle {
    static final class TrackedRandom extends Random {
        int weight,group=-1;TrackedRandom(long seed,int weight){super(seed);this.weight=weight;}
        @Override public int nextInt(int bound){if(bound==weight)group++;return super.nextInt(bound);}
    }
    static int initial(BlockPos p,int scenario){
        int x=p.getX(),y=p.getY(),z=p.getZ();if(y<0||y>=256)return 0;
        if(scenario==7)return 0;
        if(y<63)return 16;
        if(y==63)return scenario==2?7<<4:scenario==4?85<<4:2<<4;
        if(y==64&&(scenario==1||scenario==3&&Math.floorMod(x+z,3)==0))return 9<<4;
        if(y==64&&scenario==3&&Math.floorMod(x+z,5)==0)return 18<<4;
        return 0;
    }
    static IBlockState state(int raw){return Block.getBlockById(raw>>4).getStateFromMeta(raw&15);}
    static final class OracleWorld extends WorldServer {
        int scenario;IBlockState[] predicate;WorldBorder border;List<EntityLiving> entities;List<Integer> groups;
        OracleWorld(){super(null,null,null,0,null);}
        @Override public IBlockState getBlockState(BlockPos p){return predicate!=null&&p.getY()>=63&&p.getY()<=65?predicate[p.getY()-63]:state(initial(p,scenario));}
        @Override public BlockPos getTopSolidOrLiquidBlock(BlockPos p){
            for(int y=255;y>=0;y--){net.minecraft.block.material.Material m=getBlockState(new BlockPos(p.getX(),y,p.getZ())).getBlock().getMaterial();if(m.blocksMovement()&&m!=net.minecraft.block.material.Material.leaves)return new BlockPos(p.getX(),y+1,p.getZ());}
            return new BlockPos(p.getX(),-1,p.getZ());
        }
        @Override public WorldBorder getWorldBorder(){return border;}
        @Override public boolean spawnEntityInWorld(Entity entity){entities.add((EntityLiving)entity);groups.add(((TrackedRandom)rand).group);return true;}
        @Override public DifficultyInstance getDifficultyForLocation(BlockPos p){return new DifficultyInstance(EnumDifficulty.NORMAL,6000L,0L,0.0f);}
    }
    static void set(Object object,Class<?> cls,String name,Object value)throws Exception{Field f=cls.getDeclaredField(name);f.setAccessible(true);f.set(object,value);}
    static OracleWorld world(int scenario,long seed,int weight)throws Exception{
        Field u=Unsafe.class.getDeclaredField("theUnsafe");u.setAccessible(true);OracleWorld w=(OracleWorld)((Unsafe)u.get(null)).allocateInstance(OracleWorld.class);
        w.scenario=scenario;w.entities=new ArrayList<EntityLiving>();w.groups=new ArrayList<Integer>();w.border=new WorldBorder();
        if(scenario==6){w.border.setCenter(16.,16.);w.border.setTransition(8.);}
        set(w,World.class,"provider",new WorldProviderSurface());set(w,World.class,"rand",new TrackedRandom(seed,weight));
        set(w,World.class,"worldInfo",new WorldInfo(new WorldSettings(189L,WorldSettings.GameType.SURVIVAL,false,false,WorldType.DEFAULT),"SpawningOracle"));
        return w;
    }
    static int weight(BiomeGenBase.SpawnListEntry entry)throws Exception{Field f=net.minecraft.util.WeightedRandom.Item.class.getDeclaredField("itemWeight");f.setAccessible(true);return f.getInt(entry);}
    static Map<String,Object> run(long populationSeed,long worldSeed,int id,int scenario,int[] area)throws Exception{
        BiomeGenBase biome=BiomeGenBase.getBiome(id);int weight=0;for(BiomeGenBase.SpawnListEntry e:biome.getSpawnableList(EnumCreatureType.CREATURE))weight+=weight(e);
        if(weight>=100)throw new AssertionError("Group tracking requires weight distinct from sheep color bounds");
        OracleWorld world=world(scenario,worldSeed,weight);Random random=new Random(populationSeed);
        SpawnerAnimals.performWorldGenSpawning(world,biome,area[0],area[1],area[2],area[3],random);
        List<Object> events=new ArrayList<Object>();
        for(int i=0;i<world.entities.size();i++){
            EntityLiving e=world.entities.get(i);Map<String,Object> event=new LinkedHashMap<String,Object>();
            event.put("kind",EntityList.getEntityString(e));event.put("position",new double[]{e.posX,e.posY,e.posZ});event.put("yaw_bits",Integer.toUnsignedLong(Float.floatToRawIntBits(e.rotationYaw)));event.put("group",world.groups.get(i));
            event.put("sheep_color",e instanceof EntitySheep?((EntitySheep)e).getFleeceColor().getMetadata():null);events.add(event);
        }
        Map<String,Object> out=new LinkedHashMap<String,Object>();out.put("population_seed",Long.toString(populationSeed));out.put("world_seed",Long.toString(worldSeed));out.put("biome",id);out.put("scenario",scenario);out.put("area",area);out.put("events",events);out.put("population_tail",Long.toString(random.nextLong()));out.put("world_tail",Long.toString(world.rand.nextLong()));return out;
    }
    static Map<String,Object> initialization(Class<? extends EntityLiving> cls,long seed)throws Exception{
        OracleWorld world=world(0,seed^0x189L,0);IEntityLivingData group=null;List<Object> events=new ArrayList<Object>();
        for(int i=0;i<6;i++){
            EntityLiving e=cls.getConstructor(World.class).newInstance(world);long entitySeed=seed+(long)i*341873128712L;Random entityRandom=new Random(entitySeed);set(e,Entity.class,"rand",entityRandom);
            group=e.onInitialSpawn(new DifficultyInstance(EnumDifficulty.NORMAL,6000L,0L,0.0f),group);
            Map<String,Object> event=new LinkedHashMap<String,Object>();event.put("seed",Long.toString(entitySeed));event.put("age",((EntityAgeable)e).getGrowingAge());
            for(net.minecraft.entity.ai.attributes.AttributeModifier modifier:e.getEntityAttribute(SharedMonsterAttributes.followRange).func_111122_c())if(modifier.getName().equals("Random spawn bonus"))event.put("follow_bits",Long.toUnsignedString(Double.doubleToRawLongBits(modifier.getAmount())));
            if(e instanceof EntityHorse){EntityHorse h=(EntityHorse)e;event.put("horse_type",h.getHorseType());event.put("horse_variant",h.getHorseVariant());event.put("health_bits",Long.toUnsignedString(Double.doubleToRawLongBits(h.getEntityAttribute(SharedMonsterAttributes.maxHealth).getBaseValue())));event.put("speed_bits",Long.toUnsignedString(Double.doubleToRawLongBits(h.getEntityAttribute(SharedMonsterAttributes.movementSpeed).getBaseValue())));event.put("jump_bits",Long.toUnsignedString(Double.doubleToRawLongBits(h.getEntityAttribute((net.minecraft.entity.ai.attributes.IAttribute)get(EntityHorse.class,"horseJumpStrength",null)).getBaseValue())));}
            if(e instanceof EntityRabbit)event.put("rabbit_type",((EntityRabbit)e).getRabbitType());
            if(e instanceof EntitySheep)event.put("sheep_color",((EntitySheep)e).getFleeceColor().getMetadata());
            event.put("cached_gaussian_bits",Long.toUnsignedString(Double.doubleToRawLongBits(entityRandom.nextGaussian())));event.put("tail",Long.toString(entityRandom.nextLong()));events.add(event);
        }
        Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("kind",((Map<Class<?>,String>)get(EntityList.class,"classToStringMapping",null)).get(cls));row.put("world_seed",Long.toString(seed^0x189L));row.put("world_tail",Long.toString(world.rand.nextLong()));row.put("events",events);return row;
    }
    static Object get(Class<?> cls,String name,Object object)throws Exception{Field f=cls.getDeclaredField(name);f.setAccessible(true);return f.get(object);}
    static List<Object> predicates()throws Exception{
        OracleWorld world=world(0,0L,0);List<Object> result=new ArrayList<Object>();Set<Integer> seen=new TreeSet<Integer>();
        for(Object object:Block.blockRegistry){Block block=(Block)object;for(IBlockState state:block.getBlockState().getValidStates()){
            int raw=Block.getIdFromBlock(block)<<4|block.getMetaFromState(state);if(!seen.add(raw))continue;
            world.predicate=new IBlockState[]{state,Blocks.air.getDefaultState(),Blocks.air.getDefaultState()};boolean solid=World.doesBlockHaveSolidTopSurface(world,new BlockPos(0,63,0));
            List<Object> checks=new ArrayList<Object>();
            for(int placement=0;placement<5;placement++){
                int[] triple=placement==0?new int[]{raw,0,0}:placement==1?new int[]{2<<4,raw,0}:placement==2?new int[]{2<<4,0,raw}:placement==3?new int[]{9<<4,raw,0}:new int[]{9<<4,9<<4,raw};
                world.predicate=new IBlockState[]{state(triple[0]),state(triple[1]),state(triple[2])};
                checks.add(Arrays.asList(triple,SpawnerAnimals.canCreatureTypeSpawnAtLocation(EntityLiving.SpawnPlacementType.ON_GROUND,world,new BlockPos(0,64,0)),SpawnerAnimals.canCreatureTypeSpawnAtLocation(EntityLiving.SpawnPlacementType.IN_WATER,world,new BlockPos(0,64,0))));
            }
            result.add(Arrays.asList(raw,solid,checks));
        }}return result;
    }
    public static void main(String[] args)throws Exception{
        Bootstrap.register();List<Object> lists=new ArrayList<Object>();List<Integer> ids=new ArrayList<Integer>();
        Field mapping=EntityList.class.getDeclaredField("classToStringMapping");mapping.setAccessible(true);Map<Class<?>,String> names=(Map<Class<?>,String>)mapping.get(null);
        for(BiomeGenBase biome:BiomeGenBase.getBiomeGenArray())if(biome!=null){
            ids.add(biome.biomeID);List<Object> entries=new ArrayList<Object>();for(BiomeGenBase.SpawnListEntry e:biome.getSpawnableList(EnumCreatureType.CREATURE))entries.add(Arrays.asList(names.get(e.entityClass),weight(e),e.minGroupCount,e.maxGroupCount));
            Map<String,Object> row=new LinkedHashMap<String,Object>();row.put("id",biome.biomeID);row.put("chance_bits",Integer.toUnsignedLong(Float.floatToRawIntBits(biome.getSpawningChance())));row.put("entries",entries);lists.add(row);
        }
        List<Long> seeds=new ArrayList<Long>();seeds.add(0L);seeds.add(189L);for(long seed=0;seeds.size()<12;seed+=97)if(new Random(seed).nextFloat()<0.1f)seeds.add(seed);
        List<Object> cases=new ArrayList<Object>();int index=0;
        for(int id:ids)for(long seed:seeds)for(int scenario:new int[]{0,1,2,3,4,6,7}){
            int[] area=index%3==0?new int[]{2,3,7,11}:index%3==1?new int[]{-19,-7,11,7}:new int[]{8,8,16,16};
            cases.add(run(seed,189L+(long)index*341873128712L,id,scenario,area));index++;
        }
        List<Object> initializations=new ArrayList<Object>();
        for(Class<?> cls:new Class<?>[]{EntitySheep.class,EntityRabbit.class,EntityPig.class,EntityChicken.class,EntityCow.class,EntityWolf.class,EntityHorse.class,EntityMooshroom.class})for(int i=0;i<64;i++)initializations.add(initialization((Class<? extends EntityLiving>)cls,(long)i*9741L-189L));
        Map<String,Object> out=new LinkedHashMap<String,Object>();out.put("predicates",predicates());out.put("initializations",initializations);out.put("spawn_lists",lists);out.put("cases",cases);System.out.println(new Gson().toJson(out));
    }
}
