use serde_json::Value;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Click {
    Url(Arc<str>),
    Run(Arc<str>),
    Suggest(Arc<str>),
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Interaction {
    pub click: Option<Click>,
    pub insertion: Option<Arc<str>>,
    pub hover: Option<Arc<str>>,
}
#[derive(Clone, Default)]
struct Style {
    color: Option<char>,
    flags: [bool; 5],
    interaction: Interaction,
}
const FLAGS: [(&str, char); 5] = [
    ("bold", 'l'),
    ("italic", 'o'),
    ("underlined", 'n'),
    ("obfuscated", 'k'),
    ("strikethrough", 'm'),
];
const COLORS: [&str; 16] = [
    "black",
    "dark_blue",
    "dark_green",
    "dark_aqua",
    "dark_red",
    "dark_purple",
    "gold",
    "gray",
    "dark_gray",
    "blue",
    "green",
    "aqua",
    "red",
    "light_purple",
    "yellow",
    "white",
];
const PALETTE_RGB: [[i32; 3]; 16] = [
    [0, 0, 0],
    [0, 0, 170],
    [0, 170, 0],
    [0, 170, 170],
    [170, 0, 0],
    [170, 0, 170],
    [255, 170, 0],
    [170, 170, 170],
    [85, 85, 85],
    [85, 85, 255],
    [85, 255, 85],
    [85, 255, 255],
    [255, 85, 85],
    [255, 85, 255],
    [255, 255, 85],
    [255, 255, 255],
];

fn parse_rgb_hex(color: &str) -> Option<char> {
    let hex = color.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let r = i32::from(u8::from_str_radix(&hex[0..2], 16).ok()?);
    let g = i32::from(u8::from_str_radix(&hex[2..4], 16).ok()?);
    let b = i32::from(u8::from_str_radix(&hex[4..6], 16).ok()?);
    let best = PALETTE_RGB
        .iter()
        .enumerate()
        .min_by_key(|(_, [pr, pg, pb])| {
            let dr = r - pr;
            let dg = g - pg;
            let db = b - pb;
            dr * dr + dg * dg + db * db
        })
        .map(|(i, _)| i)?;
    Some(b"0123456789abcdef"[best] as char)
}

#[derive(Clone, Debug, Default)]
pub struct Run {
    pub text: String,
    pub format: String,
    pub events: Interaction,
}
impl Run {
    pub fn literal(text: String) -> Self {
        Self {
            text,
            ..Self::default()
        }
    }
}
struct Renderer<'a, F> {
    tr: &'a F,
    budget: usize,
    runs: Vec<Run>,
}
impl<F: Fn(&str) -> String> Renderer<'_, F> {
    fn text(&mut self, text: &str, style: &Style) {
        if text.is_empty() || self.budget == 0 {
            return;
        }
        let mut end = text.len().min(self.budget);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        self.budget -= end;
        let mut format = String::new();
        for code in style.color.into_iter().chain(
            FLAGS
                .iter()
                .enumerate()
                .filter(|(i, _)| style.flags[*i])
                .map(|(_, (_, code))| *code),
        ) {
            format.push('§');
            format.push(code);
        }
        self.runs.push(Run {
            text: text[..end].to_owned(),
            format,
            events: style.interaction.clone(),
        });
    }
    fn translate(&mut self, format: &str, args: &[Value], style: &Style, depth: u8) {
        let mut text = format;
        let mut next = 0;
        while let Some(i) = text.find('%') {
            self.text(&text[..i], style);
            text = &text[i + 1..];
            if let Some(rest) = text.strip_prefix('%') {
                self.text("%", style);
                text = rest;
                continue;
            }
            let n = text.bytes().take_while(u8::is_ascii_digit).count();
            let (index, suffix) = if n > 0 && text[n..].starts_with('$') {
                (
                    text[..n].parse::<usize>().unwrap_or(0).wrapping_sub(1),
                    &text[n + 1..],
                )
            } else {
                (next, text)
            };
            if suffix.starts_with(['s', 'd']) {
                if suffix.len() == text.len() {
                    next += 1;
                }
                if let Some(value) = args.get(index) {
                    self.value(value, style, depth + 1);
                }
                text = &suffix[1..];
            } else {
                self.text("%", style);
            }
            if self.budget == 0 {
                return;
            }
        }
        self.text(text, style);
    }
    fn value(&mut self, v: &Value, parent: &Style, depth: u8) -> Style {
        if depth > 32 || self.budget == 0 {
            return parent.clone();
        }
        match v {
            Value::String(text) => self.text(text, parent),
            Value::Number(_) | Value::Bool(_) => self.text(&v.to_string(), parent),
            Value::Array(values) => {
                // The array deserializer appends elements 1.. to the first component.
                if let Some((first, rest)) = values.split_first() {
                    let style = self.value(first, parent, depth + 1);
                    for item in rest {
                        self.value(item, &style, depth + 1);
                    }
                    return style;
                }
            }
            Value::Object(_) => {
                let style = self.style(v, parent, depth);
                if let Some(text) = v.get("text") {
                    if let Some(s) = text.as_str() {
                        self.text(s, &style)
                    } else if text.is_number() || text.is_boolean() {
                        self.text(&text.to_string(), &style)
                    }
                } else if let Some(key) = v["translate"].as_str() {
                    self.translate(
                        &(self.tr)(key),
                        v["with"].as_array().map(Vec::as_slice).unwrap_or_default(),
                        &style,
                        depth,
                    );
                } else if let Some(score) = v["score"]["value"].as_str() {
                    self.text(score, &style);
                } else if let Some(selector) = v["selector"].as_str() {
                    self.text(selector, &style);
                }
                if let Some(extras) = v["extra"].as_array() {
                    for part in extras {
                        self.value(part, &style, depth + 1);
                    }
                }
                return style;
            }
            _ => {}
        }
        parent.clone()
    }
    fn style(&self, v: &Value, parent: &Style, depth: u8) -> Style {
        let mut s = parent.clone();
        if let Some(color) = v.get("color").and_then(Value::as_str) {
            if color.eq_ignore_ascii_case("reset") {
                s.color = None;
                s.flags = [false; 5];
            } else if let Some((idx, _)) = FLAGS
                .iter()
                .enumerate()
                .find(|(_, (k, _))| k.eq_ignore_ascii_case(color))
            {
                s.flags[idx] = true;
            } else if let Some(c) = COLORS
                .iter()
                .position(|c| c.eq_ignore_ascii_case(color))
                .map(|i| b"0123456789abcdef"[i] as char)
                .or_else(|| parse_rgb_hex(color))
            {
                s.color = Some(c);
            }
        }
        for (i, (key, _)) in FLAGS.iter().enumerate() {
            if let Some(b) = v.get(key).and_then(Value::as_bool) {
                s.flags[i] = b;
            }
        }
        if let Some(text) = v.get("insertion").and_then(Value::as_str) {
            s.interaction.insertion = Some(text.into());
        }
        if let Some(event) = v.get("clickEvent") {
            if let (Some(action), Some(val)) = (event["action"].as_str(), event["value"].as_str()) {
                s.interaction.click = match action {
                    "open_url"
                        if (val.starts_with("https://") || val.starts_with("http://"))
                            && !val.contains(char::is_control) =>
                    {
                        Some(Click::Url(val.into()))
                    }
                    "suggest_command" => Some(Click::Suggest(val.into())),
                    "run_command" => Some(Click::Run(val.into())),
                    _ => None,
                };
            }
        }
        if let Some(hover) = v
            .get("hoverEvent")
            .filter(|h| h["action"] == "show_text" && depth < 32)
        {
            let mut renderer = Renderer {
                tr: self.tr,
                budget: 4096,
                runs: Vec::new(),
            };
            renderer.value(&hover["value"], &Style::default(), depth + 1);
            s.interaction.hover = Some(formatted(&renderer.runs).into());
        }
        s
    }
}
pub fn formatted(runs: &[Run]) -> String {
    let mut out = String::new();
    for run in runs {
        if !out.is_empty() {
            out.push_str("§r");
        }
        out.push_str(&run.format);
        out.push_str(&run.text);
    }
    out
}
pub fn parse(json: &str, tr: impl Fn(&str) -> String) -> Vec<Run> {
    let Ok(value) = serde_json::from_str(json) else {
        return vec![Run::literal(json.into())];
    };
    let mut renderer = Renderer {
        tr: &tr,
        budget: 65536,
        runs: Vec::new(),
    };
    renderer.value(&value, &Style::default(), 0);
    renderer.runs
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn translated_runs_inherit_events_and_siblings_restore_style() {
        let json = r#"{"translate":"sample","color":"gold","with":[{"text":"A","bold":true},{"text":"둘","italic":true}],"extra":[{"text":" link","clickEvent":{"action":"open_url","value":"https://example.org/test"},"hoverEvent":{"action":"show_text","value":{"text":"설명","color":"green"}},"extra":[{"text":"!"}]},{"text":" end"}]}"#;
        let runs = parse(json, |_| "%2$s + %1$s (100%%)".into());
        assert_eq!(
            runs.iter().map(|r| r.text.as_str()).collect::<String>(),
            "둘 + A (100%) link! end"
        );
        assert_eq!(runs[0].format, "§6§o");
        let bold = runs.iter().find(|r| r.text == "A").unwrap();
        assert_eq!(bold.format, "§6§l");
        let child = runs.iter().find(|r| r.text == "!").unwrap();
        assert_eq!(
            child.events.click,
            Some(Click::Url("https://example.org/test".into()))
        );
        assert_eq!(child.events.hover.as_deref(), Some("§a설명"));
        assert!(runs.last().unwrap().events.click.is_none());
        assert_eq!(runs.last().unwrap().format, "§6");
    }

    #[test]
    fn formatted_preserves_tab_json_colors_uppercase_hex_and_reset() {
        let json = r##"{"text":"","extra":[{"text":"[ADMIN] ","color":"DARK_RED","bold":true},{"text":"HexRider1","color":"#55ffff"},{"text":" (AFK)","color":"reset"}]}"##;
        let runs = parse(json, |k| k.into());
        assert_eq!(
            formatted(&runs),
            "§4§l[ADMIN] §r§bHexRider1§r (AFK)"
        );
    }
}
