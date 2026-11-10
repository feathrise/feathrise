//! 场景目录的会话多选、可见顺序连选与单个焦点兼容；完整范围构造只在后台执行。

use std::{collections::BTreeSet, sync::Arc};

use bevy::{prelude::*, tasks::AsyncComputeTaskPool};

use crate::{
    resources::{SceneBrowserIndexState, SceneBrowserState},
    types::{SceneBrowserItem, SceneBrowserSelection, SceneBrowserSelectionWork},
};

impl SceneBrowserState {
    /// 旧入口仍可直接改selected_item；其焦点不同于多选快照时立即退回兼容单选。
    pub(crate) fn selection_count(&self) -> usize {
        if self.selection.focus == self.selected_item {
            self.selection.items.len()
        } else {
            usize::from(self.selected_item.is_some())
        }
    }

    pub(crate) fn item_selected(&self, item: &SceneBrowserItem) -> bool {
        if self.selection.focus == self.selected_item {
            self.selection.items.contains(item)
        } else {
            self.selected_item.as_ref() == Some(item)
        }
    }

    pub(crate) fn select_only(&mut self, item: Option<SceneBrowserItem>) {
        let mut selection = SceneBrowserSelection::default();
        selection.anchor = item.clone();
        selection.focus = item.clone();
        selection.revision = self.selection.revision.wrapping_add(1);
        if let Some(item) = item.as_ref() {
            Arc::make_mut(&mut selection.items).insert(item.clone());
            selection.scenes = usize::from(matches!(item, SceneBrowserItem::Scene(_)));
            selection.folders = usize::from(matches!(item, SceneBrowserItem::Folder(_)));
        }
        self.selected_item = item;
        self.selection = selection;
    }

    pub(crate) fn toggle_selection(&mut self, item: SceneBrowserItem) {
        if self.selection.focus != self.selected_item {
            self.select_only(self.selected_item.clone());
        }
        if self.selection.items.contains(&SceneBrowserItem::Root) {
            self.select_only(None); // 固定根目录只是落点和创建上下文，不参与场景/文件夹混选。
        }
        // 调用前释放旧press；活动拖拽和后台连选期间拒绝新选择，正常Ctrl修改保持唯一所有权。
        let removed = Arc::make_mut(&mut self.selection.items).remove(&item);
        if !removed {
            Arc::make_mut(&mut self.selection.items).insert(item.clone());
        }
        let delta = if removed { -1_isize } else { 1 };
        match item {
            SceneBrowserItem::Scene(_) => {
                self.selection.scenes = self.selection.scenes.saturating_add_signed(delta)
            }
            SceneBrowserItem::Folder(_) => {
                self.selection.folders = self.selection.folders.saturating_add_signed(delta)
            }
            SceneBrowserItem::Root => {}
        }
        self.selected_item = if removed {
            self.selection.items.last().cloned()
        } else {
            Some(item.clone())
        };
        self.selection.focus = self.selected_item.clone();
        self.selection.anchor = Some(item);
        self.selection.revision = self.selection.revision.wrapping_add(1);
    }
}

/// 范围严格使用当前可见投影，折叠目录的隐藏后代不因Shift被额外选中。
pub(super) fn range_selection(
    entries: &[crate::types::SceneBrowserEntry],
    mut selection: SceneBrowserSelection,
    target: SceneBrowserItem,
    additive: bool,
) -> SceneBrowserSelection {
    let target_index = entries.iter().position(|entry| entry.item == target);
    let anchor = selection
        .anchor
        .as_ref()
        .and_then(|anchor| entries.iter().position(|entry| &entry.item == anchor));
    let mut items = if additive {
        (*selection.items).clone()
    } else {
        BTreeSet::new()
    };
    if let Some(end) = target_index {
        let start = anchor.unwrap_or(end);
        for entry in &entries[start.min(end)..=start.max(end)] {
            if entry.item != SceneBrowserItem::Root {
                items.insert(entry.item.clone());
            }
        }
    }
    // 锚点被折叠或热更移走时，本次从目标重新起算，后续Shift仍有可见锚点。
    if anchor.is_none() {
        selection.anchor = Some(target.clone());
    }
    selection.scenes = items
        .iter()
        .filter(|item| matches!(item, SceneBrowserItem::Scene(_)))
        .count();
    selection.folders = items.len() - selection.scenes;
    selection.items = Arc::new(items);
    selection.focus = Some(target);
    selection.revision = selection.revision.wrapping_add(1);
    selection
}

