//! 复制时只重绑场景自己拥有的内容，按结构更新标识及路径。

use super::{
    check_cancelled,
    filesystem::{self, CopyEntry}, glb, read_file,
    safe_asset_path,
};
use feathrise_editor_assets::json_fields::{authored_asset_field, authored_reference_document};
use feathrise_scene_content::{SceneCatalogEntry, SceneId, SceneManifestAsset, SceneVegetationAsset, authoring_io::bytes_hash};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

pub(super) const SCENE_CHARACTERS_FILE: &str = "characters.map_characters.json";
const BOM: &[u8] = &[0xef, 0xbb, 0xbf];

pub(super) struct RootMapping {
    pub(super) source: String,
    pub(super) target: String,
}

pub(super) struct CopyPaths<'a> {
    pub(super) assets: &'a Path,
    roots: Vec<RootMapping>,
}

impl<'a> CopyPaths<'a> {
    pub(super) fn new(
        assets: &'a Path,
        source: &SceneCatalogEntry,
        target_id: &SceneId,
        name: &str,
    ) -> Result<Self, String> {
        let source_folder = source.folder.trim_end_matches('/');
        let source_sdk = format!("scenes/{}", source.id);
        let target_sdk = format!("scenes/{target_id}");
        let source_path = safe_asset_path(assets, source_folder)?;
        filesystem::checked_directory(&source_path)?;
        if contains_folder(source_folder, "scenes") {
            return Err("源场景没有独立的作者目录".into());
        }
        let mut roots = Vec::new();
        // SDK 用场景 ID 寻址稀疏页、体素块与独立地形。旧目录若已共用该根，副本也只保留一个根。
        if same_path(source_folder, &source_sdk) {
            roots.push(RootMapping { source: source_folder.into(), target: target_sdk });
        } else {
            roots.push(RootMapping { source: source_folder.into(), target: format!("scenes/{name}") });
            let sdk_path = safe_asset_path(assets, &source_sdk)?;
            match fs::symlink_metadata(&sdk_path) {
                Ok(_) => {
                    filesystem::checked_directory(&sdk_path)?;
                    if overlaps(source_folder, &source_sdk) {
                        return Err("场景资料目录和 SDK 地形目录互相包含".into());
                    }
                    roots.push(RootMapping { source: source_sdk, target: target_sdk });
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("检查场景地形目录 {} 失败：{error}", sdk_path.display())),
            }
        }
        for root in &roots {
            if roots.iter().any(|other| overlaps(&root.source, &other.target)) {
                return Err("副本目录不能覆盖源场景的私有文件".into());
            }
        }
        Ok(Self { assets, roots })
    }

    pub(super) fn roots(&self) -> &[RootMapping] { &self.roots }

    pub(super) fn target_folder(&self) -> &str { &self.roots[0].target }

    pub(super) fn check_scene_isolation(&self, other: &SceneCatalogEntry) -> Result<(), String> {
        // 受保护的 SDK 根即使当前没有地形，也不能被另一个场景的私人目录接管。
        let other_sdk = format!("scenes/{}", other.id);
        for path in [other.folder.trim_end_matches('/'), other_sdk.as_str()] {
            if self.roots.iter().any(|root| overlaps(&root.source, path) || overlaps(&root.target, path)) {
                return Err(format!("目录 {path} 与场景 {} 的独立归属冲突", other.id));
            }
        }
        Ok(())
    }

    pub(super) fn rebase(&self, value: &str) -> Option<String> {
        let (path, label) = asset_path_label(value);
        self.roots.iter().find_map(|root| {
            let (prefix, tail) = folder_tail(&root.source, path)?;
            if prefix != root.source {
                // 大小写只作为候选。Windows 上启用大小写敏感的目录仍可包含两个不同目录。
                let same_physical = self.assets.join(prefix).canonicalize().ok()?
                    == self.assets.join(&root.source).canonicalize().ok()?;
                if !same_physical { return None; }
            }
            Some(format!("{}{tail}{label}", root.target))
        })
    }
}

fn same_path(left: &str, right: &str) -> bool {
    left == right || cfg!(windows) && left.to_lowercase() == right.to_lowercase()
}

