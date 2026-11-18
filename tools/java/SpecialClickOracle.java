import com.google.gson.Gson;
import com.mojang.authlib.GameProfile;
import java.io.PrintStream;
import java.lang.reflect.Field;
import java.security.MessageDigest;
import java.util.*;
import java.util.concurrent.atomic.AtomicLong;
import net.minecraft.block.*;
import net.minecraft.block.material.Material;
import net.minecraft.block.state.IBlockState;
import net.minecraft.client.Minecraft;
import net.minecraft.client.network.NetHandlerPlayClient;
import net.minecraft.client.particle.*;
import net.minecraft.client.renderer.*;
import net.minecraft.client.renderer.texture.TextureAtlasSprite;
import net.minecraft.entity.Entity;
import net.minecraft.init.*;
import net.minecraft.profiler.Profiler;
import net.minecraft.tileentity.*;
import net.minecraft.util.*;
import net.minecraft.world.*;
import net.minecraft.world.biome.BiomeGenBase;
import net.minecraft.world.chunk.*;
import net.minecraft.world.storage.WorldInfo;

/** Original special onBlockClicked callbacks, RNG tail and actual renderer
 * hit-particle positions. No user display / sound device is initialized. */
public final class SpecialClickOracle {
    static class W extends World {
        BlockPos at;int raw,mask,under;TileEntityNote tile;
        final Map<BlockPos,Integer> writes=new HashMap<BlockPos,Integer>();
        final List<Object> events=new ArrayList<Object>();
        final List<Object> particles=new ArrayList<Object>();
        int count;
        W(boolean remote)throws Exception {
            super(null,new WorldInfo(new WorldSettings(189,WorldSettings.GameType.SURVIVAL,false,false,WorldType.DEFAULT),"click"),
                new WorldProviderSurface(),new Profiler(),remote);
            provider.registerWorld(this);
        }
        protected IChunkProvider createChunkProvider(){return null;}
        protected int getRenderDistanceChunks(){return 8;}
        protected boolean isChunkLoaded(int x,int z,boolean empty){return true;}
        public BiomeGenBase getBiomeGenForCoords(BlockPos p){return BiomeGenBase.plains;}
        int state(BlockPos p){
            Integer v=writes.get(p);if(v!=null)return v;
            if(at==null)return 0;
            if(p.equals(at))return raw;
            if(raw>>4==122){
                int x=p.getX()-at.getX(),y=p.getY()-at.getY(),z=p.getZ()-at.getZ();
                return mask==31 || ((x*31+y*13+z*7)&mask)!=0?16:0;
            }
            if(raw>>4==25 && p.equals(at.down()))return under<<4;
            for(int s=0;s<6;s++)if(p.equals(at.offset(EnumFacing.getFront(s)))) return (mask&(1<<s))!=0?16:0;
            return 0;
        }
        public IBlockState getBlockState(BlockPos p){IBlockState s=Block.BLOCK_STATE_IDS.getByValue(state(p));return s==null?Blocks.air.getDefaultState():s;}
        public boolean setBlockState(BlockPos p,IBlockState s,int flags){
            int value=(Block.getIdFromBlock(s.getBlock())<<4)|s.getBlock().getMetaFromState(s);
            if(state(p)==value)return false;writes.put(p,value);
            events.add(Arrays.asList("set",p.getX(),p.getY(),p.getZ(),value,flags));return true;
        }
        public TileEntity getTileEntity(BlockPos p){return p.equals(at)&&raw>>4==25?tile:null;}
        public void addBlockEvent(BlockPos p,Block block,int instrument,int note){
            events.add(Arrays.asList("note",instrument,note));
            block.onBlockEventReceived(this,p,getBlockState(p),instrument,note);
        }
        public void playSoundEffect(double x,double y,double z,String clip,float vol,float pitch){events.add(Arrays.asList("sound",clip,Float.floatToRawIntBits(vol),Float.floatToRawIntBits(pitch)));}
        public void spawnParticle(EnumParticleTypes type,double x,double y,double z,double vx,double vy,double vz,int... data){
            count++;particles.add(Arrays.asList(type.getParticleID(),MovementOracle.bits(x,y,z,vx,vy,vz)));
        }
        void clear(){writes.clear();events.clear();count=0;particles.clear();}
    }
    static long state(Random r)throws Exception{Field f=Random.class.getDeclaredField("seed");f.setAccessible(true);return ((AtomicLong)f.get(r)).get();}
    static String hex(byte[] bytes){StringBuilder s=new StringBuilder();for(byte b:bytes)s.append(String.format("%02x",b&255));return s.toString();}
    static class Shapes extends BlockModelShapes{
        Shapes(){super(null);}public TextureAtlasSprite getTexture(IBlockState b){return null;}
    }
    static class FX extends EffectRenderer {
        EntityFX last;
        FX(){super(null,null);}
        public void addEffect(EntityFX e){last=e;}
    }
    static List<Object> hits(W world,Minecraft mc)throws Exception{
        EntityPickOracle.field(null,Minecraft.class,"theMinecraft",mc);
        BlockRendererDispatcher dispatcher=(BlockRendererDispatcher)MovementOracle.unsafe().allocateInstance(BlockRendererDispatcher.class);
        EntityPickOracle.field(dispatcher,BlockRendererDispatcher.class,"blockModelShapes",MovementOracle.unsafe().allocateInstance(Shapes.class));
        EntityPickOracle.field(mc,Minecraft.class,"blockRenderDispatcher",dispatcher);
        FX fx=(FX)MovementOracle.unsafe().allocateInstance(FX.class);
        EntityPickOracle.field(fx,EffectRenderer.class,"worldObj",world);
        Random rng=new Random();EntityPickOracle.field(fx,EffectRenderer.class,"rand",rng);
        List<Object> rows=new ArrayList<Object>();world.at=new BlockPos(16000003,63,-16000005);
        for(int raw=0;raw<4096;raw++) {
            IBlockState block=Block.BLOCK_STATE_IDS.getByValue(raw);if(block==null)continue;
            world.raw=raw;world.mask=0;world.clear();Block b=block.getBlock();
            Vec3 a=new Vec3(world.at).addVector(.125,.35,-1),z=a.addVector(0,0,3);
            b.collisionRayTrace(world,world.at,a,z);
            List<Object> positions=new ArrayList<Object>();
            for(int face=0;face<6;face++){
                rng.setSeed(189L*raw+face);fx.last=null;fx.addBlockHitEffects(world.at,EnumFacing.getFront(face));
                positions.add(fx.last==null?null:MovementOracle.bits(fx.last.posX,fx.last.posY,fx.last.posZ));
            }
            rows.add(Arrays.asList(raw,positions));
        }
        return rows;
    }
    public static void main(String[] args)throws Exception{
        PrintStream out=System.out;System.setOut(System.err);Bootstrap.register();
        Blocks.end_portal_frame.setBlockBoundsForItemRender();
        List<Object> rows=new ArrayList<Object>(),notes=new ArrayList<Object>();
        Minecraft mc=MovementOracle.minecraft();
        for(boolean remote:new boolean[]{false,true}){
            W w=new W(remote);NetHandlerPlayClient net=new NetHandlerPlayClient(mc,null,null,new GameProfile(new UUID(0,189),"Callback"));
            MovementOracle.Player p=new MovementOracle.Player(mc,w,net);w.tile=new TileEntityNote();w.tile.setWorldObj(w);
            for(int id:new int[]{25,73,74,92,122})for(int i=0;i<(id==25?198:64);i++){
                w.clear();w.at=(i%3==0?new BlockPos(16000003,63,-16000005):new BlockPos(-3,63,19));w.raw=id<<4|(id==92?i%7:0);
                w.mask=id==122?new int[]{0,1,3,7,15,31}[i%6]:i&63;w.under=i%198;
                w.tile.setPos(w.at);w.tile.note=(byte)(i%25);
                p.capabilities.disableDamage=(i%4==0);p.getFoodStats().setFoodLevel(16+i%5);p.getFoodStats().setFoodSaturationLevel(1.25f);
                long seed=i*9397L-189;w.rand.setSeed(seed);
                w.getBlockState(w.at).getBlock().onBlockClicked(w,w.at,p);
                rows.add(Arrays.asList(remote,w.raw,i,seed,w.events.toArray(),w.count,w.particles.toArray(),state(w.rand),
                    p.getFoodStats().getFoodLevel(),Float.floatToRawIntBits(p.getFoodStats().getSaturationLevel())));
            }
        }
        for(Block b:Block.blockRegistry){Material m=b.getMaterial();notes.add(Arrays.asList(Block.getIdFromBlock(b),m==Material.rock?1:m==Material.sand?2:m==Material.glass?3:m==Material.wood?4:0));}
        Map<String,Object> data=new LinkedHashMap<String,Object>();data.put("clicks",rows);data.put("instruments",notes);data.put("hits",hits(new W(true),mc));out.println(new Gson().toJson(data));
    }
}
