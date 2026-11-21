//! 单层逻辑目录的渐进投影；只重排树行，不改变实例和源模型。

use super::EditorGltfBrowserState;
use std::collections::BTreeMap;

pub(super) enum RootToken {
    Group {
        name: String,
        first_member: usize,
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
    fn folders_collapse_and_expand() {
        let mut state = state(3);
        state.collapsed_groups.insert("村口".into());
        let mut projection = RootProjection::default();
        assert!(projection.advance(&state));
        assert_eq!(projection.roots.len(), 1);
        assert!(
            matches!(&projection.roots[0], RootToken::Group { name, collapsed: true, count: 3, first_member: 0 } if name == "村口")
        );
        state.collapsed_groups.clear();
        let mut projection = RootProjection::default();
        assert!(projection.advance(&state));
        assert_eq!(projection.roots.len(), 4);
        assert!(matches!(
            projection.roots[1],
            RootToken::Placement { index: 0, depth: 1 }
        ));
    }

    #[test]
    fn large_projection_is_bounded_and_has_no_partial_result() {
        let state = state(8192);
        let mut projection = RootProjection::default();
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

#[derive(Default)]
pub(super) struct RootProjection {
    pub(super) linear: bool,
    cursor: usize,
    groups: BTreeMap<String, Vec<usize>>,
    current: Option<(String, std::vec::IntoIter<usize>)>,
    pub(super) roots: Vec<RootToken>,
    captured: bool,
    complete: bool,
}

impl RootProjection {
    pub(super) fn advance(&mut self, state: &EditorGltfBrowserState) -> bool {
        if self.complete {
            return true;
        }
        if !state.has_object_groups {
            self.linear = true;
            self.complete = true;
            return true; // 无目录时按摆放顺序生成树行，沿用每帧256行预算。
        }
        if !self.captured {
            let end = (self.cursor + 256).min(state.signature.len());
            let start = self.cursor;
            let mut bytes = 0;
            while self.cursor < end {
                let group = &state.signature[self.cursor].editor.group;
                bytes += group.len();
                if bytes > 128 * 1024 && self.cursor > start {
                    break;
                }
                self.groups.entry(group.clone()).or_default().push(self.cursor);
                self.cursor += 1;
            }
            self.captured = self.cursor == state.signature.len();
            if !self.captured {
                return false;
            }
        }
        // 目录、根和折叠省略均按256步限流。
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
            let collapsed = state.collapsed_groups.contains(&name);
            if !name.is_empty() {
                self.roots.push(RootToken::Group {
                    name: name.clone(),
                    first_member: indices[0], // 组开关沿用首成员的状态来决定整组切换方向。
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

    /// 投影索引随原行草稿逐项释放。
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
