//! 地点表单借用当前场景视口取两个对角，保持作者填写的垂直范围；回填后由「保存地点」统一写盘。
use super::{DocumentState, Identity, Selection, Studio, Tab, active, values};
use crate::{
    authoring::EditorAuthoringSession,
    resources::SceneEditorState,
    types::EditorTool,
    ui::{
        gameplay_sidebar::{GameplaySidebarPage, GameplaySidebarState},
        sidebars::{EditorLeftSidebarTab, SceneEditorSidebarState},
        vegetation_preset_editor::VegetationPresetEditorState,
    },
    viewport::{
        SceneEditorBrushGizmos,
        modes::{ViewportMode, ViewportModeState},
        plane_pointer::PlanePointer,
    },
};
use bevy::prelude::*;
use feathrise_editor_core::PresetWorkspaceTab;
use feathrise_scene_content::SceneId;

#[derive(Message)]
pub(super) struct DrawLocation;

struct Drawing {
    document: Identity,
    selection: Selection,
    scene: SceneId,
    anchor: Option<Vec3>,
    bottom: f32,
    top: f32,
}
#[derive(Resource, Default)]
struct LocationDrawing(Option<Drawing>);

pub(super) fn install(app: &mut App) {
    app.add_message::<DrawLocation>()
        .init_resource::<LocationDrawing>()
        .add_systems(
            Update,
            (
                start.run_if(on_message::<DrawLocation>),
                pick.run_if(|s: Res<LocationDrawing>| s.0.is_some()),
            )
                .chain()
                .after(super::controls::input)
                .before(crate::viewport::modes::sync_mode),
        );
}

fn height(text: &str) -> Option<f32> {
    let value = values::xyz(text).ok()?[1].parse::<f32>().ok()?;
    value.is_finite().then_some(value)
}

fn start(
    mut messages: MessageReader<DrawLocation>,
    docs: Res<DocumentState>,
    session: Res<EditorAuthoringSession>,
    mut s: ResMut<Studio>,
    mut drawing: ResMut<LocationDrawing>,
    world_tab: Res<VegetationPresetEditorState>,
    mut editor: ResMut<SceneEditorState>,
    mut mode: ResMut<ViewportModeState>,
) {
    if messages.read().count() == 0 || s.sel.tab != Tab::Locations || !s.editing_item {
        return;
    }
    let Some(document) = active(&docs).filter(|k| s.key.as_ref() == Some(k)) else {
        return;
    };
    if !docs.rules_in_sidebar() || world_tab.active_tab != PresetWorkspaceTab::Scene {
        s.notice = "请先在中央打开目标场景，再在交互侧栏绘制。".into();
        return;
    }
    let Some(scene) = editor.selected_scene_id.as_ref().filter(|id| {
        !editor.scene_transition_in_progress
            && session.draft.as_ref().is_some_and(|d| &d.scene_id == *id)
    }) else {
        s.notice = "请先打开要绘制地点的场景".into();
        return;
    };
    if !s.form.scene.trim().is_empty() && s.form.scene.trim() != scene.0 {
        s.notice = format!("请切换到地点所属的场景「{}」，或修改场景 ID", s.form.scene);
        return;
    }
    let Some((bottom, top)) = height(&s.form.min)
        .zip(height(&s.form.max))
        .filter(|(lo, hi)| lo < hi)
    else {
        s.notice = "请在最小、最大坐标中填写有效的 Y 值，顶部须高于底面".into();
        return;
    };
    drawing.0 = Some(Drawing {
        document,
        selection: s.sel,
        scene: scene.clone(),
        anchor: None,
        bottom,
        top,
    });
    mode.mode = ViewportMode::Edit;
    editor.tool = EditorTool::LocationRegion;
    editor.status = "地点区域 · 在视口点击第一个角，Esc 取消".into();
}

fn corners(first: Vec3, second: Vec3, bottom: f32, top: f32) -> Option<(Vec3, Vec3)> {
    let min = Vec3::new(first.x.min(second.x), bottom, first.z.min(second.z));
    let max = Vec3::new(first.x.max(second.x), top, first.z.max(second.z));
    (min.is_finite() && max.is_finite() && max.cmpgt(min).all()).then_some((min, max))
}

fn pick(
    mut drawing: ResMut<LocationDrawing>,
    docs: Res<DocumentState>,
    mut s: ResMut<Studio>,
    mut editor: ResMut<SceneEditorState>,
    mut mode: ResMut<ViewportModeState>,
    sidebar: Res<SceneEditorSidebarState>,
    pages: Res<GameplaySidebarState>,
    workspace: Res<VegetationPresetEditorState>,
    pointer: PlanePointer,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut gizmos: Gizmos<SceneEditorBrushGizmos>,
    model: Option<Res<crate::workbench::WorkbenchModel>>,
) {
    let Some(d) = drawing.0.as_mut() else { return };
    // 只有启动取点时那一行、那一份修订和那一张地图能接收结果；手动导航立即释放视口。
    let same = editor.open
        && !editor.close_requested
        && !editor.scene_transition_in_progress
        && editor.selected_scene_id.as_ref() == Some(&d.scene)
        && sidebar.tool_page_visible(EditorLeftSidebarTab::Gameplay)
        && pages.page == GameplaySidebarPage::Locations
        && mode.mode == ViewportMode::Edit
        && editor.tool == EditorTool::LocationRegion
        && workspace.active_tab == PresetWorkspaceTab::Scene
        && s.sel == d.selection
        && s.editing_item
        && s.key.as_ref() == Some(&d.document)
        && docs.rules_in_sidebar()
        && docs
            .rules_sidebar_document()
            .is_some_and(|doc| d.document.matches(doc));
    if same {
        if keys.just_pressed(KeyCode::Escape) {
            keys.clear_just_pressed(KeyCode::Escape);
            s.notice = "已取消绘制，保留原地点范围".into();
        } else {
            if mode.pointer_over_controls
                || editor.new_scene_dialog_open
                || editor.scene_switch_confirmation.is_some()
                || model.is_some_and(|m| m.modal || m.busy)
            {
                return;
            }
            let Some(point) = pointer.on_plane(d.bottom) else {
                return;
            };
            let bounds = d.anchor.and_then(|a| corners(a, point, d.bottom, d.top));
            if let Some((min, max)) = bounds {
                gizmos.cube(
                    Transform::from_translation(min * 0.5 + max * 0.5).with_scale(max - min),
                    Color::srgb(0.20, 0.86, 0.85),
                );
            }
            if !mouse.just_pressed(MouseButton::Left) {
                return;
            }
            mouse.clear_just_pressed(MouseButton::Left); // 第二角完成时，同一次点击不会变成视口点选。
            if d.anchor.is_none() {
                d.anchor = Some(point);
                editor.status = "地点区域 · 点击另一对角，Esc 取消".into();
                return;
            }
            let Some((min, max)) = bounds else {
                editor.status = "请点击另一对角，使范围具有宽度和深度".into();
                return;
            };
            let xyz = |v: Vec3| format!("{}, {}, {}", v.x, v.y, v.z);
            s.form.scene.clone_from(&d.scene.0);
            s.form.min = xyz(min);
            s.form.max = xyz(max);
            s.notice = "地点范围已回填，点击「保存地点」写入工程".into();
            s.refresh();
        }
        editor.status.clone_from(&s.notice);
    }
    if editor.tool == EditorTool::LocationRegion {
        editor.tool = EditorTool::Inspect;
        if mode.mode == ViewportMode::Edit {
            mode.mode = ViewportMode::Select;
        }
    }
    drawing.0 = None;
}
