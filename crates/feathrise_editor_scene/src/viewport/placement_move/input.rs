//! 单一手柄手势：开始前让位给 UI，取得所有权后独占到所有鼠标键静默。

use super::super::{
    LayerCandidateState, LayerTarget,
    modes::{ViewportMode, ViewportModeState},
};
use super::{
    math::{MoveCamera, handle_point},
    resources::PlacementMoveState,
    rotation::{pick_rotation, rotation_yaw},
    scale::pick_scale,
    transaction::{MoveWorld, update_preview, update_world_rotation},
    types::{MoveDrag, MoveHandle, PendingBuildingMove},
};
use crate::authoring::buildings::{
    BuildingEditRequest, BuildingEditTarget, BuildingEditValidation, clipmap_controls_identity,
};
use crate::authoring::{
    EditorAuthoringSession, PlacementTerrainSampler, TerrainExternalChangeState,
};
use crate::authoring::placement_pose::{placement_record_frame, placement_record_world_position, source_to_world};
use crate::components::SceneEditorViewport;
use crate::resources::{
    SceneBrowserDragState, SceneBrowserState, SceneDeletionTask, SceneEditorLayoutState,
    SceneEditorNumericScrubMouseGuard, SceneEditorSettingsState, SceneEditorState,
    SceneEditorViewportState,
};
use crate::types::EditorTool;
use crate::ui::{
    building_presets::BuildingPlacementControls,
    courtyard::{CourtyardUiState, CourtyardViewportMode},
    gltf_browser::EditorGltfBrowserState,
    placement_transform::PlacementTransformInputState,
    text_input::{EditorTextInput, EditorTextInputGuard},
    vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState},
};
use bevy::{
    ecs::system::SystemParam, input_focus::InputFocus, prelude::*, ui::RelativeCursorPosition,
    window::PrimaryWindow,
};
use feathrise_editor_ui::{ActiveNumericInput, ActiveNumericInputScrub};
use feathrise_scene_content::ScenePlacementRecord;
use feathrise_scene::DynamicClipmapGroundSurface;

#[derive(SystemParam)]
pub(crate) struct MoveInputContext<'w, 's> {
    pub browser: Res<'w, EditorGltfBrowserState>,
    pub candidates: Res<'w, LayerCandidateState>,
    validation: BuildingEditValidation<'w>,
    workspace: Res<'w, VegetationPresetEditorState>,
    pub(super) mode: Res<'w, ViewportModeState>,
    transform: Res<'w, PlacementTransformInputState>,
    courtyard: Res<'w, CourtyardUiState>,
    settings: Res<'w, SceneEditorSettingsState>,
    pub(crate) preferences: Option<Res<'w, feathrise_editor_core::EditorSettings>>,
    lod: crate::authoring::mesh_lod::MeshLodInputGate<'w>,
    folders: Res<'w, SceneBrowserState>,
    scene_drag: Res<'w, SceneBrowserDragState>,
    conflict: Res<'w, TerrainExternalChangeState>,
    deletion: Res<'w, SceneDeletionTask>,
    layout: Res<'w, SceneEditorLayoutState>,
    numeric: Res<'w, ActiveNumericInput>,
    scrub: Res<'w, ActiveNumericInputScrub>,
    mesh_edit: Option<Res<'w, crate::authoring::mesh_edit::MeshEditState>>,
    text: Res<'w, EditorTextInputGuard>,
    focus: Option<Res<'w, InputFocus>>,
    fields: Query<'w, 's, (), Or<(With<EditorTextInput>, With<bevy::text::EditableText>)>>, // 官方原生文字控件也独占字符键，不能仅依赖编辑器标记或IME候选。
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    controls: Query<
        'w,
        's,
        &'static RelativeCursorPosition,
        Or<(
            With<super::super::modes::ViewportControlsRoot>,
            With<super::super::modes::ViewportModeToolsRoot>,
            With<super::super::modes::ViewportModeMenuRoot>,
            With<super::super::modes::MeshEditToolbarRoot>,
            With<crate::components::SceneEditorOrientationAxisRoot>,
            With<crate::viewport::shading::ViewShadingRoot>,
        )>, // 新竖向图标栏与紧凑菜单也拥有鼠标，不能让同帧按下穿透到手柄。
    >,
    popups: Query<
        'w,
        's,
        &'static RelativeCursorPosition,
        With<super::super::content_selection::LayerCandidatePopup>,
    >,
}

