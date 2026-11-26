//! 仅替换结构化资产字段的原文字节。场景 GLB 解开摆放 extras 的内层 JSON，保留全部其他块和二进制。

use super::Moves;
use feathrise_editor_assets::{glb_metadata, json_fields::{authored_asset_field, placement_node, visit_strings}};
pub(super) use feathrise_editor_assets::json_fields::authored_reference_document as supported;
use feathrise_scene_content::ScenePlacementAssetTarget;
use serde_json::Value;
use std::{collections::{BTreeMap, BTreeSet}, fs::{self, File}, io::{self, Write}, ops::Range, path::{Component, Path, PathBuf}};

pub(super) struct Rewrite {
    pub count: usize,
    pub unrepresentable: bool,
    pub sources: BTreeMap<PathBuf, Option<PathBuf>>,
    pub fragments: BTreeSet<String>,
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

pub(super) struct SourcePaths<'a> {
    moves: &'a Moves,
    #[cfg(windows)]
    root: &'a PathBuf,
    #[cfg(windows)]
    folded: BTreeSet<PathBuf>,
}

impl<'a> SourcePaths<'a> {
    pub(super) fn new(_root: &'a PathBuf, moves: &'a Moves, _exact: bool) -> Self {
        Self {
            moves,
            #[cfg(windows)]
            root: _root,
            #[cfg(windows)]
            folded: moves.keys().filter(|_| !_exact).map(|path| feathrise_editor_project_browser_model::file_operations::path_key(path)).collect(),
        }
    }

    fn verify(&self, prefix: &Path) -> Option<PathBuf> {
        if self.moves.contains_key(prefix) { return Some(prefix.to_owned()); }
        #[cfg(windows)]
        {
            use feathrise_editor_project_browser_model::file_operations::path_key;
            if !self.folded.contains(&path_key(prefix)) { return None; }
            // 小写键只找候选。移动前检查原始路径的实际拼写及联接链，避免重写独立的同名异大小写文件。
            let (physical, _) = super::files::checked_entry(prefix, std::slice::from_ref(self.root)).ok()?;
            return self.moves.contains_key(&physical).then_some(physical);
        }
        #[cfg(not(windows))]
        None
    }
}

pub(super) fn rewrite(path: &Path, root: &Path, sources: &SourcePaths<'_>, exact: bool, cancel: &dyn Fn() -> bool) -> Result<Rewrite, String> {
    scan(path, path, root, sources.moves, &|prefix| sources.verify(prefix), exact, false, cancel)
}

pub(super) fn rewrite_as(path: &Path, kind: &Path, root: &Path, moves: &Moves, sources: &BTreeMap<PathBuf, Option<PathBuf>>, exact: bool) -> Result<Rewrite, String> {
    // 磁盘写入时原路径可能已经失效。只采用该文档预检时实际核验过的路径前缀。
    scan(path, kind, root, moves, &|prefix| {
        sources.get(prefix).map(|alias| alias.clone().unwrap_or_else(|| prefix.to_owned()))
    }, exact, true, &|| false)
}

