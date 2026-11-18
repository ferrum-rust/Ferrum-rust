import com.google.gson.Gson;
import java.io.FileWriter;
import java.lang.reflect.Field;
import java.util.*;
import net.minecraft.client.settings.GameSettings;
import net.minecraft.client.settings.KeyBinding;
import org.lwjgl.input.Keyboard;

/** Read the installed client's default bindings and exercise its real conflict dispatch. */
public final class ControlsOracle {
    public static void main(String[] args) throws Exception {
        GameSettings settings = new GameSettings();
        Field registered = KeyBinding.class.getDeclaredField("keybindArray");
        registered.setAccessible(true);
        List<?> bindings = (List<?>) registered.get(null);
        List<Object> rows = new ArrayList<>();
        Map<String, Object> result = new LinkedHashMap<>();
        for (Object entry : bindings) {
            KeyBinding binding = (KeyBinding) entry;
            rows.add(Arrays.asList(binding.getKeyDescription(), binding.getKeyCategory(), binding.getKeyCodeDefault()));
        }
        result.put("bindings", rows);
        Map<Integer, String> names = new LinkedHashMap<>();
        for (int code = 0; code < 256; code++) {
            String name = Keyboard.getKeyName(code);
            if (name != null) names.put(code, name);
        }
        result.put("names", names);
        List<String> winners = new ArrayList<>();
        for (KeyBinding a : settings.keyBindings) {
            for (KeyBinding b : settings.keyBindings) {
                a.setKeyCode(243);
                b.setKeyCode(243);
                KeyBinding.resetKeyBindingArrayAndHash();
                KeyBinding.setKeyBindState(243, true);
                for (KeyBinding k : settings.keyBindings) {
                    if (k.isKeyDown()) winners.add(k.getKeyDescription());
                }
                KeyBinding.unPressAllKeys();
                a.setKeyCode(a.getKeyCodeDefault());
                b.setKeyCode(b.getKeyCodeDefault());
            }
        }
        List<String> order = new ArrayList<>();
        for (KeyBinding k : settings.keyBindings) order.add(k.getKeyDescription());
        result.put("settings_order", order);
        result.put("conflicts", winners);
        try (FileWriter writer = new FileWriter(args[0])) { new Gson().toJson(result, writer); }
    }
}
