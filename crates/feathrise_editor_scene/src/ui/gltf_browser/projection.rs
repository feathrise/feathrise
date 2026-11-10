//! 搜索和单层逻辑目录的渐进投影；只重排树行，不改变实例和源模型。

use super::EditorGltfBrowserState;
use std::collections::BTreeMap;

pub(super) enum RootToken {
    Group {
        name: String,
        count: usize,
        collapsed: bool,
    },
    Placement {
        index: usize,
        depth: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::super::PlacementSignature;
    use super::*;
    use std::sync::Arc;

    fn state(count: usize) -> EditorGltfBrowserState {
        EditorGltfBrowserState {
            has_object_groups: true,
            signature: (0..count)
                .map(|index| PlacementSignature {
                    editor: Arc::new(feathrise_scene_content::ScenePlacementEditorMetadata {
                        name: format!("村口路灯{index}"),
                        group: "村口".into(),
                        ..Default::default()
                    }),
                    placement_id: format!("id-{index}"),
                    asset_path: "gltf/Tree.glb".into(),
                    asset_target: Default::default(),
                    water_bindings: Vec::new(),
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn chinese_names_ids_paths_and_groups_are_searchable() {
        let state = state(3);
        for (query, expected) in [
            ("村口路灯1", 1),
            ("ID-2", 1),
            ("tree.GLB", 3),
            ("村口", 3),
            ("不存在", 0),
        ] {
            let mut projection = RootProjection::new(query);
            while !projection.advance(&state) {}
            assert_eq!(
                projection
                    .roots
                    .iter()
                    .filter(|token| matches!(token, RootToken::Placement { .. }))
                    .count(),
                expected
            );
        }
    }

    #[test]
    fn folders_collapse_and_search_reveals_matching_children() {
        let mut state = state(3);
        state.collapsed_groups.insert("村口".into());
        let mut projection = RootProjection::new("");
        assert!(projection.advance(&state));
        assert_eq!(projection.roots.len(), 1);
        assert!(
            matches!(&projection.roots[0], RootToken::Group { name, collapsed: true, count: 3 } if name == "村口")
        );
        let mut projection = RootProjection::new("路灯1");
        assert!(projection.advance(&state));
        assert!(matches!(
            projection.roots[1],
            RootToken::Placement { index: 1, depth: 1 }
        ));
    }

    #[test]
    fn large_projection_is_bounded_and_has_no_partial_result() {
        let state = state(8192);
        let mut projection = RootProjection::new("");
        assert!(!projection.advance(&state));
        assert_eq!(projection.cursor, 256);
        assert!(projection.roots.is_empty());
        for _ in 0..100 {
            if projection.advance(&state) {
                break;
            }
        }
        assert!(projection.complete);
        assert_eq!(projection.roots.len(), 8193);
    }
}

pub(super) struct RootProjection {
    pub(super) linear: bool,
    cursor: usize,
    groups: BTreeMap<String, Vec<usize>>,
    current: Option<(String, std::vec::IntoIter<usize>)>,
    pub(super) roots: Vec<RootToken>,
    query: String,
    captured: bool,
    complete: bool,
}

impl RootProjection {
    pub(super) fn new(query: &str) -> Self {
        Self {
            linear: false,
            cursor: 0,
            groups: BTreeMap::new(),
            current: None,
            roots: Vec::new(),
            query: query.trim().to_lowercase(),
            captured: false,
            complete: false,
        }
    }

    pub(super) fn advance(&mut self, state: &EditorGltfBrowserState) -> bool {
        if self.complete {
            return true;
        }
        if self.query.is_empty() && !state.has_object_groups {
            self.linear = true;
            self.complete = true;
            return true; // 无目录且无搜索时直接沿用旧根顺序和每帧256行预算。
        }
        if !self.captured {
            let end = (self.cursor + 256).min(state.signature.len());
            let start = self.cursor;
            let mut bytes = 0;
            while self.cursor < end {
                let p = &state.signature[self.cursor];
                bytes += p.editor.name.len()
                    + p.editor.group.len()
                    + p.asset_path.len()
                    + p.placement_id.len();
                if bytes > 128 * 1024 && self.cursor > start {
                    break;
                }
                let matches = self.query.is_empty()
                    || [
                        &p.editor.name,
                        &p.editor.group,
                        &p.placement_id,
                        &p.asset_path,
                    ]
                    .into_iter()
                    .any(|text| text.to_lowercase().contains(&self.query));
                if matches {
                    self.groups
                        .entry(p.editor.group.clone())
                        .or_default()
                        .push(self.cursor);
                }
                self.cursor += 1;
            }
            self.captured = self.cursor == state.signature.len();
            if !self.captured {
                return false;
            }
        }
        // 目录、根和折叠省略均按256步限流，搜索期间主动展开匹配目录。
        for _ in 0..256 {
            if let Some((name, indices)) = &mut self.current {
                if let Some(index) = indices.next() {
                    self.roots.push(RootToken::Placement {
                        index,
                        depth: usize::from(!name.is_empty()),
                    });
                    continue;
                }
                self.current = None;
            }
            let Some((name, indices)) = self.groups.pop_first() else {
                self.complete = true;
                return true;
            };
            let collapsed = self.query.is_empty() && state.collapsed_groups.contains(&name);
            if !name.is_empty() {
                self.roots.push(RootToken::Group {
                    name: name.clone(),
                    count: indices.len(),
                    collapsed,
                });
            }
            if !collapsed {
                self.current = Some((name, indices.into_iter()));
            }
        }
        false
    }

    /// 旧投影含最多8192条小型索引，取消后随原行草稿逐项释放。
    pub(super) fn retire_one(&mut self) -> bool {
        if self.roots.pop().is_some() {
            return false;
        }
        if let Some((_, indices)) = &mut self.current {
            if indices.next().is_some() {
                return false;
            }
            self.current = None;
        }
        if let Some((_, indices)) = self.groups.first_key_value() {
            if !indices.is_empty() {
                self.groups.first_entry().unwrap().get_mut().pop();
                return false;
            }
            self.groups.pop_first();
            return false;
        }
        true
    }
}
