import com.google.gson.Gson;
import java.io.*;
import java.nio.*;
import java.nio.file.*;
import java.util.*;
import net.minecraft.client.renderer.*;
import net.minecraft.client.renderer.entity.*;
import net.minecraft.entity.item.EntityItem;
import net.minecraft.entity.Entity;
import net.minecraft.client.renderer.block.model.*;
import net.minecraft.client.renderer.texture.*;
import net.minecraft.client.renderer.tileentity.TileEntityItemStackRenderer;
import net.minecraft.client.resources.model.*;
import net.minecraft.creativetab.CreativeTabs;
import net.minecraft.item.*;
import net.minecraft.util.*;
import net.minecraft.world.*;
import org.lwjgl.BufferUtils;
import org.lwjgl.opengl.GL11;
import org.lwjgl.util.glu.GLU;

/** Original model-space geometry and transforms. Texture pixels remain in the user's installation. */
public final class WorldItemModelOracle {
    final VanillaFrameBenchmark.Client mc;
    final List<TextureAtlasSprite> sprites;
    final Map<String,Integer> ids=new TreeMap<>(), unique=new HashMap<>();
    final List<Object> models=new ArrayList<>();
    final Gson gson=new Gson();
    WorldItemModelOracle(VanillaFrameBenchmark.Client mc)throws Exception{
        this.mc=mc;
        Map<?,TextureAtlasSprite> atlas=(Map<?,TextureAtlasSprite>)MenuOracle.field(TextureMap.class,"mapUploadedSprites").get(mc.getTextureMapBlocks());
        sprites=new ArrayList<>(atlas.values());
    }
    List<Object> baked(ItemStack s, IBakedModel model) {
        List<BakedQuad> quads=new ArrayList<>(); List<Object> out=new ArrayList<>();
        for(EnumFacing face:EnumFacing.values())quads.addAll(model.getFaceQuads(face));
        quads.addAll(model.getGeneralQuads());
        for(BakedQuad quad:quads){
            int[] vd=quad.getVertexData(),xyzuv=new int[20]; float u=0,v=0;
            for(int i=0;i<4;i++){u+=Float.intBitsToFloat(vd[7*i+4]);v+=Float.intBitsToFloat(vd[7*i+5]);}
            TextureAtlasSprite tex=null;
            for(TextureAtlasSprite t:sprites)if(u/4>=t.getMinU()&&u/4<=t.getMaxU()&&v/4>=t.getMinV()&&v/4<=t.getMaxV()){tex=t;break;}
            if(tex==null)throw new AssertionError("unresolved sprite "+s);
            for(int i=0;i<4;i++) {
                for(int j=0;j<3;j++)xyzuv[i*5+j]=Math.round(Float.intBitsToFloat(vd[i*7+j])*65536);
                xyzuv[i*5+3]=Math.round((Float.intBitsToFloat(vd[i*7+4])-tex.getMinU())/(tex.getMaxU()-tex.getMinU())*65536);
                xyzuv[i*5+4]=Math.round((Float.intBitsToFloat(vd[i*7+5])-tex.getMinV())/(tex.getMaxV()-tex.getMinV())*65536);
            }
            int tint=quad.hasTintIndex()?s.getItem().getColorFromItemStack(s,quad.getTintIndex()):0xffffff;
            out.add(Arrays.asList(tex.getIconName().replace("minecraft:",""),tint&0xffffff,quad.getTintIndex(),quad.getFace().ordinal(),xyzuv));
        }
        return out;
    }
    static String tileTexture(ItemStack s){
        int id=Item.getIdFromItem(s.getItem());
        if(id==425)return "creative/banner_"+(s.getMetadata()&15);
        if(id==397)return new String[]{"entity/skeleton/skeleton","entity/skeleton/wither_skeleton","entity/zombie/zombie","entity/steve","entity/creeper/creeper"}[Math.min(4,s.getMetadata())];
        return "entity/chest/"+(id==130?"ender":id==146?"trapped":"normal");
    }
    List<Object> tile(ItemStack s) {
        // Read every primitive in model space. The RenderItem half-scale, centering and
        // builtin 180-degree yaw are applied in Rust in the same order for all perspectives.
        FloatBuffer model=BufferUtils.createFloatBuffer(16),proj=BufferUtils.createFloatBuffer(16),point=BufferUtils.createFloatBuffer(3);
        IntBuffer vp=BufferUtils.createIntBuffer(16);FloatBuffer buf=BufferUtils.createFloatBuffer(160000);
        GlStateManager.matrixMode(GL11.GL_PROJECTION);GlStateManager.pushMatrix();GlStateManager.loadIdentity();GL11.glOrtho(-4,4,-4,4,-4,4);
        GlStateManager.matrixMode(GL11.GL_MODELVIEW);GlStateManager.pushMatrix();GlStateManager.loadIdentity();
        GlStateManager.rotate(21,1,0,0);GlStateManager.rotate(32,0,1,0);GL11.glGetFloat(GL11.GL_MODELVIEW_MATRIX,model);
        GL11.glGetFloat(GL11.GL_PROJECTION_MATRIX,proj);GL11.glGetInteger(GL11.GL_VIEWPORT,vp);
        GlStateManager.disableCull();GlStateManager.disableLighting();GL11.glDisable(GL11.GL_CULL_FACE);
        GL11.glFeedbackBuffer(GL11.GL_3D_COLOR_TEXTURE,buf);GL11.glRenderMode(GL11.GL_FEEDBACK);
        TileEntityItemStackRenderer.instance.renderByItem(s);
        int n=GL11.glRenderMode(GL11.GL_RENDER);GlStateManager.popMatrix();GlStateManager.matrixMode(GL11.GL_PROJECTION);
        GlStateManager.popMatrix();GlStateManager.matrixMode(GL11.GL_MODELVIEW);
        List<Object> faces=new ArrayList<>();
        for(int p=0;p<n;){int token=(int)buf.get(p++); if(token!=GL11.GL_POLYGON_TOKEN)throw new AssertionError(token);
            int count=(int)buf.get(p++); int[][] poly=new int[count][5];
            for(int j=0;j<count;j++,p+=11){GLU.gluUnProject(buf.get(p),buf.get(p+1),buf.get(p+2),model,proj,vp,point);
                for(int k=0;k<3;k++)poly[j][k]=Math.round(point.get(k)*65536);
                poly[j][3]=Math.round(buf.get(p+7)*65536);poly[j][4]=Math.round(buf.get(p+8)*65536);
            }
            for(int i=2;i<count;i++){int[] verts=new int[20];int j=0;
                for(int k:new int[]{0,i-1,i,i}){System.arraycopy(poly[k],0,verts,j,5);j+=5;}
                faces.add(Arrays.asList(tileTexture(s),0xffffff,-1,-1,verts));
            }
        }
        return faces;
    }
    void add(ItemStack s,IBakedModel model,String key){
        if(ids.containsKey(key))return;
        ItemCameraTransforms t=model.getItemCameraTransforms(); List<float[]> transforms=new ArrayList<>();
        for(ItemCameraTransforms.TransformType kind:new ItemCameraTransforms.TransformType[]{ItemCameraTransforms.TransformType.FIRST_PERSON,ItemCameraTransforms.TransformType.THIRD_PERSON,ItemCameraTransforms.TransformType.GROUND}){
            GlStateManager.pushMatrix();GlStateManager.loadIdentity();t.applyTransform(kind);FloatBuffer fb=BufferUtils.createFloatBuffer(16); GL11.glGetFloat(GL11.GL_MODELVIEW_MATRIX,fb);
            float[] matrix=new float[16];fb.get(matrix);transforms.add(matrix);GlStateManager.popMatrix();
        }
        List<Object> row=Arrays.asList(model.isGui3d(),model.isBuiltInRenderer(),t.ground.scale,transforms,model.isBuiltInRenderer()?tile(s):baked(s,model));
        String signature=gson.toJson(row);Integer index=unique.get(signature);
        if(index==null){index=models.size();unique.put(signature,index);models.add(row);} ids.put(key,index);
    }
    void capture(File game,Path output)throws Exception {
        ItemRenderer ir=mc.getItemRenderer();
        mc.thePlayer.rotationYaw=mc.thePlayer.prevRotationYaw=mc.thePlayer.rotationPitch=mc.thePlayer.prevRotationPitch=0;
        mc.thePlayer.renderArmPitch=mc.thePlayer.prevRenderArmPitch=mc.thePlayer.renderArmYaw=mc.thePlayer.prevRenderArmYaw=0;
        mc.thePlayer.swingProgress=mc.thePlayer.prevSwingProgress=0;
        mc.thePlayer.setPosition(0,190,0);
        java.lang.reflect.Method first=ItemRenderer.class.getDeclaredMethod("transformFirstPersonItem",float.class,float.class);
        first.setAccessible(true);List<Object> transforms=new ArrayList<>();
        for(float swing:new float[]{0,.25f,.5f,.75f,1})for(float equip:new float[]{0,.4f,1}){
            GlStateManager.pushMatrix();GlStateManager.loadIdentity();first.invoke(ir,equip,swing);
            float[] m=new float[16];FloatBuffer fb=BufferUtils.createFloatBuffer(16);
            GL11.glGetFloat(GL11.GL_MODELVIEW_MATRIX,fb);fb.get(m);GlStateManager.popMatrix();
            transforms.add(Arrays.asList(swing,equip,m));
        }
        RenderEntityItem renderer=(RenderEntityItem)(Object)mc.getRenderManager().getEntityClassRenderObject(EntityItem.class);
        java.lang.reflect.Method copies=RenderEntityItem.class.getDeclaredMethod("func_177078_a",ItemStack.class);
        copies.setAccessible(true);List<Integer> counts=new ArrayList<>();
        for(int n=0;n<=64;n++)counts.add((Integer)copies.invoke(renderer,new ItemStack(Item.getItemById(2),n)));
        try(Writer w=Files.newBufferedWriter(output.resolveSibling("transform-checks.json"))){gson.toJson(Arrays.asList(transforms,counts),w);}
        for(String name:new String[]{"held-grass","held-stairs","held-furnace","held-pickaxe","dropped-items"}){
            mc.getFramebuffer().bindFramebuffer(true);GlStateManager.viewport(0,0,1600,900);
            GlStateManager.clearColor(1,0,1,1);GlStateManager.depthMask(true);GlStateManager.clearDepth(1);GlStateManager.clear(GL11.GL_COLOR_BUFFER_BIT|GL11.GL_DEPTH_BUFFER_BIT);
            GlStateManager.matrixMode(GL11.GL_PROJECTION);GlStateManager.loadIdentity();GLU.gluPerspective(70,1600f/900f,.05f,512f);
            GlStateManager.matrixMode(GL11.GL_MODELVIEW);GlStateManager.loadIdentity();GlStateManager.disableFog();GlStateManager.enableDepth();GlStateManager.depthFunc(GL11.GL_LEQUAL);
            GlStateManager.enableAlpha();GlStateManager.enableCull();GlStateManager.cullFace(GL11.GL_BACK);GlStateManager.enableTexture2D();
            GlStateManager.color(1,1,1,1);OpenGlHelper.setLightmapTextureCoords(OpenGlHelper.lightmapTexUnit,240,240);
            mc.entityRenderer.disableLightmap();
            if(!name.equals("dropped-items")){
                int id=name.endsWith("grass")?2:name.endsWith("stairs")?53:name.endsWith("furnace")?61:257;
                ItemStack stack=new ItemStack(Item.getItemById(id));
                mc.thePlayer.inventory.mainInventory[mc.thePlayer.inventory.currentItem]=stack;
                MenuOracle.field(ItemRenderer.class,"itemToRender").set(ir,stack);
                MenuOracle.field(ItemRenderer.class,"prevEquippedProgress").setFloat(ir,1);
                MenuOracle.field(ItemRenderer.class,"equippedProgress").setFloat(ir,1);
                ir.renderItemInFirstPerson(1);
            }else{
                // EntityRenderer.orientCamera offsets first-person world geometry by -0.1 in GL space.
                GlStateManager.translate(0,0,-0.1f);
                RenderHelper.enableStandardItemLighting();
                int[][] cases={{2,1},{53,2},{61,17},{257,49}};
                for(int i=0;i<cases.length;i++){
                    EntityItem e=new EntityItem(mc.theWorld,0,190,0,new ItemStack(Item.getItemById(cases[i][0]),cases[i][1]));
                    MenuOracle.field(EntityItem.class,"age").setInt(e,20);e.hoverStart=.7f;
                    renderer.doRender(e,-1.5+i,-.3,-3,0,1);
                }
            }
            ScreenShotHelper.saveScreenshot(game,name+".png",mc.displayWidth,mc.displayHeight,mc.getFramebuffer());
        }
    }
    void run(Path output)throws Exception {
        for(Item item:Item.itemRegistry){
            List<ItemStack> sub=new ArrayList<>();item.getSubItems(item,CreativeTabs.tabAllSearch,sub); sub.add(new ItemStack(item));
            int id=Item.getIdFromItem(item);for(ItemStack s:sub)add(s,mc.getRenderItem().getItemModelMesher().getItemModel(s),id+":"+s.getMetadata());
        }
        for(String variant:new String[]{"bow_pulling_0","bow_pulling_1","bow_pulling_2","fishing_rod_cast"}){
            ItemStack s=new ItemStack(variant.startsWith("bow")?net.minecraft.init.Items.bow:net.minecraft.init.Items.fishing_rod);
            add(s,mc.getRenderItem().getItemModelMesher().getModelManager().getModel(new ModelResourceLocation(variant,"inventory")),variant);
        }
        Map<String,Object> result=new LinkedHashMap<>();result.put("items",ids);result.put("models",models);
        try(Writer w=Files.newBufferedWriter(output)){gson.toJson(result,w);}
    }
    public static void main(String[] args){try{
        File game=new File(args[0]);VanillaFrameBenchmark.Client mc=MenuOracle.startClient(game,new File(args[1]),"1.8.9 held and dropped items");
        mc.gameSettings.renderDistanceChunks=2;mc.launchIntegratedServer("fixture","Fixture",new WorldSettings(189,WorldSettings.GameType.CREATIVE,false,false,WorldType.FLAT));
        java.lang.invoke.MethodHandle loop=VanillaFrameBenchmark.privateMethod(mc,"runGameLoop");long start=System.nanoTime();
        while(mc.thePlayer==null||mc.currentScreen!=null||mc.getRenderManager().livingPlayer!=mc.thePlayer){
            VanillaFrameBenchmark.runFrame(loop);if(System.nanoTime()-start>70_000_000_000L)throw new IllegalStateException("player unavailable");
        }
        WorldItemModelOracle oracle=new WorldItemModelOracle(mc);
        oracle.run(Paths.get(args[2]));oracle.capture(game,Paths.get(args[2]));System.exit(0);
    }catch(Throwable ex){ex.printStackTrace();System.exit(1);}}
}
