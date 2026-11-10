//! 右键冻结目录目标，菜单项释放后经过原静默门，转发同一按钮的一次动作脉冲。

use feathrise_editor_ui::UiInteraction;
use super::{resources::SceneDirectoryMenuState, types::*};
use crate::components::{
    SceneEditorActionButton, SceneEditorSceneRootRow, SceneEditorSceneRow, SceneEditorSceneTreeScroll,
    SceneEditorRoot,
};
use crate::resources::{
    SceneBrowserDragState, SceneBrowserIndexState, SceneBrowserState, SceneCreationTask, SceneDeletionTask,
    SceneEditorLayoutState, SceneEditorNumericScrubMouseGuard, SceneEditorSettingsState, SceneEditorState,
    SceneFolderPersistence,
};
use crate::types::SceneBrowserItem;
use crate::ui::scene_sidebar::SceneSidebarActionBinding;
use bevy::{
    ecs::system::SystemParam, input_focus::InputFocus, prelude::*, ui::RelativeCursorPosition,
    window::PrimaryWindow,
};
use feathrise_editor_ui::{ActiveNumericInput, ActiveNumericInputScrub, EditorButtonClick};
use feathrise_scene_content::SceneContentRegistry;

#[derive(SystemParam)]
pub(super) struct MenuContext<'w, 's> {
    sidebar: Option<Res<'w, crate::ui::sidebars::SceneEditorSidebarState>>,
    layout: Option<Res<'w, SceneEditorLayoutState>>,
    settings: Option<Res<'w, SceneEditorSettingsState>>,
    conflict: Option<Res<'w, crate::authoring::TerrainExternalChangeState>>,
    active: Option<Res<'w, ActiveNumericInput>>,
    scrub: Option<Res<'w, ActiveNumericInputScrub>>,
    session: Option<Res<'w, crate::authoring::EditorAuthoringSession>>,
    drag: Res<'w, SceneBrowserDragState>,
    deletion: Res<'w, SceneDeletionTask>,
    persistence: Res<'w, SceneFolderPersistence>,
    creation: Res<'w, SceneCreationTask>,
    registry: Option<Res<'w, SceneContentRegistry>>,
    asset_menu: Option<Res<'w, crate::ui::asset_context_menu::AssetContextMenuState>>,
    scene_menu: Option<Res<'w, crate::ui::scene_context_menu::SceneContextMenuState>>,
    tab_menu: Option<Res<'w, crate::ui::asset_preview::AssetTabMenuState>>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    roots: Query<'w, 's, (&'static ComputedNode, &'static UiGlobalTransform), With<SceneEditorRoot>>,
}

impl MenuContext<'_, '_> {
    fn enabled(&self, editor: &SceneEditorState) -> bool {
        editor.open
            && !editor.close_requested
            && !editor.new_scene_dialog_open
            && !editor.scene_transition_in_progress
            && editor.pending_scene_transition.is_none()
            && editor.scene_switch_confirmation.is_none()
            && !editor.save_requested
            && self.session.as_ref().is_none_or(|session| !session.saving)
            && self
                .sidebar
                .as_ref()
                .is_none_or(|sidebar| sidebar.scene_browser_visible())
            && self.layout.as_ref().is_none_or(|layout| {
                layout.content_shown() && layout.drag.is_none() && !layout.owns_mouse_this_frame
            })
            && self
                .settings
                .as_ref()
                .is_none_or(|settings| !settings.open && !settings.owns_mouse_this_frame)
            && self
                .conflict
                .as_ref()
                .is_none_or(|conflict| !conflict.modal_open())
            && self
                .active
                .as_ref()
                .is_none_or(|active| active.entity.is_none())
            && self
                .scrub
                .as_ref()
                .is_none_or(|scrub| scrub.entity.is_none())
            && self
                .asset_menu
                .as_ref()
                .is_none_or(|menu| !menu.owns_mouse())
            && self
                .scene_menu
                .as_ref()
                .is_none_or(|menu| !menu.owns_mouse())
            && self.tab_menu.as_ref().is_none_or(|menu| !menu.owns_mouse())
            && self.windows.single().is_ok_and(|window| window.focused)
    }
}

pub(super) fn target_current(
    target: &DirectoryMenuTarget,
    browser: &SceneBrowserState,
    index: &SceneBrowserIndexState,
    editor: &SceneEditorState,
    catalog_revision: u64,
) -> bool {
    browser.selected_item == target.selected
        && browser.selection_count() <= 1
        && browser.document_revision == target.projection.0.folder_revision
        && browser.expansion_revision == target.projection.1
        && catalog_revision == target.projection.0.catalog_revision
        && editor.selected_scene_id == target.scene_id
        && !index.rebuilding
        && index.applied_projection.as_ref() == Some(&target.projection)
        && index.desired_projection == index.applied_projection
        && index.applied_signature.as_ref() == Some(&target.projection.0)
}

