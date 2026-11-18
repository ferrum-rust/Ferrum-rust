import com.google.gson.Gson;
import java.io.File;
import java.lang.reflect.Constructor;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import net.minecraft.entity.*;
import net.minecraft.entity.ai.EntityAIPanic;
import net.minecraft.entity.passive.EntityCow;
import net.minecraft.init.*;
import net.minecraft.item.*;
import net.minecraft.nbt.NBTTagCompound;
import net.minecraft.server.MinecraftServer;
import net.minecraft.world.WorldServer;

public final class AnimalMeleeOracle {
    public static void main(String[] args)throws Exception{
        Bootstrap.register();Constructor<?> c=Class.forName("VanillaWorldBridge$Host").getDeclaredConstructor(File.class);c.setAccessible(true);
        MinecraftServer h=(MinecraftServer)c.newInstance(new File(args[1]));RuntimeUseOracle.field(MinecraftServer.class,"serverThread").set(h,Thread.currentThread());
        RuntimeUseOracle.Fixture w=new RuntimeUseOracle.Fixture(h);h.worldServers=new WorldServer[]{w};w.init();w.theChunkProviderServer.loadChunk(0,0);
        List<Object> cases=new ArrayList<>();
        for(int variant=0;variant<7;++variant){
            RuntimeUseOracle.Player p=new RuntimeUseOracle.Player(h,w);new RuntimeUseOracle.Handler(h,p);p.setPosition(3,42,4);
            EntityCow cow=new EntityCow(w);cow.setPosition(6,42,8);cow.motionX=0.1;cow.motionY=-0.2;cow.motionZ=0.3;
            boolean inv=variant==2||variant==3,noAI=variant==4;boolean creative=variant==3; p.capabilities.isCreativeMode=creative;
            if(inv){NBTTagCompound tag=new NBTTagCompound();cow.writeToNBT(tag);tag.setBoolean("Invulnerable",true);cow.readFromNBT(tag);}
            cow.setNoAI(noAI);cow.setInLove(p);p.rotationYaw=90;p.onGround=true;
            List<Object> rows=new ArrayList<>();
            for(int i=0;i<5;++i){
                int item=i==0?0:i==1?(variant==1?276:0):i==2?268:i==3?269:0;
                if(variant==5)item=i==0?267:0;if(variant==6)item=i==0?284:0;
                if(i==3){for(int t=0;t<10;t++){cow.ticksExisted++;cow.onEntityUpdate();}}
                if(i==4){for(int t=0;t<101;t++){cow.ticksExisted++;cow.onEntityUpdate();}}
                if(i!=4){
                    ItemStack old=p.getHeldItem();if(old!=null)p.getAttributeMap().removeAttributeModifiers(old.getAttributeModifiers());
                    ItemStack stack=item==0?null:new ItemStack(Item.getItemById(item));
                    p.inventory.setInventorySlotContents(0,stack);if(stack!=null)p.getAttributeMap().applyAttributeModifiers(stack.getAttributeModifiers());
                    p.attackTargetEntityWithCurrentItem(cow);
                }
                boolean eligible=new EntityAIPanic(cow,2.).shouldExecute();
                rows.add(Arrays.asList(item,cow.getHealth(),cow.hurtTime,cow.motionX,cow.motionY,cow.motionZ,cow.getAITarget()!=null,cow.isInLove(),eligible,p.getHeldItem()==null?0:p.getHeldItem().getItemDamage()));
            }
            cases.add(Arrays.asList(variant,rows));
        }
        Files.write(Paths.get(args[0]),new Gson().toJson(cases).getBytes(StandardCharsets.UTF_8));
    }
}