pub(super) fn begin_range(
    browser: &mut SceneBrowserState,
    index: &SceneBrowserIndexState,
    work: &mut SceneBrowserSelectionWork,
    target: SceneBrowserItem,
    additive: bool,
    start_physical: Option<Vec2>,
) {
    if browser.selection.focus != browser.selected_item {
        browser.select_only(browser.selected_item.clone());
    }
    let entries = Arc::clone(&index.entries);
    let selection = browser.selection.clone();
    work.projection = index.applied_projection.clone();
    work.start_physical = start_physical;
    work.cancelled = false;
    work.task = Some(
        AsyncComputeTaskPool::get()
            .spawn(async move { range_selection(&entries, selection, target, additive) }),
    );
    browser.selection_pending = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SceneBrowserEntry;

    fn map(id: &str) -> SceneBrowserItem {
        SceneBrowserItem::Scene(feathrise_scene_content::SceneId::parse(id).unwrap())
    }

    fn entries() -> Vec<SceneBrowserEntry> {
        [
            map("MAP-1"),
            SceneBrowserItem::Folder("F".into()),
            map("MAP-2"),
            map("MAP-3"),
        ]
        .into_iter()
        .map(|item| SceneBrowserEntry {
            item,
            display_name: "测试".into(),
            depth: 1,
            published: false,
            parent_folder_id: None,
        })
        .collect()
    }

    #[test]
    fn ctrl_toggles_mixed_items_and_never_changes_the_current_scene() {
        let mut browser = SceneBrowserState::default();
        browser.select_only(Some(map("MAP-1")));
        browser.toggle_selection(SceneBrowserItem::Folder("F".into()));
        browser.toggle_selection(map("MAP-2"));
        assert_eq!(browser.selection_count(), 3);
        assert_eq!((browser.selection.scenes, browser.selection.folders), (2, 1));
        browser.toggle_selection(map("MAP-1"));
        assert!(!browser.item_selected(&map("MAP-1")));
        browser.toggle_selection(map("MAP-2"));
        browser.toggle_selection(SceneBrowserItem::Folder("F".into()));
        assert_eq!(browser.selection_count(), 0);
        assert!(browser.selected_item.is_none());
    }

    #[test]
    fn shift_uses_visible_order_in_both_directions_and_ctrl_shift_adds() {
        let mut browser = SceneBrowserState::default();
        browser.select_only(Some(map("MAP-3")));
        let range = range_selection(
            &entries(),
            browser.selection,
            SceneBrowserItem::Folder("F".into()),
            false,
        );
        assert_eq!(range.items.len(), 3);
        assert!(!range.items.contains(&map("MAP-1")));
        assert_eq!(range.anchor, Some(map("MAP-3")));
        let range = range_selection(&entries(), range, map("MAP-1"), true);
        assert_eq!((range.scenes, range.folders), (3, 1));
    }

    #[test]
    fn hidden_anchor_restarts_at_target_and_legacy_focus_invalidates_old_group() {
        let mut browser = SceneBrowserState::default();
        browser.select_only(Some(map("MAP-hidden")));
        browser.selection = range_selection(&entries(), browser.selection, map("MAP-2"), false);
        browser.selected_item = Some(map("MAP-2"));
        assert_eq!(browser.selection_count(), 1);
        assert_eq!(browser.selection.anchor, Some(map("MAP-2")));
        browser.toggle_selection(map("MAP-3"));
        browser.selected_item = Some(map("MAP-created"));
        assert_eq!(browser.selection_count(), 1);
        assert!(!browser.item_selected(&map("MAP-2")));
        assert!(browser.item_selected(&map("MAP-created")));
    }
}