fn scan(path: &Path, kind: &Path, root: &Path, moves: &Moves, source_path: &dyn Fn(&Path) -> Option<PathBuf>, exact: bool, prepare: bool, cancel: &dyn Fn() -> bool) -> Result<Rewrite, String> {
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
    let mut sources = BTreeMap::<PathBuf, Option<PathBuf>>::new();
    let mut fragments = BTreeSet::new();
    let mut unrepresentable = false;
    let inspect_bindings = exact && !prepare;
    let mut remap = |value: &str, fragments: &mut BTreeSet<String>| -> Option<Option<String>> {
        let (physical, fragment) = value.find('#').map_or((value, ""), |i| value.split_at(i));
        if physical.is_empty() || physical.contains(['\\', ':']) || physical.split('/').any(|c| c.is_empty() || c == "." || c == "..")
            || !Path::new(physical).components().all(|c| matches!(c, Component::Normal(_))) { return None; }
        let path = root.join(physical);
        let (prefix, source, target) = path.ancestors().take(if exact { 1 } else { usize::MAX }).find_map(|prefix| {
            let source = match sources.get(prefix) {
                Some(alias) => alias.clone().unwrap_or_else(|| prefix.to_owned()),
                None => source_path(prefix)?,
            };
            let target = moves.get(&source)?;
            Some((prefix, source, target))
        })?;
        let suffix = path.strip_prefix(prefix).ok()?;
        let target = if suffix.as_os_str().is_empty() { target.clone() } else { target.join(suffix) };
        if !sources.contains_key(prefix) {
            sources.insert(prefix.to_owned(), (source.as_path() != prefix).then_some(source));
        }
        count += 1;
        if inspect_bindings && let Some(label) = fragment.strip_prefix('#') { fragments.insert(label.to_owned()); }
        let relative = target.strip_prefix(root).ok().and_then(Path::to_str);
        let Some(relative) = relative.filter(|s| !s.is_empty() && !s.contains('#')) else {
            unrepresentable = true; return None;
        };
        // 外层表示命中，只有暂存阶段才需要生成内层的替换字符串。
        Some(prepare.then(|| format!("{}{fragment}", relative.replace('\\', "/"))))
    };
    let is_catalog = kind.file_name().is_some_and(|n| n == "catalog.scene_catalog.json");
    let records = kind.file_name().and_then(|n| n.to_str()).and_then(|name| {
        if name.ends_with(".scene_placements.json") { Some("placements") }
        else if name.ends_with(".building_presets.json") { Some("presets") }
        else { None }
    });
    let mut matched_records = Vec::new();
    let mut edits = Vec::new();
    visit_strings(text, None, cancel, |pointer, key, value, range| {
        let replacement = if glb {
            if placement_node(pointer).is_none() { return Ok(()); }
            let mut inner = Vec::new();
            visit_strings(value, None, cancel, |pointer, _, path, range| {
                if pointer == "/asset_path" && let Some(replacement) = remap(path, &mut fragments) {
                    if inspect_bindings {
                        let record = serde_json::from_str(value).map_err(|e| e.to_string())?;
                        require_target(&record, path.contains('#'), &mut fragments)?;
                    }
                    if let Some(next) = replacement { inner.push((range, json_string(&next)?)); }
                }
                Ok(())
            })?;
            (!inner.is_empty()).then(|| replace_ranges(value, &inner))
        } else if authored_asset_field(key) || key == "asset_roots"
            || is_catalog && key == "folder" && pointer.starts_with("/scenes/") {
            let matched = remap(value, &mut fragments);
            if inspect_bindings && matched.is_some() && key == "asset_path"
                && let Some((list, index)) = pointer.strip_prefix('/').and_then(|s| s.strip_suffix("/asset_path")).and_then(|s| s.split_once('/'))
                && records == Some(list) && let Ok(index) = index.parse::<usize>() {
                matched_records.push((index, value.contains('#')));
            }
            matched.flatten()
        } else { None };
        if let Some(next) = replacement { edits.push((range, json_string(&next)?)); }
        Ok(())
    }).map_err(|e| format!("无法解析引用文件 {}：{e}", path.display()))?;
    // 字段顺序无关。只有命中正式摆放或建筑预设时才读取相邻绑定，普通模型清单保持原有语义。
    if !matched_records.is_empty() {
        let document: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if let Some(items) = records.and_then(|key| document[key].as_array()) {
            for (index, has_fragment) in matched_records {
                let record = items.get(index).ok_or_else(|| format!("引用文档包含冲突的记录索引 {index}"))?;
                require_target(record, has_fragment, &mut fragments)?;
            }
        }
    }
    Ok(Rewrite { count, sources, fragments, unrepresentable, json: if edits.is_empty() { String::new() } else { replace_ranges(text, &edits) }, bom, tail })
}

fn require_target(record: &Value, has_fragment: bool, fragments: &mut BTreeSet<String>) -> Result<(), String> {
    let target = match record.get("asset_target") {
        Some(raw) => serde_json::from_value::<ScenePlacementAssetTarget>(raw.clone())
            .map_err(|e| format!("无法识别资产引用的摆放目标：{e}"))?,
        None if has_fragment => return Ok(()), // 旧文件把标签直接放在路径后，以该标签为准。
        None => ScenePlacementAssetTarget::Scene0,
    };
    if target.is_scene0() { fragments.insert("Scene0".into()); }
    // 与运行时消费同一组 Mesh/StandardMaterial 标签；只验证绑定，不把标签补写进物理路径。
    for label in [target.mesh_reference(""), target.material_reference("")].into_iter().flatten() {
        if let Some(label) = label.strip_prefix('#') { fragments.insert(label.into()); }
    }
    Ok(())
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
