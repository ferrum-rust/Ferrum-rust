import com.google.gson.Gson;
import com.mojang.authlib.GameProfile;
import java.nio.file.*;
import java.nio.charset.StandardCharsets;
import java.util.*;
import net.minecraft.client.Minecraft;
import net.minecraft.client.multiplayer.PlayerControllerMP;
import net.minecraft.client.network.NetHandlerPlayClient;
import net.minecraft.client.settings.KeyBinding;
import net.minecraft.entity.Entity;
import net.minecraft.entity.item.EntityBoat;
import net.minecraft.entity.item.EntityMinecartEmpty;
import net.minecraft.entity.passive.EntityHorse;
import net.minecraft.init.Bootstrap;
import net.minecraft.network.Packet;
import net.minecraft.network.play.client.C0BPacketEntityAction;

public final class RemoteVehicleOracle {
    static class Handler extends NetHandlerPlayClient{
        int jump=-1;
        Handler(Minecraft mc){super(mc,null,null,new GameProfile(new UUID(0,189),"Oracle"));}
        public void addToSendQueue(Packet packet){
            if(packet instanceof C0BPacketEntityAction){C0BPacketEntityAction a=(C0BPacketEntityAction)packet;
                if(a.getAction()==C0BPacketEntityAction.Action.RIDING_JUMP)jump=a.getAuxData();}
        }
    }
    public static void main(String[] args)throws Exception{
        Bootstrap.register();List<Object> cases=new ArrayList<>();
        for(int kind=0;kind<7;kind++){
            int terrain=kind==0?4:kind==1?6:kind==2?12:0;
            MovementOracle.TestWorld world=new MovementOracle.TestWorld(terrain,0,0);
            Minecraft mc=MovementOracle.minecraft();Handler net=new Handler(mc);mc.playerController=new PlayerControllerMP(mc,net);
            MovementOracle.Player p=new MovementOracle.Player(mc,world,net);
            double y=kind==0?3.8:kind==1?2:kind==2?1:4;
            Entity v=kind<4?new EntityBoat(world,8.5,y,8.5):kind==4?new EntityMinecartEmpty(world,8.5,y,8.5):new EntityHorse(world);
            v.setPosition(8.5,y,8.5);v.rotationYaw=0;v.rotationPitch=0;
            p.ridingEntity=v;v.riddenByEntity=p;if(v instanceof EntityBoat)((EntityBoat)v).setIsBoatEmpty(kind==3);
            if(v instanceof EntityHorse)((EntityHorse)v).setHorseSaddled(true);
            List<Object> ticks=new ArrayList<>();KeyBinding.setKeyBindState(mc.gameSettings.keyBindJump.getKeyCode(),false);
            for(int tick=0;tick<45;tick++){
                if(kind==3)v.riddenByEntity=null;
                float yaw=tick<15?0f:tick<30?53.25f:180f,strafe=tick>=10&&tick<20?.98f:0,forward=tick<34?.98f:0;
                p.moveStrafing=strafe;p.moveForward=forward;p.rotationYaw=yaw;p.rotationPitch=0;
                if(tick==5){v.setVelocity(.11,.06,-.07);}
                double[] correction=null;
                if(tick==8||tick==21||tick==32){correction=tick==8?new double[]{8.7,y,8.5,20,0}:tick==21?new double[]{9.7,y+.3,10.8,70,0}:new double[]{8.3,y,9.5,120,0};v.setPositionAndRotation2(correction[0],correction[1],correction[2],(float)correction[3],0,3,tick==32);v.onGround=false;}
                if(v instanceof EntityHorse){
                    EntityHorse h=(EntityHorse)v;h.setRearing(kind==6&&tick>=10&&tick<24);h.renderYawOffset=yaw;
                }
                v.onUpdate();v.updateRiderPosition();
                boolean jump=tick>=2&&tick<7||tick>=12&&tick<24||tick>=30&&tick<44;
                net.jump=-1;
                if(v instanceof EntityHorse){
                    KeyBinding.setKeyBindState(mc.gameSettings.keyBindJump.getKeyCode(),jump);p.onLivingUpdate();
                }
                if(kind==3)v.riddenByEntity=p;v.updateRiderPosition();
                ticks.add(Arrays.asList(tick,Arrays.asList(yaw,strafe,forward),correction,jump,net.jump,
                    Arrays.asList(v.posX,v.posY,v.posZ,v.motionX,v.motionY,v.motionZ,(double)v.rotationYaw),
                    Arrays.asList(p.posX,p.posY,p.posZ)));
            }
            cases.add(Arrays.asList(kind,terrain,y,ticks));
        }
        Files.write(Paths.get(args[0]),new Gson().toJson(cases).getBytes(StandardCharsets.UTF_8));System.out.println("315 original client vehicle/rider ticks");
    }
}