pub(super) fn advance_empty_check(target: &mut DirectoryMenuTarget, index: &SceneBrowserIndexState) {
    if !matches!(target.selected, Some(SceneBrowserItem::Folder(_))) || target.folder_empty.is_some()
    {
        return;
    }
    let end = target
        .scan_cursor
        .saturating_add(64)
        .min(index.full_entries.len());
    for position in target.scan_cursor..end {
        let entry = &index.full_entries[position];
        if Some(&entry.item) == target.selected.as_ref() {
            target.folder_empty = Some(
                index
                    .full_entries
                    .get(position + 1)
                    .is_none_or(|next| next.depth <= entry.depth),
            );
            break;
        }
    }
    target.scan_cursor = end;
}

/// 所有行命中只检查28个固定槽；目录数量只影响分帧空目录定位，绝不逐帧全量扫描。
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn control_menu(
    context: MenuContext,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    keyboard: Option<Res<ButtonInput<KeyCode>>>,
    guard: Res<SceneEditorNumericScrubMouseGuard>,
    mut editor: ResMut<SceneEditorState>,
    mut browser: ResMut<SceneBrowserState>,
    index: Res<SceneBrowserIndexState>,
    rows: Query<(&UiInteraction, &SceneEditorSceneRow, &Node), Without<SceneSidebarActionBinding>>,
    roots: Query<
        (&UiInteraction, &Node),
        (With<SceneEditorSceneRootRow>, Without<SceneSidebarActionBinding>),
    >,
    scroll: Query<(&RelativeCursorPosition, &ScrollPosition), With<SceneEditorSceneTreeScroll>>,
    mut buttons: Query<(
        Entity,
        &SceneEditorActionButton,
        &mut UiInteraction,
        &mut EditorButtonClick,
        &Node,
        &mut SceneSidebarActionBinding,
    )>,
    mut menu: ResMut<SceneDirectoryMenuState>,
    mut focus: Option<ResMut<InputFocus>>,
) {
    // 静默桥转发的动作只存活一帧；实体身份和官方输入树都不重建。
    if let Some(previous) = menu.dispatched.take()
        && let Ok((_, _, mut interaction, mut click, _, mut binding)) = buttons.get_mut(previous.entity)
    {
        interaction.set_if_neq(UiInteraction::None);
        click.invalidate();
        binding.dispatched = false;
    }
    let had_menu = menu.target.is_some();
    menu.captured_this_frame = had_menu;
    let Some(mouse) = mouse.as_deref() else {
        menu.target = None;
        menu.queued = None;
        return;
    };
    let revision = context
        .registry
        .as_ref()
        .map_or(0, |registry| registry.revision);
    let enabled = context.enabled(&editor)
        && browser.initial_load_resolved
        && browser.dialog.is_none()
        && !browser.selection_pending
        && !context.drag.blocks_other_mouse()
        && context.drag.press.is_none()
        && !context.deletion.busy()
        && !context.persistence.busy()
        && context.creation.task.is_none()
        && context.creation.pending_after_save.is_none();
    if keyboard
        .as_ref()
        .is_some_and(|keys| keys.just_pressed(KeyCode::F12))
    {
        menu.target = None;
        menu.queued = None;
        menu.captured_this_frame = false; // F12继续交给原安全关闭门，不要求用户再按一次。
        return;
    }
    if !enabled {
        menu.target = None;
        menu.queued = None;
        return;
    }
    if keyboard
        .as_ref()
        .is_some_and(|keys| keys.just_pressed(KeyCode::Escape))
    {
        menu.captured_this_frame |= menu.queued.is_some();
        menu.target = None;
        menu.queued = None;
        return;
    }
    if let Some(request) = menu.queued.as_ref() {
        if [MouseButton::Left, MouseButton::Right, MouseButton::Middle]
            .into_iter()
            .any(|button| mouse.just_pressed(button))
        {
            menu.queued = None;
            menu.captured_this_frame = true;
            return; // 等待静默期间的新手势取消旧意图，不能在稍后补执行另一次操作。
        }
        if !target_current(&request.target, &browser, &index, &editor, revision) {
            menu.queued = None;
            return;
        }
        let quiet = !guard.blocked()
            && !mouse.any_pressed([MouseButton::Left, MouseButton::Right, MouseButton::Middle])
            && ![MouseButton::Left, MouseButton::Right, MouseButton::Middle]
                .into_iter()
                .any(|button| mouse.just_released(button));
        if quiet {
            let request = menu.queued.take().unwrap();
            if let Ok((_, action, mut interaction, mut click, _, mut binding)) =
                buttons.get_mut(request.entity)
                && action.0 == request.action
                && binding.enabled
                && binding.selected == request.target.selected
            {
                binding.dispatched = true;
                interaction.set_if_neq(UiInteraction::None);
                *click = EditorButtonClick::triggered(); // 已确认释放且身份有效的队列转发，不伪造鼠标按下。
                menu.dispatched = Some(request);
            }
        }
        return;
    }
    if had_menu {
        if !menu
            .target
            .as_ref()
            .is_some_and(|target| target_current(target, &browser, &index, &editor, revision))
            || mouse.just_pressed(MouseButton::Right)
            || mouse.just_pressed(MouseButton::Middle)
        {
            menu.target = None;
            return;
        }
        advance_empty_check(menu.target.as_mut().unwrap(), &index);
        let clicked =
            buttons
                .iter_mut()
                .find_map(|(entity, action, _, click, node, binding)| {
                        (click.is_clicked()
                            && node.display != Display::None
                            && binding.enabled
                            && binding.selected == menu.target.as_ref().unwrap().selected
                            && menu.action_visible(action.0))
                        .then_some((entity, action.0))
                    });
        if let Some((entity, action)) = clicked {
            let target = menu.target.take().unwrap();
                if let Ok((_, _, mut interaction, mut click, _, mut binding)) = buttons.get_mut(entity) {
                    interaction.set_if_neq(UiInteraction::None);
                    click.invalidate();
                    binding.dispatched = false;
                } // 冻结本次释放意图；静默帧产生独立Changed脉冲，不依赖隐藏节点刷新。
                menu.queued = Some(DirectoryMenuRequest {
                    target,
                    entity,
                    action,
                });
        } else if mouse.just_pressed(MouseButton::Left)
            && buttons.iter().all(|(_, _, interaction, _, _, _)| *interaction == UiInteraction::None)
        {
            menu.target = None; // 外部按下只关闭，菜单项按住期间继续等待共享释放确认。
        }
        return;
    }
    if !mouse.just_pressed(MouseButton::Right)
        || guard.blocked()
        || mouse.any_pressed([MouseButton::Left, MouseButton::Middle])
        || [MouseButton::Left, MouseButton::Middle]
            .into_iter()
            .any(|button| mouse.just_released(button))
    {
        return;
    }
    let Some(projection) = index.applied_projection.as_ref().filter(|projection| {
        !index.rebuilding
            && index.desired_projection.as_ref() == Some(*projection)
            && projection.0.folder_revision == browser.document_revision
            && projection.0.catalog_revision == revision
            && projection.1 == browser.expansion_revision
    }) else {
        return;
    };
    let viewport = scroll.single().ok();
    let start = viewport.map_or(0, |(_, scroll)| {
        ((scroll.y.max(0.0) / crate::constants::SCENE_BROWSER_ROW_PITCH).floor() as usize)
            .saturating_sub(1)
    });
    let row_item = rows.iter().find_map(|(interaction, row, node)| {
        (node.display != Display::None && *interaction == UiInteraction::Hovered)
            .then_some(row.item.as_ref())
            .flatten()
            .filter(|item| {
                index
                    .entries
                    .get(start.saturating_add(row.slot))
                    .is_some_and(|entry| &entry.item == *item)
            })
            .cloned()
    });
    let selected = if let Some(item) = row_item {
        Some(item)
    } else if roots.iter().any(|(interaction, node)| {
        *interaction == UiInteraction::Hovered && node.display != Display::None
    }) {
        Some(SceneBrowserItem::Root)
    } else if viewport.is_some_and(|(cursor, _)| cursor.cursor_over()) {
        None
    } else {
        return;
    };
    let Some(cursor) = context
        .windows
        .single()
        .ok()
        .and_then(Window::physical_cursor_position)
    else {
        return;
    };
    let Ok((root, transform)) = context.roots.single() else {
        return;
    };
    let Some(anchor) = crate::ui::asset_context_menu::helpers::logical_anchor(
        cursor,
        Rect::from_center_size(transform.translation, root.size()),
        root.inverse_scale_factor(),
    ) else {
        return;
    };
    // 右键只设置操作上下文，不创建拖动候选，不更改展开集合，也不触发场景双击链。
    browser.select_only(selected.clone());
    editor.scene_list_selection_id = match selected.as_ref() {
        Some(SceneBrowserItem::Scene(id)) => Some(id.clone()),
        _ => None,
    };
    let mut target = DirectoryMenuTarget {
        selected,
        projection: projection.clone(),
        scene_id: editor.selected_scene_id.clone(),
        folder_empty: None,
        scan_cursor: 0,
    };
    advance_empty_check(&mut target, &index);
    menu.anchor = anchor;
    menu.target = Some(target);
    menu.captured_this_frame = true;
    if let Some(focus) = focus.as_mut() {
        focus.clear();
    }
}
