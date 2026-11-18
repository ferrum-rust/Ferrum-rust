import com.google.gson.Gson;
import com.mojang.authlib.GameProfile;
import java.io.PrintStream;
import java.lang.reflect.Field;
import java.util.*;
import net.minecraft.network.PacketBuffer;
import net.minecraft.network.play.client.C02PacketUseEntity;
import io.netty.buffer.Unpooled;
import net.minecraft.block.Block;
import net.minecraft.block.state.IBlockState;
import net.minecraft.client.Minecraft;
import net.minecraft.client.entity.EntityOtherPlayerMP;
import net.minecraft.client.multiplayer.*;
import net.minecraft.client.network.NetHandlerPlayClient;
import net.minecraft.client.renderer.EntityRenderer;
import net.minecraft.entity.*;
import net.minecraft.entity.item.*;
import net.minecraft.entity.passive.EntityCow;
import net.minecraft.entity.player.EntityPlayer;
import net.minecraft.entity.projectile.EntityLargeFireball;
import net.minecraft.init.*;
import net.minecraft.profiler.Profiler;
import net.minecraft.util.*;
import net.minecraft.world.*;
import net.minecraft.world.chunk.Chunk;

/** Execute EntityRenderer.getMouseOver with real World/Chunk entity-list traversal
 * and real entity classes. Only the renderer constructor (requires OpenGL) is bypassed. */