impl MoveInputContext<'_, '_> {
    pub(crate) fn snap_settings(&self) -> feathrise_editor_core::EditorSnapSettings {
        self.preferences.as_ref().map_or_else(Default::default, |settings| settings.editor_snapping.sanitized())
    }

    fn available(&self, editor: &SceneEditorState) -> bool {
        self.pointer_available(editor)
            && !self.mesh_edit_active()
            && editor.tool == EditorTool::Inspect
            && self.mode.mode.transforms_placement() && editor.placement_transform_enabled
    }

    pub(crate) fn selection_available(&self, editor: &SceneEditorState) -> bool {
        !self.mesh_edit_active() && self.pointer_available(editor) && editor.tool == EditorTool::Inspect && self.mode.mode == ViewportMode::Select
    }

    pub(crate) fn sculpt_available(&self, editor: &SceneEditorState) -> bool {
        !self.mesh_edit_active() && self.scene_commands_available(editor) && editor.tool.is_volume_sculpt() && self.mode.mode == ViewportMode::Edit
    }

    pub(crate) fn sculpt_parameters_available(&self, editor: &SceneEditorState) -> bool {
        !self.mesh_edit_active() && self.document_available(editor) && editor.tool.is_volume_sculpt() && self.mode.mode == ViewportMode::Edit
    }

    pub(crate) fn batch_available(&self, editor: &SceneEditorState) -> bool { !self.mesh_edit_active() && self.pointer_available(editor) }

    fn mesh_edit_active(&self) -> bool {
        self.mesh_edit.as_ref().is_some_and(|state| state.active())
    }

    pub(crate) fn pointer_over_ui(&self) -> bool {
        self.mode.pointer_over_controls || self.candidates.pointer_over_ui
            || self.controls.iter().any(|cursor| cursor.cursor_over())
            || self.popups.iter().any(|cursor| cursor.cursor_over())
    }

    pub(crate) fn pointer_available(&self, editor: &SceneEditorState) -> bool {
        self.scene_commands_available(editor) && editor.tool == EditorTool::Inspect
    }

    pub(crate) fn scene_commands_available(&self, editor: &SceneEditorState) -> bool {
        self.document_available(editor)
            && self.lod.ready()
            && self.numeric.entity.is_none()
            && self.scrub.entity.is_none()
            && !self.text.block_shortcuts
            && !self.focus.as_ref().and_then(|focus| focus.get()).is_some_and(|entity| self.fields.contains(entity))
    }

    /// 属性框本身拥有数值焦点；只检查文档和模态门，不能用视口的“无数值输入”门取消自己。
    pub(crate) fn document_available(&self, editor: &SceneEditorState) -> bool {
        editor.open
            && !editor.close_requested
            && !editor.new_scene_dialog_open
            && !editor.suppress_world_pick_this_frame
            && !editor.scene_transition_in_progress
            && editor.pending_scene_transition.is_none()
            && editor.scene_switch_confirmation.is_none()
            && self.workspace.active_tab == PresetWorkspaceTab::Scene
            && !self.settings.open
            && !self.settings.owns_mouse_this_frame
            && self.folders.dialog.is_none()
            && !self.scene_drag.blocks_other_mouse()
            && !self.conflict.modal_open()
            && !self.deletion.busy()
            && self.courtyard.mode == CourtyardViewportMode::Idle
            && !self.transform.relocation_armed()
            && self.layout.drag.is_none()
            && !self.layout.owns_mouse_this_frame
            && self.windows.single().map_or(true, |window| window.focused)
    }
}

