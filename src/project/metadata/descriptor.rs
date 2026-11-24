//! 生效清单的定点资料编辑；TOML 只替换目标值并保留注释与未知字段。

use super::{Metadata, MetadataSnapshot};
use serde::Deserialize;
use std::{ops::Range, path::{Component, Path}};
use toml::Spanned;

#[derive(Deserialize)]
struct TomlFields {
    name: Spanned<String>,
    description: Option<Spanned<String>>,
    icon: Option<Spanned<String>>,
}

pub(super) fn update(before: &MetadataSnapshot, after: &Metadata) -> Result<Vec<u8>, String> {
    let icon = after.icon.as_deref().filter(|_| before.metadata.icon != after.icon)
        .map(portable_path).transpose()?;
    let utf8_bom = [0xef, 0xbb, 0xbf];
    let data = before.raw.strip_prefix(&utf8_bom).unwrap_or(&before.raw);
    let output = if before.file_name == crate::project::TOML_DESCRIPTOR {
        update_toml(data, &before.metadata, after, icon.as_deref())?
    } else {
        update_json(data, &before.metadata, after, icon.as_deref())?
    };
    if data.len() == before.raw.len() {
        return Ok(output);
    }
    Ok([utf8_bom.as_slice(), &output].concat())
}

fn update_toml(data: &[u8], before: &Metadata, after: &Metadata, icon: Option<&str>) -> Result<Vec<u8>, String> {
    let source = std::str::from_utf8(data).map_err(|error| format!("工程清单不是 UTF-8：{error}"))?;
    let fields: TomlFields = toml::from_str(source).map_err(|error| format!("工程清单 TOML 无效：{error}"))?;
    let changes = [
        ("name", Some(fields.name.span()), Some(after.name.as_str()), before.name != after.name),
        ("description", fields.description.as_ref().map(Spanned::span),
            Some(after.description.as_str()).filter(|value| !value.is_empty()), before.description != after.description),
        ("icon", fields.icon.as_ref().map(Spanned::span), icon, before.icon != after.icon),
    ];
    let mut replacements = Vec::<(Range<usize>, String)>::new();
    let mut new_fields = String::new();
    let newline = if source.contains("\r\n") { "\r\n" } else { "\n" };
    for (key, span, next, changed) in changes {
        if !changed { continue; }
        let value = next.map(|value| toml::Value::String(value.to_owned()).to_string());
        match (span, value) {
            (Some(span), Some(value)) => replacements.push((span, value)),
            (Some(span), None) => replacements.push((line_start(source, span.start)..span.end, String::new())),
            (None, Some(value)) => new_fields.push_str(&format!("{key} = {value}{newline}")),
            (None, None) => (),
        }
    }
    if !new_fields.is_empty() {
        // 从已解析的根级 name 定位新字段，避免插入未知 TOML 表或字符串内部。
        let insertion = line_start(source, fields.name.span().start);
        replacements.push((insertion..insertion, new_fields));
    }
    let mut output = source.to_owned();
    replacements.sort_by_key(|(span, _)| std::cmp::Reverse((span.start, span.end)));
    for (span, value) in replacements {
        output.replace_range(span, &value);
    }
    Ok(output.into_bytes())
}

fn line_start(source: &str, position: usize) -> usize {
    source[..position].rfind('\n').map_or(0, |newline| newline + 1)
}

fn update_json(data: &[u8], before: &Metadata, after: &Metadata, icon: Option<&str>) -> Result<Vec<u8>, String> {
    let mut value: serde_json::Value = serde_json::from_slice(data).map_err(|error| format!("工程清单 JSON 无效：{error}"))?;
    let fields = value.as_object_mut().ok_or("工程清单必须是 JSON 对象")?;
    if before.name != after.name { fields.insert("name".into(), after.name.clone().into()); }
    if before.description != after.description {
        if after.description.is_empty() { fields.remove("description"); }
        else { fields.insert("description".into(), after.description.clone().into()); }
    }
    if before.icon != after.icon {
        if let Some(icon) = icon { fields.insert("icon".into(), icon.into()); }
        else { fields.remove("icon"); }
    }
    let mut output = serde_json::to_vec_pretty(&value).map_err(|error| format!("无法写入工程清单：{error}"))?;
    output.push(b'\n');
    Ok(output)
}

fn portable_path(path: &Path) -> Result<String, String> {
    path.components().map(|part| match part {
        Component::Normal(name) => name.to_str().ok_or("工程图标路径必须是 UTF-8"),
        _ => Err("工程图标必须是工程根内的相对路径"),
    }).collect::<Result<Vec<_>, _>>().map(|parts| parts.join("/")).map_err(str::to_owned)
}
