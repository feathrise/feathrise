//! 用 serde_json 原始值定位容器成员，只替换发生变化的字段。无关缩进、键序、数字与转义保持原字节。
use serde::{
    Deserialize, Deserializer,
    de::{MapAccess, Visitor},
};
use serde_json::{Value, value::RawValue};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fmt,
};

struct Fields<'a>(Vec<(String, &'a RawValue)>);
impl<'de> Deserialize<'de> for Fields<'de> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Read;
        impl<'de> Visitor<'de> for Read {
            type Value = Fields<'de>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("JSON 对象")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut rows = Vec::new();
                while let Some(row) = map.next_entry()? {
                    rows.push(row);
                }
                Ok(Fields(rows))
            }
        }
        d.deserialize_map(Read)
    }
}

pub(super) fn keys(source: &str, section: &str) -> Result<Vec<String>, String> {
    let fields = members(source)?;
    let Some((_, raw)) = fields.into_iter().find(|(key, _)| key == section) else {
        return Ok(Vec::new());
    };
    Ok(members(raw)?.into_iter().map(|(key, _)| key).collect())
}

pub(super) fn members(source: &str) -> Result<Vec<(String, &str)>, String> {
    let fields: Fields = serde_json::from_str(source).map_err(|e| e.to_string())?;
    Ok(fields
        .0
        .into_iter()
        .map(|(key, raw)| (key, raw.get()))
        .collect())
}

/// 改名按范围定位原始对象键，同一 ID 的其它作用域不会误用旧词元。
pub(super) struct Rename {
    pub scene: Option<String>,
    pub section: &'static str,
    pub from: String,
    pub to: String,
}
struct Part<'a> {
    key: Option<String>,
    key_text: &'a str,
    colon: &'a str,
    gap: &'a str,
    value: &'a str,
}
struct Container<'a> {
    parts: Vec<Part<'a>>,
    leading: &'a str,
    trailing: &'a str,
    open: char,
    close: char,
}
impl<'a> Container<'a> {
    fn parse(raw: &'a str) -> Result<Self, String> {
        let object = raw.starts_with('{');
        let entries: Vec<_> = if object {
            let rows: Fields = serde_json::from_str(raw).map_err(|e| e.to_string())?;
            rows.0
                .into_iter()
                .map(|(k, v)| (Some(k), v.get()))
                .collect()
        } else {
            let rows: Vec<&RawValue> = serde_json::from_str(raw).map_err(|e| e.to_string())?;
            rows.into_iter().map(|v| (None, v.get())).collect()
        };
        let mut parts = Vec::with_capacity(entries.len());
        let mut end = 1;
        for (key, value) in entries {
            // 借用的 RawValue 指向输入的精确子串；字段名的转义不影响其字节边界。
            let v = value.as_ptr() as usize - raw.as_ptr() as usize;
            let gap = &raw[end..v];
            let gap_len = gap
                .bytes()
                .take_while(|c| c.is_ascii_whitespace() || *c == b',')
                .count();
            let key_and_colon = &gap[gap_len..];
            let key_text = key_and_colon
                .trim_end()
                .strip_suffix(':')
                .unwrap_or("")
                .trim_end();
            parts.push(Part {
                key,
                key_text,
                colon: &key_and_colon[key_text.len()..],
                gap: &gap[..gap_len],
                value,
            });
            end = v + value.len();
        }
        let leading = parts.first().map_or("", |p| p.gap);
        Ok(Self {
            parts,
            leading,
            trailing: &raw[end..raw.len() - 1],
            open: if object { '{' } else { '[' },
            close: if object { '}' } else { ']' },
        })
    }
    fn assemble(&self, values: Vec<String>) -> String {
        let mut out = self.open.to_string();
        out.push_str(self.leading);
        let separator = self
            .parts
            .get(1)
            .map(|p| p.gap.to_owned())
            .unwrap_or_else(|| format!(",{}", self.leading));
        for (i, value) in values.into_iter().enumerate() {
            if i != 0 {
                out.push_str(self.parts.get(i).map_or(&separator, |p| p.gap));
            }
            out.push_str(&value);
        }
        out.push_str(self.trailing);
        out.push(self.close);
        out
    }
}

