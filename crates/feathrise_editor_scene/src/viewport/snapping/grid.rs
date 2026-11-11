//! 发布固定世界原点的无限参考网格；相对吸附与真实落点十字独立使用原手势规则。
use crate::{
    authoring::EditorAuthoringSession,
    resources::{SceneEditorState, SceneEditorViewportState},
    types::EditorView,
    ui::{
        gltf_browser::EditorGltfBrowserState,
        vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState},
    },
    viewport::{multi_select::PlacementSelectionSet, placement_move::PlacementMoveState},
};
use bevy::{camera::visibility::RenderLayers, gizmos::config::GizmoConfigStore, prelude::*};
use feathrise_editor_core::EditorSettings;
use feathrise_editor_core::EditorSnapMode;

#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct EditorGridGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct EditorGridOutlineGizmos;

pub(crate) fn configure_grid(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<EditorGridGizmos>();
    config.line.width = 2.0; // 仅保留真实落点十字的清晰芯线；参考网格独立使用GPU细线。
    config.line.perspective = false;
    config.depth_bias = -1.0; // 真实落点十字需越过起伏地面前置显示；只改绘制深度，不抬高真实吸附落点。
    config.render_layers = RenderLayers::layer(super::super::SCENE_EDITOR_BRUSH_RENDER_LAYER);
    let (outline, _) = store.config_mut::<EditorGridOutlineGizmos>();
    outline.line.width = 3.5;
    outline.line.perspective = false;
    outline.depth_bias = -0.98; // 分离两层深度，正交时也留足间距；配合深度写入稳定常规编辑范围的浅芯与深边。
    outline.render_layers = RenderLayers::layer(super::super::SCENE_EDITOR_BRUSH_RENDER_LAYER);
}

/// 一份坐标同时发给两层，芯线必须不透明，避免背景纹理/阴影和组画序改变其辨识度。
fn outlined_line(core: &mut Gizmos<EditorGridGizmos>, outline: &mut Gizmos<EditorGridOutlineGizmos>, start: Vec3, end: Vec3, color: Color) {
    outline.line(start, end, Color::srgb(0.035, 0.030, 0.024));
    core.line(start, end, color);
}

struct GridFrame {
    stride: f32,
    axes: [usize; 2],
}
impl GridFrame {
    fn new(step: f32, view: EditorView) -> Option<Self> {
        if !step.is_finite() || step <= 0.0 {
            return None;
        }
        let axes = match view {
            EditorView::Front => [0, 1],
            EditorView::Side => [2, 1],
            _ => [0, 2],
        };
        Some(Self { stride: step, axes })
    }

    fn view(&self) -> super::components::GridView {
        let fixed = (0..3).find(|axis| !self.axes.contains(axis)).unwrap();
        let basis = [Vec3::X, Vec3::Y, Vec3::Z];
        super::components::GridView {
            origin_stride: Vec3::ZERO.extend(self.stride),
            plane: basis[fixed].extend(0.0),
            axis_u: basis[self.axes[0]].extend(0.0),
            axis_v: basis[self.axes[1]].extend(0.0),
        }.world_anchored()
    }
}

