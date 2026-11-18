import com.google.gson.Gson;
import com.mojang.authlib.GameProfile;
import io.netty.buffer.Unpooled;
import java.io.PrintStream;
import java.lang.reflect.*;
import java.util.*;
import net.minecraft.block.*;
import net.minecraft.block.state.IBlockState;
import net.minecraft.client.Minecraft;
import net.minecraft.client.audio.*;
import net.minecraft.client.multiplayer.PlayerControllerMP;
import net.minecraft.client.network.NetHandlerPlayClient;
import net.minecraft.client.particle.EffectRenderer;
import net.minecraft.entity.Entity;
import net.minecraft.entity.passive.EntityCow;
import net.minecraft.init.*;
import net.minecraft.item.*;
import net.minecraft.nbt.NBTTagCompound;
import net.minecraft.network.*;
import net.minecraft.network.play.client.*;
import net.minecraft.util.*;
import net.minecraft.world.*;

/** Actual Minecraft.clickMouse/sendClickBlockToController + PlayerControllerMP.
 * The scene isolates block callback/render/sound outputs from the state machine.
 * No socket, OpenGL context or user save is opened. */
public final class ClickControllerOracle {
    static List<Object> events;
    static final BlockPos A=new BlockPos(8,2,10),B=new BlockPos(9,2,10);
    static List<Integer> pos(BlockPos p){return Arrays.asList(p.getX(),p.getY(),p.getZ());}
    static void add(Object...v){events.add(Arrays.asList(v));}
    static class Net extends NetHandlerPlayClient{
        Net(Minecraft mc){super(mc,null,null,new GameProfile(new UUID(0,189),"Click"));}
        public void addToSendQueue(Packet p){
            int id=p instanceof C07PacketPlayerDigging?7:p instanceof C0APacketAnimation?10:p instanceof C02PacketUseEntity?2:p instanceof C09PacketHeldItemChange?9:-1;
            if(id>=0){
                try{PacketBuffer b=new PacketBuffer(Unpooled.buffer());p.writePacketData(b);add("net",id,MiddlePickOracle.hex(b));}
                catch(Exception e){throw new RuntimeException(e);}
            }
        }
    }
    static class Scene extends EntityPickOracle.Scene{
        Scene(Net net){super(net);}
        public boolean setBlockState(BlockPos p,IBlockState state,int flags){
            int before=raw(p);int after=Block.getIdFromBlock(state.getBlock())<<4|state.getBlock().getMetaFromState(state);
            if(before==after)return false;
            blocks.put(p,after);add("set",pos(p),after);return true;
        }
        int raw(BlockPos p){Integer b=blocks.get(p);return b==null?0:b;}
        public void sendBlockBreakProgress(int id,BlockPos p,int stage){add("stage",pos(p),stage);}
        public void playAuxSFX(int sound,BlockPos p,int data){add("aux",sound,pos(p),data);}
    }
    static class Effects extends EffectRenderer{
        Effects(){super(null,null);}
        public void addBlockHitEffects(BlockPos p,EnumFacing face){add("particle",pos(p),face.getIndex());}
    }
    static class Sounds extends SoundHandler{
        Sounds(){super(null,null);}
        public void playSound(ISound s){add("sound",s.getSoundLocation().getResourcePath(),s.getVolume(),s.getPitch());}
    }
    static Object field(Object o,Class<?> c,String n)throws Exception {Field f=c.getDeclaredField(n);f.setAccessible(true);return f.get(o);}
    static Method method(Class<?> c,String n,Class<?>...args)throws Exception{Method m=c.getDeclaredMethod(n,args);m.setAccessible(true);return m;}
    static List<Object> scenario(int test,boolean creative)throws Exception{
        events=new ArrayList<Object>();
        Minecraft mc=MovementOracle.minecraft();Net net=new Net(mc);Scene w=new Scene(net);mc.theWorld=w;
        MovementOracle.Player p=new MovementOracle.Player(mc,w,net);p.setEntityId(189);p.setPosition(8.5,1.,8.5);p.onGround=true;
        mc.thePlayer=p;PlayerControllerMP pc=new PlayerControllerMP(mc,net);mc.playerController=pc;
        pc.setGameType(creative?WorldSettings.GameType.CREATIVE:WorldSettings.GameType.SURVIVAL);
        mc.effectRenderer=(Effects)MovementOracle.unsafe().allocateInstance(Effects.class);
        EntityPickOracle.field(mc,Minecraft.class,"mcSoundHandler",(Sounds)MovementOracle.unsafe().allocateInstance(Sounds.class));
        EntityCow cow=new EntityCow(w){public boolean attackEntityFrom(DamageSource s,float d){return false;}};cow.setEntityId(88);
        Method click=method(Minecraft.class,"clickMouse"),held=method(Minecraft.class,"sendClickBlockToController",boolean.class);
        Field counter=Minecraft.class.getDeclaredField("leftClickCounter");counter.setAccessible(true);
        int block=new int[]{16,16,20<<4,35<<4|3,7<<4,31<<4|1,16,16,16,16,16,16,16,16,16,16}[test];
        int tool=new int[]{278,0,278,359,278,0,278,278,267,267,278,278,278,278,278,278}[test];
        w.blocks.put(A,block);w.blocks.put(B,block);
        if(test==9)w.blocks.put(A.up(),Blocks.fire==null?0:51<<4);
        if(test==14){w.getWorldBorder().setCenter(0.,0.);w.getWorldBorder().setTransition(5.);}
        p.inventory.mainInventory[0]=tool==0?null:new ItemStack(Item.getItemById(tool));
        ItemStack heldObject=p.getHeldItem();int identity=0;
        List<Object> rows=new ArrayList<Object>();
        for(int tick=0;tick<63;tick++){
            int aim=0,face=(test==9?1:(tick/5)%6),press=tick==0?1:0;
            boolean down=tick<55;
            if(test==6){if(tick%16>=7&&tick%16<11)aim=1;if(tick%16==12)aim=2;}
            if(test==7){if(tick<9)aim=2;if(tick==3||tick==8||tick==10||tick==15)press=1;}
            if(test==8){if(tick<15||tick>25)aim=3;if(tick==14||tick==16||tick==26||tick==32)press=1;}
            if(test==10){if(tick==2||tick==5||tick==12){p.inventory.mainInventory[0]=new ItemStack(tick==2?Items.iron_pickaxe:Items.diamond_pickaxe);if(tick==12)p.getHeldItem().setItemDamage(7);}}
            if(test==11){if(tick==2)p.getHeldItem().setTagInfo("custom",new NBTTagCompound());
                if(tick==4)p.inventory.mainInventory[0]=p.getHeldItem().copy();
                if(tick==8){p.inventory.mainInventory[0]=new ItemStack(Items.diamond_pickaxe);p.getHeldItem().setItemDamage(3);}}
            if(test==12){down=tick%9<5;press=tick%9==0?2:0;}
            if(test==13){p.use=tick%11>=3 && tick%11<7;press=tick%7==0?1:0;}
            if(test==15){if(tick<3)aim=4;if(tick==5)press=2;if(tick==11)down=false;}
            if(heldObject != p.getHeldItem()){heldObject=p.getHeldItem();identity++;}
            BlockPos target=aim==1?B:A;
            EnumFacing side=EnumFacing.getFront(face);
            mc.objectMouseOver=aim==4?null:aim==3?new MovingObjectPosition(cow):aim==2?new MovingObjectPosition(MovingObjectPosition.MovingObjectType.MISS,new Vec3(A),null,A):new MovingObjectPosition(new Vec3(target),side,target);
            if(tick==29||tick==53)w.blocks.put(A,block);
            int state=w.raw(target);
            IBlockState current=w.getBlockState(target);
            float rate=current.getBlock().getPlayerRelativeBlockHardness(p,w,target);
            List<Object> row=new ArrayList<Object>(Arrays.asList(tick,aim,face,press,down,p.use,state,Float.floatToRawIntBits(rate),
                p.getHeldItem()==null?0:Item.getIdFromItem(p.getHeldItem().getItem()),p.getHeldItem()==null?0:p.getHeldItem().getMetadata(),
                p.getHeldItem()!=null&&p.getHeldItem().hasTagCompound(),identity,w.raw(target.offset(side))>>4==51,w.getWorldBorder().contains(target)));
            if(counter.getInt(mc)>0)counter.setInt(mc,counter.getInt(mc)-1);
            events=new ArrayList<Object>();
            if(!p.use) for(int count=0;count<press;count++)click.invoke(mc);
            held.invoke(mc,down);
            row.add(events);row.add(counter.getInt(mc));
            row.add(field(pc,PlayerControllerMP.class,"isHittingBlock"));
            row.add(pos((BlockPos)field(pc,PlayerControllerMP.class,"currentBlock")));
            row.add(Float.floatToRawIntBits((Float)field(pc,PlayerControllerMP.class,"curBlockDamageMP")));
            row.add(Float.floatToRawIntBits((Float)field(pc,PlayerControllerMP.class,"stepSoundTickCounter")));
            row.add(field(pc,PlayerControllerMP.class,"blockHitDelay"));row.add(w.raw(A));row.add(w.raw(B));rows.add(row);
        }
        return rows;
    }
    public static void main(String[] args)throws Exception{
        PrintStream out=System.out;System.setOut(System.err);Bootstrap.register();List<Object> suites=new ArrayList<Object>();
        if(args.length>0 && args[0].equals("sounds")){
            for(Block b:Block.blockRegistry)suites.add(Arrays.asList(Block.getIdFromBlock(b),b.stepSound.getStepSound(),
                (b.stepSound.getVolume()+1f)/8f,b.stepSound.getFrequency()*.5f));
            out.println(new Gson().toJson(suites));return;
        }
        for(int test=0;test<16;test++)for(boolean creative:new boolean[]{false,true})suites.add(Arrays.asList(test,creative,scenario(test,creative)));
        out.println(new Gson().toJson(suites));
    }
}
