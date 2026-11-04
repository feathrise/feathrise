//! 固定立方体幽灵与空地图工作平面；真实地图继续消费现有后台光标的完整命中。

use bevy::{camera::visibility::RenderLayers, prelude::*, ui::RelativeCursorPosition};
use feathrise_editor_core::EditorSettings;

use super::{CUBOID_SELECTION, components::CuboidGhost, resources::BasicShapeState};
use crate::{
    authoring::EditorAuthoringSession,
    components::MapEditorViewport,
    resources::{MapEditorNumericScrubMouseGuard, MapEditorState, MapEditorViewportState},
    types::EditorTool,
    viewport::placement_move::MoveCamera,
};

pub(crate) fn is_cuboid_selection(editor: &MapEditorState) -> bool {
    editor.tool == EditorTool::PlaceAsset
        && editor.selected_asset.as_deref() == Some(CUBOID_SELECTION)
}

/// 仅明确的零地形对象集合提供Y=0工作平面；加载在途、有隐藏地形或真实未命中均不回退。
pub(crate) fn work_plane_is_available(session: &EditorAuthoringSession) -> bool {
    session.draft.as_ref().is_some_and(|draft| {
        draft
            .manifest
            .terrains
            .as_ref()
            .is_some_and(|terrains| terrains.is_empty())
    })
}

pub(super) fn setup_preview(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut mesh = Mesh::from(Cuboid::new(2.0, 2.0, 2.0));
    if let Some(bevy::mesh::VertexAttributeValues::Float32x3(normals)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
        let colors = normals.iter().map(|normal| {
            let shade = if normal[1] > 0.5 { 1.0 } else if normal[1] < -0.5 { 0.55 }
                else if normal[0].abs() > 0.5 { 0.70 } else { 0.85 };
            [shade, shade, shade, 1.0]
        }).collect::<Vec<[f32; 4]>>();
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    } // 固定24顶点按法线区分面色阶，夜间无光时也能看清半透明盒体的体积。
    commands.spawn((
        Name::new("编辑器立方体点放预览"),
        CuboidGhost,
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgba(0.50, 0.67, 0.79, 0.40),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.7,
            unlit: true, // 点放意图不受夜晚、曝光或缺少场景灯光影响。
            ..default()
        })),
        Transform::IDENTITY,
        Visibility::Hidden,
        RenderLayers::layer(crate::viewport::MAP_EDITOR_BRUSH_RENDER_LAYER),
        bevy::light::NotShadowCaster,
        bevy::light::NotShadowReceiver,
    )); // 全会话只分配24顶点盒网格与一份材质，逐帧只改固定变换和显隐。
}

pub(crate) fn plane_hit(ray: Ray3d) -> Option<Vec3> {
    if ray.direction.y.abs() < 0.0001 {
        return None;
    }
    let distance = -ray.origin.y / ray.direction.y;
    if !distance.is_finite() || !(0.0..=1_000_000.0).contains(&distance) {
        return None;
    }
    let point = ray.get_point(distance).with_y(0.0);
    point.is_finite().then_some(point)
}

