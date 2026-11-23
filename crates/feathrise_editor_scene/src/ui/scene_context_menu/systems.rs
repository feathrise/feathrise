//! 右键冻结可见行或场景树空白的根级目标，关闭后经静默门分派。

use feathrise_editor_ui::UiInteraction;
use super::{components::*, resources::SceneContextMenuState, types::*};
use crate::{
    authoring::{
        EditorAuthoringSession,
        blender_mesh::{BlenderMainSceneAction, BlenderMeshAction},
    },
    components::SceneEditorRoot,
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState},
    ui::scene_motion::OpenSceneAnimation,
    ui::{gltf_browser::SceneTreeContents, scene_inspector::InspectorTree},
};
use bevy::{
    ecs::query::QueryFilter,
    ecs::system::SystemParam,
    input::mouse::MouseWheel,
    input_focus::{FocusCause, InputFocus},
    prelude::*,
    text::EditableText,
    time::Real,
    ui::RelativeCursorPosition,
    window::PrimaryWindow,
};
use feathrise_editor_ui::{ActiveNumericInput, ActiveNumericInputScrub, EditorButtonClick};

const TREE_DOUBLE_CLICK_SECONDS: f64 = 0.36; // 沿用原场景对象的双击间隔，独立于玩法虚拟时间。

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
    domain: Option<Res<'w, crate::game_authoring::scene_interaction::SceneInteractionState>>,
    water_stroke: Option<Res<'w, crate::authoring::WaterBrushStroke>>,
    water_network: Option<Res<'w, crate::authoring::WaterNetworkEditState>>,
    vegetation_stroke: Option<Res<'w, crate::authoring::VegetationBrushStroke>>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    pub(super) roots:
        Query<'w, 's, (&'static ComputedNode, &'static UiGlobalTransform), With<SceneEditorRoot>>,
    tree: Query<'w, 's, (Entity, &'static ComputedNode, &'static RelativeCursorPosition), With<InspectorTree>>,
    tree_content: Query<'w, 's, (&'static ComputedNode, &'static RelativeCursorPosition), With<SceneTreeContents>>,
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
            && self.windows.single().is_ok_and(|window| window.focused)
    }

    fn composing(&self) -> bool {
        self.text
            .as_ref()
            .is_some_and(|state| state.block_shortcuts)
    }

    fn blank_tree_target(&self, editor: &SceneEditorState) -> Option<MenuTarget> {
        let (row, layout, cursor) = self.tree.single().ok()?;
        if layout.is_empty() || !cursor.cursor_over()
            || self.tree_content.iter().any(|(layout, cursor)| !layout.is_empty() && cursor.cursor_over())
        {
            return None; // 行内按钮和分类标题也占用正文，只有正文之外是空白。
        }
        let draft = self.session.draft.as_ref()?;
        let object = SceneObjectTarget {
            scene_id: draft.scene_id.clone(), revision: self.session.revision,
            key: SceneObjectKey::PlacementTreeRoot, name: String::new(),
            hidden: false, locked: false, copyable: false,
        };
        object.current(editor, &self.session).then_some(MenuTarget { row, object })
    }
}

