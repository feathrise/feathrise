//! 场景文件夹的渐进投影。折叠分支仍计算完整的成员摘要。

use super::EditorGltfBrowserState;
use std::collections::BTreeMap;

pub(super) enum RootToken {
    Group {
        name: String,
        first_member: Option<usize>,
        depth: usize,
        count: usize,
        collapsed: bool,
        expandable: bool,
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
    use crate::authoring::placement_folders::PlacementFolders;
    use std::sync::Arc;

    fn state(count: usize) -> EditorGltfBrowserState {
        EditorGltfBrowserState {
            placement_group_names: ["村口".into()].into(),
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
            matches!(&projection.roots[0], RootToken::Group { name, collapsed: true, count: 3, first_member: Some(0), .. } if name == "村口")
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
        let mut folders = PlacementFolders::default();
        folders.insert("空的文件夹".into());
        let mut folders = Arc::new(folders);
        let mut state = EditorGltfBrowserState {
            main_glb_path: Some("scenes/test/scene.glb".into()),
            object_folders: std::sync::Arc::downgrade(&folders),
            ..Default::default()
        };
        let mut projection = RootProjection::default();
        assert!(projection.advance(&state));
        assert_eq!(projection.main_glb_root_count(), 0);
        assert!(matches!(projection.root(0), Some(RootToken::Group {
            name, count: 0, first_member: None, expandable: false, ..
        }) if name == "空的文件夹"));

        let child = folders.prepare_move("新建子文件夹", Some("空的文件夹")).unwrap().unwrap();
        child.apply(&mut folders, false).unwrap();
        state.object_folders = Arc::downgrade(&folders);
        state.expanded_groups.insert("空的文件夹".into());
        let mut projection = RootProjection::default();
        assert!(projection.advance(&state));
        assert!(matches!(projection.root(0), Some(RootToken::Group { count: 0, expandable: true, .. })));
        assert!(matches!(projection.root(1), Some(RootToken::Group { name, depth: 1, expandable: false, .. })
            if name == "新建子文件夹"));
    }

    #[test]
    fn nested_folders_keep_their_members_and_fold_as_a_branch() {
        let mut state = state(3);
        let mut folders = Arc::new(PlacementFolders::default());
        for (child, parent) in [("村口", "街区"), ("街区", "城镇")] {
            let edit = folders.prepare_move(child, Some(parent)).unwrap().unwrap();
            edit.apply(&mut folders, false).unwrap();
        }
        state.object_folders = Arc::downgrade(&folders);
        state.expanded_groups.extend(["城镇".to_owned(), "街区".to_owned()]);
        let mut projection = RootProjection::default();
        assert!(projection.advance(&state));
        for (depth, expected) in ["城镇", "街区", "村口"].into_iter().enumerate() {
            assert!(matches!(projection.root(depth), Some(RootToken::Group {
                name, count: 3, first_member: Some(0), depth: actual, ..
            }) if name == expected && *actual == depth));
        }
        assert!(projection.root(3).is_none());
        state.expanded_groups.insert("村口".into());
        let mut projection = RootProjection::default();
        assert!(projection.advance(&state));
        assert!(matches!(projection.root(3), Some(RootToken::Placement { index: 0, depth: 3 })));
        assert!(matches!(projection.root(5), Some(RootToken::Placement { index: 2, depth: 3 })));
    }
}

struct GroupMembers {
    all_embedded: bool,
    indices: Vec<usize>,
}

struct FolderVisit {
    name: String,
    row: Option<usize>,
    depth: usize,
    embedded: bool,
    expanded: bool,
    indices: std::vec::IntoIter<usize>,
    first_member: Option<usize>,
    count: usize,
}

#[derive(Default)]
pub(super) struct RootProjection {
    pub(super) linear: bool,
    cursor: usize,
    groups: BTreeMap<String, BTreeMap<String, GroupMembers>>, // 按直属父级归档，空键是根。
    current: Vec<FolderVisit>,
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
        if state.placement_group_names.is_empty() && state.main_glb_path.is_none()
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
                let parent = folders.as_deref().and_then(|folders| folders.parent(group)).unwrap_or_default();
                bytes += group.len() + parent.len();
                if bytes > 128 * 1024 && self.cursor > start {
                    break;
                }
                let embedded = placement.is_embedded_in(state.main_glb_path.as_deref());
                if group.is_empty() {
                    self.push_root(embedded, RootToken::Placement { index, depth: 0 });
                } else {
                    let members = self.groups.entry(parent.to_owned()).or_default().entry(group.clone()).or_insert_with(|| GroupMembers {
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
            let mut names = folders.names_after(self.folder_cursor.as_deref());
            for name in names.by_ref().take(256) {
                // 显式登记的分支属于场景归类层，即使没有直属模型。
                let parent = folders.parent(name).unwrap_or_default();
                self.groups.entry(parent.to_owned()).or_default().entry(name.to_owned()).or_insert_with(|| GroupMembers {
                    all_embedded: false, indices: Vec::new(),
                }).all_embedded = false;
                self.folder_cursor = Some(name.to_owned());
            }
            self.folders_captured = names.next().is_none();
            if !self.folders_captured { return false; }
        }
        // 深度优先，每次只消费一个文件夹边界或一条可见摆放。先列子文件夹，再列直属模型。
        for _ in 0..256 {
            let parent = self.current.last();
            let key = parent.map_or("", |parent| parent.name.as_str());
            if let Some((name, members)) = self.groups.get_mut(key).and_then(BTreeMap::pop_first) {
                let depth = parent.map_or(0, |parent| parent.depth + 1);
                let visible = parent.is_none_or(|parent| parent.expanded);
                let embedded = parent.map_or(members.all_embedded, |parent| parent.embedded);
                let collapsed = !state.expanded_groups.contains(&name);
                // 下一级即使只有空文件夹，也应当可以展开。
                let expandable = !members.indices.is_empty()
                    || self.groups.get(&name).is_some_and(|children| !children.is_empty());
                let row = visible.then(|| self.push_root(embedded, RootToken::Group {
                    name: name.clone(), depth, collapsed, expandable, first_member: None, count: 0,
                }));
                self.current.push(FolderVisit {
                    name, row, depth, embedded, expanded: visible && !collapsed,
                    first_member: members.indices.first().copied(), count: members.indices.len(),
                    indices: members.indices.into_iter(),
                });
                continue;
            }
            let Some(folder) = self.current.last_mut() else { self.complete = true; return true; };
            if folder.expanded && let Some(index) = folder.indices.next() {
                let (embedded, depth) = (folder.embedded, folder.depth + 1);
                self.push_root(embedded, RootToken::Placement { index, depth });
                continue;
            }
            let folder = self.current.pop().expect("已确认当前文件夹");
            self.finish_folder(folder);
        }
        false
    }

    fn push_root(&mut self, embedded: bool, token: RootToken) -> usize {
        let roots = if embedded { &mut self.main_roots } else { &mut self.roots };
        let row = roots.len();
        roots.push(token);
        row
    }

    fn finish_folder(&mut self, folder: FolderVisit) {
        // 每条父级边只汇总一次，折叠后的 GLB 仍参与祖先计数和整夹操作。
        if let Some(parent) = self.current.last_mut() {
            parent.count += folder.count;
            if let Some(first) = folder.first_member {
                parent.first_member = Some(parent.first_member.map_or(first, |old| old.min(first)));
            }
        }
        if let Some(row) = folder.row {
            let roots = if folder.embedded { &mut self.main_roots } else { &mut self.roots };
            if let RootToken::Group { count, first_member, .. } = &mut roots[row] {
                *count = folder.count;
                *first_member = folder.first_member;
            }
        }
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
        if let Some(folder) = self.current.last_mut() {
            if folder.indices.next().is_none() { self.current.pop(); }
            return false;
        }
        if let Some(mut children) = self.groups.first_entry() {
            if let Some(mut group) = children.get_mut().first_entry() {
                if group.get_mut().indices.pop().is_none() { group.remove_entry(); }
            } else {
                children.remove_entry();
            }
            return false;
        }
        true
    }
}
