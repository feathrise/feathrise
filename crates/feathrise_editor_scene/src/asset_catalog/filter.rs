//! 工程目录的资源类型、GLB 元数据与列表、缩略图共用的扫描快照。

use std::{
    collections::HashMap,
    fs::Metadata,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

use crate::{
    types::EditorAssetEntry,
    ui::asset_preview::{AssetPreviewFileKind, preview_file_kind},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum AssetTypeFilter {
    #[default]
    All,
    Asset,
    Glb,
    Model,
    Animation,
    Skeleton,
    Image,
    Audio,
    Video,
}

impl AssetTypeFilter {
    pub(super) fn needs_glb_metadata(self) -> bool {
        matches!(self, Self::Model | Self::Animation | Self::Skeleton)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct DirectoryFilters {
    pub(super) kind: AssetTypeFilter,
    pub(super) filename: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AnimationClipInfo {
    pub(crate) index: usize,
    pub(crate) label: String,
}

pub(super) type DirectoryAnimations = HashMap<String, Arc<[AnimationClipInfo]>>;

#[derive(Debug, Default)]
pub(super) struct DirectorySnapshot {
    pub(super) entries: Vec<EditorAssetEntry>,
    pub(super) animations: DirectoryAnimations,
}

impl DirectorySnapshot {
    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.animations.is_empty()
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct ResourceTypes(u16);

impl ResourceTypes {
    pub(super) fn matches(self, filter: AssetTypeFilter) -> bool {
        filter == AssetTypeFilter::All || self.0 & (1 << filter as u8) != 0
    }

    pub(super) fn merge(&mut self, other: Self) {
        self.0 |= other.0;
    }

    fn insert(&mut self, kind: AssetTypeFilter) {
        self.0 |= 1 << kind as u8;
    }

    pub(super) fn from_path(path: &Path, registered_asset: bool) -> Self {
        use AssetTypeFilter::*;
        let mut types = Self::default();
        if registered_asset {
            types.insert(Asset);
        }
        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        match extension.as_str() {
            "glb" => types.insert(Glb),
            "mp4" | "m4v" | "webm" | "mov" | "avi" | "mkv" | "wmv" | "mpg" | "mpeg" | "ogv" => {
                types.insert(Video)
            }
            "bmp" | "webp" | "gif" | "tif" | "tiff" | "tga" | "dds" | "hdr" | "exr" | "avif"
            | "ico" | "svg" => types.insert(Image),
            "aac" | "m4a" | "opus" | "aif" | "aiff" | "wma" => types.insert(Audio),
            _ => match preview_file_kind(&path.to_string_lossy()) {
                AssetPreviewFileKind::Image => types.insert(Image),
                AssetPreviewFileKind::Audio => types.insert(Audio),
                _ => {}
            },
        }
        types
    }
}

#[derive(Clone, Default)]
pub(super) struct GlbMetadata {
    pub(super) types: ResourceTypes,
    pub(super) clips: Arc<[AnimationClipInfo]>,
}

impl GlbMetadata {
    fn read(path: &Path) -> Self {
        let Ok(json) = feathrise_editor_assets::glb_metadata::read_directory(path) else {
            return Self::default();
        };
        let array = |key: &str| {
            json.get(key)
                .and_then(serde_json::Value::as_array)
                .map_or(&[][..], Vec::as_slice)
        };
        let mut types = ResourceTypes::default();
        if !array("meshes").is_empty() {
            types.insert(AssetTypeFilter::Model);
        }
        let nodes = array("nodes");
        if array("skins").iter().any(|skin| {
            skin.get("joints")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|joints| {
                    joints.iter().any(|joint| {
                        joint
                            .as_u64()
                            .is_some_and(|index| index < nodes.len() as u64)
                    })
                })
        }) {
            types.insert(AssetTypeFilter::Skeleton);
        }
        let animations = array("animations");
        if !animations.is_empty() {
            types.insert(AssetTypeFilter::Animation);
        }
        let names = animations
            .iter()
            .map(|clip| clip["name"].as_str().unwrap_or_default());
        let mut name_counts = HashMap::new();
        for name in names.clone() {
            *name_counts.entry(name).or_insert(0) += 1;
        }
        // 使用 GLB 数组的原下标，空名和同名只影响显示标签，不会重排片段身份。
        let clips = names
            .enumerate()
            .map(|(index, name)| AnimationClipInfo {
                index,
                label: if name.trim().is_empty() {
                    format!("#{}", index + 1)
                } else if name_counts[name] > 1 {
                    format!("#{} · {name}", index + 1)
                } else {
                    name.to_owned()
                },
            })
            .collect();
        Self { types, clips }
    }
}

/// 文件路径之外还校验写入时间、大小及创建时间，目录事件会使同尺寸同时间戳的替换失效。
#[derive(Clone, Copy, PartialEq, Eq)]
struct FileRevision {
    bytes: u64,
    modified: SystemTime,
    created: Option<SystemTime>,
}

impl FileRevision {
    fn of(metadata: &Metadata) -> Option<Self> {
        Some(Self {
            bytes: metadata.len(),
            modified: metadata.modified().ok()?,
            created: metadata.created().ok(),
        })
    }
}

#[derive(Default)]
pub(super) struct GlbMetadataCache {
    entries: HashMap<PathBuf, (FileRevision, GlbMetadata, usize)>,
    label_bytes: usize,
}

impl GlbMetadataCache {
    pub(super) fn read(&mut self, path: &Path, metadata: &Metadata) -> GlbMetadata {
        let Some(revision) = FileRevision::of(metadata) else {
            return GlbMetadata::read(path);
        };
        if let Some((old, cached, _)) = self.entries.get(path)
            && *old == revision
        {
            return cached.clone();
        }
        self.invalidate(path, false);
        let parsed = GlbMetadata::read(path);
        let bytes = parsed
            .clips
            .iter()
            .map(|clip| clip.label.len() + std::mem::size_of::<AnimationClipInfo>())
            .sum::<usize>();
        // 目录结果完整交付；缓存超预算时只停止留存新的解析副本，不截断作者条目。
        if self.entries.len() < 16_384
            && self.label_bytes + bytes < 32 * 1024 * 1024
            && std::fs::metadata(path)
                .ok()
                .as_ref()
                .and_then(FileRevision::of)
                == Some(revision)
        {
            self.entries
                .insert(path.to_path_buf(), (revision, parsed.clone(), bytes));
            self.label_bytes += bytes;
        }
        parsed
    }

    pub(super) fn invalidate(&mut self, path: &Path, subtree: bool) {
        if subtree {
            self.entries.retain(|file, (_, _, bytes)| {
                let keep = !file.starts_with(path);
                if !keep {
                    self.label_bytes -= *bytes;
                }
                keep
            });
        } else if let Some((_, _, bytes)) = self.entries.remove(path) {
            self.label_bytes -= bytes;
        }
    }
}
