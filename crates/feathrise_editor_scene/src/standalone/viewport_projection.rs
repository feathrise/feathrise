//! 只读投影原作者视口的几何与遮挡状态；宿主不获取私有草稿、组件或输入门的写权限。

use crate::{components::SceneEditorViewport, resources::*};
use crate::ui::{
    asset_preview::{AssetPreviewWorkspace, gltf_tree},
    vegetation_preset_editor::{VegetationPresetEditorState, activate_scene_tab},
};
use crate::workbench::WorkbenchHostPageState;
use bevy::prelude::*;

#[path = "embedded_render_controls.rs"]
mod controls;
pub use controls::{EmbeddedRenderView, capture as capture_render};
#[path = "viewport_occlusion.rs"]
mod occlusion;
#[path = "viewport_axis_occlusion.rs"]
mod axis_occlusion;

/// 作者控件实际绘制的物理轮廓；场景作者层不依赖进程协议或原生窗口类型。
#[derive(Clone, Debug, PartialEq)]
pub enum ViewportControlShape {
    Ellipse { outer: Rect, inner: Option<Rect> },
    Quad { points: [Vec2; 4] },
}

/// 坐标直接来自完成布局的 UI 节点，使用主窗口客户区物理像素。
#[derive(Resource, Default, Clone)]
pub struct ViewportHostProjection {
    pub layout: Option<(Vec2, Vec2)>,
    /// 仅表示须整窗让出的模态或无效上下文；普通浮层和作者输入门不切换3D画面来源。
    pub obscured: bool,
    /// 原生child只在这些作者UI像素区域让出窗口；相机、RTT和拾取投影尺寸保持完整。
    pub excluded_rects: Vec<Rect>,
    /// XYZ 透明根不留矩形孔，只让出三条线、六个端点和中心实际占用的像素。
    pub excluded_shapes: Vec<ViewportControlShape>,
    pub error: Option<String>,
}

#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ViewportProjectionSet;

/// 固定的中央 GLB 标签始终使用作者画布，即使地图保留了工程 3D 模式。
pub fn central_glb_active(
    editor: Option<&SceneEditorState>,
    workspace: Option<&VegetationPresetEditorState>,
    media: Option<&AssetPreviewWorkspace>,
    host_page: Option<&WorkbenchHostPageState>,
) -> bool {
    editor.is_some_and(|editor| editor.open)
        && !host_page.is_some_and(|page| page.active)
        && workspace.zip(media).is_some_and(|(workspace, media)| {
            gltf_tree::central_glb(media, workspace.active_tab).is_some()
        })
}

/// 工程 3D 按钮返回现有场景标签，媒体标签和导航按原工作区规则留存。
pub fn return_from_central_glb(world: &mut World) {
    if central_glb_active(
        world.get_resource(),
        world.get_resource(),
        world.get_resource(),
        world.get_resource(),
    ) {
        let _ = world.run_system_cached(activate_scene_tab);
    }
}

