import java.io.*;
import java.lang.reflect.*;
import java.nio.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import java.util.regex.*;
import com.google.gson.*;
import net.minecraft.client.gui.*;
import net.minecraft.client.gui.inventory.*;
import net.minecraft.client.renderer.*;
import net.minecraft.client.renderer.block.model.*;
import net.minecraft.client.renderer.texture.*;
import net.minecraft.client.resources.model.*;
import net.minecraft.creativetab.CreativeTabs;
import net.minecraft.item.*;
import net.minecraft.inventory.*;
import net.minecraft.init.Items;
import net.minecraft.nbt.*;
import net.minecraft.util.*;
import net.minecraft.world.*;
import net.minecraft.enchantment.*;
import org.lwjgl.BufferUtils;
import org.lwjgl.opengl.GL11;

/** Extract creative tab ordering, translated tooltip expressions and original GUI mesh projections.
 * Texture pixels stay in the user's resource packs. Test windows and worlds are disposable. */
public class CreativeInventoryOracle {
    final VanillaFrameBenchmark.Client mc;
    final List<Map<String,Object>> entries=new ArrayList<>();
    final Map<String,Integer> indices=new LinkedHashMap<>();
    final List<TextureAtlasSprite> sprites;
    final GuiContainerCreative gui;
    final List<ItemStack> items=new ArrayList<>();
    final Map<String,Object> screens=new LinkedHashMap<>();
    final Method setTab, click;
    CreativeInventoryOracle(VanillaFrameBenchmark.Client client) throws Exception {
        mc=client;
        Map<?,TextureAtlasSprite> loaded=(Map<?,TextureAtlasSprite>)MenuOracle.field(TextureMap.class,"mapUploadedSprites").get(mc.getTextureMapBlocks());
        sprites=new ArrayList<>(loaded.values());
        gui=new GuiContainerCreative(mc.thePlayer); mc.displayGuiScreen(gui);
        setTab=GuiContainerCreative.class.getDeclaredMethod("setCurrentCreativeTab",CreativeTabs.class);setTab.setAccessible(true);
        click=GuiContainerCreative.class.getDeclaredMethod("handleMouseClick",Slot.class,int.class,int.class,int.class);click.setAccessible(true);
    }
    List<Object> tileMesh(ItemStack stack) {
        int id=Item.getIdFromItem(stack.getItem());
        String texture;
        if(id==425)texture="creative/banner_"+stack.getMetadata();
        else if(id==397)texture=new String[]{"entity/skeleton/skeleton","entity/skeleton/wither_skeleton","entity/zombie/zombie","entity/steve","entity/creeper/creeper"}[stack.getMetadata()];
        else texture="entity/chest/"+(id==130?"ender":id==146?"trapped":"normal");
        // The tile renderer issues display lists instead of IBakedModel quads. OpenGL feedback
        // exposes their transformed textured triangles, without reading or bundling texture pixels.
        mc.entityRenderer.setupOverlayRendering(); GlStateManager.enableTexture2D(); GlStateManager.enableDepth();
        RenderHelper.enableGUIStandardItemLighting(); GlStateManager.color(1,1,1,1);
        FloatBuffer buffer=BufferUtils.createFloatBuffer(65536);
        GL11.glFeedbackBuffer(GL11.GL_3D_COLOR_TEXTURE,buffer);GL11.glRenderMode(GL11.GL_FEEDBACK);
        mc.getRenderItem().renderItemIntoGUI(stack,100,100);
        int count=GL11.glRenderMode(GL11.GL_RENDER);if(count<0)throw new AssertionError("tile feedback overflow");
        TreeMap<Double,List<Object>> sorted=new TreeMap<>();
        for(int pos=0;pos<count;){
            int type=(int)buffer.get(pos++);if(type!=GL11.GL_POLYGON_TOKEN)throw new AssertionError("unexpected GL token "+type);
            int n=(int)buffer.get(pos++); List<Integer> coords=new ArrayList<>();double depth=0;
            int color=0;
            for(int j=0;j<n;j++,pos+=11){
                double x=buffer.get(pos)/2.-100,y=(mc.displayHeight-buffer.get(pos+1))/2.-100;
                coords.add((int)Math.round(x*4096));coords.add((int)Math.round(y*4096));
                depth+=buffer.get(pos+2);
                color=(Math.round(buffer.get(pos+3)*255)<<16)|(Math.round(buffer.get(pos+4)*255)<<8)|Math.round(buffer.get(pos+5)*255);
                coords.add(Math.round(buffer.get(pos+7)*4096));coords.add(Math.round(buffer.get(pos+8)*4096));
            }
            if(n<3)continue;
            for(int j=1;j<n-1;j++){
                List<Integer> triangle=new ArrayList<>();
                for(int k:new int[]{0,j,j+1,j+1})triangle.addAll(coords.subList(4*k,4*k+4));
                sorted.computeIfAbsent(-depth/n,d->new ArrayList<>()).add(Arrays.asList(texture,color,4096,triangle));
            }
        }
        List<Object> faces=new ArrayList<>();for(List<Object> fs:sorted.values())faces.addAll(fs);
        if(faces.isEmpty())throw new AssertionError("missing tile geometry "+id);
        return faces;
    }
    List<Object> mesh(ItemStack s) {
        IBakedModel m=mc.getRenderItem().getItemModelMesher().getItemModel(s);
        List<Object> faces=new ArrayList<>();
        if(m.isBuiltInRenderer())return tileMesh(s);
        boolean lit=m.isGui3d();
        GlStateManager.matrixMode(GL11.GL_MODELVIEW); GlStateManager.pushMatrix(); GlStateManager.loadIdentity();
        GlStateManager.scale(1f,1f,-1f);
        GlStateManager.scale(lit?20f:32f,lit?20f:32f,lit?20f:32f);
        if(lit){GlStateManager.rotate(210,1,0,0);GlStateManager.rotate(-135,0,1,0);}
        else GlStateManager.rotate(180,1,0,0);
        m.getItemCameraTransforms().applyTransform(ItemCameraTransforms.TransformType.GUI);
        GlStateManager.scale(.5f,.5f,.5f);GlStateManager.translate(-.5f,-.5f,-.5f);
        FloatBuffer fb=BufferUtils.createFloatBuffer(16);GL11.glGetFloat(GL11.GL_MODELVIEW_MATRIX,fb);
        GlStateManager.popMatrix();
        List<BakedQuad> quads=new ArrayList<>();
        for(EnumFacing f:EnumFacing.values())quads.addAll(m.getFaceQuads(f));
        quads.addAll(m.getGeneralQuads());
        TreeMap<Double,List<Object>> sorted=new TreeMap<>();
        for(BakedQuad q:quads){
            int[] vd=q.getVertexData(); double[][] v=new double[4][5]; double z=0,u=0,t=0;
            for(int i=0;i<4;i++){
                int o=7*i; float x=Float.intBitsToFloat(vd[o]),y=Float.intBitsToFloat(vd[o+1]),zz=Float.intBitsToFloat(vd[o+2]);
                for(int j=0;j<3;j++)v[i][j]=fb.get(j)*x+fb.get(4+j)*y+fb.get(8+j)*zz+fb.get(12+j);
                z+=v[i][2]; u+=(v[i][3]=Float.intBitsToFloat(vd[o+4])); t+=(v[i][4]=Float.intBitsToFloat(vd[o+5]));
            }
            double area=(v[1][0]-v[0][0])*(v[2][1]-v[0][1])-(v[1][1]-v[0][1])*(v[2][0]-v[0][0]);
            if(area>=-0.00001)continue;
            TextureAtlasSprite tex=null;
            for(TextureAtlasSprite candidate:sprites){
                if(u/4>=candidate.getMinU()&&u/4<=candidate.getMaxU()&&t/4>=candidate.getMinV()&&t/4<=candidate.getMaxV()){tex=candidate;break;}
            }
            if(tex==null)continue;
            double[] normal=new double[3];Vec3i axis=q.getFace().getDirectionVec();
            for(int j=0;j<3;j++)normal[j]=fb.get(j)*axis.getX()+fb.get(4+j)*axis.getY()+fb.get(8+j)*axis.getZ();
            double norm=Math.sqrt(normal[0]*normal[0]+normal[1]*normal[1]+normal[2]*normal[2]);
            double light=.4;
            for(int sign:new int[]{1,-1}){
                double lx=sign*.2, ly=1,lz=sign*(-.7), a=Math.toRadians(165),b=Math.toRadians(-30);
                double y=ly*Math.cos(a)-lz*Math.sin(a),z1=ly*Math.sin(a)+lz*Math.cos(a);
                double x=lx*Math.cos(b)+z1*Math.sin(b),zz=-lx*Math.sin(b)+z1*Math.cos(b);
                light+=.6*Math.max(0,(x*normal[0]+y*normal[1]+zz*normal[2])/(norm*Math.sqrt(1+.04+.49)));
            }
            int color=q.hasTintIndex()?s.getItem().getColorFromItemStack(s,q.getTintIndex()):0xffffff;
            List<Integer> coords=new ArrayList<>();
            for(double[] p:v){
                coords.add((int)Math.round((8+p[0])*4096));coords.add((int)Math.round((8+p[1])*4096));
                coords.add((int)Math.round((p[3]-tex.getMinU())/(tex.getMaxU()-tex.getMinU())*4096));
                coords.add((int)Math.round((p[4]-tex.getMinV())/(tex.getMaxV()-tex.getMinV())*4096));
            }
            sorted.computeIfAbsent(z,d->new ArrayList<>()).add(Arrays.asList(tex.getIconName().replace("minecraft:",""),color&0xffffff,(int)Math.round((lit?Math.min(1,light):1)*4096),coords));
        }
        for(List<Object> fs:sorted.values())faces.addAll(fs);return faces;
    }
    int index(ItemStack s) throws Exception {
        String key=Item.getIdFromItem(s.getItem())+":"+s.getMetadata()+":"+s.getTagCompound();
        if(indices.containsKey(key))return indices.get(key);
        int i=indices.size();indices.put(key,i);items.add(s);
        Map<String,Object> row=new LinkedHashMap<>();entries.add(row);
        row.put("id",Item.getIdFromItem(s.getItem()));row.put("meta",s.getMetadata());
        if(s.hasTagCompound()){
            ByteArrayOutputStream out=new ByteArrayOutputStream();CompressedStreamTools.write(s.getTagCompound(),new DataOutputStream(out));
            List<Integer> bytes=new ArrayList<>();for(byte b:out.toByteArray())bytes.add(b&255);row.put("nbt",bytes);
        }
        row.put("category",s.getItem().getCreativeTab()==null?-1:s.getItem().getCreativeTab().getTabIndex());
        if(s.getItem()==Items.enchanted_book){
            for(int e:EnchantmentHelper.getEnchantments(s).keySet()){
                for(CreativeTabs tab:CreativeTabs.creativeTabArray)if(tab.hasRelevantEnchantmentType(Enchantment.getEnchantmentById(e).type)){row.put("category",tab.getTabIndex());break;}
            }
        }
        row.put("glows",s.hasEffect());row.put("rarity",s.getRarity().rarityColor.toString());
        row.put("mesh",mesh(s));return i;
    }
    void tab(int n) throws Exception {setTab.invoke(gui,CreativeTabs.creativeTabArray[n]);}
    void run(File game,Path output) throws Exception {
        Map<String,Object> data=new LinkedHashMap<>();List<Object> tabs=new ArrayList<>();data.put("tabs",tabs);
        for(CreativeTabs tab:CreativeTabs.creativeTabArray){
            tab(tab.getTabIndex());
            List<ItemStack> stacks=(List<ItemStack>)MenuOracle.field(gui.inventorySlots.getClass(),"itemList").get(gui.inventorySlots);
            List<Integer> list=new ArrayList<>();for(ItemStack s:stacks)list.add(index(s));
            tabs.add(Arrays.asList(tab.getTranslatedTabLabel(),index(tab.getIconItemStack()),list));
        }
        mc.thePlayer.ticksExisted=0;
        for(int t:new int[]{0,5,11,1}){
            tab(t);if(t==5){
                GuiTextField search=(GuiTextField)MenuOracle.field(GuiContainerCreative.class,"searchField").get(gui);search.setText("다이아몬드");
                Method update=gui.getClass().getDeclaredMethod("updateCreativeSearch");update.setAccessible(true);update.invoke(gui);
            }
            MenuOracle.record(mc,gui,t==0?"creative":t==5?"creative-search":t==1?"creative-decorations":"creative-inventory",game,screens);
        }
        tab(0);List<Object> trace=new ArrayList<>();
        for(int[] step:new int[][]{{0,0,0},{0,0,0},{0,1,0},{1,0,0},{2,0,1},{2,1,0},{3,2,2},{44,0,0},{4,2,3}}){
            Slot slot=gui.inventorySlots.getSlot(step[0]); click.invoke(gui,slot,step[0],step[1],step[2]);
            ItemStack c=mc.thePlayer.inventory.getItemStack(),h=mc.thePlayer.inventory.getStackInSlot(2);
            trace.add(Arrays.asList(step,summary(c),summary(h)));
        }
        data.put("clicks",trace);
        // Distinct nested token expressions preserve resource-pack translations, including reordered format arguments.
        Map<String,String> original=(Map<String,String>)MenuOracle.field(StringTranslate.class,"languageList").get(MenuOracle.field(StringTranslate.class,"instance").get(null));
        Map<String,String> tokens=new HashMap<>();
        Pattern args=Pattern.compile("%(?:(\\d+)\\$)?s");
        for(Map.Entry<String,String> e:original.entrySet()){
            Matcher matcher=args.matcher(e.getValue());int n=0,seq=0;
            while(matcher.find())n=Math.max(n,matcher.group(1)==null?++seq:Integer.parseInt(matcher.group(1)));
            StringBuilder val=new StringBuilder("⟦").append(e.getKey());
            for(int j=1;j<=n;j++)val.append("|%").append(j).append("$s");
            tokens.put(e.getKey(),val.append("⟧").toString());
        }
        StringTranslate.replaceWith(tokens);
        for(int i=0;i<entries.size();i++){
            ItemStack s=items.get(i);entries.get(i).put("tip",s.getTooltip(mc.thePlayer,false));entries.get(i).put("advanced",s.getTooltip(mc.thePlayer,true));
        }
        data.put("entries",entries);
        List<Integer> dyes=new ArrayList<>();for(int i=0;i<16;i++)dyes.add(EnumDyeColor.byDyeDamage(i).getMapColor().colorValue);
        data.put("dyes",dyes);
        Map<String,Object> regions=new TreeMap<>();
        for(TextureAtlasSprite tex:sprites)regions.put(tex.getIconName().replace("minecraft:",""),new float[]{tex.getMinU(),tex.getMinV(),tex.getMaxU()-tex.getMinU(),tex.getMaxV()-tex.getMinV()});data.put("regions",regions);
        try(Writer out=Files.newBufferedWriter(output,StandardCharsets.UTF_8)){new Gson().toJson(data,out);}
    }
    static Object summary(ItemStack s){return s==null?null:Arrays.asList(Item.getIdFromItem(s.getItem()),s.getMetadata(),s.stackSize);}
    public static void main(String[] args){
        try{
            File game=new File(args[0]); VanillaFrameBenchmark.Client mc=MenuOracle.startClient(game,new File(args[1]),"1.8.9 creative inventory oracle");
            mc.gameSettings.renderDistanceChunks=2;
            mc.launchIntegratedServer("fixture","Fixture",new WorldSettings(189,WorldSettings.GameType.CREATIVE,false,false,WorldType.FLAT));
            java.lang.invoke.MethodHandle loop=VanillaFrameBenchmark.privateMethod(mc,"runGameLoop");long start=System.nanoTime();
            while(mc.thePlayer==null||mc.currentScreen!=null||!mc.playerController.isInCreativeMode()){
                VanillaFrameBenchmark.runFrame(loop);if(System.nanoTime()-start>70_000_000_000L)throw new IllegalStateException("Creative player unavailable");
            }
            for(int i=0;i<12 || mc.getRenderManager().livingPlayer!=mc.thePlayer;i++){
                VanillaFrameBenchmark.runFrame(loop);if(System.nanoTime()-start>75_000_000_000L)throw new IllegalStateException("Player renderer unavailable");
            }
            System.out.println("Original preview skin "+mc.thePlayer.getSkinType());
            new CreativeInventoryOracle(mc).run(game,Paths.get(args[2]));System.exit(0);
        }catch(Throwable e){e.printStackTrace();System.exit(1);}
    }
}

