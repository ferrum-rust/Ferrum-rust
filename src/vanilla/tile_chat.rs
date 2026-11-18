//! Gson's canonical 1.8.9 chat component output used inside sign/command NBT.
use anyhow::{anyhow, bail, ensure, Result};
use serde_json::{Map, Value};

pub(super) fn quote(s: &str) -> String {
    serde_json::to_string(s)
        .unwrap()
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('=', "\\u003d")
        .replace('\'', "\\u0027")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}
fn text(v: &Value) -> Result<String> {
    match v {
        Value::String(s) => Ok(s.clone()),
        Value::Bool(_) | Value::Number(_) => Ok(v.to_string()),
        _ => bail!("Expected a primitive chat component"),
    }
}
fn array(v: &Value) -> Result<&Vec<Value>> {
    v.as_array()
        .ok_or_else(|| anyhow!("Expected chat component array"))
}
fn map(v: &Value) -> Result<&Map<String, Value>> {
    v.as_object()
        .ok_or_else(|| anyhow!("Expected chat component object"))
}
fn field(name: &str, value: String) -> String {
    format!("{}:{value}", quote(name))
}
fn object(fields: Vec<String>) -> String {
    format!("{{{}}}", fields.join(","))
}
const COLORS: &[&str] = &[
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
    "obfuscated",
    "bold",
    "strikethrough",
    "underline",
    "italic",
    "reset",
];
struct Component {
    value: Body,
    style: Vec<String>,
    siblings: Vec<Self>,
}
enum Body {
    Text(String),
    Translate(String, Vec<Component>),
    Score(String, String, String),
    Selector(String),
}
impl Component {
    fn parse(v: &Value) -> Result<Self> {
        if let Value::Array(entries) = v {
            let (first, rest) = entries
                .split_first()
                .ok_or_else(|| anyhow!("Empty chat component array"))?;
            let mut first = Self::parse(first)?;
            for v in rest {
                first.siblings.push(Self::parse(v)?);
            }
            return Ok(first);
        }
        let Some(m) = v.as_object() else {
            return Ok(Self {
                value: Body::Text(text(v)?),
                style: vec![],
                siblings: vec![],
            });
        };
        let value = if let Some(v) = m.get("text") {
            Body::Text(text(v)?)
        } else if let Some(v) = m.get("translate") {
            let terms = match m.get("with") {
                Some(list) => array(list)?
                    .iter()
                    .map(Self::parse)
                    .collect::<Result<_>>()?,
                None => vec![],
            };
            Body::Translate(text(v)?, terms)
        } else if let Some(v) = m.get("score") {
            let score = map(v)?;
            let name = score
                .get("name")
                .ok_or_else(|| anyhow!("Score name missing"))?;
            let objective = score
                .get("objective")
                .ok_or_else(|| anyhow!("Score objective missing"))?;
            Body::Score(
                text(name)?,
                text(objective)?,
                score
                    .get("value")
                    .map(text)
                    .transpose()?
                    .unwrap_or_default(),
            )
        } else if let Some(v) = m.get("selector") {
            Body::Selector(text(v)?)
        } else {
            bail!("Unknown chat component")
        };
        let siblings = match m.get("extra") {
            Some(v) => {
                let extra = array(v)?;
                ensure!(!extra.is_empty(), "Empty extra components");
                extra.iter().map(Self::parse).collect::<Result<_>>()?
            }
            None => vec![],
        };
        Ok(Self {
            value,
            siblings,
            style: style(m)?,
        })
    }
    fn render(&self) -> String {
        if self.style.is_empty() && self.siblings.is_empty() {
            if let Body::Text(s) = &self.value {
                return quote(s);
            }
        }
        let mut fields = self.style.clone();
        if !self.siblings.is_empty() {
            fields.push(field("extra", render_list(&self.siblings)));
        }
        match &self.value {
            Body::Text(s) => fields.push(field("text", quote(s))),
            Body::Translate(s, args) => {
                fields.push(field("translate", quote(s)));
                if !args.is_empty() {
                    fields.push(field("with", render_list(args)));
                }
            }
            Body::Score(name, objective, value) => fields.push(field(
                "score",
                object(vec![
                    field("name", quote(name)),
                    field("objective", quote(objective)),
                    field("value", quote(value)),
                ]),
            )),
            Body::Selector(s) => fields.push(field("selector", quote(s))),
        }
        object(fields)
    }
}
fn render_list(list: &[Component]) -> String {
    format!(
        "[{}]",
        list.iter()
            .map(Component::render)
            .collect::<Vec<_>>()
            .join(",")
    )
}
fn style(m: &Map<String, Value>) -> Result<Vec<String>> {
    let mut fields = vec![];
    for name in [
        "bold",
        "italic",
        "underlined",
        "strikethrough",
        "obfuscated",
    ] {
        if let Some(v) = m.get(name) {
            fields.push(field(
                name,
                text(v)?.eq_ignore_ascii_case("true").to_string(),
            ));
        }
    }
    if let Some(v) = m.get("color") {
        let s = text(v)?;
        if COLORS.contains(&s.as_str()) {
            fields.push(field("color", quote(&s)));
        }
    }
    if let Some(v) = m.get("insertion") {
        fields.push(field("insertion", quote(&text(v)?)));
    }
    for key in ["clickEvent", "hoverEvent"] {
        let Some(event) = m.get(key) else { continue };
        let e = map(event)?;
        let (Some(action), Some(value)) = (e.get("action"), e.get("value")) else {
            continue;
        };
        let action = text(action)?;
        let value = if key == "clickEvent" {
            if !matches!(
                action.as_str(),
                "open_url" | "run_command" | "suggest_command" | "change_page"
            ) {
                continue;
            }
            quote(&text(value)?)
        } else {
            if !matches!(
                action.as_str(),
                "show_text" | "show_achievement" | "show_item" | "show_entity"
            ) {
                continue;
            }
            Component::parse(value)?.render()
        };
        fields.push(field(
            key,
            object(vec![field("action", quote(&action)), field("value", value)]),
        ));
    }
    Ok(fields)
}
pub(super) fn canonical(raw: &str) -> Result<String> {
    Ok(Component::parse(&serde_json::from_str(raw)?)?.render())
}