pub fn install(app: &mut App) {
    app.add_systems(
        Update,
        controls::sync.before(crate::SceneEditorSystemSet::Input),
    );
    app.init_resource::<ViewportHostProjection>().add_systems(
        PostUpdate,
        publish
            .in_set(ViewportProjectionSet)
            .after(bevy::ui::UiSystems::PostLayout)
            .after(crate::ui::tooltips::TooltipPositioned)
            .after(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
    );
}

/// 模式切换当帧读取真实事务标记，不能使用上一帧几何投影放行保存或切换场景。
pub fn switch_pending(world: &World) -> bool {
    world
        .get_resource::<SceneEditorState>()
        .is_some_and(|editor| {
            editor.save_requested
                || editor.scene_transition_in_progress
                || editor.pending_scene_transition.is_some()
                || editor.scene_switch_confirmation.is_some()
        })
}

fn publish(
    mut projection: ResMut<ViewportHostProjection>,
    nodes: Query<
        (&ComputedNode, &UiGlobalTransform, &InheritedVisibility),
        With<SceneEditorViewport>,
    >,
    editor: Option<Res<SceneEditorState>>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    mut release: Local<SceneEditorNumericScrubMouseGuard>,
    browser: Option<Res<SceneBrowserState>>,
    deletion: Option<Res<SceneDeletionTask>>,
    terrain: Option<Res<crate::authoring::TerrainExternalChangeState>>,
    menus: (
        Option<Res<crate::ui::asset_context_menu::AssetContextMenuState>>,
        Option<Res<crate::ui::scene_context_menu::SceneContextMenuState>>,
        Option<Res<crate::ui::scene_sidebar::SceneDirectoryMenuState>>,
        Option<Res<crate::ui::asset_preview::AssetTabMenuState>>,
        Option<Res<crate::ui::project_browser::ProjectBrowserPopup>>,
    ),
    settings: Option<Res<SceneEditorSettingsState>>,
    roots: Query<
        (
            &Node,
            &ComputedNode,
            &UiGlobalTransform,
            &InheritedVisibility,
        ),
        occlusion::UiRoots,
    >,
    borders: Query<
        (
            &Node,
            &ComputedNode,
            &UiGlobalTransform,
            &InheritedVisibility,
        ),
        occlusion::SelectionBorders,
    >,
    axes: axis_occlusion::AxisOcclusion,
) {
    projection.layout = nodes.iter().find_map(|(node, transform, visible)| {
        (visible.get() && !node.is_empty()).then_some((node.size(), transform.translation))
    });
    projection.error = None;
    projection.excluded_rects.clear();
    projection.excluded_shapes.clear();
    if let Some((size, center)) = projection.layout {
        match occlusion::capture(
            Rect::from_center_size(center, size),
            roots.iter(),
            borders.iter(),
        ) {
            Ok(rects) => projection.excluded_rects = rects,
            Err(error) => projection.error = Some(error),
        }
        match axes.capture(Rect::from_center_size(center, size)) {
            Ok(shapes) => projection.excluded_shapes = shapes,
            Err(error) => projection.error = Some(error),
        }
    }
    // Windows child 总在父窗口 GPU 图层之上：原管理模态仍让出整个 child。
    // 提示、详情和帮助只按实际矩形留孔；公共输入门与单帧拾取抑制不能隐藏整个游戏画面，
    // 否则只是阅读说明也会露出父编辑器的另一套World，造成天空、XYZ和场景颜色突变。
    let obscured = menus.0.as_ref().is_some_and(|state| state.owns_mouse())
        || menus.1.as_ref().is_some_and(|state| state.owns_mouse())
        || menus.2.as_ref().is_some_and(|state| state.owns_mouse())
        || menus.3.as_ref().is_some_and(|state| state.owns_mouse())
        || menus.4.as_ref().is_some_and(|state| state.owns_mouse())
        || settings
            .as_ref()
            .is_some_and(|state| state.open || state.owns_mouse_this_frame)
        || browser
            .as_ref()
            .is_some_and(|browser| browser.dialog.is_some())
        || deletion
            .as_ref()
            .is_some_and(|deletion| deletion.confirmation.is_some())
        || terrain.as_ref().is_some_and(|terrain| terrain.modal_open())
        || editor.as_ref().is_none_or(|editor| {
            !editor.open
                || editor.new_scene_dialog_open
                || editor.scene_switch_confirmation.is_some()
                || editor.scene_transition_in_progress
        });
    let pressed = buttons.as_ref().is_some_and(|buttons| {
        [MouseButton::Left, MouseButton::Middle, MouseButton::Right]
            .into_iter()
            .any(|button| buttons.pressed(button))
    });
    let released = buttons.as_ref().is_some_and(|buttons| {
        [MouseButton::Left, MouseButton::Middle, MouseButton::Right]
            .into_iter()
            .any(|button| buttons.just_released(button))
    });
    release.advance(obscured || projection.error.is_some(), pressed, released);
    projection.obscured = release.blocked();
}

#[cfg(test)]
#[path = "viewport_overlay_tests.rs"]
mod overlay_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirty_scene_switch_confirmation_owns_the_child_surface_until_dismissed() {
        let mut app = App::new();
        app.insert_resource(SceneEditorState {
            open: true,
            scene_switch_confirmation: Some(crate::types::SceneSwitchConfirmation {
                source_scene_id: None,
                source_revision: 1,
                global_revision: 0,
                target_scene_id: feathrise_scene_content::SceneId("MAP-switch".into()),
                target_display_name: "目标场景".into(),
                diagnostic: None,
                stage: crate::types::SceneSwitchStage::Choosing,
            }),
            ..default()
        });
        install(&mut app);
        app.update();
        assert!(switch_pending(app.world()));
        assert!(app.world().resource::<ViewportHostProjection>().obscured);
        app.world_mut()
            .resource_mut::<SceneEditorState>()
            .scene_switch_confirmation = None;
        app.update();
        assert!(!switch_pending(app.world()));
        assert!(!app.world().resource::<ViewportHostProjection>().obscured);
    }

    #[test]
    fn author_gesture_keeps_game_visible_but_dialog_close_waits_for_silent_mouse_frame() {
        let mut app = App::new();
        app.insert_resource(SceneEditorState {
            open: true,
            ..default()
        })
        .init_resource::<SceneEditorSettingsState>()
        .init_resource::<SceneEditorNumericScrubMouseGuard>()
        .init_resource::<ButtonInput<MouseButton>>();
        install(&mut app);
        app.world_mut()
            .resource_mut::<SceneEditorNumericScrubMouseGuard>()
            .advance(true, true, false);
        app.update();
        assert!(
            !app.world().resource::<ViewportHostProjection>().obscured,
            "手柄/刷子拥有鼠标不等于UI遮挡"
        );
        app.world_mut()
            .resource_mut::<SceneEditorSettingsState>()
            .open = true;
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        assert!(app.world().resource::<ViewportHostProjection>().obscured);
        app.world_mut()
            .resource_mut::<SceneEditorSettingsState>()
            .open = false;
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        app.update();
        assert!(app.world().resource::<ViewportHostProjection>().obscured);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        app.update();
        assert!(!app.world().resource::<ViewportHostProjection>().obscured);
    }
}
