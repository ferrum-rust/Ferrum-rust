import java.io.*;
import java.lang.reflect.Constructor;
import java.util.*;
import java.util.zip.GZIPOutputStream;
import com.google.gson.Gson;
import net.minecraft.entity.Entity;
import net.minecraft.entity.item.EntityItem;
import net.minecraft.init.Bootstrap;
import net.minecraft.init.Blocks;
import net.minecraft.inventory.*;
import net.minecraft.item.*;
import net.minecraft.nbt.NBTTagCompound;
import net.minecraft.network.play.client.C08PacketPlayerBlockPlacement;
import net.minecraft.server.MinecraftServer;
import net.minecraft.util.BlockPos;
import net.minecraft.world.*;

/** Actual ContainerWorkbench results, inventory leftovers and close drops. */
public final class CraftWindowOracle {
    static ItemStack item(int id,int count,int metadata){return new ItemStack(Item.getItemById(id),count,metadata);}
    static List<Object> sparse(ItemStack[] stacks)throws Exception{List<Object> r=new ArrayList<>();for(int i=0;i<stacks.length;i++)if(stacks[i]!=null)r.add(Arrays.asList(i,CraftRegistryOracle.stack(stacks[i])));return r;}
    static List<Object> grid(InventoryCrafting c)throws Exception{ItemStack[] items=new ItemStack[c.getSizeInventory()];for(int i=0;i<items.length;i++)items[i]=c.getStackInSlot(i);return sparse(items);}
    static List<Object> frame(RuntimeUseOracle.Fixture w,RuntimeUseOracle.Player p,ContainerWorkbench table)throws Exception{
        List<Object> drops=new ArrayList<>();for(Entity e:w.loadedEntityList)if(e instanceof EntityItem)drops.add(CraftRegistryOracle.stack(((EntityItem)e).getEntityItem()));
        return Arrays.asList(sparse(p.inventory.mainInventory),CraftRegistryOracle.stack(p.inventory.getItemStack()),grid(table.craftMatrix),grid(((ContainerPlayer)p.inventoryContainer).craftMatrix),drops);
    }
    static Map<String,Object> run(MinecraftServer h,int mode,int profile,int test)throws Exception{
        RuntimeUseOracle.Fixture w=new RuntimeUseOracle.Fixture(h);h.worldServers=new WorldServer[]{w,null,null};w.init();w.theChunkProviderServer.loadChunk(0,0);
        RuntimeUseOracle.Player p=new RuntimeUseOracle.Player(h,w);RuntimeUseOracle.Handler net=new RuntimeUseOracle.Handler(h,p);
        p.theItemInWorldManager.setGameType(test==4?WorldSettings.GameType.CREATIVE:WorldSettings.GameType.SURVIVAL);p.setPosition(8.5,2.,8.5);
        BlockPos pos=new BlockPos(8,1,8);w.setBlockState(pos,Blocks.crafting_table.getDefaultState(),2);
        ((ContainerPlayer)p.inventoryContainer).craftMatrix.setInventorySlotContents(0,item(296,9,0));
        net.processPlayerBlockPlacement(new C08PacketPlayerBlockPlacement(pos,1,null,.5f,1f,.5f));
        ContainerWorkbench table=(ContainerWorkbench)p.openContainer;ItemStack[] slots=new ItemStack[9];
        switch(mode){
            case 0:slots[2]=item(17,3,2);break;
            case 1:for(int i=0;i<9;i++)slots[i]=item(i<3?335:i==4?344:i<6?353:296,profile%2==0?1:2,0);break;
            case 2:slots[2]=item(276,1,1510);slots[7]=item(276,1,887);break;
            case 3:slots[2]=item(387,1,0);NBTTagCompound n=new NBTTagCompound();n.setInteger("generation",0);n.setString("title","Letter");slots[2].setTagCompound(n);slots[6]=item(386,2,0);slots[7]=item(386,2,0);break;
            case 4:slots[0]=item(299,1,37);slots[8]=item(351,2,4);break;
            case 5:slots[4]=item(358,1,1337);slots[0]=item(395,2,0);slots[8]=item(395,2,0);break;
        }
        for(int i=0;i<9;i++)table.craftMatrix.setInventorySlotContents(i,slots[i]);
        ItemStack result=table.getSlot(0).getStack();if(result==null)throw new AssertionError("Missing fixture recipe "+mode);
        if(profile>=1){for(int i=0;i<36;i++)p.inventory.mainInventory[i]=item(4,64,0);}
        if(profile==1){p.inventory.mainInventory[8]=result.copy();p.inventory.mainInventory[8].stackSize=Math.max(1,result.getMaxStackSize()-1);}
        if(profile==3)p.inventory.mainInventory[8]=null;
        List<Object> initial=frame(w,p,table);int button=test==1?1:test==3?1:test==4?2:0;int click=test<=1?0:test==2?1:test==3?4:test==4?3:2;
        table.slotClick(0,button,click,p);
        List<Object> clicked=frame(w,p,table);
        table.onContainerClosed(p);Map<String,Object> row=new LinkedHashMap<>();
        row.put("mode",mode);row.put("profile",profile);row.put("test",test);row.put("initial",initial);row.put("clicked",clicked);row.put("closed",frame(w,p,table));
        return row;
    }
    public static void main(String[] argv)throws Exception{
        Bootstrap.register();File dir=new File(argv[1]);dir.mkdirs();Constructor<?> c=Class.forName("VanillaWorldBridge$Host").getDeclaredConstructor(File.class);c.setAccessible(true);MinecraftServer host=(MinecraftServer)c.newInstance(dir);
        RuntimeUseOracle.field(MinecraftServer.class,"serverThread").set(host,Thread.currentThread());
        List<Object> rows=new ArrayList<>();for(int m=0;m<6;m++)for(int p=0;p<4;p++)for(int t=0;t<6;t++)rows.add(run(host,m,p,t));
        try(Writer out=new OutputStreamWriter(new GZIPOutputStream(new FileOutputStream(argv[0])),"UTF-8")){new Gson().toJson(rows,out);}
        System.out.println("Original ContainerWorkbench click/close fixtures "+rows.size());
    }
}