public final class EntityPickOracle {
    static class Scene extends WorldClient {
        final Map<BlockPos, Integer> blocks = new LinkedHashMap<BlockPos, Integer>();
        final Map<Long, Chunk> chunks = new HashMap<Long, Chunk>();
        Scene(NetHandlerPlayClient net) {
            super(net, new WorldSettings(189, WorldSettings.GameType.SURVIVAL, false, false,
                WorldType.DEFAULT), 0, EnumDifficulty.NORMAL, new Profiler());
        }
        long key(int x, int z) { return (long)x << 32 ^ (z & 0xffffffffL); }
        protected boolean isChunkLoaded(int x, int z, boolean allowEmpty) {
            return chunks != null && chunks.containsKey(key(x,z));
        }
        public Chunk getChunkFromChunkCoords(int x, int z) {
            long k = key(x,z);
            Chunk c = chunks.get(k);
            if (c == null) {
                c = new Chunk(this,x,z);
                c.setChunkLoaded(true);
                chunks.put(k,c);
            }
            return c;
        }
        public IBlockState getBlockState(BlockPos pos) {
            Integer state = blocks == null ? null : blocks.get(pos);
            return state == null ? Blocks.air.getDefaultState() : Block.BLOCK_STATE_IDS.getByValue(state);
        }
        void insert(Entity e) {
            getChunkFromChunkCoords(MathHelper.floor_double(e.posX/16.), MathHelper.floor_double(e.posZ/16.)).addEntity(e);
        }
    }
    static void field(Object target, Class<?> type, String name, Object value) throws Exception {
        Field f = type.getDeclaredField(name);
        f.setAccessible(true);
        f.set(target,value);
    }
    static List<String> vec(Vec3 v) { return MovementOracle.bits(v.xCoord,v.yCoord,v.zCoord); }
    static List<String> box(AxisAlignedBB b) {
        return MovementOracle.bits(b.minX,b.minY,b.minZ,b.maxX,b.maxY,b.maxZ);
    }
    static Entity actor(Scene w, int kind) {
        switch(kind) {
            case 1: return new EntityBoat(w);
            case 2: return new EntityItemFrame(w,new BlockPos(0,1,0),EnumFacing.SOUTH);
            case 3: return new EntityLargeFireball(w);
            case 4: return new EntityItem(w);
            case 5: return new EntityOtherPlayerMP(w,new GameProfile(new UUID(0,9),"spectator")) {
                public boolean isSpectator() { return true; }
            };
            default: return new EntityCow(w);
        }
    }
    static Map<String,Object> run(int scenario,int place,boolean creative,float alpha) throws Exception {
        Minecraft mc = MovementOracle.minecraft();
        field(mc,Minecraft.class,"mcProfiler",new Profiler());
        NetHandlerPlayClient net=new NetHandlerPlayClient(mc,null,null,new GameProfile(new UUID(0,189),"Pick"));
        Scene w = new Scene(net);
        mc.theWorld=w;
        MovementOracle.Player p=new MovementOracle.Player(mc,w,net);
        mc.thePlayer=p;
        mc.playerController=new PlayerControllerMP(mc,net);
        mc.playerController.setGameType(creative ? WorldSettings.GameType.CREATIVE : WorldSettings.GameType.SURVIVAL);
        field(mc,Minecraft.class,"renderViewEntity",p);
        EntityRenderer renderer=(EntityRenderer)MovementOracle.unsafe().allocateInstance(EntityRenderer.class);
        field(renderer,EntityRenderer.class,"mc",mc);
        double x=new double[]{15.94,-16.0625,16000000.0625}[place], y=15., z=place==2?-16000000.1875:15.9375;
        p.setPosition(x,y,z);
        p.prevPosX=x-.4; p.prevPosY=y-.125; p.prevPosZ=z+.3;
        p.rotationYaw=place==1?37.8f:0f; p.rotationPitch=place==1?-10.25f:0f;
        p.prevRotationYaw=p.rotationYaw-16f; p.prevRotationPitch=p.rotationPitch+5f;
        p.rotationYawHead=p.rotationYaw;
        p.prevRotationYawHead=p.prevRotationYaw;
        Vec3 eye=p.getPositionEyes(alpha), dir=p.getLook(alpha);
        Map<String,Object> row=new LinkedHashMap<String,Object>();
        row.put("case",Arrays.asList(scenario,place,creative,alpha));
        row.put("eye",vec(eye)); row.put("look",vec(dir)); row.put("view",box(p.getEntityBoundingBox()));
        List<Object> list=new ArrayList<Object>();
        int count = scenario==0 ? 0 : scenario==15 ? 8 : scenario>=21 && scenario<=23 ? 1 : 3;
        for(int i=0;i<count;i++) {
            int kind=scenario<=5 && i==0?scenario-1:(scenario+i)%6;
            if(scenario>=21) kind=0;
            Entity e=actor(w,kind);
            e.setEntityId(10+i);
            double d=new double[]{1.625,4.125,2.5}[i%3], side=i==0?0.:0.03*i;
            if(scenario==6 || scenario==7) d=2.; // equal intercepts, including different sections
            if(scenario==8) d=0.; // overlaps the eye, last intersecting entity wins
            if(scenario==9) d=i==0?0.:2.+i;
            if(scenario==10 || scenario==11) d=i==0?1.2:0.;
            if(scenario==12) d=3.225000001;
            if(scenario==13) d=5.65;
            if(scenario==14) d=1.; 
            if(scenario==15) d=(i%2==0?1.1:3.2);
            if(scenario>=16) d=2.+(i*.25);
            if(scenario==21) d=3.75;
            if(scenario==22) d=3.+.125+(double)0.1f;
            if(scenario==23) d=5.25;
            if(scenario==24) d=2.;
            if(scenario==25) d=i==0?0.:2.;
            if(scenario==26) d=i==1?0.:1.75;
            Vec3 center=eye.addVector(dir.xCoord*d,dir.yCoord*d,dir.zCoord*d);
            e.setPosition(center.xCoord+side,center.yCoord-.2,center.zCoord);
            if(scenario>=6) {
                double r=scenario==14?1e-8:.125;
                e.setEntityBoundingBox(new AxisAlignedBB(center.xCoord-r,center.yCoord-.4,center.zCoord-r,
                                                     center.xCoord+r,center.yCoord+.4,center.zCoord+r));
            }
            if((scenario==7 || scenario==24) && i==0) { e.posX+=1.; e.posY-=2.; } // chunk/section order differs from insertion
            if((scenario==10 && i==0)||(scenario==11 && i==2)||(scenario==25 && i==1)||(scenario==26 && i==2)) p.ridingEntity=e;
            w.insert(e);
            Map<String,Object> a=new LinkedHashMap<String,Object>();
            a.put("feet",MovementOracle.bits(e.posX,e.posY,e.posZ));
            a.put("id",e.getEntityId()); a.put("bounds",box(e.getEntityBoundingBox()));
            a.put("section",Arrays.asList(e.chunkCoordX,e.chunkCoordZ,e.chunkCoordY));
            a.put("border",e.getCollisionBorderSize());
            a.put("allow",e.canBeCollidedWith() && !(e instanceof EntityPlayer && ((EntityPlayer)e).isSpectator()));
            a.put("pointed",e instanceof EntityLivingBase || e instanceof EntityItemFrame);
            list.add(a);
        }
        row.put("riding",p.ridingEntity==null?0:p.ridingEntity.getEntityId());
        row.put("entities",list);
        List<Object> blocks=new ArrayList<Object>();
        if(scenario>=16 && scenario<=20) {
            double d=new double[]{.9,1.8,2.2,3.9,4.8}[scenario-16];
            Vec3 at=eye.addVector(dir.xCoord*d,dir.yCoord*d,dir.zCoord*d);
            BlockPos pos=new BlockPos(at);
            int state=new int[]{16,20<<4,101<<4,53<<4|3,16}[scenario-16];
            w.blocks.put(pos,state);
            blocks.add(Arrays.asList(pos.getX(),pos.getY(),pos.getZ(),state));
        }
        row.put("blocks",blocks);
        renderer.getMouseOver(alpha);
        MovingObjectPosition hit=mc.objectMouseOver;
        row.put("kind",hit==null?"NONE":hit.typeOfHit.name());
        row.put("pointed",mc.pointedEntity==null?0:mc.pointedEntity.getEntityId());
        if(hit!=null) {
            row.put("point",vec(hit.hitVec));
            if (hit.entityHit!=null) {
                Entity t=hit.entityHit;
                Vec3 relative=new Vec3(hit.hitVec.xCoord-t.posX,hit.hitVec.yCoord-t.posY,hit.hitVec.zCoord-t.posZ);
                PacketBuffer packet=new PacketBuffer(Unpooled.buffer());
                new C02PacketUseEntity(t,relative).writePacketData(packet);
                StringBuilder bytes=new StringBuilder();
                while(packet.isReadable()) bytes.append(String.format("%02x",packet.readUnsignedByte()));
                row.put("interact_at",bytes.toString());
            }
            row.put("entity",hit.entityHit==null?0:hit.entityHit.getEntityId());
            row.put("face",hit.sideHit==null?-1:hit.sideHit.getIndex());
            if(hit.getBlockPos()!=null) row.put("pos",Arrays.asList(hit.getBlockPos().getX(),hit.getBlockPos().getY(),hit.getBlockPos().getZ()));
        }
        return row;
    }
    public static void main(String[] args) throws Exception {
        PrintStream out=System.out; System.setOut(System.err); Bootstrap.register();
        List<Object> rows=new ArrayList<Object>();
        for(int i=0;i<=26;i++) for(int place=0;place<3;place++)
            for(boolean creative:new boolean[]{false,true}) for(float alpha:new float[]{0f,.25f,1f})
                rows.add(run(i,place,creative,alpha));
        out.println(new Gson().toJson(rows));
    }
}