pub(super) fn valid_target<F: QueryFilter>(
    target: &MenuTarget,
    editor: &SceneEditorState,
    context: &MenuContext,
    rows: &Query<(Entity, &SceneObjectRow, &Node, &UiInteraction), F>,
) -> bool {
    target.object.current(editor, &context.session)
        && (rows.get(target.row).is_ok_and(|(_, row, node, _)| {
            node.display != Display::None && row.0.as_ref() == Some(&target.object)
        }) || matches!(target.object.key, SceneObjectKey::PlacementTreeRoot)
            && context.tree.get(target.row).is_ok_and(|(_, layout, _)| !layout.is_empty()))
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
                    .is_some_and(|target| valid_target(target, &editor, &context, &rows)));
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
                let action = menu.name_action.unwrap_or(SceneMenuAction::Rename);
                if name.is_empty() && action != SceneMenuAction::Rename {
                    editor.status = "名称不能为空".into();
                    return;
                }
                if let Err(error) = menu.target.as_ref().unwrap().object.validate_name(&name) {
                    editor.status = error.into();
                    return;
                }
                let target = menu.target.take().unwrap();
                menu.queued = Some((
                    target.clone(),
                    SceneObjectRequest {
                        target: target.object,
                        action,
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
            if !target.object.allows(action)
                || !target.object.menu_has(action, &context.session)
            {
                return;
            }
            if action.requests_name() {
                if let Ok((entity, mut input)) = inputs.single_mut() {
                    let initial = match action {
                        SceneMenuAction::Rename => target.object.name.as_str(),
                        SceneMenuAction::RenameSourceNode => match &target.object.key {
                            SceneObjectKey::Placement { mesh: Some(mesh), .. } => {
                                mesh.node_path.last().map_or("", String::as_str)
                            }
                            _ => "",
                        },
                        _ => "",
                    };
                    set_name(&mut input, initial);
                    focus.set(entity, FocusCause::Navigated);
                    menu.renaming = true;
                    menu.name_action = Some(action);
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
    let Some(target) = rows.iter().find_map(|(entity, row, node, interaction)| {
        (node.display != Display::None && *interaction == UiInteraction::Hovered)
            .then_some(row.0.as_ref())
            .flatten()
            .filter(|target| target.current(&editor, &context.session)) // 隐藏/换绑中的旧槽不能遮住另一条有效悬停行。
            .map(|target| MenuTarget { row: entity, object: target.clone() })
    }).or_else(|| context.blank_tree_target(&editor)) else {
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
    menu.anchor = anchor;
    menu.target = Some(target);
    menu.captured_this_frame = true;
    focus.clear(); // 菜单持有输入时让后台文本框失焦。
}

/// 双击只认两次共用按钮系统已验收的同一行释放；空白或子按钮不会产出行点击。
pub(super) fn queue_tree_double_click(
    context: MenuContext,
    editor: Res<SceneEditorState>,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    time: Res<Time<Real>>,
    mut scroll: MessageReader<MouseWheel>,
    guard: Res<SceneEditorNumericScrubMouseGuard>,
    mut rows: Query<(Entity, &SceneObjectRow, &Node, &mut EditorButtonClick), Changed<EditorButtonClick>>,
    mut menu: ResMut<SceneContextMenuState>,
    mut animation: MessageWriter<OpenSceneAnimation>,
) {
    let scrolled = !scroll.is_empty();
    scroll.clear();
    if !mouse.just_released(MouseButton::Left) && menu.last_tree_click.is_none() && !menu.tree_focus_queued {
        return;
    }
    let other_buttons = [MouseButton::Right, MouseButton::Middle];
    let interrupted = scrolled || guard.blocked() || menu.owns_mouse() || context.composing()
        || !context.enabled(&editor)
        || mouse.any_pressed(other_buttons) || other_buttons.into_iter().any(|button| mouse.just_released(button))
        || keyboard.any_just_pressed([KeyCode::Escape, KeyCode::F12])
        || keyboard.any_pressed([
            KeyCode::ShiftLeft, KeyCode::ShiftRight, KeyCode::ControlLeft, KeyCode::ControlRight,
            KeyCode::AltLeft, KeyCode::AltRight, KeyCode::SuperLeft, KeyCode::SuperRight,
        ]);
    if menu.tree_focus_queued && (interrupted || mouse.just_pressed(MouseButton::Left)) {
        menu.queued = None; // 已开始的新手势优先，旧双击不能在下一静默帧再抢镜头。
        menu.tree_focus_queued = false;
    }
    let seconds = time.elapsed_secs_f64();
    if interrupted || menu.queued.is_some() {
        menu.bypass_change_detection().last_tree_click = None;
        return;
    }
    if !mouse.just_released(MouseButton::Left) || mouse.pressed(MouseButton::Left) {
        if menu.last_tree_click.as_ref().is_some_and(|(_, then)| seconds - then > TREE_DOUBLE_CLICK_SECONDS) {
            menu.bypass_change_detection().last_tree_click = None;
        }
        return;
    }
    let previous = menu.bypass_change_detection().last_tree_click.take();
    let Some((row, object, mut click)) = rows.iter_mut().find_map(|(row, binding, node, click)| {
        let object = binding.0.as_ref()?;
        (click.is_clicked() && node.display != Display::None
            && object.current(&editor, &context.session)
            && (matches!(object.key, SceneObjectKey::MainSceneAnimation(_)) || object.allows(SceneMenuAction::Focus)
                || (matches!(object.key, SceneObjectKey::SourceGlbAnimation { .. })
                    && object.allows(SceneMenuAction::EditSourceGlbAnimation))))
            .then_some((row, object, click))
    }) else { return; };
    let target = MenuTarget { row, object: object.clone() };
    if previous.is_some_and(|(first, then)| first.row == row && first.object == *object
        && (0.0..=TREE_DOUBLE_CLICK_SECONDS).contains(&(seconds - then))) {
        click.invalidate(); // 双击接管第二次释放，避免同一释放同时触发单击选择。
        if matches!(object.key, SceneObjectKey::MainSceneAnimation(_)) {
            animation.write(OpenSceneAnimation(object.clone()));
        } else {
            let action = if matches!(object.key, SceneObjectKey::SourceGlbAnimation { .. }) {
                SceneMenuAction::EditSourceGlbAnimation
            } else { SceneMenuAction::Focus };
            menu.queued = Some((target, SceneObjectRequest { target: object.clone(), action, name: String::new() }));
            menu.tree_focus_queued = true;
        }
    } else {
        menu.bypass_change_detection().last_tree_click = Some((target, seconds));
    }
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
        || !valid_target(target, &editor, &context, &rows)
        || keyboard.any_just_pressed([KeyCode::Escape, KeyCode::F12])
        || mouse.any_just_pressed([MouseButton::Right, MouseButton::Middle])
    {
        menu.queued = None;
        menu.tree_focus_queued = false;
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
    menu.tree_focus_queued = false;
}

pub(super) fn send_blender_action(
    menu: Res<SceneContextMenuState>,
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    mut actions: MessageWriter<BlenderMeshAction>,
    mut main_scenes: MessageWriter<BlenderMainSceneAction>,
) {
    let Some(request) = menu.ready.as_ref().filter(|request| request.action.is_blender()) else {
        return;
    };
    let target = &request.target;
    // 菜单按已呈现的行冻结场景和对象，此处不会跟随后来被选中的另一条记录。
    if !target.current(&editor, &session)
        || !target.allows(request.action)
        || !target.menu_has(request.action, &session)
    {
        return;
    }
    match &target.key {
        SceneObjectKey::MainSceneGlb => {
            main_scenes.write(BlenderMainSceneAction { scene_id: target.scene_id.clone() });
        }
        SceneObjectKey::Placement { id, mesh: None, .. } => {
            actions.write(BlenderMeshAction { scene_id: target.scene_id.clone(), placement_id: id.clone() });
        }
        _ => {}
    }
}

pub(super) fn reveal_glb_file(
    menu: Res<SceneContextMenuState>,
    mut editor: ResMut<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    files: Res<crate::ui::file_manager::FileManagerState>,
    mut navigation: MessageWriter<crate::ui::file_manager::browser::BrowserNavigate>,
) {
    let Some(request) = menu.ready.as_ref()
        .filter(|request| request.action == SceneMenuAction::RevealGlb && request.target.current(&editor, &session))
    else {
        return;
    };
    let Some(path) = request.target.glb_path(&session) else {
        return;
    };
    match crate::asset_catalog::normalize_asset_reference(path) {
        Ok(path) => {
            navigation.write(crate::ui::file_manager::browser::BrowserNavigate::RevealFile {
                context: files.service_context(),
                path,
            });
        }
        Err(error) => editor.status = error,
    }
}