pub(crate) fn draw_grid(
    editor: Res<SceneEditorState>,
    settings: Res<EditorSettings>,
    viewport: Option<Res<SceneEditorViewportState>>,
    session: Res<EditorAuthoringSession>,
    browser: Res<EditorGltfBrowserState>,
    selection: Res<PlacementSelectionSet>,
    state: Res<PlacementMoveState>,
    workspace: Res<VegetationPresetEditorState>,
    (mut gizmos, mut outline): (
        Gizmos<EditorGridGizmos>,
        Gizmos<EditorGridOutlineGizmos>,
    ),
    cursor: Res<super::MeshSnapCursor>,
    batch: Res<crate::authoring::placement_batch::PlacementBatchState>,
    transform: Res<crate::ui::placement_transform::PlacementTransformInputState>,
    keyboard: Res<ButtonInput<KeyCode>>,
    controls: Res<crate::ui::building_presets::BuildingPlacementControls>,
    terrain_transform: Option<Res<super::super::terrain_transform::TransformState>>,
    (mut commands, cameras): (
        Commands,
        Query<(Entity, Option<&super::components::GridView>), With<crate::components::SceneEditorCamera>>,
    ),
) {
    // 所有退出门也必须撤下上一帧组件；否则关闭网格/切换场景后渲染世界仍会保留旧平面。
    let desired = (|| {
        if !editor.open
            || editor.scene_transition_in_progress
            || workspace.active_tab != PresetWorkspaceTab::Scene
        {
            return None;
        }
        let (Some(viewport), Some(draft)) = (viewport.as_deref(), session.draft.as_ref()) else {
            return None;
        };
        if viewport.framed_scene_id.as_ref() != Some(&draft.scene_id)
            || editor.selected_scene_id.as_ref() != Some(&draft.scene_id)
            || !viewport.center.is_finite()
            || !viewport.extent.is_finite()
        {
            return None;
        }
        let snap = settings.editor_snapping.sanitized();
        if !snap.show_grid {
            return None;
        }
        // 参考网格只采用配置间距与观察平面，选择、落点、冻结吸附相位及观察目标均不能移动它。
        // GPU继续按像素足迹抽稀；透明合成不读地形深度，因此无需抬高平面来避免重叠。
        let frame = GridFrame::new(snap.translation_meters, editor.view)?;
        let (selected, selected_building) = if editor.tool != crate::types::EditorTool::Inspect
            && !transform.relocation_armed()
        {
            (None, false)
        } else if selection.members.len() > 1 && selection.ready_for(&session) {
            (Some(selection.center), false)
        } else {
            browser
                .selected_placement_id
                .as_deref()
                .and_then(|id| browser.placement_record_index(id, session.placement_structure_revision))
                .and_then(|index| draft.placements.placements.get(index))
                .filter(|record| browser.selected_placement_id.as_deref() == Some(record.id.as_str()))
                .map(|record| (Some(Vec3::from_array(record.translation)), record.building_preset_id.is_some()))
                .unwrap_or((None, false))
        };
        let control = keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
        let terrain_reference = terrain_transform.as_ref().and_then(|state| state.grid_reference());
        let (_focus, translation_snap) = if let Some((focus, frozen)) = terrain_reference {
            (focus, frozen) // 下列原吸附策略只决定真实落点十字大小，不再决定参考网格原点或高度。
        } else if let Some((_, focus, frozen)) = batch.grid_reference().or_else(|| state.grid_reference(control)) {
            (focus, frozen) // 已有手势返回真实步长与相位，不能再按当前工具或全局偏好猜测建筑规则。
        } else {
            let focus = selected
                .or(cursor.preview)
                .or(editor.cursor_world)
                .unwrap_or(viewport.center);
            let mut reference = snap;
            if (editor.tool == crate::types::EditorTool::Building || selected_building)
                && (!snap.enabled || !snap.active(control))
                && controls.grid_step_meters > 0.0
            {
                reference.translation_meters = controls.grid_step_meters;
                reference.mode = EditorSnapMode::WorldGrid;
            } // 落点十字保留建筑原步长；手柄移动则以上面的本笔实际策略为准。
            (focus, reference.translation_snap(control, selected.unwrap_or(Vec3::ZERO).to_array()))
        };
        if let Some(point) = cursor.preview.or_else(|| {
            (!cursor.requires_query()
                && (editor.tool == crate::types::EditorTool::PlaceAsset
                    || transform.relocation_armed()))
            .then_some(editor.cursor_world)
            .flatten()
        }) {
            let radius = (translation_snap.step() * 0.2).clamp(0.05, 0.5);
            let color = Color::srgb(0.92, 0.72, 0.40);
            outlined_line(&mut gizmos, &mut outline, point - Vec3::X * radius, point + Vec3::X * radius, color);
            outlined_line(&mut gizmos, &mut outline, point - Vec3::Z * radius, point + Vec3::Z * radius, color);
        }
        Some(frame.view())
    })();
    for (entity, installed) in &cameras {
        if installed == desired.as_ref() {
            continue;
        }
        if let Some(desired) = desired {
            commands.entity(entity).insert(desired);
        } else {
            commands.entity(entity).remove::<super::components::GridView>();
        }
    } // 只处理作者相机的常数参数；没有逐线、逐像素CPU编码或整图扫描。
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grid_keeps_world_origin_and_uses_the_configured_display_step() {
        let grid = GridFrame::new(0.5, EditorView::Top).unwrap().view();
        assert_eq!(grid.origin_stride, Vec4::new(0.0, 0.0, 0.0, 0.5));
        assert_eq!(grid.plane, Vec3::Y.extend(0.0));
        for step in [0.1, 1.0, 100.0] {
            let grid = GridFrame::new(step, EditorView::Top).unwrap().view();
            assert_eq!(grid.origin_stride.truncate(), Vec3::ZERO);
            assert_eq!(grid.origin_stride.w, step);
            assert_eq!(grid.plane.w, 0.0);
        }
    }
    #[test]
    fn grid_supports_front_side_and_rejects_invalid_inputs() {
        assert_eq!(
            GridFrame::new(1.0, EditorView::Front)
                .unwrap()
                .axes,
            [0, 1]
        );
        assert_eq!(
            GridFrame::new(1.0, EditorView::Side)
                .unwrap()
                .axes,
            [2, 1]
        );
        for step in [f32::NAN, f32::INFINITY, 0.0, -1.0] {
            assert!(GridFrame::new(step, EditorView::Top).is_none());
        }
    }

    #[test]
    fn world_reference_grid_does_not_change_real_building_or_relative_drag_snapping() {
        use crate::viewport::placement_move::interaction_tests::{app, camera, step};
        use feathrise_editor_core::EditorSnapSettings;

        for (building, enabled, control, native) in [
            (true, false, false, true),
            (true, false, true, true),
            (true, true, false, false),
            (true, true, true, true),
            (false, true, false, false),
            (false, true, true, false),
        ] {
            let (mut app, root, node) = app();
            let origin = Vec3::new(5.3, 4.0, 5.0); // 非整数起点区分原世界格与相对步长，不能只比较格线间距。
            {
                let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
                let record = &mut session.draft.as_mut().unwrap().placements.placements[0];
                record.translation = origin.to_array();
                record.building_preset_id = building.then(|| "grid-test-building".into());
            }
            app.world_mut().get_mut::<Transform>(root).unwrap().translation = origin;
            app.world_mut().resource_mut::<crate::ui::building_presets::BuildingPlacementControls>().grid_step_meters = 2.0;
            app.insert_resource(EditorSettings {
                editor_snapping: EditorSnapSettings { enabled, mode: EditorSnapMode::Relative, ..default() },
                ..default()
            });
            let camera = camera(&app);
            let grabbed = origin + Vec3::X * (camera.handle_length(origin).unwrap() * 0.65);
            if control {
                app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ControlLeft);
            }
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
            step(&mut app, node, camera.project(grabbed).unwrap());
            assert_eq!(app.world().resource::<SceneEditorState>().tool, crate::types::EditorTool::Inspect);
            assert!(app.world().resource::<PlacementMoveState>().grid_reference(control).is_some());
            app.world_mut().resource_mut::<EditorSettings>().editor_snapping.translation_meters = 4.0; // 真实手势仍使用本笔冻结偏好。
            let cursor = camera.project(grabbed + Vec3::X * 1.6).unwrap();
            step(&mut app, node, cursor);
            let (_, focus, snap) = app.world().resource::<PlacementMoveState>().grid_reference(control).unwrap();
            let grid = GridFrame::new(4.0, EditorView::Top).unwrap().view();
            assert_eq!(snap.step(), if native { 2.0 } else { 1.0 });
            assert_eq!(snap.mode, if native { EditorSnapMode::WorldGrid } else { EditorSnapMode::Relative });
            let snap_origin = if snap.mode == EditorSnapMode::Relative { Vec3::from_array(snap.origin()) } else { Vec3::ZERO };
            assert_eq!(snap_origin, if native { Vec3::ZERO } else { origin });
            assert_eq!(grid.origin_stride, Vec4::new(0.0, 0.0, 0.0, 4.0));
            assert_eq!(grid.plane.w, 0.0);
            let actual = app.world().get::<Transform>(root).unwrap().translation;
            assert!(actual.distance(focus) < 0.002);
            assert!((actual.x - snap.axis(origin.x + 1.6, 0).unwrap()).abs() < 0.002);
            if snap.active() {
                let phase = (actual.x - snap_origin.x) / snap.step();
                assert!((phase - phase.round()).abs() < 0.002);
            }
            if building {
                app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
                step(&mut app, node, cursor);
                // 松手后Ctrl释放仍不能改写已提交建筑候选的实际吸附策略。
                let (_, _, waiting) = app.world().resource::<PlacementMoveState>().grid_reference(!control).unwrap();
                assert_eq!(waiting, snap);
            }
        }
    }
}