fn folder_tail<'a>(folder: &str, path: &'a str) -> Option<(&'a str, &'a str)> {
    // 按完整分量定界，不能让 scenes/门 误匹配 scenes/门楼。
    let split = path.match_indices('/').nth(folder.matches('/').count())
        .map_or(path.len(), |(index, _)| index);
    let (prefix, tail) = path.split_at(split);
    same_path(folder, prefix).then_some((prefix, tail))
}

fn contains_folder(folder: &str, path: &str) -> bool { folder_tail(folder, path).is_some() }

fn overlaps(a: &str, b: &str) -> bool { contains_folder(a, b) || contains_folder(b, a) }

fn asset_path_label(value: &str) -> (&str, &str) {
    value.find('#').map_or((value, ""), |index| value.split_at(index))
}

pub(super) fn require_private_file(paths: &CopyPaths, relative: &str) -> Result<(), String> {
    let path = safe_asset_path(paths.assets, relative)?;
    if paths.rebase(relative).is_none() || !filesystem::ordinary(&path)?.is_file() {
        return Err(format!("缺少源场景的私有作者文件：{relative}"));
    }
    Ok(())
}

fn is_standard_model_block(source: &SceneCatalogEntry, path: &str) -> bool {
    let prefix = format!("{}/vegetation/models/", source.folder.trim_end_matches('/'));
    path.strip_prefix(&prefix).and_then(|name| name.strip_suffix(".json"))
        .is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
}

pub(super) struct Documents<'a> {
    pub(super) paths: &'a CopyPaths<'a>,
    pub(super) source: &'a SceneCatalogEntry,
    pub(super) target: &'a SceneCatalogEntry,
    roles: Option<&'a str>,
    model_blocks: BTreeMap<PathBuf, String>,
    copied_block_targets: BTreeSet<String>,
}

impl<'a> Documents<'a> {
    pub(super) fn new(
        paths: &'a CopyPaths<'a>,
        source: &'a SceneCatalogEntry,
        target: &'a SceneCatalogEntry,
        roles: Option<&'a str>,
    ) -> Self {
        Self { paths, source, target, roles, model_blocks: BTreeMap::new(), copied_block_targets: BTreeSet::new() }
    }

    pub(super) fn copy_model_blocks(&mut self, entries: &[CopyEntry], cancelled: &AtomicBool) -> Result<BTreeSet<usize>, String> {
        let mut declared = BTreeMap::<String, BTreeSet<PathBuf>>::new();
        let vegetation_path = self.source.vegetation_path.as_deref();
        for entry in entries.iter().filter(|entry| !entry.directory
            && (entry.source.ends_with(".scene_vegetation.json")
                || vegetation_path.is_some_and(|path| same_path(path, &entry.source)))) {
            check_cancelled(cancelled)?;
            let (document, _) = self.read_json(&entry.source)?;
            if document.get("model_surfaces").is_none() { continue; }
            let vegetation: SceneVegetationAsset = serde_json::from_value(document)
                .map_err(|error| format!("读取模型植被索引 {} 失败：{error}", entry.source))?;
            for path in vegetation.model_surfaces.iter().flat_map(|surface| surface.sample_blocks.iter()) {
                require_private_file(self.paths, path)?;
                declared.entry(path.to_lowercase()).or_default()
                    .insert(canonical_source_path(self.paths.assets, path)?);
            }
        }
        let mut copied = BTreeSet::new();
        for (index, entry) in entries.iter().enumerate().filter(|(_, entry)| !entry.directory) {
            check_cancelled(cancelled)?;
            let expected = (!declared.is_empty()).then(|| declared.get_mut(&entry.source.to_lowercase())).flatten();
            let content_addressed = is_standard_model_block(self.source, &entry.source);
            if expected.is_none() && !content_addressed { continue; }
            let canonical = canonical_source_path(self.paths.assets, &entry.source)?;
            // Windows 大小写只用于查找候选。通过磁盘身份确认具体块，兼容自定义文件名及别名。
            let registered = expected.is_some_and(|identities| identities.remove(&canonical));
            if registered || content_addressed {
                self.copy_model_block(entry, canonical, content_addressed)?;
                copied.insert(index);
            }
        }
        if let Some(path) = declared.values().find_map(|identities| identities.first()) {
            return Err(format!("模型植被索引引用未纳入副本的源块：{}", path.display()));
        }
        Ok(copied)
    }

