//! 单层逻辑目录的渐进投影；只重排树行，不改变实例和源模型。

use super::EditorGltfBrowserState;
use std::collections::BTreeMap;

pub(super) enum RootToken {
    Group {
        name: String,
        first_member: Option<usize>,
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
    use std::collections::BTreeSet;
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
        let mut projection = RootProjection::default();
        assert!(projection.advance(&state));
        assert_eq!(projection.roots.len(), 1);
        assert!(
            matches!(&projection.roots[0], RootToken::Group { name, collapsed: true, count: 3, first_member: Some(0) } if name == "村口")
        );
        state.expanded_groups.insert("村口".into());
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
        let mut state = state(8192);
        state.expanded_groups.insert("村口".into());
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

    #[test]
    fn declared_empty_folder_stays_at_the_outer_root() {
        let folders = std::sync::Arc::new(BTreeSet::from(["空的文件夹".into()]));
        let state = EditorGltfBrowserState {
            main_glb_path: Some("scenes/test/scene.glb".into()),
            object_folders: std::sync::Arc::downgrade(&folders),
            ..Default::default()
        };
        let mut projection = RootProjection::default();
        assert!(projection.advance(&state));
        assert_eq!(projection.main_glb_root_count(), 0);
        assert!(matches!(projection.root(0), Some(RootToken::Group { name, count: 0, first_member: None, .. }) if name == "空的文件夹"));
    }
}

struct GroupMembers {
    all_embedded: bool,
    indices: Vec<usize>,
}

#[derive(Default)]
pub(super) struct RootProjection {
    pub(super) linear: bool,
    cursor: usize,
    groups: BTreeMap<String, GroupMembers>,
    current: Option<(bool, std::vec::IntoIter<usize>)>,
    main_roots: Vec<RootToken>,
    roots: Vec<RootToken>,
    captured: bool,
    folder_cursor: Option<String>,
    folders_captured: bool,
    complete: bool,
}

impl RootProjection {
    pub(super) fn advance(&mut self, state: &EditorGltfBrowserState) -> bool {
        if self.complete {
            return true;
        }
        let folders = state.object_folders.upgrade();
        if !state.has_object_groups && state.main_glb_path.is_none()
            && folders.as_ref().is_none_or(|folders| folders.is_empty()) {
            self.linear = true;
            self.complete = true;
            return true; // 无目录时按摆放顺序生成树行，沿用每帧256行预算。
        }
        if !self.captured {
            let end = (self.cursor + 256).min(state.signature.len());
            let start = self.cursor;
            let mut bytes = 0;
            while self.cursor < end {
                let index = self.cursor;
                let placement = &state.signature[index];
                let group = &placement.editor.group;
                bytes += group.len();
                if bytes > 128 * 1024 && self.cursor > start {
                    break;
                }
                let embedded = placement.is_embedded_in(state.main_glb_path.as_deref());
                if group.is_empty() {
                    self.push_root(embedded, RootToken::Placement { index, depth: 0 });
                } else {
                    let members = self.groups.entry(group.clone()).or_insert_with(|| GroupMembers {
                        all_embedded: embedded,
                        indices: Vec::new(),
                    });
                    // 跨来源仍用同一个逻辑组，保持原组命令的成员范围。
                    members.all_embedded &= embedded;
                    members.indices.push(index);
                }
                self.cursor += 1;
            }
            self.captured = self.cursor == state.signature.len();
            if !self.captured {
                return false;
            }
        }
        if !self.folders_captured && let Some(folders) = folders {
            use std::ops::Bound::{Excluded, Unbounded};
            let start = self.folder_cursor.as_deref().map_or(Unbounded, Excluded);
            let mut names = folders.range::<str, _>((start, Unbounded));
            for name in names.by_ref().take(256) {
                // 旧成员归属可与新目录同名。显式登记的文件夹在外层，即使没有成员。
                self.groups.entry(name.clone()).or_insert_with(|| GroupMembers {
                    all_embedded: false, indices: Vec::new(),
                }).all_embedded = false;
                self.folder_cursor = Some(name.clone());
            }
            self.folders_captured = names.next().is_none();
            if !self.folders_captured { return false; }
        }
        // 目录、根和折叠省略均按256步限流。
        for _ in 0..256 {
            if let Some((embedded, indices)) = &mut self.current {
                if let Some(index) = indices.next() {
                    let embedded = *embedded;
                    self.push_root(embedded, RootToken::Placement { index, depth: 1 });
                    continue;
                }
                self.current = None;
            }
            let Some((name, GroupMembers { all_embedded, indices })) = self.groups.pop_first() else {
                self.complete = true;
                return true;
            };
            let collapsed = !state.expanded_groups.contains(&name);
            self.push_root(all_embedded, RootToken::Group {
                name,
                first_member: indices.first().copied(), // 空文件夹不伪造对象操作或首成员。
                count: indices.len(),
                collapsed,
            });
            if !collapsed {
                self.current = Some((all_embedded, indices.into_iter()));
            }
        }
        false
    }

    fn push_root(&mut self, embedded: bool, token: RootToken) {
        let roots = if embedded { &mut self.main_roots } else { &mut self.roots };
        roots.push(token);
    }

    pub(super) fn main_glb_root_count(&self) -> usize {
        self.main_roots.len()
    }

    pub(super) fn root(&self, index: usize) -> Option<&RootToken> {
        self.main_roots.get(index).or_else(|| self.roots.get(index - self.main_roots.len()))
    }

    /// 投影索引随原行草稿逐项释放。
    pub(super) fn retire_one(&mut self) -> bool {
        if self.roots.pop().is_some() || self.main_roots.pop().is_some() {
            return false;
        }
        if let Some((_, indices)) = &mut self.current {
            if indices.next().is_some() {
                return false;
            }
            self.current = None;
        }
        if let Some(mut group) = self.groups.first_entry() {
            if group.get_mut().indices.pop().is_some() {
                return false;
            }
            group.remove_entry();
            return false;
        }
        true
    }
}