/// 当前完整实例按索引借用，候选悬停或其他类型不能激活残留资产选择。
pub(crate) fn selected_record<'a>(
    editor: &SceneEditorState,
    session: &'a EditorAuthoringSession,
    browser: &EditorGltfBrowserState,
    candidates: &LayerCandidateState,
    viewport: &SceneEditorViewportState,
) -> Option<(usize, &'a ScenePlacementRecord)> {
    let draft = session.draft.as_ref()?;
    if editor.selected_scene_id.as_ref() != Some(&draft.scene_id)
        || viewport.framed_scene_id.as_ref() != Some(&draft.scene_id)
    {
        return None;
    }
    let id = browser.selected_placement_id.as_deref()?;
    if candidates.selected.as_ref().is_some_and(
        |selected| !matches!(selected, LayerTarget::Placement(selected) if selected == id),
    ) || candidates.has_viewport_candidates() && candidates.selected.is_none()
    {
        return None;
    }
    let index = browser.placement_record_index(id, session.placement_structure_revision)?;
    let record = draft
        .placements
        .placements
        .get(index)
        .filter(|record| record.id == id && record.editor.selectable())?;
    Some((index, record))
}

fn drag_is_current(
    drag: &MoveDrag,
    editor: &SceneEditorState,
    session: &EditorAuthoringSession,
    context: &MoveInputContext,
    viewport: &SceneEditorViewportState,
    camera: MoveCamera,
    controls: &BuildingPlacementControls,
) -> bool {
    session.revision == drag.author_revision
        && context.mode.mode == drag.handle.mode()
        && session.terrain_revision == drag.terrain_revision
        && session.placement_structure_revision == drag.structure_revision
        && camera == drag.camera
        && (drag.before.building_preset_id.is_none()
            || clipmap_controls_identity(controls) == drag.building_controls)
        && selected_record(
            editor,
            session,
            &context.browser,
            &context.candidates,
            viewport,
        )
        .is_some_and(|(index, record)| {
            index == drag.record_index
                && record.id == drag.before.id
                && record.translation == drag.before.translation
                && record.height_mode == drag.before.height_mode
                && record.terrain_offset == drag.before.terrain_offset
                && record.yaw_degrees == drag.before.yaw_degrees
                && record.scale == drag.before.scale
                && record.terrain_patch == drag.before.terrain_patch
                && session.draft.as_ref().is_some_and(|draft| placement_record_frame(draft, record) == Ok(drag.frame))
        })
} // 派生变换可能不推进作者revision，仍核对常数大小的姿态字段，不能用旧预览覆盖它。