    fn copy_model_block(&mut self, entry: &CopyEntry, canonical: PathBuf, content_addressed: bool) -> Result<(), String> {
        let (mut document, _) = self.read_json(&entry.source)?;
        self.set_owner(&mut document, "scene_id")?;
        let bytes = serde_json::to_vec(&document).map_err(|error| error.to_string())?;
        let destination = if content_addressed {
            let (parent, _) = entry.target.rsplit_once('/').ok_or("模型植被块没有父目录")?;
            format!("{parent}/{}.json", bytes_hash(&bytes))
        } else {
            entry.target.clone()
        };
        if self.copied_block_targets.insert(destination.clone()) {
            let path = self.paths.assets.join(&destination);
            let mut output = filesystem::open_output(&path)?;
            write_bytes(&path, &mut output, &bytes)?;
            sync_file(&path, &output)?;
        }
        self.model_blocks.insert(canonical, destination);
        Ok(())
    }

    pub(super) fn copy_file(&self, entry: &CopyEntry, cancelled: &AtomicBool) -> Result<(), String> {
        let destination = self.paths.assets.join(&entry.target);
        let absolute = self.paths.assets.join(&entry.source);
        let mut output = filesystem::open_output(&destination)?;
        let is_scene_glb = self.source.placements_glb && same_path(&entry.source, &self.source.placements_path());
        let extension = absolute.extension().and_then(|ext| ext.to_str()).unwrap_or("");
        let result = if self.roles.is_some_and(|role| same_path(role, &entry.source))
            || self.is_known_scene_document(entry)
            || extension.eq_ignore_ascii_case("json") && authored_reference_document(&absolute) {
            self.copy_json(entry, &destination, &mut output)
        } else if extension.eq_ignore_ascii_case("glb") {
            glb::copy(self, entry, &mut output, is_scene_glb, cancelled)
        } else if extension.eq_ignore_ascii_case("gltf") {
            self.copy_gltf(entry, &destination, &mut output, cancelled)
        } else {
            self.copy_bytes(entry, &mut output, cancelled)
        };
        result.map_err(|error| format!("复制场景文件 {} 失败：{error}", absolute.display()))?;
        sync_file(&destination, &output)
    }

    fn copy_bytes(&self, entry: &CopyEntry, output: &mut File, cancelled: &AtomicBool) -> Result<(), String> {
        let source = File::open(self.paths.assets.join(&entry.source)).map_err(|error| error.to_string())?;
        filesystem::stream(source, output, entry.length, cancelled)
    }

    fn copy_gltf(&self, entry: &CopyEntry, destination: &Path, output: &mut File, cancelled: &AtomicBool) -> Result<(), String> {
        if let Ok((mut document, bom)) = self.read_json(&entry.source)
            && glb::rewrite_uris(self.paths, entry, &mut document) {
            return write_json(destination, output, &document, bom);
        }
        // 普通素材的未知格式或非必要改写仍走原始字节，保留制作中的外部资产。
        self.copy_bytes(entry, output, cancelled)
    }

    fn is_known_scene_document(&self, entry: &CopyEntry) -> bool {
        let known = [
            self.source.surface_path.as_deref(),
            self.source.vegetation_path.as_deref(),
            self.source.water_path.as_deref(),
            self.source.boundary_path.as_deref(),
        ].into_iter().flatten().any(|declared| same_path(declared, &entry.source));
        known || same_path(&self.source.manifest_path(), &entry.source)
            || same_path(&self.source.portals_path(), &entry.source)
            || (!self.source.placements_glb && same_path(&self.source.placements_path(), &entry.source))
    }

