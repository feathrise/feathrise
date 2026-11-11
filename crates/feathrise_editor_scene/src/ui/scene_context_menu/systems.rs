//! 右键只接受真实可见行；关闭点击通过静默门后将冻结操作发布一帧。

use feathrise_editor_ui::UiInteraction;
use super::{components::*, resources::SceneContextMenuState, types::*};
use crate::{
    authoring::EditorAuthoringSession,
    components::SceneEditorRoot,
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState},
};
use bevy::{
    ecs::system::SystemParam,
    input_focus::{FocusCause, InputFocus},
    prelude::*,
    text::EditableText,
    window::PrimaryWindow,
};
use feathrise_editor_ui::{ActiveNumericInput, ActiveNumericInputScrub, EditorButtonClick};

#[derive(SystemParam)]
pub(crate) struct MenuContext<'w, 's> {
    pub session: Res<'w, EditorAuthoringSession>,
    sidebars: Option<Res<'w, crate::ui::sidebars::SceneEditorSidebarState>>,
    workspace: Option<Res<'w, crate::ui::vegetation_preset_editor::VegetationPresetEditorState>>,
    settings: Option<Res<'w, crate::resources::SceneEditorSettingsState>>,
    folders: Option<Res<'w, crate::resources::SceneBrowserState>>,
    deletion: Option<Res<'w, crate::resources::SceneDeletionTask>>,
    conflict: Option<Res<'w, crate::authoring::TerrainExternalChangeState>>,
    drag: Option<Res<'w, crate::resources::SceneBrowserDragState>>,
    layout: Option<Res<'w, crate::resources::SceneEditorLayoutState>>,
    active: Option<Res<'w, ActiveNumericInput>>,
    scrub: Option<Res<'w, ActiveNumericInputScrub>>,
    text: Option<Res<'w, crate::ui::text_input::EditorTextInputGuard>>,
    asset_menu: Option<Res<'w, crate::ui::asset_context_menu::AssetContextMenuState>>,
    tab_menu: Option<Res<'w, crate::ui::asset_preview::AssetTabMenuState>>,
    movement: Option<Res<'w, crate::viewport::placement_move::PlacementMoveState>>,
    subscene_move: Option<Res<'w, crate::viewport::subscenes::SubsceneTransformState>>,
    multi: Option<Res<'w, crate::viewport::multi_select::PlacementSelectionSet>>,
    batch: Option<Res<'w, crate::authoring::placement_batch::PlacementBatchState>>,
    objects: Option<Res<'w, crate::ui::scene_objects::SceneObjectState>>,
    courtyards: Option<Res<'w, crate::authoring::courtyards::CourtyardAuthoringState>>,
    terrains: Option<Res<'w, crate::ui::terrain_objects::TerrainObjectsEditorState>>,
    subscenes: Option<Res<'w, crate::ui::subscenes::SubsceneEditorState>>,
    spatial: Option<Res<'w, crate::viewport::spatial_brush::SpatialBrushState>>,
    modes: Option<Res<'w, crate::viewport::modes::ViewportModeState>>,
    domain: Option<Res<'w, crate::game_authoring::scene_interaction::SceneInteractionState>>,
    water_stroke: Option<Res<'w, crate::authoring::WaterBrushStroke>>,
    water_network: Option<Res<'w, crate::authoring::WaterNetworkEditState>>,
    vegetation_stroke: Option<Res<'w, crate::authoring::VegetationBrushStroke>>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    pub(super) roots:
        Query<'w, 's, (&'static ComputedNode, &'static UiGlobalTransform), With<SceneEditorRoot>>,
}

