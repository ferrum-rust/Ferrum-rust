import com.google.gson.Gson;
import java.io.*;
import java.nio.charset.StandardCharsets;
import java.lang.reflect.*;
import java.util.*;
import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.*;
import net.minecraft.client.resources.I18n;
import net.minecraft.client.settings.GameSettings;
import net.minecraft.init.Bootstrap;
import sun.misc.Unsafe;

/** Calls the owned Java classes' initGui methods. No Display or game files are opened. */
public final class OptionsLayoutOracle {
    static Field field(Class<?> type, String name) throws Exception {
        Field f=type.getDeclaredField(name);
        f.setAccessible(true);
        return f;
    }
    public static void main(String[] args) throws Exception {
        Bootstrap.register();
        Unsafe unsafe=(Unsafe)field(Unsafe.class,"theUnsafe").get(null);
        Minecraft game=(Minecraft)unsafe.allocateInstance(Minecraft.class);
        field(Minecraft.class,"theMinecraft").set(null,game);
        GameSettings settings=new GameSettings();
        game.gameSettings=settings;
        Method setLocale=I18n.class.getDeclaredMethod("setLocale",net.minecraft.client.resources.Locale.class);
        setLocale.setAccessible(true);setLocale.invoke(null,new net.minecraft.client.resources.Locale());
        int[][] sizes={{320,240},{427,241},{640,360},{853,480},{800,450}};
        List<Object> cases=new ArrayList<>();
        for(int[] size:sizes) {
            GuiOptions root=new GuiOptions(null,settings);
            GuiScreen[] screens={root,new GuiCustomizeSkin(root),new GuiScreenOptionsSounds(root,settings),new ScreenChatOptions(root,settings)};
            String[] pages={"options","skin","sounds","chat"};
            for(int i=0;i<pages.length;i++) {
                GuiScreen screen=screens[i];
                screen.setWorldAndResolution(game,size[0],size[1]);
                List<Object> rows=new ArrayList<>();
                for(Object raw:(List<?>)field(GuiScreen.class,"buttonList").get(screen)){
                    GuiButton b=(GuiButton)raw;
                    rows.add(Arrays.asList(b.id,b.xPosition,b.yPosition,b.getButtonWidth(),field(GuiButton.class,"height").getInt(b),b.enabled,b.displayString));
                }
                cases.add(Arrays.asList(pages[i],size[0],size[1],rows));
            }
        }
        try(Writer output=new OutputStreamWriter(new FileOutputStream(args[0]),StandardCharsets.UTF_8)) {new Gson().toJson(cases,output);}
    }
}
