//! 离屏验收只冻结真实目录上下文；复用正式浮层与表现校验，不模拟用户点击或场景业务。

use super::{
    components::{DirectoryMenuBackdrop, DirectoryMenuPopup},
    resources::SceneDirectoryMenuState,
    systems::advance_empty_check,
    types::DirectoryMenuTarget,
};
use crate::{
    resources::{SceneBrowserIndexState, SceneBrowserState, SceneEditorState},
    types::SceneBrowserItem,
};
use bevy::prelude::*;
use feathrise_scene_content::SceneContentRegistry;

#[derive(Resource, Default)]
pub(crate) struct DirectoryMenuSnapshot {
    pub active: bool,
    pub anchor: Vec2,
}

pub(crate) fn install_snapshot(app: &mut App) {
    app.init_resource::<DirectoryMenuSnapshot>().add_systems(
        Update,
        freeze_current_selection
            .before(super::super::systems::sync_sidebar)
            .in_set(crate::SceneEditorSystemSet::Presentation),
    );
}

fn freeze_current_selection(
    request: Res<DirectoryMenuSnapshot>,
    browser: Res<SceneBrowserState>,
    index: Res<SceneBrowserIndexState>,
    editor: Res<SceneEditorState>,
    registry: Res<SceneContentRegistry>,
    mut state: ResMut<SceneDirectoryMenuState>,
) {
    if !request.active {
        state.target = None;
        return;
    }
    let projection = index
        .applied_projection
        .as_ref()
        .expect("截图需要真实已采用目录");
    assert_eq!(index.desired_projection.as_ref(), Some(projection));
    assert_eq!(projection.0.folder_revision, browser.document_revision);
    assert_eq!(projection.0.catalog_revision, registry.revision);
    assert_eq!(projection.1, browser.expansion_revision);
    assert!(
        browser
            .selected_item
            .as_ref()
            .is_some_and(|item| index.entries.iter().any(|entry| &entry.item == item)),
        "截图所选项必须来自当前真实可见目录"
    );
    let mut target = DirectoryMenuTarget {
        selected: browser.selected_item.clone(),
        projection: projection.clone(),
        scene_id: editor.selected_scene_id.clone(),
        folder_empty: None,
        scan_cursor: 0,
    };
    advance_empty_check(&mut target, &index);
    state.target = Some(target);
    state.anchor = request.anchor;
    state.captured_this_frame = true;
}

pub(crate) fn assert_snapshot_bounds(world: &mut World, root: Entity, selected: &SceneBrowserItem) {
    let state = world.resource::<SceneDirectoryMenuState>();
    assert_eq!(
        state
            .target
            .as_ref()
            .and_then(|target| target.selected.as_ref()),
        Some(selected),
        "正式sync必须保留真实来源的菜单，而不是输出已关闭的伪截图"
    );
    let bounds = |world: &World, entity| {
        Rect::from_center_size(
            world.get::<UiGlobalTransform>(entity).unwrap().translation,
            world.get::<ComputedNode>(entity).unwrap().size(),
        )
    };
    let root_bounds = bounds(world, root);
    let popup = world
        .query_filtered::<Entity, With<DirectoryMenuPopup>>()
        .single(world)
        .unwrap();
    let backdrop = world
        .query_filtered::<Entity, With<DirectoryMenuBackdrop>>()
        .single(world)
        .unwrap();
    assert_eq!(world.get::<Node>(backdrop).unwrap().display, Display::Flex);
    let popup_bounds = bounds(world, popup);
    assert!(popup_bounds.width() > 100.0 && popup_bounds.height() > 60.0);
    assert!(
        popup_bounds.min.cmpge(root_bounds.min - Vec2::ONE).all()
            && popup_bounds.max.cmple(root_bounds.max + Vec2::ONE).all(),
        "场景目录右键菜单不能超出编辑器根：{popup_bounds:?} / {root_bounds:?}"
    );
}