impl MenuContext<'_, '_> {
    pub(crate) fn enabled(&self, editor: &SceneEditorState) -> bool {
        editor.open && !editor.close_requested && !editor.new_scene_dialog_open
            && !editor.scene_transition_in_progress && editor.pending_scene_transition.is_none()
            && editor.scene_switch_confirmation.is_none() && !editor.save_requested && !self.session.saving
            && self.sidebars.as_ref().is_none_or(|state| state.scene_tree_visible())
            && self.workspace.as_ref().is_none_or(|state| state.active_tab == crate::ui::vegetation_preset_editor::PresetWorkspaceTab::Scene)
            && self.settings.as_ref().is_none_or(|state| !state.open && !state.owns_mouse_this_frame)
            && self.folders.as_ref().is_none_or(|state| state.dialog.is_none())
            && self.deletion.as_ref().is_none_or(|state| !state.busy())
            && self.conflict.as_ref().is_none_or(|state| !state.modal_open())
            && self.drag.as_ref().is_none_or(|state| !state.blocks_other_mouse() && state.press.is_none())
            && self.layout.as_ref().is_none_or(|state| state.content_shown() && state.drag.is_none() && !state.owns_mouse_this_frame)
            && self.active.as_ref().is_none_or(|state| state.entity.is_none())
            && self.scrub.as_ref().is_none_or(|state| state.entity.is_none())
            && self.asset_menu.as_ref().is_none_or(|state| !state.owns_mouse())
            && self.tab_menu.as_ref().is_none_or(|state| !state.owns_mouse())
            && self.movement.as_ref().is_none_or(|state| !state.owns_mouse())
            && self.subscene_move.as_ref().is_none_or(|state| !state.owns_mouse())
            && self.multi.as_ref().is_none_or(|state| !state.owns_mouse())
            && self.batch.as_ref().is_none_or(|state| !state.busy())
            && self.objects.as_ref().is_none_or(|state| state.accepts_commands())
            && self.courtyards.as_ref().is_none_or(|state| !state.operation_pending() && !state.visual_sync_pending())
            && self.terrains.as_ref().is_none_or(|state| !state.busy())
            && self.subscenes.as_ref().is_none_or(|state| !state.busy())
            && self.spatial.as_ref().is_none_or(|state| !state.owns_mouse())
            && self.domain.as_ref().is_none_or(|state| !state.busy() && !state.text_input_active && !state.owns_mouse())
            && self.water_stroke.as_ref().is_none_or(|state| !state.active)
            && self.water_network.as_ref().is_none_or(|state| !state.dragging())
            && self.vegetation_stroke.as_ref().is_none_or(|state| !state.active)
            // 场景树和视口沿用先选择再变换的同一权限，菜单不能暗换正在变换的目标。
            && self.modes.as_ref().is_none_or(|state| state.selection_allowed(editor))
            && self.windows.single().is_ok_and(|window| window.focused)
    }

    fn composing(&self) -> bool {
        self.text
            .as_ref()
            .is_some_and(|state| state.block_shortcuts)
    }
}

pub(super) fn valid_target(
    target: &MenuTarget,
    editor: &SceneEditorState,
    session: &EditorAuthoringSession,
    rows: &Query<(Entity, &SceneObjectRow, &Node, &UiInteraction)>,
) -> bool {
    target.object.current(editor, session)
        && rows.get(target.row).is_ok_and(|(_, row, node, _)| {
            node.display != Display::None && row.0.as_ref() == Some(&target.object)
        }) // 翻页、滚动换绑及列表重建同样取消菜单，绝不追随新占用该槽的对象。
}

