import com.google.gson.Gson;
import java.io.*;
import java.lang.reflect.*;
import java.nio.*;
import java.nio.file.*;
import java.util.*;
import net.minecraft.block.Block;
import net.minecraft.client.Minecraft;
import net.minecraft.client.model.*;
import net.minecraft.client.renderer.texture.LayeredColorMaskTexture;
import net.minecraft.init.*;
import net.minecraft.item.*;
import net.minecraft.nbt.*;
import net.minecraft.server.MinecraftServer;
import net.minecraft.tileentity.TileEntityBanner;
import net.minecraft.util.*;
import net.minecraft.world.*;
import org.lwjgl.BufferUtils;
import org.lwjgl.opengl.GL11;

/** Capture original onItemUse, the three ModelBoxes and real LayeredColorMaskTexture output. */
public final class BannerOracle {
    static ItemStack stack() {
        ItemStack item = new ItemStack(Items.banner, 2, 4);
        NBTTagCompound data = new NBTTagCompound();
        data.setShort("Base", (short)11);
        NBTTagList list = new NBTTagList();
        for (String[] row : new String[][] {{"gra","1"}, {"cre","12"}, {"bo","15"}}) {
            NBTTagCompound part = new NBTTagCompound();
            part.setString("Pattern",row[0]); part.setInteger("Color",Integer.parseInt(row[1])); list.appendTag(part);
        }
        data.setTag("Patterns",list);
        item.setTagInfo("BlockEntityTag",data);
        return item;
    }
    public static void main(String[] args) throws Throwable {
        File dir = new File(args[0]);
        Minecraft mc = MenuOracle.startClient(dir, new File(args[1]), "Banner reference");
        Constructor<?> ctor=Class.forName("VanillaWorldBridge$Host").getDeclaredConstructor(File.class);ctor.setAccessible(true);
        MinecraftServer host=(MinecraftServer)ctor.newInstance(dir);
        RuntimeUseOracle.field(MinecraftServer.class,"serverThread").set(host,Thread.currentThread());
        RuntimeUseOracle.Fixture w = new RuntimeUseOracle.Fixture(host);
        host.worldServers=new WorldServer[]{w,null,null};w.init();w.theChunkProviderServer.loadChunk(0,0);
        RuntimeUseOracle.Player player=new RuntimeUseOracle.Player(host,w); new RuntimeUseOracle.Handler(host,player);
        BlockPos p=new BlockPos(8,64,8);
        List<Object> rows=new ArrayList<Object>();
        for (EnumFacing face:EnumFacing.values()) for(int n=0; n<(face==EnumFacing.UP?32:1);++n) {
            float yaw=n*11.25f-180f;
            w.setBlockState(p,Blocks.stone.getDefaultState(),2);
            w.setBlockState(p.offset(face),Blocks.air.getDefaultState(),2);
            player.rotationYaw=yaw;ItemStack item=stack();
            boolean ok=item.onItemUse(player,w,p,face,0.5f,0.5f,0.5f);
            int state=RuntimeUseOracle.state(w.getBlockState(p.offset(face)));
            rows.add(Arrays.asList(face.getIndex(),yaw,ok,state,item.stackSize));
        }
        ModelBanner m=new ModelBanner();
        List<Object> boxes=new ArrayList<Object>();
        Field quads=ModelBox.class.getDeclaredField("quadList");quads.setAccessible(true);
        for(ModelRenderer part:new ModelRenderer[]{m.bannerSlate,m.bannerStand,m.bannerTop}) {
            List<Object> corners =new ArrayList<Object>();
            for(TexturedQuad q:(TexturedQuad[])quads.get(part.cubeList.get(0))) for(PositionTextureVertex v:q.vertexPositions)
                corners.add(new double[]{v.vector3D.xCoord,v.vector3D.yCoord,v.vector3D.zCoord,v.texturePositionX,v.texturePositionY});
            boxes.add(corners);
        }
        TileEntityBanner tile=new TileEntityBanner();tile.setItemValues(stack());
        List<String> masks=new ArrayList<String>();
        for(TileEntityBanner.EnumBannerPattern ptn:tile.getPatternList()) masks.add("textures/entity/banner/"+ptn.getPatternName()+".png");
        LayeredColorMaskTexture texture=new LayeredColorMaskTexture(new ResourceLocation("textures/entity/banner_base.png"),masks,tile.getColorList());
        texture.loadTexture(mc.getResourceManager());
        ByteBuffer bytes=BufferUtils.createByteBuffer(64*64*4);
        GL11.glGetTexImage(GL11.GL_TEXTURE_2D,0,GL11.GL_RGBA,GL11.GL_UNSIGNED_BYTE,bytes);
        byte[] rgba=new byte[bytes.remaining()];bytes.get(rgba);
        Map<String,Object> out=new LinkedHashMap<>();out.put("placements",rows);out.put("boxes",boxes);out.put("rgba",Base64.getEncoder().encodeToString(rgba));
        Files.write(Paths.get(args[2]),new Gson().toJson(out).getBytes("UTF-8"));
        System.exit(0);
    }
}
