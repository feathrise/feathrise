//! GLB 内容选择与跨文件片段组装。所有跨文件通道都重新绑定目标节点，沿用原片段保存器。

use super::{reader::array, types::*};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub enum ContentKind {
    #[default]
    Animation,
    Node,
    Skin,
    Material,
    Texture,
    Image,
    Camera,
}

impl ContentKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Animation => "animations",
            Self::Node => "nodes",
            Self::Skin => "skins",
            Self::Material => "materials",
            Self::Texture => "textures",
            Self::Image => "images",
            Self::Camera => "cameras",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Animation => "动画",
            Self::Node => "对象",
            Self::Skin => "骨架",
            Self::Material => "材质",
            Self::Texture => "纹理",
            Self::Image => "图像",
            Self::Camera => "相机",
        }
    }
}

/// 动作使用当前作者会话的稳定 ID；其余资源使用同一份 GLB 的原始数组索引。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ContentItem {
    pub kind: ContentKind,
    pub id: usize,
}

/// 选区按可见行区间记录，大型文件全选只产生一个区间；实际资源列表在后台冻结的文档中展开。
#[derive(Clone, Copy, Debug)]
pub struct ContentSpan {
    pub kind: ContentKind,
    pub first: usize,
    pub last: usize,
}

/// 一次粘贴的节点索引和映射缓存可供整批片段复用。同层重名在源和目标两端都视为歧义。
pub struct DirectClipMapping<'a> {
    source: &'a AnimationDocument,
    target: &'a AnimationDocument,
    source_children: HashMap<(Option<usize>, &'a str), Option<usize>>,
    target_children: HashMap<(Option<usize>, &'a str), Option<usize>>,
    mapped: HashMap<usize, usize>,
}

fn named_children(
    document: &AnimationDocument,
    indices: impl IntoIterator<Item = usize>,
) -> HashMap<(Option<usize>, &str), Option<usize>> {
    let mut names = HashMap::new();
    for i in indices {
        if let Some(name) = document.source.json["nodes"][i]["name"]
            .as_str()
            .filter(|s| !s.is_empty())
        {
            names
                .entry((document.nodes[i].parent, name))
                .and_modify(|id| *id = None)
                .or_insert(Some(i));
        }
    }
    names
}

impl AnimationDocument {
    pub fn resolve_content(&self, spans: &[ContentSpan]) -> Result<Vec<ContentItem>, String> {
        let mut result = std::collections::BTreeSet::new();
        for &ContentSpan { kind, first, last } in spans {
            if first > last || last >= self.content_count(kind) {
                return Err("来源内容已改变，请重新选择".into());
            }
            result.extend((first..=last).map(|row| ContentItem {
                kind,
                id: if kind == ContentKind::Animation {
                    self.clips[row].source_index
                } else {
                    row
                },
            }));
        }
        Ok(result.into_iter().collect())
    }
    pub fn content_count(&self, kind: ContentKind) -> usize {
        if kind == ContentKind::Animation {
            self.clips.len()
        } else {
            array(&self.source.json, kind.key()).len()
        }
    }

    pub fn content_at(&self, kind: ContentKind, row: usize) -> Option<(ContentItem, String)> {
        if kind == ContentKind::Animation {
            let clip = self.clips.get(row)?;
            return Some((
                ContentItem {
                    kind,
                    id: clip.source_index,
                },
                clip.name.clone(),
            ));
        }
        let entry = array(&self.source.json, kind.key()).get(row)?;
        let name = entry["name"]
            .as_str()
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{} {}", kind.label(), row + 1));
        Some((ContentItem { kind, id: row }, name))
    }

    pub fn selected_clip(&self, id: usize) -> Result<&Clip, String> {
        let clip = self
            .clips
            .iter()
            .find(|c| c.source_index == id)
            .ok_or("所选片段已改变")?;
        // 解析器遇到未知通道会保留原字节。跨文件改绑无法原样沿用该索引，拒绝整段而非丢轨。
        let preserved = self.preserved_channel_counts.get(id).copied().unwrap_or(0);
        if clip.channels.is_empty()
            || clip.channels.len() < preserved
            || clip.channels[..preserved]
                .iter()
                .enumerate()
                .any(|(i, c)| c.source_index != i)
        {
            return Err(format!(
                "「{}」包含无法重映射的通道或没有可导入的通道",
                clip.name
            ));
        }
        if let Some((_, animation)) = self.original_clip(id) {
            // 新片段会重建通道与采样器，可能引用源文档索引的扩展必须先有明确映射规则。
            let channels = array(animation, "channels");
            let extended = std::iter::once(animation)
                .chain(array(animation, "samplers"))
                .chain(channels)
                .chain(channels.iter().map(|channel| &channel["target"]))
                .any(|part| {
                    part.get("extensions").is_some_and(|extensions| {
                        !extensions
                            .as_object()
                            .is_some_and(|values| values.is_empty())
                    })
                });
            if extended {
                return Err(format!("片段「{}」含有无法跨文件重映射的扩展", clip.name));
            }
        }
        if let Some((_, animation)) = self.original_clip(id) {
            // 新片段会重建通道与采样器，可能引用源文档索引的扩展必须先有明确映射规则。
            let channels = array(animation, "channels");
            let extended = std::iter::once(animation)
                .chain(array(animation, "samplers"))
                .chain(channels)
                .chain(channels.iter().map(|channel| &channel["target"]))
                .any(|part| {
                    part.get("extensions").is_some_and(|extensions| {
                        !extensions
                            .as_object()
                            .is_some_and(|values| values.is_empty())
                    })
                });
            if extended {
                return Err(format!("片段「{}」含有无法跨文件重映射的扩展", clip.name));
            }
        }
        Ok(clip)
    }

    pub fn direct_clip_mapping<'a>(&'a self, source: &'a Self) -> DirectClipMapping<'a> {
        DirectClipMapping {
            source,
            target: self,
            source_children: named_children(source, 0..source.nodes.len()),
            target_children: named_children(self, self.scene_node_indices()),
            mapped: HashMap::new(),
        }
    }

    pub fn new_imported_clip(&self, name: &str) -> Clip {
        Clip {
            name: name.into(),
            channels: Vec::new(),
            source_index: self.next_clip_index(),
        }
    }
}

impl DirectClipMapping<'_> {
    /// 直接匹配需要祖先、名字和静置坐标逐级等价。不同文件中的数组序号永远不是节点身份。
    pub fn copy(&mut self, clip: &Clip) -> Result<Clip, String> {
        let source = self.source;
        let mapped = &mut self.mapped;
        let mut result = self.target.new_imported_clip(&clip.name);
        for channel in &clip.channels {
            let mut path = Vec::new();
            let mut ancestor = Some(channel.node);
            while let Some(index) = ancestor.filter(|i| !mapped.contains_key(i)) {
                let node = source.nodes.get(index).ok_or("动画源节点无效")?;
                path.push(index);
                ancestor = node.parent;
                if path.len() > source.nodes.len() {
                    return Err("动作节点父级存在循环".into());
                }
            }
            let mut parent = ancestor.and_then(|i| mapped.get(&i).copied());
            for index in path.into_iter().rev() {
                let name = source.source.json["nodes"][index]["name"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .ok_or("动作存在无名祖先或目标，无法确定跨文件映射")?;
                if self
                    .source_children
                    .get(&(source.nodes[index].parent, name))
                    .copied()
                    .flatten()
                    != Some(index)
                {
                    return Err(format!("来源中存在同层重名节点「{name}」"));
                }
                let target = self
                    .target_children
                    .get(&(parent, name))
                    .copied()
                    .flatten()
                    .ok_or_else(|| format!("目标中找不到唯一的同层节点「{name}」"))?;
                let from = super::structure::local_matrix(&source.source.json["nodes"][index])?;
                let to = super::structure::local_matrix(&self.target.source.json["nodes"][target])?;
                if !from.abs_diff_eq(to, 1e-5) {
                    return Err(format!("节点「{name}」的静置坐标不同"));
                }
                mapped.insert(index, target);
                parent = Some(target);
            }
            let index = mapped[&channel.node];
            if !self.target.can_animate_node(index) {
                return Err(format!("节点「{}」不可写入 TRS 通道", channel.node_name));
            }
            let mut channel = channel.clone();
            channel.node = index;
            channel.node_name.clone_from(&self.target.nodes[index].name);
            result.push_existing_channel(channel)?;
        }
        Ok(result)
    }
}