pub(super) fn control_menu(
    context: MenuContext,
    mut editor: ResMut<SceneEditorState>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
    guard: Res<SceneEditorNumericScrubMouseGuard>,
    rows: Query<(Entity, &SceneObjectRow, &Node, &UiInteraction)>,
    actions: Query<(&MenuButton, &UiInteraction, Ref<EditorButtonClick>)>,
    rename_buttons: Query<
        (&UiInteraction, Ref<EditorButtonClick>, Has<RenameConfirm>, Has<RenameCancel>),
        Or<(With<RenameConfirm>, With<RenameCancel>)>,
    >,
    fields: Query<&UiInteraction, With<RenameField>>,
    mut inputs: Query<(Entity, &mut EditableText), With<RenameInput>>,
    mut focus: ResMut<InputFocus>,
    mut menu: ResMut<SceneContextMenuState>,
) {
    let had_menu = menu.target.is_some();
    menu.bypass_change_detection().captured_this_frame = had_menu;
    if !had_menu && !mouse.just_pressed(MouseButton::Right) {
        return;
    }
    let escape = keyboard.just_pressed(KeyCode::Escape) && !(menu.renaming && context.composing());
    let close = !context.enabled(&editor)
        || keyboard.just_pressed(KeyCode::F12)
        || had_menu
            && (escape
                || mouse.just_pressed(MouseButton::Right)
                || mouse.just_pressed(MouseButton::Middle)
                || !menu
                    .target
                    .as_ref()
                    .is_some_and(|target| valid_target(target, &editor, &context.session, &rows)));
    if close {
        menu.target = None;
        menu.renaming = false;
        if inputs.iter().any(|(entity, _)| focus.get() == Some(entity)) {
            focus.clear();
        }
        if keyboard.just_pressed(KeyCode::Escape) && had_menu {
            keyboard.clear_just_pressed(KeyCode::Escape);
        }
        if keyboard.just_pressed(KeyCode::F12) {
            menu.captured_this_frame = false;
        }
        return;
    }
    if had_menu {
        if menu.renaming {
            let enter = keyboard.just_pressed(KeyCode::Enter)
                || keyboard.just_pressed(KeyCode::NumpadEnter);
            let confirm = rename_buttons
                .iter()
                .any(|(_, click, confirm, _)| click.is_changed() && click.is_clicked() && confirm);
            let cancel = rename_buttons
                .iter()
                .any(|(_, click, _, cancel)| click.is_changed() && click.is_clicked() && cancel);
            let over_button = rename_buttons.iter().any(|(interaction, _, _, _)| *interaction != UiInteraction::None);
            let editing = fields
                .iter()
                .any(|interaction| *interaction != UiInteraction::None);
            if cancel || mouse.just_pressed(MouseButton::Left) && !over_button && !editing {
                menu.target = None;
                menu.renaming = false;
                focus.clear();
                return;
            }
            if editing && mouse.just_pressed(MouseButton::Left) {
                if let Ok((entity, _)) = inputs.single() {
                    focus.set(entity, FocusCause::Navigated);
                }
            }
            if (enter || confirm)
                && !context.composing()
                && let Ok((_, input)) = inputs.single()
                && !input.is_composing()
            {
                let name = input.value().to_string().trim().to_owned();
                if let Err(error) = menu.target.as_ref().unwrap().object.validate_name(&name) {
                    editor.status = error.into();
                    return;
                }
                let target = menu.target.take().unwrap();
                menu.queued = Some((
                    target.clone(),
                    SceneObjectRequest {
                        target: target.object,
                        action: SceneMenuAction::Rename,
                        name,
                    },
                ));
                menu.renaming = false;
                focus.clear();
                keyboard.clear_just_pressed(KeyCode::Enter);
                keyboard.clear_just_pressed(KeyCode::NumpadEnter);
            }
            return;
        }
        let action = actions.iter().find_map(|(button, _, click)| {
            (click.is_changed() && click.is_clicked()).then_some(button.0)
        });
        if let Some(action) = action {
            // 菜单项按住只显示反馈，完整动作在同一项内松开并复核冻结目标后登记。
            let target = menu.target.as_ref().unwrap();
            if !target.object.allows(action) {
                return;
            }
            if action == SceneMenuAction::Rename {
                if let Ok((entity, mut input)) = inputs.single_mut() {
                    set_name(&mut input, &target.object.name);
                    focus.set(entity, FocusCause::Navigated);
                    menu.renaming = true;
                }
            } else {
                let target = menu.target.take().unwrap();
                menu.queued = Some((
                    target.clone(),
                    SceneObjectRequest {
                        target: target.object,
                        action,
                        name: String::new(),
                    },
                ));
                focus.clear();
            }
        } else if mouse.just_pressed(MouseButton::Left)
            && actions.iter().all(|(_, interaction, _)| *interaction == UiInteraction::None)
        {
            menu.target = None; // 外部按下只关闭；不能提前吞掉菜单项等待松开的手势。
        }
        return;
    }
    if guard.blocked()
        || context.composing()
        || menu.queued.is_some()
        || mouse.any_pressed([MouseButton::Left, MouseButton::Middle])
        || [MouseButton::Left, MouseButton::Middle]
            .into_iter()
            .any(|key| mouse.just_released(key))
    {
        return;
    }
    let Some((entity, object)) = rows.iter().find_map(|(entity, row, node, interaction)| {
        (node.display != Display::None && *interaction == UiInteraction::Hovered)
            .then_some(row.0.as_ref())
            .flatten()
            .filter(|target| target.current(&editor, &context.session)) // 隐藏/换绑中的旧槽不能遮住另一条有效悬停行。
            .map(|target| (entity, target.clone()))
    }) else {
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
    let Some(anchor) = super::super::asset_context_menu::helpers::logical_anchor(
        cursor,
        Rect::from_center_size(transform.translation, root.size()),
        root.inverse_scale_factor(),
    ) else {
        return;
    };
    if !object.current(&editor, &context.session) {
        return;
    }
    menu.anchor = anchor;
    menu.target = Some(MenuTarget {
        row: entity,
        object,
    });
    menu.captured_this_frame = true;
    focus.clear(); // 背景搜索框失去键盘权限，菜单期间输入不能继续改写树投影。
}

/// 原位替换保留官方已经同步的字体、字号和布局代次；重建EditableText会令再次打开的中文消失。
pub(super) fn set_name(input: &mut EditableText, name: &str) {
    input.clear();
    input.editor.set_text(name);
    input.queue_edit(bevy::text::TextEdit::TextEnd(false));
}

pub(super) fn dispatch(
    context: MenuContext,
    editor: Res<SceneEditorState>,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    guard: Res<SceneEditorNumericScrubMouseGuard>,
    rows: Query<(Entity, &SceneObjectRow, &Node, &UiInteraction)>,
    mut menu: ResMut<SceneContextMenuState>,
) {
    if menu.ready.is_some() {
        menu.ready = None;
    }
    let Some((target, _)) = menu.queued.as_ref() else {
        return;
    };
    if !context.enabled(&editor)
        || !valid_target(target, &editor, &context.session, &rows)
        || keyboard.any_just_pressed([KeyCode::Escape, KeyCode::F12])
        || mouse.any_just_pressed([MouseButton::Right, MouseButton::Middle])
    {
        menu.queued = None;
        return;
    }
    if guard.blocked()
        || context.composing()
        || mouse.any_pressed([MouseButton::Left, MouseButton::Right, MouseButton::Middle])
        || [MouseButton::Left, MouseButton::Right, MouseButton::Middle]
            .into_iter()
            .any(|key| mouse.just_released(key))
    {
        return;
    }
    menu.ready = menu.queued.take().map(|(_, request)| request);
}