pub(crate) fn handle_placement_move(
    mut state: ResMut<PlacementMoveState>,
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
    guard: Res<SceneEditorNumericScrubMouseGuard>,
    viewport: Option<Res<SceneEditorViewportState>>,
    nodes: Query<(&RelativeCursorPosition, &ComputedNode), With<SceneEditorViewport>>,
    context: MoveInputContext,
    controls: Res<BuildingPlacementControls>,
    clipmap: Option<Res<DynamicClipmapGroundSurface>>,
    mut world: MoveWorld,
    mut buildings: MessageWriter<BuildingEditRequest>,
    mut multi_selection: Option<ResMut<crate::viewport::multi_select::PlacementSelectionSet>>,
    batch: Option<Res<crate::authoring::placement_batch::PlacementBatchState>>,
) {
    if state.group.is_some() && keyboard.just_pressed(KeyCode::F12) {
        editor.toggle_close_request();
        keyboard.clear_just_pressed(KeyCode::F12);
    } // 整组手势先恢复当前作者姿态，再沿统一安全关闭流程收尾，不让输入占用永久锁住F12。
    state.release_when_quiet(&mouse);
    state.hover = None;
    let node = nodes.single().ok();
    let camera = viewport.as_ref().and_then(|viewport| {
        node.and_then(|(_, node)| MoveCamera::new(&editor, viewport, node.inverse_scale_factor()))
    });
    let available = context.available(&editor) && !session.saving && batch.as_ref().is_none_or(|batch| !batch.busy());
    let terrain_content = clipmap
        .as_ref()
        .and_then(|surface| surface.snapshot.as_ref())
        .filter(|snapshot| {
            session
                .draft
                .as_ref()
                .is_some_and(|draft| draft.scene_id == snapshot.scene_id)
                && snapshot.authoring_height_revision() == Some(session.terrain_revision)
        })
        .map(|snapshot| snapshot.content_revision_words());
    let group_available = available && (!guard.blocked() || state.group.is_some());
    if super::group::handle_group(&mut state, multi_selection.as_deref_mut(), &mut editor, &mut session,
        &mouse, &mut keyboard, &context, viewport.as_deref(), node, camera, group_available,
        terrain_content, clipmap.as_deref(), &mut world) { return; }
    if let Some(pending) = state.pending_building.take() {
        let drag = &pending.drag;
        let current = editor.open
            && !editor.close_requested
            && !editor.scene_transition_in_progress
            && !editor.new_scene_dialog_open
            && !session.saving
            && session.revision == drag.author_revision
            && session.terrain_revision == drag.terrain_revision
            && session.placement_structure_revision == drag.structure_revision
            && clipmap_controls_identity(&controls) == drag.building_controls
            && context.browser.selected_placement_id.as_deref() == Some(drag.before.id.as_str())
            && editor.selected_scene_id.as_ref() == Some(&drag.scene_id);
        if current
            && context.validation.pending_for(
                &pending.target,
                Vec3::from_array(drag.preview.translation).xz(),
                drag.preview.yaw_degrees,
            )
        {
            state.pending_building = Some(pending);
        } else {
            // 【需求设计】松手不闪回。校验结束再恢复当前权威值：成功是新落位，失败仍是原位。
            // 拒绝请求也会经过这里，不能让模型留在未写入草稿的候选位置。
            world.cancel(&mut session, drag);
        }
    }
    if let Some(mut drag) = state.drag.take() {
        let escaped = keyboard.just_pressed(KeyCode::Escape);
        let chord = mouse.any_pressed([MouseButton::Right, MouseButton::Middle]);
        keyboard.clear(); // 只消费边沿，保留 Ctrl 电平用于吸附；F12/保存/撤销不能泄漏到并行入口。
        let current = available
            && drag.formal_runtime_current(world.runtime_status.as_deref())
            && session.draft.as_ref().is_some_and(|draft| drag.patch_source.as_ref()
                .is_none_or(|source| world.patch_source_current(draft, source)))
            && node.is_some_and(|(_, node)| node.size() == drag.viewport_ui_size)
            && drag
                .terrain_content
                .is_none_or(|content| terrain_content == Some(content))
            && viewport
                .as_ref()
                .zip(camera)
                .is_some_and(|(viewport, camera)| {
                    drag_is_current(
                        &drag, &editor, &session, &context, viewport, camera, &controls,
                    )
                });
        if escaped || chord || !current || node.and_then(|(cursor, _)| cursor.normalized).is_none()
        {
            world.cancel(&mut session, &drag);
            editor.status = "已取消变换，恢复当前资产姿态".into();
            return; // owns_mouse 留到静默帧，失焦和和弦均不能立刻归还。
        }
        let normalized = node
            .and_then(|(cursor, _)| cursor.normalized)
            .expect("已确认指针存在");
        if drag.terrain_content.is_none() {
            drag.terrain_content = terrain_content;
        } // 首次等待可在同作者修订下绑定完整快照，此后内容换代必须取消。
        let origin = drag.world_origin;
        let Some(point) = drag
            .camera
            .ray(normalized)
            .and_then(|ray| handle_point(ray, origin, drag.handle))
        else {
            world.cancel(&mut session, &drag);
            editor.status = "手柄与视线接近平行，已取消变换".into();
            return;
        };
        let moved =
            ((normalized - drag.start_cursor) * drag.camera.size * drag.camera.inverse_scale)
                .length_squared()
                > 4.0;
        drag.translation_control = keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
        let translation_snap = drag.translation_snap();
        let grid = if translation_snap.active() { translation_snap.step() } else { 0.0 };
        let bounds = session
            .draft
            .as_ref()
            .and_then(|draft| draft.manifest.world_bounds())
            .expect("按下时已复核场景范围");
        let position = if moved && drag.handle.mode() == ViewportMode::Move {
            super::math::handle_translation_mode(
                origin,
                point - drag.start_point,
                drag.handle,
                grid,
                translation_snap.mode,
                bounds.min,
                bounds.max,
            )
        } else {
            Some(origin)
        };
        let sampler = if drag.before.terrain_patch.is_none()
            && (drag.before.height_mode.follows_terrain() || drag.before.terrain_binding.is_some())
            && drag.before.building_preset_id.is_none()
            && drag.handle.mode() == ViewportMode::Move
        {
            session.draft.as_ref().and_then(|draft| {
                PlacementTerrainSampler::from_sources(
                    draft,
                    clipmap.as_deref(),
                    session.terrain_revision,
                )
                .ok()
            })
        } else {
            None
        };
        let previous = (
            drag.preview.translation,
            drag.preview.terrain_offset,
            drag.preview.yaw_degrees,
            drag.preview.scale,
        );
        let was_valid = drag.valid;
        let snapping = drag.snapping.active(keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]));
        let candidate = match drag.handle {
            MoveHandle::RotationY => {
                let step = if drag.before.building_preset_id.is_some() {
                    90.0
                } else if snapping {
                    drag.snapping.rotation_degrees
                } else {
                    0.0
                };
                let patch = drag.before.terrain_patch.is_some();
                let base = if patch { 0.0 } else { drag.before.yaw_degrees };
                let yaw = if moved {
                    rotation_yaw(
                        base,
                        origin,
                        drag.start_point,
                        point,
                        step,
                    )
                } else {
                    Some(base)
                };
                yaw.ok_or_else(|| "指针接近旋转轴心，角度无法确定".to_owned())
                    .and_then(|yaw| if patch { update_world_rotation(&mut drag, yaw) }
                        else { drag.preview.yaw_degrees = yaw; Ok(()) })
            }
            MoveHandle::UniformScale => {
                let delta =
                    (normalized - drag.start_cursor) * drag.camera.size * drag.camera.inverse_scale;
                let scale = if moved {
                    super::scale::uniform_scale_step(drag.before.scale, delta,
                        if snapping { drag.snapping.scale_factor } else { 0.0 })
                } else {
                    Some(drag.before.scale)
                };
                scale
                    .ok_or_else(|| "资产原缩放或目标倍率无效".to_owned())
                    .map(|scale| drag.preview.scale = scale)
            }
            _ => position
                .ok_or_else(|| "移动位置无效".to_owned())
                .and_then(|position| update_preview(&mut drag, position, sampler.as_ref())),
        }; // 三类手柄只改自己的标量；旋转和缩放不重采样高度或覆盖离地偏移。
        drag.valid = candidate.is_ok();
        if let Err(error) = candidate {
            if drag.handle.mode() == ViewportMode::Move
                && let Some(snapshot) = clipmap
                    .as_ref()
                    .and_then(|surface| surface.snapshot.as_ref())
                    .filter(|snapshot| {
                        snapshot.scene_id == drag.scene_id
                            && snapshot.authoring_height_revision() == Some(drag.terrain_revision)
                    })
            {
                let position = position.unwrap_or(origin);
                let _ = snapshot.request_authoritative_rect(
                    position.xz() - Vec2::splat(0.01),
                    position.xz() + Vec2::splat(0.01),
                ); // 复用已有有界高度页通道，不创建新的任务槽。
            }
            editor.status = format!("暂不能变换：{error}；松手将取消");
        } else if !was_valid
            || previous
                != (
                    drag.preview.translation,
                    drag.preview.terrain_offset,
                    drag.preview.yaw_degrees,
                    drag.preview.scale,
                )
        {
            if let Err(error) = world.preview(&mut session, &drag.preview) {
                drag.valid = false;
                editor.status = format!("变换预览失败：{error}");
            } else {
                drag.preview_applied = true;
                // 原World已通过完整记录/源父校验；失败分支必须保留上次实际显示的矩阵。
                drag.applied_world = drag.frame.as_transform().to_matrix()
                    * Mat4::from_scale_rotation_translation(
                        Vec3::from_array(drag.preview.scale),
                        Quat::from_rotation_y(drag.preview.yaw_degrees.to_radians()),
                        Vec3::from_array(drag.preview.translation),
                    );
            }
        }
        if mouse.pressed(MouseButton::Left)
            && drag.valid
            && (!was_valid
                || previous
                    != (
                        drag.preview.translation,
                        drag.preview.terrain_offset,
                        drag.preview.yaw_degrees,
                        drag.preview.scale,
                    ))
        {
            let [x, y, z] = drag.preview.translation;
            let label = if drag.handle == MoveHandle::RotationY {
                "旋转预览"
            } else if drag.handle == MoveHandle::UniformScale {
                "缩放预览"
            } else if drag.before.building_preset_id.is_some() {
                "建筑移动预览"
            } else {
                "移动预览"
            };
            let detail = match drag.handle {
                MoveHandle::RotationY => format!("{:.1}°", drag.preview.yaw_degrees),
                MoveHandle::UniformScale => {
                    let [a, b, c] = drag.preview.scale;
                    format!("({a:.3}, {b:.3}, {c:.3})")
                }
                _ => format!("({x:.3}, {y:.3}, {z:.3})"),
            };
            editor.status = format!("{label} {}：{detail}；Esc 取消", drag.handle.label());
        } // 高度页到达后恢复有效预览时必须清除旧等待提示，不能仍告知用户“松手将取消”。
        if !mouse.pressed(MouseButton::Left) {
            if !mouse.just_released(MouseButton::Left) || !drag.valid {
                world.cancel(&mut session, &drag);
                editor.status = "目标尚未就绪，已取消变换并恢复姿态".into();
            } else if drag.before.building_preset_id.is_some() {
                if drag.preview.translation != drag.before.translation
                    || drag.preview.yaw_degrees != drag.before.yaw_degrees
                {
                    let target = BuildingEditTarget {
                        scene_id: drag.scene_id.clone(),
                        placement_id: drag.before.id.clone(),
                        record_index: drag.record_index,
                        structure_revision: drag.structure_revision,
                        author_revision: drag.author_revision,
                    };
                    let request = if drag.handle == MoveHandle::RotationY {
                        BuildingEditRequest::RotateTo {
                            target: target.clone(),
                            yaw_degrees: drag.preview.yaw_degrees,
                        }
                    } else {
                        BuildingEditRequest::Relocate {
                            target: target.clone(),
                            world_xz: Vec3::from_array(drag.preview.translation).xz(),
                            terrain_hit: None,
                        }
                    };
                    buildings.write(request);
                    state.pending_building = Some(PendingBuildingMove { target, drag });
                    editor.status = "建筑候选已提交；通过地基与重叠校验后采用姿态".into();
                } else {
                    world.cancel(&mut session, &drag);
                    editor.status = "姿态未变化".into();
                }
            } else {
                // commit 的失败不能遗留暂存预览；恢复操作使用当前权威记录。
                world.cancel(&mut session, &drag);
                match world.commit(&mut session, &mut editor, drag) {
                    Ok(true) => editor.status = "已更新资产变换；本次拖动可撤销".into(),
                    Ok(false) => editor.status = "姿态未变化".into(),
                    Err(error) => editor.status = format!("变换未应用：{error}"),
                }
            }
            return;
        }
        state.drag = Some(drag);
        return;
    }
    if state.owns_mouse {
        keyboard.clear();
        return;
    }
    if !available
        || guard.blocked()
        || context.mode.pointer_over_controls
        || context.candidates.pointer_over_ui
        || context
            .controls
            .iter()
            .any(RelativeCursorPosition::cursor_over)
        || context
            .popups
            .iter()
            .any(RelativeCursorPosition::cursor_over)
    {
        return;
    }
    let (Some(viewport), Some(camera), Some((cursor, _))) = (viewport, camera, node) else {
        return;
    };
    if !cursor.cursor_over() {
        return;
    }
    let Some(normalized) = cursor.normalized else {
        return;
    };
    let Some((record_index, record)) = selected_record(
        &editor,
        &session,
        &context.browser,
        &context.candidates,
        &viewport,
    ) else {
        return;
    };
    let Some(draft) = session.draft.as_ref() else { return; };
    let Ok(frame) = placement_record_frame(draft, record) else { return; };
    let Ok(origin) = placement_record_world_position(draft, record) else { return; };
    let Ok(source_bounds) = crate::authoring::placement_record_bounds(draft, record) else { return; };
    let Ok(world_before) = source_to_world(feathrise_scene_content::TerrainObjectTransform {
        translation: record.translation, yaw_degrees: record.yaw_degrees, scale: record.scale, ..default()
    }, frame) else { return; };
    let handle = match context.mode.mode {
        ViewportMode::Move => {
            camera.pick_handle(origin, normalized, record.building_preset_id.is_some())
        }
        ViewportMode::Rotate => {
            pick_rotation(camera, origin, normalized).then_some(MoveHandle::RotationY)
        }
        ViewportMode::Scale if record.building_preset_id.is_none() => {
            pick_scale(camera, origin, normalized).then_some(MoveHandle::UniformScale)
        }
        _ => None,
    };
    let Some(handle) = handle else {
        return;
    };
    state.hover = Some(handle);
    if !mouse.just_pressed(MouseButton::Left)
        || mouse.any_pressed([MouseButton::Right, MouseButton::Middle])
    {
        return;
    }
    state.owns_mouse = true;
    if record.building_preset_id.is_some() && context.validation.pending() {
        editor.status = "上一项建筑操作仍在校验，请等待完成后继续变换".into();
        keyboard.clear();
        return;
    }
    if record.building_preset_id.is_some()
        && controls.level_terrain
        && handle.mode() == ViewportMode::Move
    {
        editor.status = "联合整平需要真实地形表层；请用“吸附地面”，或关闭联合整平后拖动手柄".into();
        keyboard.clear();
        return;
    }
    let patch_source = match world.patch_source_snapshot(draft, record) {
        Ok(source) => source,
        Err(error) => { editor.status = format!("暂不能变换：{error}"); keyboard.clear(); return; }
    };
    if session
        .draft
        .as_ref()
        .and_then(|draft| draft.manifest.world_bounds())
        .is_none()
    {
        return;
    }
    let Some(start_point) = camera
        .ray(normalized)
        .and_then(|ray| handle_point(ray, origin, handle))
    else {
        return;
    };
    state.drag = Some(MoveDrag {
        scene_id: session
            .draft
            .as_ref()
            .expect("选中草稿已就绪")
            .scene_id
            .clone(),
        record_index,
        structure_revision: session.placement_structure_revision,
        author_revision: session.revision,
        terrain_revision: session.terrain_revision,
        terrain_content,
        before: record.clone(),
        preview: record.clone(),
        applied_world: world_before,
        frame,
        world_origin: origin,
        world_before,
        source_bounds,
        patch_source,
        formal_runtime_identity: world.group_runtime_identity(),
        handle,
        start_point,
        start_cursor: normalized,
        camera,
        viewport_ui_size: node.expect("已确认视口节点存在").1.size(),
        building_controls: clipmap_controls_identity(&controls),
        snapping: context.preferences.as_ref().map_or_else(Default::default, |settings| settings.editor_snapping.sanitized()),
        translation_control: keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]),
        valid: true,
        preview_applied: false,
    });
    editor.status = format!("正在拖动 {}；Ctrl 吸附，Esc 取消", handle.label());
    keyboard.clear();
}
