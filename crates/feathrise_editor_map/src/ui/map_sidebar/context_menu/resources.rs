//! 目录右键菜单只持有一个临时目标及一次等待静默帧的原按钮请求。

use super::types::{DirectoryMenuRequest, DirectoryMenuTarget};
use crate::types::{EditorAction, MapBrowserItem};
use bevy::prelude::*;

#[derive(Resource, Default)]
pub(crate) struct MapDirectoryMenuState {
    pub(super) target: Option<DirectoryMenuTarget>,
    pub(super) queued: Option<DirectoryMenuRequest>,
    pub(super) dispatched: Option<DirectoryMenuRequest>,
    pub(super) anchor: Vec2,
    pub(super) captured_this_frame: bool,
}

impl MapDirectoryMenuState {
    pub(crate) fn owns_mouse(&self) -> bool {
        self.target.is_some() || self.captured_this_frame
    }

    pub(in crate::ui::map_sidebar) fn binding_target(&self) -> Option<&Option<MapBrowserItem>> {
        self.target
            .as_ref()
            .map(|target| &target.selected)
            .or_else(|| self.queued.as_ref().map(|request| &request.target.selected))
            .or_else(|| {
                self.dispatched
                    .as_ref()
                    .map(|request| &request.target.selected)
            })
    }

    pub(in crate::ui::map_sidebar) fn permits(&self, entity: Entity) -> bool {
        self.dispatched
            .as_ref()
            .is_some_and(|request| request.entity == entity)
    }

    pub(in crate::ui::map_sidebar) fn action_visible(&self, action: EditorAction) -> bool {
        let target = self
            .target
            .as_ref()
            .or_else(|| self.queued.as_ref().map(|request| &request.target))
            .or_else(|| self.dispatched.as_ref().map(|request| &request.target));
        target.is_some_and(|target| match action {
            EditorAction::EnterSelectedMap => {
                matches!(target.selected, Some(MapBrowserItem::Map(_)))
            }
            EditorAction::MapFolderRename => matches!(
                target.selected,
                Some(MapBrowserItem::Map(_) | MapBrowserItem::Folder(_))
            ),
            EditorAction::DeleteMap => {
                matches!(target.selected, Some(MapBrowserItem::Map(_)))
                    || matches!(target.selected, Some(MapBrowserItem::Folder(_)))
                        && target.folder_empty == Some(true)
            }
            EditorAction::NewMap | EditorAction::MapFolderCreate => true,
            _ => false,
        })
    }
}
