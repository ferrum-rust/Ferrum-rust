use super::*;
fn joined(ctx: &Context<'_>, s: &str) -> CmdResult<String> {
    let mut out = Vec::new();
    for t in ctx.targets(s)? {
        out.push(ctx.target_name(t))
    }
    if out.len() > 1 {
        let last = out.pop().unwrap();
        Ok(format!("{} and {last}", out.join(", ")))
    } else {
        Ok(out.join(""))
    }
}
fn message(ctx: &Context<'_>, a: &[&str], expand: bool) -> CmdResult<Value> {
    let mut words = Vec::new();
    for &s in a {
        if expand
            && s.starts_with('@')
            && matches!(s.as_bytes().get(1), Some(b'p' | b'a' | b'r' | b'e'))
        {
            let names = joined(ctx, s)?;
            if names.is_empty() {
                return Err(error("commands.generic.selector.notFound", vec![json!(s)]));
            }
            words.push(names)
        } else {
            words.push(s.into())
        }
    }
    Ok(json!({"text":words.join(" ")}))
}
fn malformed(s: &str) -> Value {
    error("commands.tellraw.jsonException", vec![json!(s)])
}
fn resolve(ctx: &Context<'_>, value: &mut Value, depth: usize) -> CmdResult<()> {
    if depth > 64 {
        return Err(malformed("Chat component too deeply nested"));
    }
    match value {
        Value::Array(list) => {
            if list.is_empty() {
                return Err(malformed("Empty component array"));
            }
            for part in list {
                resolve(ctx, part, depth + 1)?;
            }
        }
        Value::Object(m) => {
            if let Some(sel) = m.remove("selector") {
                let sel = sel
                    .as_str()
                    .ok_or_else(|| malformed("Expected selector to be a string"))?;
                m.insert("text".into(), json!(joined(ctx, sel)?));
            } else if let Some(score) = m.remove("score") {
                let mut name = score["name"]
                    .as_str()
                    .ok_or_else(|| malformed("A score component needs a name"))?
                    .to_owned();
                let obj = score["objective"]
                    .as_str()
                    .ok_or_else(|| malformed("A score component needs an objective"))?;
                if name.starts_with('@') {
                    name = ctx.target_name(ctx.one(&name)?)
                } else if name == "*" {
                    name = ctx.options.username.clone()
                }
                let shown = score["value"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .unwrap_or_else(|| {
                        ctx.sim
                            .commands
                            .board
                            .score(&name, obj)
                            .map(|v| v.to_string())
                            .unwrap_or_default()
                    });
                m.insert("text".into(), json!(shown));
            } else if !m.contains_key("text") && !m.contains_key("translate") {
                return Err(malformed("Unknown chat component type"));
            }
            for key in ["with", "extra"] {
                if let Some(parts) = m.get_mut(key) {
                    let parts = parts
                        .as_array_mut()
                        .ok_or_else(|| malformed("Expected component array"))?;
                    if key == "extra" && parts.is_empty() {
                        return Err(malformed("Unexpected empty array of components"));
                    }
                    for part in parts {
                        resolve(ctx, part, depth + 1)?;
                    }
                }
            }
        }
        Value::Null => return Err(malformed("Expected a chat component")),
        _ => (),
    }
    Ok(())
}
fn json_component(ctx: &Context<'_>, a: &[&str]) -> CmdResult<Value> {
    let mut value = serde_json::from_str(&a.join(" ")).map_err(|e| malformed(&e.to_string()))?;
    resolve(ctx, &mut value, 0)?;
    Ok(value)
}
pub(super) fn run(ctx: &mut Context<'_>, name: &str, a: &[&str]) -> Reply {
    let sender = ctx.target_name(ctx.sender);
    match name {
        "say" | "me" => {
            if a.is_empty() || a[0].is_empty() {
                return Err(usage(name));
            }
            let value = message(ctx, a, name == "say" || ctx.sender != Target::Player)?;
            Ok(vec![tr(
                if name == "say" {
                    "chat.type.announcement"
                } else {
                    "chat.type.emote"
                },
                vec![json!(sender), value],
            )])
        }
        "tell" | "msg" | "w" => {
            if a.len() < 2 {
                return Err(usage("tell"));
            }
            let target = player_target(ctx, Some(a[0]))?;
            if ctx.sender == target {
                return Err(error("commands.message.sameTarget", vec![]));
            }
            let mut incoming = tr(
                "commands.message.display.incoming",
                vec![json!(sender), message(ctx, &a[1..], true)?],
            );
            incoming["color"] = json!("gray");
            incoming["italic"] = json!(true);
            Ok(vec![incoming])
        }
        "tellraw" => {
            if a.len() < 2 {
                return Err(usage(name));
            }
            player_target(ctx, Some(a[0]))?;
            Ok(vec![json_component(ctx, &a[1..])?])
        }
        "title" => {
            if a.len() < 2 {
                return Err(usage(name));
            }
            if a.len() < 3 && matches!(a[1], "title" | "subtitle" | "times") {
                let key = if a[1] == "times" {
                    "commands.title.usage.times"
                } else {
                    "commands.title.usage.title"
                };
                return Err(error("commands.generic.usage", vec![tr(key, vec![])]));
            }
            player_target(ctx, Some(a[0]))?;
            let (kind, text, times) = match a[1].to_ascii_lowercase().as_str() {
                "times" if a.len() == 5 => (
                    2,
                    String::new(),
                    [
                        integer(a[2], i32::MIN, i32::MAX)?,
                        integer(a[3], i32::MIN, i32::MAX)?,
                        integer(a[4], i32::MIN, i32::MAX)?,
                    ],
                ),
                "clear" | "reset" if a.len() == 2 => (
                    if a[1].eq_ignore_ascii_case("clear") {
                        3
                    } else {
                        4
                    },
                    String::new(),
                    [0; 3],
                ),
                "clear" | "reset" | "times" => return Err(usage("title")),
                _ if a.len() >= 3 => (
                    if a[1].eq_ignore_ascii_case("subtitle") {
                        1
                    } else {
                        0
                    },
                    json_component(ctx, &a[2..])?.to_string(),
                    [0; 3],
                ),
                _ => return Err(usage("title")),
            };
            ctx.sim
                .presentation
                .apply(crate::net_presentation::Packet::Title { kind, text, times });
            notify(ctx, "commands.title.success", vec![])
        }
        _ => unreachable!(),
    }
}
