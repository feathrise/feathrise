//! 作者工具当帧语义投影；输入、选择解析和编辑历史继续由原工具拥有。

pub(crate) mod foreground;

use crate::{components::SceneEditorViewport, resources::{SceneEditorState, SceneEditorViewportState}};
use super::placement_move::MoveCamera;
use bevy::{camera::visibility::RenderLayers, gizmos::config::GizmoConfigStore, prelude::*};
use feathrise_author_tools::{AuthorTool, AuthorToolPreview, TransformPreview, MAX_AUTHOR_BRUSHES, MAX_AUTHOR_TRANSFORMS, MAX_AUTHOR_SELECTIONS};

#[derive(Resource, Default)]
pub(crate) struct AuthorToolFrame {
    pub(crate) preview: AuthorToolPreview,
    pub(crate) error: Option<String>,
    brushes: usize,
    transforms: usize,
    selections: usize,
}

impl AuthorToolFrame {
    pub(crate) fn push(&mut self, item: AuthorTool) {
        if self.error.is_some() { return; }
        let (count, limit) = match &item {
            AuthorTool::Brush(_) => (&mut self.brushes, MAX_AUTHOR_BRUSHES),
            AuthorTool::Transform(_) => (&mut self.transforms, MAX_AUTHOR_TRANSFORMS),
            AuthorTool::Selection(_) => (&mut self.selections, MAX_AUTHOR_SELECTIONS),
        };
        *count += 1;
        if *count > limit {
            self.preview.items.clear();
            self.error = Some("作者工具预览超过分类预算，本帧预览已清空".into());
            return; // 超限不发送部分选择，下一帧重新收集完整结果。
        }
        self.preview.items.push(item);
    }
}

#[derive(Default, Reflect, GizmoConfigGroup)]
struct AuthorToolGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
struct AuthorSpatialToolGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
struct AuthorTransformGizmos;

pub(crate) fn install(app: &mut App) {
    foreground::install(app);
    app.init_resource::<AuthorToolFrame>()
        .init_gizmo_group::<AuthorToolGizmos>()
        .init_gizmo_group::<AuthorSpatialToolGizmos>()
        .init_gizmo_group::<AuthorTransformGizmos>()
        .add_systems(Startup, configure)
        .add_systems(First, clear)
        .add_systems(PostUpdate, draw);
}

fn clear(mut frame: ResMut<AuthorToolFrame>, mut foreground: ResMut<foreground::Foreground>) {
    frame.preview.items.clear();
    frame.error = None;
    frame.brushes = 0;
    frame.transforms = 0;
    frame.selections = 0;
    foreground.clear();
}

fn configure(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<AuthorToolGizmos>();
    config.render_layers = RenderLayers::layer(31);
    feathrise_scene::author_view::tools::configure(config, false);
    let (config, _) = store.config_mut::<AuthorSpatialToolGizmos>();
    config.render_layers = RenderLayers::layer(31);
    feathrise_scene::author_view::tools::configure(config, true);
    let (config, _) = store.config_mut::<AuthorTransformGizmos>();
    config.render_layers = RenderLayers::layer(31);
    feathrise_scene::author_view::tools::configure(config, false);
    config.depth_bias = -1.; // 单轴旋转与中心标记同样盖过物体表面。
}

fn draw(
    frame: Res<AuthorToolFrame>,
    editor: Res<SceneEditorState>,
    viewport: Option<Res<SceneEditorViewportState>>,
    nodes: Query<&ComputedNode, With<SceneEditorViewport>>,
    snapshot: Option<Res<crate::standalone::embedded_snapshot::EmbeddedSnapshotState>>,
    mut gizmos: Gizmos<AuthorToolGizmos>,
    mut spatial: Gizmos<AuthorSpatialToolGizmos>,
    mut transforms: Gizmos<AuthorTransformGizmos>,
    mut foreground: ResMut<foreground::Foreground>,
) {
    if frame.error.is_some() || snapshot.is_some_and(|state| state.enabled) {
        foreground.clear();
        return;
    }
    let camera = if frame.transforms == 0 { None } else {
        viewport.as_deref().zip(nodes.single().ok())
            .and_then(|(viewport, node)| MoveCamera::new(&editor, viewport, node.inverse_scale_factor()))
    };
    use feathrise_scene::author_view::tools::{draw_brush, draw_selection, draw_transform};
    for item in &frame.preview.items {
        match item {
            AuthorTool::Brush(brush) if brush.spatial => draw_brush(&mut spatial, brush),
            AuthorTool::Brush(brush) => draw_brush(&mut gizmos, brush),
            AuthorTool::Selection(selection) => draw_selection(&mut gizmos, selection),
            AuthorTool::Transform(handle @ TransformPreview::Move { spatial: true, .. }) => draw_transform(&mut spatial, handle),
            AuthorTool::Transform(handle @ TransformPreview::Move { .. }) => match camera {
                Some(camera) => foreground::draw_move(&mut foreground, camera, handle),
                None => draw_transform(&mut transforms, handle),
            },
            AuthorTool::Transform(handle @ TransformPreview::Scale { cube: false, .. }) if let Some(camera) = camera => {
                foreground::draw_scale_center(&mut foreground, camera, handle);
            }
            AuthorTool::Transform(handle) => draw_transform(&mut transforms, handle),
        }
    }
}
