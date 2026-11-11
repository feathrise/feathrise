//! 模型预览共用地图方向球的显示和对轴动作，只更新资产相机的观察状态。

use bevy::{ecs::system::SystemParam, prelude::*, ui::RelativeCursorPosition};
use feathrise_editor_ui::EditorButtonClick;

use crate::{
    components::SceneEditorOrientationAxisEndpoint,
    ui::vegetation_preset_editor::VegetationPresetEditorState,
    viewport::orientation_axis::*,
};

use super::{
    AssetPreviewWorkspace, DocumentState, preview_shading, rigging::RiggingState,
    types::*, view_math::AssetPreviewView,
};

#[derive(Component)]
pub(super) struct PreviewAxisRoot;

pub(super) fn setup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    canvases: Query<Entity, Added<AssetPreviewCanvas>>,
) {
    let Ok(canvas) = canvases.single() else {
        return;
    };
    let font = feathrise_editor_core::editor_font(&assets, ORIENTATION_FONT_PATH);
    let mut layout = orientation_axis_layout(8.0); // 窄侧栏仍贴顶，中央给四球留出第一行。
    layout.display = Display::None;
    commands.entity(canvas).with_children(|parent| {
        spawn_orientation_axis(
            parent,
            &font,
            (Name::new("模型预览方向球"), PreviewAxisRoot),
            PreviewOrientationAxis,
            layout,
            ["X", "Y", "前"],
        );
    });
}

fn yaw(
    rigging: Option<&RiggingState>,
    documents: Option<&DocumentState>,
    preview: &AssetPreviewState,
) -> f32 {
    rigging.zip(documents).map_or(0.0, |(rigging, documents)| {
        rigging.front_yaw(documents.asset_root(), preview)
    })
}

fn model_ready(preview: &AssetPreviewState) -> bool {
    preview.external_preview.is_none()
        && matches!(
            preview.content,
            AssetPreviewContent::Model {
                phase: AssetPreviewPhase::Ready,
                ..
            }
        )
}

pub(super) fn sync(
    preview: Res<AssetPreviewState>,
    rigging: Option<Res<RiggingState>>,
    documents: Option<Res<DocumentState>>,
    media: Option<Res<AssetPreviewWorkspace>>,
    workspace: Option<Res<VegetationPresetEditorState>>,
    cameras: Query<&Transform, With<AssetPreviewCamera>>,
    mut root: Query<(Entity, &mut Node), (With<PreviewAxisRoot>, Without<PreviewOrientationAxis>)>,
    mut lines: OrientationLines<With<PreviewOrientationAxis>>,
    mut endpoints: OrientationEndpoints<With<PreviewOrientationAxis>>,
    mut previous: Local<Option<(Entity, Quat)>>,
) {
    let Ok((entity, mut root)) = root.single_mut() else {
        return;
    };
    let visible = preview.render_visible && model_ready(&preview);
    let display = if visible {
        Display::Flex
    } else {
        Display::None
    };
    if root.display != display {
        root.display = display;
    }
    if !visible {
        return;
    }
    let expanded = media.as_deref().zip(workspace.as_deref()).is_some_and(|(media, workspace)|
        preview_shading::expanded_model(&preview, media, workspace));
    let top = px(if expanded { 42.0 } else { 8.0 });
    if root.top != top {
        root.top = top;
    }
    let Ok(camera) = cameras.single() else { return };
    let rotation = Quat::from_rotation_y(-yaw(rigging.as_deref(), documents.as_deref(), &preview))
        * camera.rotation;
    if *previous != Some((entity, rotation)) {
        sync_orientation_nodes(rotation, &mut lines, &mut endpoints);
        *previous = Some((entity, rotation));
    }
}

#[derive(SystemParam)]
pub(super) struct AxisInput<'w, 's> {
    root: Query<'w, 's, (&'static Node, &'static RelativeCursorPosition), With<PreviewAxisRoot>>,
    endpoints: Query<
        'w,
        's,
        (
            &'static SceneEditorOrientationAxisEndpoint,
            &'static EditorButtonClick,
            &'static Visibility,
        ),
        (With<PreviewOrientationAxis>, Changed<EditorButtonClick>),
    >,
    toggles: Query<
        'w,
        's,
        &'static EditorButtonClick,
        (
            With<PreviewOrientationAxis>,
            With<OrientationProjectionToggle>,
            Changed<EditorButtonClick>,
        ),
    >,
    rigging: Option<Res<'w, RiggingState>>,
    documents: Option<Res<'w, DocumentState>>,
}

impl AxisInput<'_, '_> {
    pub(super) fn cursor_over(&self) -> bool {
        self.root
            .single()
            .is_ok_and(|(node, cursor)| node.display != Display::None && cursor.cursor_over())
    }

    pub(super) fn apply(&self, preview: &AssetPreviewState, view: &mut AssetPreviewView) -> bool {
        if !model_ready(preview)
            || !preview.render_visible
            || !self
                .root
                .single()
                .is_ok_and(|(node, _)| node.display != Display::None)
        {
            return false;
        }
        if let Some((end, _, _)) = self
            .endpoints
            .iter()
            .find(|(_, click, visibility)| click.is_clicked() && **visibility != Visibility::Hidden)
        {
            let base = yaw(self.rigging.as_deref(), self.documents.as_deref(), preview);
            let desired = Quat::from_rotation_y(base)
                * (end.world_axis * if end.positive { 1.0 } else { -1.0 });
            let aligned = (view.rotation() * Vec3::Z).dot(desired) > 0.9999;
            let (yaw, pitch) = axis_angles(end.world_axis, end.positive != aligned);
            // X/Y/Z 是方向球的局部轴。绑骨时以人物中线为基准，任意朝向的模型都能正视。
            view.yaw = base + yaw;
            view.pitch = pitch;
            view.orthographic = true;
            return true;
        }
        if self.toggles.iter().any(EditorButtonClick::is_clicked) {
            view.orthographic = !view.orthographic;
            return true;
        }
        false
    }
}
