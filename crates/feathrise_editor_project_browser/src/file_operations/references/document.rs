//! 仅替换结构化资产字段的原文字节。场景 GLB 解开摆放 extras 的内层 JSON，保留全部其他块和二进制。

use super::Moves;
use feathrise_editor_assets::{glb_metadata, json_fields::visit_strings};
use std::{collections::BTreeSet, fs::{self, File}, io::{self, Write}, ops::Range, path::{Component, Path, PathBuf}};

pub(super) fn supported(path: &Path) -> bool {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or_default();
    name.eq_ignore_ascii_case("scene.glb") || name.ends_with(".json")
        && (name.contains(".scene_") || name.contains(".map_") || name.ends_with("presets.json") || name.ends_with(".preset.json"))
}

/// 扫描搜索可以报告疑似路径，改写只接受场景和预设格式已定义的资源字段。
/// 例如 water_surfaces 的 node_path 保存 GLB 内节点名，不能随同同名目录搬移。
fn stored_asset_field(key: &str) -> bool {
    let key = key.strip_suffix("_paths").unwrap_or_else(|| key.strip_suffix("_path").unwrap_or(""));
    matches!(key, "asset" | "author_asset" | "model" | "mesh" | "material" | "glb" | "height" | "skybox" |
        "texture" | "source" | "peer_source" | "surface" | "surface_fields" | "vegetation" | "water" | "boundary" | "characters")
        || key.ends_with("_texture") || key.ends_with("_source") || key.ends_with("_array")
}

pub(super) struct Rewrite {
    pub count: usize,
    pub unrepresentable: bool,
    pub sources: BTreeSet<PathBuf>,
    json: String,
    bom: bool,
    tail: Option<(File, u64)>,
}

impl Rewrite {
    pub fn write(mut self, output: &mut dyn Write) -> Result<(), String> {
        let mut stage = || -> io::Result<()> {
            if let Some((mut tail, tail_length)) = self.tail.take() {
                let json = self.json.trim_end_matches(' ').as_bytes();
                let padded = json.len().next_multiple_of(4);
                let total = u32::try_from(20u64 + padded as u64 + tail_length)
                    .map_err(|_| io::Error::other("更新后的文件超出 GLB v2 长度范围"))?;
                output.write_all(b"glTF")?;
                output.write_all(&2u32.to_le_bytes())?;
                output.write_all(&total.to_le_bytes())?;
                output.write_all(&(padded as u32).to_le_bytes())?;
                output.write_all(b"JSON")?;
                output.write_all(json)?;
                output.write_all(&b"   "[..padded - json.len()])?;
                if io::copy(&mut tail, output)? != tail_length { return Err(io::Error::other("GLB 后续块在复制时发生变化")); }
            } else {
                if self.bom { output.write_all(&[0xef, 0xbb, 0xbf])?; }
                output.write_all(self.json.as_bytes())?;
            }
            Ok(())
        };
        stage().map_err(|e| e.to_string())
    }
}

pub(super) fn rewrite(path: &Path, root: &Path, moves: &Moves, cancel: &dyn Fn() -> bool) -> Result<Rewrite, String> {
    scan(path, path, root, moves, false, cancel)
}

pub(super) fn rewrite_as(path: &Path, kind: &Path, root: &Path, moves: &Moves, cancel: &dyn Fn() -> bool) -> Result<Rewrite, String> {
    scan(path, kind, root, moves, true, cancel)
}

fn scan(path: &Path, kind: &Path, root: &Path, moves: &Moves, prepare: bool, cancel: &dyn Fn() -> bool) -> Result<Rewrite, String> {
    let glb = kind.extension().is_some_and(|e| e.eq_ignore_ascii_case("glb"));
    let (bytes, tail) = if glb {
        let (json, tail, len) = glb_metadata::json_prefix(path, u64::from(u32::MAX), None)?;
        (json, Some((tail, len)))
    } else {
        (fs::read(path).map_err(|e| format!("无法读取引用文件 {}：{e}", path.display()))?, None)
    };
    let text = std::str::from_utf8(&bytes).map_err(|e| format!("引用文件 {} 不是 UTF-8：{e}", path.display()))?;
    let bom = !glb && text.starts_with('\u{feff}');
    let text = if bom { &text[3..] } else { text };
    let mut count = 0;
    let mut sources = BTreeSet::new();
    let mut unrepresentable = false;
    let mut remap = |value: &str| -> Option<String> {
        let (physical, fragment) = value.find('#').map_or((value, ""), |i| value.split_at(i));
        if physical.is_empty() || physical.contains(['\\', ':']) || physical.split('/').any(|c| c.is_empty() || c == "." || c == "..")
            || !Path::new(physical).components().all(|c| matches!(c, Component::Normal(_))) { return None; }
        let path = root.join(physical);
        let (source, target) = path.ancestors().find_map(|source| moves.get_key_value(source))?;
        let suffix = path.strip_prefix(source).ok()?;
        let target = if suffix.as_os_str().is_empty() { target.clone() } else { target.join(suffix) };
        if !sources.contains(source) { sources.insert(source.clone()); }
        count += 1;
        let relative = target.strip_prefix(root).ok().and_then(Path::to_str);
        let Some(relative) = relative.filter(|s| !s.is_empty() && !s.contains('#')) else {
            unrepresentable = true; return None;
        };
        // 首次预览与确认复核只需统计命中。仅为即将暂存的文档生成替换文本。
        prepare.then(|| format!("{}{fragment}", relative.replace('\\', "/")))
    };
    let is_catalog = kind.file_name().is_some_and(|n| n == "catalog.scene_catalog.json");
    let mut edits = Vec::new();
    visit_strings(text, None, cancel, |pointer, key, value, range| {
        let replacement = if glb {
            let Some(node) = pointer.strip_prefix("/nodes/").and_then(|s| s.strip_suffix("/extras/feathrise_placement")) else { return Ok(()); };
            if !node.bytes().all(|b| b.is_ascii_digit()) { return Ok(()); }
            let mut inner = Vec::new();
            visit_strings(value, None, cancel, |pointer, _, path, range| {
                if pointer == "/asset_path" && let Some(next) = remap(path) { inner.push((range, json_string(&next)?)); }
                Ok(())
            })?;
            (!inner.is_empty()).then(|| replace_ranges(value, &inner))
        } else if stored_asset_field(key) || key == "asset_roots"
            || is_catalog && key == "folder" && pointer.starts_with("/scenes/") {
            remap(value)
        } else { None };
        if let Some(next) = replacement { edits.push((range, json_string(&next)?)); }
        Ok(())
    }).map_err(|e| format!("无法解析引用文件 {}：{e}", path.display()))?;
    Ok(Rewrite { count, sources, unrepresentable, json: if edits.is_empty() { String::new() } else { replace_ranges(text, &edits) }, bom, tail })
}

fn json_string(value: &str) -> Result<String, String> { serde_json::to_string(value).map_err(|e| e.to_string()) }

fn replace_ranges(source: &str, edits: &[(Range<usize>, String)]) -> String {
    let mut result = String::with_capacity(source.len());
    let mut last = 0;
    for (range, text) in edits {
        result.push_str(&source[last..range.start]);
        result.push_str(text);
        last = range.end;
    }
    result.push_str(&source[last..]);
    result
}
