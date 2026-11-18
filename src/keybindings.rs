//! LWJGL input identifiers and Minecraft's original binding registration order.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use winit::{event::MouseButton, keyboard::KeyCode};

macro_rules! bindings {
    ($( $name:ident => ($key:literal, $category:literal, $code:literal), )*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Binding { $( $name, )* }
        impl Binding {
            pub const ALL: &'static [Self] = &[$(Self::$name,)*];
            pub fn key(self) -> &'static str { match self { $(Self::$name => $key,)* } }
            pub fn category(self) -> &'static str { match self { $(Self::$name => $category,)* } }
            pub fn default_code(self) -> i32 { match self { $(Self::$name => $code,)* } }
        }
    };
}
bindings! {
    Forward => ("key.forward", "key.categories.movement", 17),
    Left => ("key.left", "key.categories.movement", 30),
    Back => ("key.back", "key.categories.movement", 31),
    Right => ("key.right", "key.categories.movement", 32),
    Jump => ("key.jump", "key.categories.movement", 57),
    Sneak => ("key.sneak", "key.categories.movement", 42),
    Sprint => ("key.sprint", "key.categories.movement", 29),
    Inventory => ("key.inventory", "key.categories.inventory", 18),
    Use => ("key.use", "key.categories.gameplay", -99),
    Drop => ("key.drop", "key.categories.gameplay", 16),
    Attack => ("key.attack", "key.categories.gameplay", -100),
    Pick => ("key.pickItem", "key.categories.gameplay", -98),
    Chat => ("key.chat", "key.categories.multiplayer", 20),
    PlayerList => ("key.playerlist", "key.categories.multiplayer", 15),
    Command => ("key.command", "key.categories.multiplayer", 53),
    Screenshot => ("key.screenshot", "key.categories.misc", 60),
    Perspective => ("key.togglePerspective", "key.categories.misc", 63),
    SmoothCamera => ("key.smoothCamera", "key.categories.misc", 0),
    Fullscreen => ("key.fullscreen", "key.categories.misc", 87),
    SpectatorOutlines => ("key.spectatorOutlines", "key.categories.misc", 0),
    StreamStartStop => ("key.streamStartStop", "key.categories.stream", 64),
    StreamPause => ("key.streamPauseUnpause", "key.categories.stream", 65),
    StreamCommercial => ("key.streamCommercial", "key.categories.stream", 0),
    StreamMic => ("key.streamToggleMic", "key.categories.stream", 0),
    Hotbar1 => ("key.hotbar.1", "key.categories.inventory", 2),
    Hotbar2 => ("key.hotbar.2", "key.categories.inventory", 3),
    Hotbar3 => ("key.hotbar.3", "key.categories.inventory", 4),
    Hotbar4 => ("key.hotbar.4", "key.categories.inventory", 5),
    Hotbar5 => ("key.hotbar.5", "key.categories.inventory", 6),
    Hotbar6 => ("key.hotbar.6", "key.categories.inventory", 7),
    Hotbar7 => ("key.hotbar.7", "key.categories.inventory", 8),
    Hotbar8 => ("key.hotbar.8", "key.categories.inventory", 9),
    Hotbar9 => ("key.hotbar.9", "key.categories.inventory", 10),
}

// Minecraft.dispatchKeypresses checks these independently of KeyBinding's gameplay map.
pub const GLOBAL: [Binding; 6] = [
    Binding::StreamStartStop,
    Binding::StreamPause,
    Binding::StreamCommercial,
    Binding::StreamMic,
    Binding::Fullscreen,
    Binding::Screenshot,
];

impl Binding {
    pub fn hotbar(self) -> Option<usize> {
        (self as usize).checked_sub(Self::Hotbar1 as usize)
    }
}

/// Store only overrides, using the original stable option keys.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Bindings(BTreeMap<String, i32>);