pub(super) fn update_preview(
    mut state: ResMut<BasicShapeState>,
    editor: Res<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    viewport: Option<Res<MapEditorViewportState>>,
    guard: Res<MapEditorNumericScrubMouseGuard>,
    settings: Res<EditorSettings>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    nodes: Query<(&RelativeCursorPosition, &ComputedNode), With<MapEditorViewport>>,
    snap: Option<Res<crate::viewport::snapping::MeshSnapCursor>>,
    mode: Option<Res<crate::viewport::modes::ViewportModeState>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    workspace: Option<Res<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>>,
    transform_input: Option<Res<crate::ui::placement_transform::PlacementTransformInputState>>,
    mut ghost: Query<(&mut Transform, &mut Visibility, &MeshMaterial3d<StandardMaterial>), With<CuboidGhost>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    state.work_plane = None; // 工作平面必须每帧重新证明当前相机、视口和地图身份。
    let adding = is_cuboid_selection(&editor);
    let target = state.placement_target(&session);
    // 复制后重定位和“吸附地面”可复用同一空图平面，但必须核对冻结的完整记录身份。
    let relocation_origin = transform_input.as_deref().and_then(|input| {
        let (map, index, id, structure, revision) = input.relocation_ground_identity()?;
        let draft = session.draft.as_ref()?;
        (map == &draft.map_id
            && structure == session.placement_structure_revision
            && revision == session.revision
            && work_plane_is_available(&session))
        .then_some(())?;
        draft
            .placements
            .placements
            .get(index)
            .filter(|record| record.id == id && record.asset_target.is_builtin())
            .map(|record| record.translation)
    });
    let point = (|| {
        if !editor.open
            || ((!adding || target.is_none()) && relocation_origin.is_none())
            || state.owns_mouse()
            || guard.blocked()
            || editor.close_requested
            || editor.map_transition_in_progress
            || editor.new_map_dialog_open
            || editor.suppress_world_pick_this_frame
            || session.saving
            || windows.single().is_ok_and(|window| !window.focused)
            || workspace.as_ref().is_some_and(|workspace| {
                workspace.active_tab != crate::ui::vegetation_preset_editor::PresetWorkspaceTab::Map
            })
            || mouse.any_pressed([MouseButton::Right, MouseButton::Middle])
            || mode.as_ref().is_some_and(|mode| mode.pointer_over_controls)
        {
            return None;
        }
        let draft = session.draft.as_ref()?;
        if editor.selected_map_id.as_ref() != Some(&draft.map_id) {
            return None;
        }
        let viewport = viewport.as_deref()?;
        if viewport.framed_map_id.as_ref() != Some(&draft.map_id) {
            return None;
        }
        let (cursor, node) = nodes.single().ok()?;
        if !cursor.cursor_over() {
            return None;
        }
        if work_plane_is_available(&session) {
            let ray = MoveCamera::new(&editor, viewport, node.inverse_scale_factor())?
                .ray(cursor.normalized?)?;
            let point = plane_hit(ray)?;
            let snap = settings.editor_snapping.translation_snap(
                keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]),
                relocation_origin.unwrap_or([0.0; 3]),
            );
            let point = Vec3::from_array(snap.horizontal(point.to_array())?);
            state.work_plane = Some((draft.map_id.clone(), session.revision, point));
            return Some(point);
        }
        if let Some(snap) = snap.as_deref().filter(|snap| snap.requires_query()) {
            snap.preview
        } else {
            editor.cursor_world
        }
    })();
    for (mut transform, mut visibility, material) in &mut ghost {
        let shape = target.and_then(|target| match target {
            feathrise_map_content::MapPlacementAssetTarget::Cuboid { size_mm, color_srgb, roughness_milli, .. } =>
                Some((Vec3::from_array(size_mm.map(|size| size as f32 * 0.001)), color_srgb, roughness_milli)),
            _ => None,
        });
        let visible = point.filter(|_| adding).zip(shape).map(|(point, (size, _, _))| point + Vec3::Y * size.y * 0.5);
        let next = if visible.is_some() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != next {
            *visibility = next;
        }
        if let Some(point) = visible
            && transform.translation != point
        {
            transform.translation = point;
        }
        if let Some((size, color, roughness)) = shape {
            let scale = size * 0.5; // 共享2米盒体只改变换，参数更新不逐帧构造Mesh。
            if transform.scale != scale { transform.scale = scale; }
            if let Some(mut material) = materials.get_mut(&material.0) {
                let next_color = Color::srgba(color[0] as f32 / 255.0, color[1] as f32 / 255.0, color[2] as f32 / 255.0, 0.40);
                if material.base_color != next_color { material.base_color = next_color; }
                let next_roughness = roughness as f32 * 0.001;
                if material.perceptual_roughness != next_roughness { material.perceptual_roughness = next_roughness; }
            }
        }
    }
}