    fn copy_json(&self, entry: &CopyEntry, destination: &Path, output: &mut File) -> Result<(), String> {
        let (mut document, bom) = self.read_json(&entry.source)?;
        let path = &entry.source;
        if same_path(path, &self.source.manifest_path()) {
            let manifest: SceneManifestAsset = serde_json::from_value(document.clone()).map_err(|error| error.to_string())?;
            if manifest.runtime_scene != "data_driven" {
                return Err(format!("当前场景的运行类型 {} 不支持复制", manifest.runtime_scene));
            }
            self.set_owner(&mut document, "id")?;
            document["display_name"] = Value::String(self.target.display_name.clone());
        } else if self.roles.is_some_and(|role| same_path(role, path)) {
            // RPG 私有文档只通过目录协议标识。角色模板、点位和自定义字段继续按原含义保存。
            self.set_owner(&mut document, "map_id")?;
            return write_json(destination, output, &document, bom);
        } else if document.get("scene_id").is_some() || self.is_known_scene_document(entry) {
            self.set_owner(&mut document, "scene_id")?;
        }
        if same_path(path, &self.source.portals_path()) {
            let portals = document.get_mut("portals").and_then(Value::as_array_mut).ok_or("场景传送文件缺少传送表")?;
            for portal in portals {
                let target = portal.get_mut("target").and_then(Value::as_object_mut)
                    .and_then(|target| target.get_mut("scene_id")).ok_or("场景传送点缺少目标场景")?;
                let target_id = target.as_str().ok_or("传送目标场景 ID 无效")?;
                SceneId::parse(target_id)?;
                if target_id == self.source.id.as_str() {
                    // 只重绑该副本自己的回路，跨场景传送和原图的入向引用都保持原义。
                    *target = Value::String(self.target.id.to_string());
                }
            }
        }
        self.rewrite_paths("", &mut document);
        write_json(destination, output, &document, bom)
    }

    fn read_json(&self, relative: &str) -> Result<(Value, bool), String> {
        let bytes = read_file(&self.paths.assets.join(relative))?;
        let json = bytes.strip_prefix(BOM).unwrap_or(&bytes);
        let document = serde_json::from_slice(json)
            .map_err(|error| format!("解析 {relative} 失败：{error}"))?;
        Ok((document, bytes.starts_with(BOM)))
    }

    pub(super) fn set_owner(&self, document: &mut Value, field: &str) -> Result<(), String> {
        let owner = document.get_mut(field)
            .ok_or_else(|| format!("缺少源场景归属字段 {field}"))?;
        if owner.as_str() != Some(self.source.id.as_str()) {
            return Err(format!("源场景归属字段 {field} 与目录场景 {} 不一致", self.source.id));
        }
        *owner = Value::String(self.target.id.to_string());
        Ok(())
    }

    pub(super) fn rewrite_paths(&self, field: &str, data: &mut Value) {
        match data {
            Value::Object(fields) => {
                for (field, value) in fields { self.rewrite_paths(field, value); }
            }
            Value::Array(values) => {
                for value in values { self.rewrite_paths(field, value); }
            }
            Value::String(value) if authored_asset_field(field) || matches!(field, "asset_roots" | "sample_blocks") => {
                let (path, label) = asset_path_label(value);
                let model_block = (field == "sample_blocks").then(|| {
                    safe_asset_path(self.paths.assets, path).ok()?.canonicalize().ok()
                }).flatten().and_then(|absolute| self.model_blocks.get(&absolute));
                if let Some(model_block) = model_block {
                    *value = format!("{model_block}{label}");
                } else if let Some(rebased) = self.paths.rebase(value) {
                    *value = rebased;
                }
            }
            _ => {}
        }
    }
}

fn canonical_source_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    safe_asset_path(root, relative)?.canonicalize()
        .map_err(|error| format!("复核模型植被块 {relative} 失败：{error}"))
}

fn write_bytes(path: &Path, output: &mut File, bytes: &[u8]) -> Result<(), String> {
    output.write_all(bytes).map_err(|error| format!("写入 {} 失败：{error}", path.display()))
}

fn write_json(path: &Path, output: &mut File, document: &Value, bom: bool) -> Result<(), String> {
    if bom { write_bytes(path, output, BOM)?; }
    serde_json::to_writer_pretty(&mut *output, document)
        .map_err(|error| format!("写入 {} 失败：{error}", path.display()))?;
    write_bytes(path, output, b"\n")
}

fn sync_file(path: &Path, file: &File) -> Result<(), String> {
    file.sync_all().map_err(|error| format!("刷新 {} 失败：{error}", path.display()))
}