pub(super) fn rewrite(
    source: &str,
    before: &Value,
    after: &Value,
    renames: &[Rename],
) -> Result<String, String> {
    let raw = source.trim();
    let start = raw.as_ptr() as usize - source.as_ptr() as usize;
    Ok(format!(
        "{}{}{}",
        &source[..start],
        merge(raw, before, after, "", 0, None, renames)?,
        &source[start + raw.len()..]
    ))
}
fn merge(
    raw: &str,
    before: &Value,
    after: &Value,
    field: &str,
    depth: usize,
    scene: Option<&str>,
    renames: &[Rename],
) -> Result<String, String> {
    if before == after {
        return Ok(raw.into());
    }
    if let (Some(old), Some(new)) = (before.as_object(), after.as_object()) {
        // 切换触发形式时连同原形式显式的 null 一起替换；不能在事件触发上遗留 from/to。
        if field == "when" && old.contains_key("changed") != new.contains_key("changed") {
            return Ok(after.to_string());
        }
        let body = Container::parse(raw)?;
        let mut present = HashSet::new();
        let mut output = Vec::new();
        for part in &body.parts {
            let key = part.key.as_deref().unwrap();
            let dest = renames
                .iter()
                .find(|rename| {
                    rename.section == field
                        && rename.from == key
                        && rename.scene.as_deref() == scene
                        && depth == if scene.is_some() { 3 } else { 1 }
                })
                .map_or(key, |rename| rename.to.as_str());
            let child_scene = if depth == 1 && field == "scenes" {
                Some(dest)
            } else {
                scene
            };
            let a = old.get(key);
            let b = new.get(dest);
            let encoded = match (a, b) {
                (Some(_), None) => continue,
                (Some(a), Some(b)) => {
                    merge(part.value, a, b, dest, depth + 1, child_scene, renames)?
                }
                (None, Some(b)) => b.to_string(),
                (None, None) => part.value.to_owned(), // 旧文档可能显式写出 once:false、if:[] 或可选 null。
            };
            present.insert(dest);
            let name = if dest == key {
                part.key_text.to_owned()
            } else {
                Value::String(dest.into()).to_string()
            };
            output.push(format!("{name}{}{encoded}", part.colon));
        }
        let colon = body.parts.first().map_or(": ", |p| p.colon);
        for (k, v) in new {
            if !present.contains(k.as_str()) && old.get(k) != Some(v) {
                output.push(format!("{}{colon}{v}", Value::String(k.to_owned())));
            }
        }
        return Ok(body.assemble(output));
    }
    if let (Some(old), Some(new)) = (before.as_array(), after.as_array()) {
        let body = Container::parse(raw)?;
        // 同值的行按原顺序复用，用原始词元移动条件和动作。未匹配的同位行再做逐字段编辑。
        let mut by_value: HashMap<&Value, VecDeque<usize>> = HashMap::new();
        for (i, value) in old.iter().enumerate() {
            by_value.entry(value).or_default().push_back(i);
        }
        let matches: Vec<_> = new
            .iter()
            .map(|v| by_value.get_mut(v).and_then(VecDeque::pop_front))
            .collect();
        let taken: HashSet<_> = matches.iter().flatten().copied().collect();
        let mut output = Vec::with_capacity(new.len());
        for (i, (v, matched)) in new.iter().zip(matches).enumerate() {
            let prior = matched.or_else(|| (i < old.len() && !taken.contains(&i)).then_some(i));
            output.push(if let Some(j) = prior {
                merge(
                    body.parts[j].value,
                    &old[j],
                    v,
                    "",
                    depth + 1,
                    scene,
                    renames,
                )?
            } else {
                v.to_string()
            });
        }
        return Ok(body.assemble(output));
    }
    Ok(after.to_string())
}
