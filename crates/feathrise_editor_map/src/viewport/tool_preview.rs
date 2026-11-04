//! 作者工具当帧语义投影；输入、选择解析和编辑历史继续由原工具拥有。

use bevy::{camera::visibility::RenderLayers, gizmos::config::GizmoConfigStore, prelude::*};
use feathrise_author_tools::{AuthorTool, AuthorToolPreview, MAX_AUTHOR_BRUSHES, MAX_AUTHOR_TRANSFORMS, MAX_AUTHOR_SELECTIONS};

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

pub(crate) fn install(app: &mut App) {
    app.init_resource::<AuthorToolFrame>()
        .init_gizmo_group::<AuthorToolGizmos>()
        .init_gizmo_group::<AuthorSpatialToolGizmos>()
        .add_systems(Startup, configure)
        .add_systems(First, clear)
        .add_systems(PostUpdate, draw);
}

fn clear(mut frame: ResMut<AuthorToolFrame>) {
    frame.preview.items.clear();
    frame.error = None;
    frame.brushes = 0;
    frame.transforms = 0;
    frame.selections = 0;
}

fn configure(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<AuthorToolGizmos>();
    config.render_layers = RenderLayers::layer(31);
    feathrise_scene::author_view::tools::configure(config, false);
    let (config, _) = store.config_mut::<AuthorSpatialToolGizmos>();
    config.render_layers = RenderLayers::layer(31);
    feathrise_scene::author_view::tools::configure(config, true);
}

fn draw(
    frame: Res<AuthorToolFrame>,
    snapshot: Option<Res<crate::standalone::embedded_snapshot::EmbeddedSnapshotState>>,
    mut gizmos: Gizmos<AuthorToolGizmos>,
    mut spatial: Gizmos<AuthorSpatialToolGizmos>,
) {
    if frame.error.is_none() && !snapshot.is_some_and(|state| state.enabled) {
        feathrise_scene::author_view::tools::draw_layer(&mut gizmos, &frame.preview, false);
        feathrise_scene::author_view::tools::draw_layer(&mut spatial, &frame.preview, true);
    }
}