impl Bindings {
    pub fn code(&self, binding: Binding) -> i32 {
        self.0
            .get(binding.key())
            .copied()
            .unwrap_or_else(|| binding.default_code())
    }
    pub fn set(&mut self, binding: Binding, code: i32) {
        if code == binding.default_code() {
            self.reset(binding);
        } else {
            self.0.insert(binding.key().to_owned(), code);
        }
    }
    pub fn reset(&mut self, binding: Binding) {
        self.0.remove(binding.key());
    }
    pub fn reset_all(&mut self) {
        self.0.clear();
    }
    pub fn action_for(&self, code: i32) -> Option<Binding> {
        // KeyBinding.resetKeyBindingArrayAndHash registers in constructor order.
        // IntHashMap replaces an existing key with the last matching binding.
        if code == 0 {
            return None;
        }
        Binding::ALL
            .iter()
            .rev()
            .copied()
            .find(|&b| self.code(b) == code)
    }
    pub fn conflicts(&self, binding: Binding) -> bool {
        let code = self.code(binding);
        code != 0
            && Binding::ALL
                .iter()
                .any(|&b| b != binding && self.code(b) == code)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlAction {
    Edit(Binding),
    Reset(Binding),
    ResetAll,
    InvertMouse,
    Touchscreen,
    Sensitivity,
}
pub fn mouse_code(button: MouseButton) -> i32 {
    let index = match button {
        MouseButton::Left => 0,
        MouseButton::Right => 1,
        MouseButton::Middle => 2,
        MouseButton::Back => 3,
        MouseButton::Forward => 4,
        MouseButton::Other(i) => i32::from(i),
    };
    if index < 100 {
        index - 100
    } else {
        0
    }
}
pub fn key_code(key: KeyCode) -> Option<i32> {
    use KeyCode::*;
    Some(match key {
        Escape => 1,
        Digit1 => 2,
        Digit2 => 3,
        Digit3 => 4,
        Digit4 => 5,
        Digit5 => 6,
        Digit6 => 7,
        Digit7 => 8,
        Digit8 => 9,
        Digit9 => 10,
        Digit0 => 11,
        Minus => 12,
        Equal => 13,
        Backspace => 14,
        Tab => 15,
        KeyQ => 16,
        KeyW => 17,
        KeyE => 18,
        KeyR => 19,
        KeyT => 20,
        KeyY => 21,
        KeyU => 22,
        KeyI => 23,
        KeyO => 24,
        KeyP => 25,
        BracketLeft => 26,
        BracketRight => 27,
        Enter => 28,
        ControlLeft => 29,
        KeyA => 30,
        KeyS => 31,
        KeyD => 32,
        KeyF => 33,
        KeyG => 34,
        KeyH => 35,
        KeyJ => 36,
        KeyK => 37,
        KeyL => 38,
        Semicolon => 39,
        Quote => 40,
        Backquote => 41,
        ShiftLeft => 42,
        Backslash | IntlBackslash => 43,
        KeyZ => 44,
        KeyX => 45,
        KeyC => 46,
        KeyV => 47,
        KeyB => 48,
        KeyN => 49,
        KeyM => 50,
        Comma => 51,
        Period => 52,
        Slash => 53,
        ShiftRight => 54,
        NumpadMultiply => 55,
        AltLeft => 56,
        Space => 57,
        CapsLock => 58,
        F1 => 59,
        F2 => 60,
        F3 => 61,
        F4 => 62,
        F5 => 63,
        F6 => 64,
        F7 => 65,
        F8 => 66,
        F9 => 67,
        F10 => 68,
        NumLock => 69,
        ScrollLock => 70,
        Numpad7 => 71,
        Numpad8 => 72,
        Numpad9 => 73,
        NumpadSubtract => 74,
        Numpad4 => 75,
        Numpad5 => 76,
        Numpad6 => 77,
        NumpadAdd => 78,
        Numpad1 => 79,
        Numpad2 => 80,
        Numpad3 => 81,
        Numpad0 => 82,
        NumpadDecimal => 83,
        F11 => 87,
        F12 => 88,
        F13 => 100,
        F14 => 101,
        F15 => 102,
        F16 => 103,
        F17 => 104,
        F18 => 105,
        KanaMode => 112,
        F19 => 113,
        Convert => 121,
        NonConvert => 123,
        IntlYen => 125,
        NumpadEqual => 141,
        NumpadEnter => 156,
        ControlRight => 157,
        NumpadComma => 179,
        NumpadDivide => 181,
        PrintScreen => 183,
        AltRight => 184,
        Fn => 196,
        Pause => 197,
        Home => 199,
        ArrowUp => 200,
        PageUp => 201,
        ArrowLeft => 203,
        ArrowRight => 205,
        End => 207,
        ArrowDown => 208,
        PageDown => 209,
        Insert => 210,
        Delete => 211,
        NumpadClear => 218,
        SuperLeft => 219,
        SuperRight => 220,
        ContextMenu => 221,
        Power => 222,
        Sleep => 223,
        _ => return None,
    })
}

const KEY_NAMES: [&str; 256] = [
    "NONE",
    "ESCAPE",
    "1",
    "2",
    "3",
    "4",
    "5",
    "6",
    "7",
    "8",
    "9",
    "0",
    "MINUS",
    "EQUALS",
    "BACK",
    "TAB",
    "Q",
    "W",
    "E",
    "R",
    "T",
    "Y",
    "U",
    "I",
    "O",
    "P",
    "LBRACKET",
    "RBRACKET",
    "RETURN",
    "LCONTROL",
    "A",
    "S",
    "D",
    "F",
    "G",
    "H",
    "J",
    "K",
    "L",
    "SEMICOLON",
    "APOSTROPHE",
    "GRAVE",
    "LSHIFT",
    "BACKSLASH",
    "Z",
    "X",
    "C",
    "V",
    "B",
    "N",
    "M",
    "COMMA",
    "PERIOD",
    "SLASH",
    "RSHIFT",
    "MULTIPLY",
    "LMENU",
    "SPACE",
    "CAPITAL",
    "F1",
    "F2",
    "F3",
    "F4",
    "F5",
    "F6",
    "F7",
    "F8",
    "F9",
    "F10",
    "NUMLOCK",
    "SCROLL",
    "NUMPAD7",
    "NUMPAD8",
    "NUMPAD9",
    "SUBTRACT",
    "NUMPAD4",
    "NUMPAD5",
    "NUMPAD6",
    "ADD",
    "NUMPAD1",
    "NUMPAD2",
    "NUMPAD3",
    "NUMPAD0",
    "DECIMAL",
    "",
    "",
    "",
    "F11",
    "F12",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "F13",
    "F14",
    "F15",
    "F16",
    "F17",
    "F18",
    "",
    "",
    "",
    "",
    "",
    "",
    "KANA",
    "F19",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "CONVERT",
    "",
    "NOCONVERT",
    "",
    "YEN",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "NUMPADEQUALS",
    "",
    "",
    "CIRCUMFLEX",
    "AT",
    "COLON",
    "UNDERLINE",
    "KANJI",
    "STOP",
    "AX",
    "UNLABELED",
    "",
    "",
    "",
    "",
    "NUMPADENTER",
    "RCONTROL",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "SECTION",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "NUMPADCOMMA",
    "",
    "DIVIDE",
    "",
    "SYSRQ",
    "RMENU",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "FUNCTION",
    "PAUSE",
    "",
    "HOME",
    "UP",
    "PRIOR",
    "",
    "LEFT",
    "",
    "RIGHT",
    "",
    "END",
    "DOWN",
    "NEXT",
    "INSERT",
    "DELETE",
    "",
    "",
    "",
    "",
    "",
    "",
    "CLEAR",
    "LMETA",
    "RMETA",
    "APPS",
    "POWER",
    "SLEEP",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
];
pub fn key_label(code: i32, mouse_format: &str) -> String {
    if code < 0 {
        let index = (code + 101).to_string();
        return mouse_format.replace("%1$s", &index).replace("%s", &index);
    }
    if code < 256 {
        return KEY_NAMES[code as usize].to_owned();
    }
    char::from_u32(((code - 256) as u16).into())
        .unwrap_or(char::REPLACEMENT_CHARACTER)
        .to_uppercase()
        .collect()
}

#[derive(Clone, Copy)]
pub struct ControlsList {
    width: f32,
    bottom: i32,
}
impl ControlsList {
    pub const TOP: i32 = 63;
    const HEIGHT: i32 = (33 + 6) * 20;
    pub fn new([width, height]: [f32; 2]) -> Self {
        Self {
            width,
            bottom: height as i32 - 32,
        }
    }
    pub fn contains(self, [x, y]: [f32; 2]) -> bool {
        x >= 0. && x < self.width && y >= Self::TOP as f32 && y <= self.bottom as f32
    }
    fn max_scroll(self) -> i32 {
        (Self::HEIGHT - (self.bottom - Self::TOP - 4)).max(0)
    }
    pub fn clamp(self, scroll: f32) -> i32 {
        (scroll as i32).clamp(0, self.max_scroll())
    }
    pub fn wheel(self, scroll: i32, delta: f32) -> i32 {
        if delta == 0. {
            return scroll;
        }
        self.clamp(scroll as f32 - delta.signum() * 10.)
    }
    pub fn thumb(self, scroll: i32) -> Option<(f32, f32, f32)> {
        let limit = self.max_scroll();
        let height = self.bottom - Self::TOP;
        if limit == 0 || height < 8 {
            return None;
        }
        let thumb = (height * height / Self::HEIGHT).max(32).min(height - 8);
        let y = Self::TOP + self.clamp(scroll as f32) * (height - thumb) / limit;
        Some(((self.width * 0.5).floor() + 139., y as f32, thumb as f32))
    }
    pub fn drag_factor(self, cursor: [f32; 2]) -> Option<f32> {
        if !self.contains(cursor) {
            return None;
        }
        if let Some((left, _, thumb)) = self.thumb(0) {
            if cursor[0] >= left && cursor[0] <= left + 6. {
                return Some(self.max_scroll() as f32 / ((self.bottom - Self::TOP) as f32 - thumb));
            }
        }
        Some(-1.)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    #[derive(Deserialize)]
    struct Original {
        bindings: Vec<(String, String, i32)>,
        names: BTreeMap<i32, String>,
        settings_order: Vec<String>,
        conflicts: Vec<String>,
    }
    #[test]
    fn original_lwjgl_names_defaults_and_all_conflict_winners() {
        let original: Original =
            serde_json::from_str(include_str!("../tests/fixtures/vanilla189-controls.json"))
                .unwrap();
        let lookup = |key: &str| *Binding::ALL.iter().find(|b| b.key() == key).unwrap();
        assert_eq!(original.bindings.len(), Binding::ALL.len());
        for ((key, category, code), binding) in original.bindings.iter().zip(Binding::ALL) {
            assert_eq!(
                (key.as_str(), category.as_str(), *code),
                (binding.key(), binding.category(), binding.default_code())
            );
        }
        for (code, name) in original.names {
            assert_eq!(key_label(code, ""), name);
        }
        let mut winners = original.conflicts.iter();
        let mut bindings = Bindings::default();
        for a in &original.settings_order {
            for b in &original.settings_order {
                bindings.set(lookup(a), 243);
                bindings.set(lookup(b), 243);
                let winner = bindings.action_for(243).unwrap();
                assert_eq!(winner.key(), winners.next().unwrap());
                assert_eq!(bindings.conflicts(winner), a != b);
                bindings.reset(lookup(a));
                bindings.reset(lookup(b));
            }
        }
        assert!(winners.next().is_none());
        assert_eq!(bindings.action_for(0), None);
    }
    #[test]
    fn persisted_mouse_unbound_character_and_reset() {
        let mut bindings = Bindings::default();
        bindings.set(Binding::Forward, mouse_code(MouseButton::Right));
        bindings.set(Binding::Use, 0);
        bindings.set(Binding::Inventory, 256 + i32::from('가' as u16));
        let mut saved: Bindings =
            serde_json::from_str(&serde_json::to_string(&bindings).unwrap()).unwrap();
        assert_eq!(saved.action_for(-99), Some(Binding::Forward));
        assert_eq!(key_label(-99, "버튼 %1$s"), "버튼 2");
        assert_eq!(key_label(saved.code(Binding::Inventory), ""), "가");
        assert_eq!(key_label(saved.code(Binding::Use), ""), "NONE");
        saved.reset_all();
        assert_eq!(
            saved.action_for(key_code(KeyCode::KeyW).unwrap()),
            Some(Binding::Forward)
        );
        assert_eq!(
            saved.action_for(mouse_code(MouseButton::Right)),
            Some(Binding::Use)
        );
        assert_eq!(
            saved
                .action_for(key_code(KeyCode::Digit9).unwrap())
                .unwrap()
                .hotbar(),
            Some(8)
        );
    }
}
