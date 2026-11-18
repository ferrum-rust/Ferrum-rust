import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import java.util.zip.GZIPOutputStream;
import com.google.gson.Gson;
import net.minecraft.init.Bootstrap;
import net.minecraft.entity.passive.EntitySheep;
import net.minecraft.entity.player.EntityPlayer;
import net.minecraft.inventory.*;
import net.minecraft.item.*;
import net.minecraft.item.crafting.*;
import net.minecraft.nbt.*;
import net.minecraft.tileentity.TileEntityBanner;
import net.minecraft.world.*;
import net.minecraft.world.storage.*;

/** Export ordered original recipes and evaluate actual CraftingManager grids. */
public final class CraftRegistryOracle {
    static final Gson JSON=new Gson();
    static Object field(Object o,Class<?> type,String name)throws Exception{Field f=type.getDeclaredField(name);f.setAccessible(true);return f.get(o);}
    static ItemStack item(int id,int meta){return new ItemStack(Item.getItemById(id),1,meta);}
    static List<Object> stack(ItemStack s)throws Exception{
        if(s==null)return null;
        String hex="";
        if(s.hasTagCompound()){
            ByteArrayOutputStream bytes=new ByteArrayOutputStream();CompressedStreamTools.write(s.getTagCompound(),new DataOutputStream(bytes));StringBuilder text=new StringBuilder();
            for(byte v:bytes.toByteArray())text.append(String.format(Locale.ROOT,"%02x",v&255));hex=text.toString();
        }
        return Arrays.asList(Item.getIdFromItem(s.getItem()),s.stackSize,s.getMetadata(),hex);
    }
    static List<Object> stacks(ItemStack[] slots)throws Exception{List<Object> out=new ArrayList<>();for(ItemStack s:slots)out.add(stack(s));return out;}
    static ItemStack[] ingredients(IRecipe r)throws Exception{
        if(r.getClass()==ShapedRecipes.class)return (ItemStack[])field(r,ShapedRecipes.class,"recipeItems");
        if(r.getClass()==ShapelessRecipes.class)return ((List<ItemStack>)field(r,ShapelessRecipes.class,"recipeItems")).toArray(new ItemStack[0]);
        return null;
    }
    static int width(IRecipe r)throws Exception{return r.getClass()==ShapedRecipes.class?(int)field(r,ShapedRecipes.class,"recipeWidth"):0;}
    static final Container OWNER=new Container(){public boolean canInteractWith(EntityPlayer p){return true;}};
    static final class MapWorld extends FallingEntityOracle.TestWorld {
        MapWorld(){super(0);}
        public WorldSavedData loadItemData(Class<? extends WorldSavedData> c,String name){MapData d=new MapData(name);d.scale=(byte)(Integer.parseInt(name.substring(4))%5);return d;}
    }
    static MapWorld world;
    static List<Object> caseOf(ItemStack[] input,int width)throws Exception{
        InventoryCrafting grid=new InventoryCrafting(OWNER,width,width);
        for(int i=0;i<input.length;i++)grid.setInventorySlotContents(i,input[i]);
        CraftingManager m=CraftingManager.getInstance();
        ItemStack output=m.findMatchingRecipe(grid,world);
        return Arrays.asList(width,stacks(input),stack(output),output==null?null:stacks(m.func_180303_b(grid,world)));
    }
    static ItemStack varied(ItemStack s,Random r){
        if(s==null)return null;
        ItemStack out=s.copy();if(out.getMetadata()==32767)out.setItemDamage(r.nextInt(16));out.stackSize=1+r.nextInt(3);
        if(r.nextInt(5)==0){NBTTagCompound tag=new NBTTagCompound();tag.setInteger("variant",r.nextInt(7));out.setTagCompound(tag);}return out;
    }
    static void staticCases(List<Object> fixtures,IRecipe r,ItemStack[] slots,Random rand)throws Exception{
        int w=width(r);
        for(int size=2;size<=3;size++){
            if(w>0){int h=slots.length/w;for(int x=0;x<=size-w;x++)for(int y=0;y<=size-h;y++)for(int mirror=0;mirror<2;mirror++){
                ItemStack[] grid=new ItemStack[size*size];for(int j=0;j<h;j++)for(int i=0;i<w;i++)grid[(y+j)*size+x+i]=varied(slots[j*w+(mirror==1?w-1-i:i)],rand);
                fixtures.add(caseOf(grid,size));
            }}else if(slots.length<=size*size){for(int k=0;k<5;k++){
                List<Integer> indexes=new ArrayList<>();for(int i=0;i<size*size;i++)indexes.add(i);Collections.shuffle(indexes,rand);ItemStack[] grid=new ItemStack[size*size];
                for(int i=0;i<slots.length;i++)grid[indexes.get(i)]=varied(slots[i],rand);fixtures.add(caseOf(grid,size));
            }}
        }
    }
    static void add(List<Object> fx,int width,Random r,ItemStack... inputs)throws Exception{
        List<Integer> index=new ArrayList<>();for(int i=0;i<width*width;i++)index.add(i);Collections.shuffle(index,r);ItemStack[] grid=new ItemStack[width*width];
        for(int i=0;i<inputs.length;i++)grid[index.get(i)]=inputs[i];fx.add(caseOf(grid,width));
    }
    public static void main(String[] argv)throws Exception{
        Bootstrap.register();world=new MapWorld();Random rand=new Random(189);List<Object> recipes=new ArrayList<>(),fixtures=new ArrayList<>(),containers=new ArrayList<>(),banner=new ArrayList<>(),colors=new ArrayList<>();
        for(IRecipe r:CraftingManager.getInstance().getRecipeList()){
            ItemStack[] slots=ingredients(r);boolean copy=r.getClass()==ShapedRecipes.class&&(boolean)field(r,ShapedRecipes.class,"copyIngredientNBT");
            recipes.add(Arrays.asList(r.getClass().getName().replace("net.minecraft.item.crafting.",""),width(r),slots==null?null:stacks(slots),stack(r.getRecipeOutput()),copy));
            if(slots!=null)staticCases(fixtures,r,slots,rand);
        }
        for(Item i:Item.itemRegistry){
            if(i.hasContainerItem())containers.add(Arrays.asList(Item.getIdFromItem(i),Item.getIdFromItem(i.getContainerItem())));
            if(i.getMaxDamage()>0)for(int v=0;v<6;v++)add(fixtures,2,rand,item(Item.getIdFromItem(i),v*i.getMaxDamage()/5),item(Item.getIdFromItem(i),rand.nextInt(i.getMaxDamage()+1)));
        }
        for(TileEntityBanner.EnumBannerPattern p:TileEntityBanner.EnumBannerPattern.values()){
            banner.add(Arrays.asList(p.getPatternID(),p.getCraftingLayers(),stack(p.getCraftingStack())));
            for(int color=0;color<16;color++){
                ItemStack flag=item(425,color);
                if(p.getCraftingStack()!=null){add(fixtures,2,rand,flag,p.getCraftingStack().copy(),item(351,color));add(fixtures,2,rand,flag,p.getCraftingStack().copy());continue;}
                String[] layers=p.getCraftingLayers();if(layers[0]==null)continue;ItemStack[] grid=new ItemStack[9];List<Integer> gaps=new ArrayList<>();
                for(int i=0;i<9;i++){if(layers[i/3].charAt(i%3)==' ')gaps.add(i);else grid[i]=item(351,color);}
                for(int i:gaps){grid[i]=flag;fixtures.add(caseOf(grid,3));grid[i]=null;}
            }
        }
        for(int m=0;m<16;m++){
            colors.add(EntitySheep.getDyeRgb(EnumDyeColor.byDyeDamage(m)));
            for(int a=298;a<=301;a++){add(fixtures,2,rand,item(a,77),item(351,m));add(fixtures,3,rand,item(a,31),item(351,m),item(351,15-m));}
            add(fixtures,3,rand,item(289,0),item(351,m),item(264,0),item(348,0),item(397,2));
            add(fixtures,2,rand,item(289,0),item(339,0));add(fixtures,2,rand,item(358,m),item(395,0),item(395,0));
            ItemStack book=item(387,0);NBTTagCompound n=new NBTTagCompound();n.setString("author","Reader");n.setString("title","Chapter");n.setInteger("generation",m%4);book.setTagCompound(n);
            add(fixtures,2,rand,book,item(386,0),item(386,0));
            ItemStack star=item(402,0);NBTTagCompound base=new NBTTagCompound(),ex=new NBTTagCompound();
            ex.setIntArray("Colors",new int[]{m,m+1});ex.setByte("Type",(byte)(m%5));base.setTag("Explosion",ex);star.setTagCompound(base);
            add(fixtures,3,rand,star,item(351,m),item(351,15-m));add(fixtures,3,rand,star,item(339,0),item(289,0),item(289,0));
            add(fixtures,3,rand,item(402,0),item(351,m));add(fixtures,3,rand,item(402,0),item(339,0),item(289,0));
            ItemStack colored=item(299,18);((ItemArmor)colored.getItem()).setColor(colored,m*0x111111);add(fixtures,3,rand,colored,item(351,15-m));
            ItemStack patterned=item(425,m);NBTTagCompound pattern=new NBTTagCompound();pattern.setString("Pattern","bo");pattern.setInteger("Color",m);
            NBTTagList patterns=new NBTTagList();patterns.appendTag(pattern);patterned.getSubCompound("BlockEntityTag",true).setTag("Patterns",patterns);
            add(fixtures,2,rand,patterned,item(425,m));add(fixtures,2,rand,patterned,item(425,(m+1)%16));
            ItemStack[] grid=new ItemStack[9];for(int i=0;i<9;i++)grid[i]=i==4?item(358,m):item(339,0);fixtures.add(caseOf(grid,3));
        }
        Map<String,Object> data=new LinkedHashMap<>();data.put("recipes",recipes);data.put("container_items",containers);data.put("patterns",banner);data.put("sheep_rgb",colors);data.put("firework_rgb",ItemDye.dyeColors);
        try(Writer out=new OutputStreamWriter(new FileOutputStream(argv[0]),"UTF-8")){JSON.toJson(data,out);}
        try(Writer out=new OutputStreamWriter(new GZIPOutputStream(new FileOutputStream(argv[1])),"UTF-8")){JSON.toJson(fixtures,out);}
        System.out.println("Original CraftingManager recipes "+recipes.size()+", grids "+fixtures.size());
    }
}
