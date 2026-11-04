//! 与世界米制笔刷严格同尺寸的编辑视口光标。

use crate::authoring::EditorAuthoringSession;
use bevy::camera::visibility::RenderLayers; // 笔刷只进入F12离屏相机，不污染玩法相机。
use bevy::gizmos::config::GizmoConfigStore; // 独立配置线宽、深度偏移和渲染层。
use bevy::prelude::*; // Gizmos、Isometry3d、颜色和编辑状态。
use feathrise_author_tools::{AuthorTool, BrushPreview, BrushShape, ClipmapBrushSurface};
use feathrise_scene_core::input::{EditorInputState, InputGateState}; // 自定义笔刷与系统鼠标共用同一发布状态，避免双光标。
use feathrise_map_content::MapTerrainKind; // 接管 Clipmap 植被与静水刷圈。
use feathrise_scene::{ClipmapVegetationBrush, DynamicClipmapVegetationOverlay, TerrainObjectClipmapGround}; // 向唯一正式地形发布小覆盖参数。 // 当前草稿确认地形种类和地图身份。

use crate::resources::MapEditorState;
use crate::types::{EditorTool, EditorView};
use feathrise_map_terrain::volume::VolumeBrushShape;

/// F12笔刷使用独立Gizmo组，避免修改项目其他调试线框的显示设置。
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct MapEditorBrushGizmos;

pub(crate) const MAP_EDITOR_BRUSH_RENDER_LAYER: usize = 31;

/// 启动时固定笔刷线的像素宽度和前置深度，坡面上仍能辨认真实覆盖范围。
pub(crate) fn configure_map_editor_brush_gizmos(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<MapEditorBrushGizmos>();
    config.line.width = 3.0;
    config.line.perspective = false;
    config.depth_bias = -0.02;
    config.render_layers = RenderLayers::layer(MAP_EDITOR_BRUSH_RENDER_LAYER);
}

pub(crate) fn uses_brush(tool: EditorTool) -> bool {
    matches!(
        tool,
        EditorTool::Raise
            | EditorTool::VolumeDig
            | EditorTool::VolumeFill
            | EditorTool::VolumeSmooth
            | EditorTool::VolumeFlatten
            | EditorTool::Lower
            | EditorTool::Smooth
            | EditorTool::Flatten
            | EditorTool::Water
            | EditorTool::Puddles // 米制圆刷支持独立积水层。
            | EditorTool::Surface
            | EditorTool::Vegetation
    )
}

/// 固定空间轮廓是工具预览；只有仍跟随鼠标的笔刷才能接管指针显示。
pub(crate) fn map_editor_brush_follows_cursor(
    editor: &MapEditorState,
    spatial: Option<&super::spatial_brush::SpatialBrushState>,
) -> bool {
    !editor.tool.is_volume_sculpt() || spatial.is_none_or(|state| state.follows_cursor())
}

/// 圆环与系统鼠标共用纯显示落点；二维查询在途时沿最后真点高度面跟随当前射线。
/// 该值不携带本笔写权限，雕刻和涂绘必须另取共享游标的paint_hit。
pub(crate) fn map_editor_brush_display_hit(
    editor: &MapEditorState,
    objects: Option<&crate::authoring::terrain_ground::TerrainGroundCursor>,
) -> Option<(Vec3, bool)> {
    if editor.tool == EditorTool::Water && !editor.domain_brush_active {
        return editor.brush_cursor_world.map(|point| (point, false)); // 水圈跟随独立水位平面，不能借用旧地形显示点。
    }
    if uses_brush(editor.tool) && !editor.domain_brush_active
        && let Some(cursor) = objects
        && let Some(hit) = cursor.display_hit()
    {
        // 行内显隐可能晚于后台命中采用；纯显示也核对稳定目标，不能在刚隐藏的网格上留圈或隐藏鼠标。
        if editor.terrain_hidden.contains(&hit.terrain) { return None; }
        return cursor.display_point().map(|point| (point, hit.volume_surface));
    }
    editor.brush_cursor_world.map(|point| (point, false))
}

/// 判断中央视口本帧是否应由跟随笔刷接管鼠标显示；还须核对空间笔刷的跟随状态。
pub(crate) fn map_editor_brush_cursor_visible(
    editor: &MapEditorState,
    viewport_hovered: bool,
    deletion_modal_open: bool,
    ordinary_ui_requires_cursor: bool,
    volume_surface: bool,
    display_point: Option<Vec3>,
) -> bool {
    editor.open
        && !editor.close_requested // 关闭门等待期间停止作者输入并归还可见系统鼠标。
        && !editor.new_map_dialog_open
        && !deletion_modal_open
        && !ordinary_ui_requires_cursor
        && viewport_hovered
        && (matches!(editor.view, EditorView::Perspective | EditorView::Top) || editor.tool.is_volume_sculpt()
            || volume_surface && matches!(editor.tool,EditorTool::Surface|EditorTool::Vegetation|EditorTool::Puddles))
        && (uses_brush(editor.tool) || editor.domain_brush_active)
        && display_point.is_some()
}

/// 把任意笔刷半径收敛为可提交给Gizmo的有限正数。
fn safe_outer_radius(outer_radius: f32) -> f32 {
    if outer_radius.is_finite() {
        outer_radius.max(0.01) // 非负有限外圈与实际绘制使用同一最小世界半径。
    } else {
        0.01 // 非法输入退回最小可画半径，避免向Gizmo提交NaN。
    }
}

/// 把笔刷强度线性映射为内圈半径；零强度收于中心，满强度与外圈重合。
fn strength_ring_radius(outer_radius: f32, strength: f32) -> f32 {
    let safe_strength = if strength.is_finite() {
        strength.clamp(0.0, 1.0) // 内圈只表达标准零到一强度，不越过实际作用外圈。
    } else {
        0.0 // 非法强度回退为最弱视觉，不让光标几何失效。
    };
    safe_outer_radius(outer_radius) * safe_strength // 强度越高内圈越大，百分比与半径占比直观一致。
}

/// 每帧仅发布固定大小笔刷语义；两种视口共享外圈、强度内圈与体积轮廓绘制。
pub(crate) fn draw_map_editor_brush(
    editor: Res<MapEditorState>,
    session: Res<EditorAuthoringSession>, // 只读取原草稿已确认的来源，不改变作者业务。
    mut clipmap: Option<ResMut<DynamicClipmapVegetationOverlay>>, // 无场景插件时不发布覆盖。
    ground_cursor: Option<Res<super::vegetation_ground::VegetationGroundCursor>>, // 刷圈携带真实命中原高度身份。
    objects_cursor: Option<Res<crate::authoring::terrain_ground::TerrainGroundCursor>>,
    volume: Option<Res<crate::authoring::volume_sculpt::VolumeAuthoringState>>,
    spatial: Option<Res<super::spatial_brush::SpatialBrushState>>,
    blocks: Res<EditorInputState>,
    gate: Res<InputGateState>,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    ground_surface: TerrainObjectClipmapGround,
    mut frame: ResMut<super::tool_preview::AuthorToolFrame>,
) {
    if let Some(clipmap) = clipmap.as_mut() {
        clipmap.brush = None;
    } // 每帧先撤销旧命中，关闭或缺页不会留下旧圈。
    if !map_editor_brush_follows_cursor(&editor, spatial.as_deref()) {
        return;
    }
    if !blocks.map_editor_custom_cursor_visible || gate.system_cursor_required {
        return;
    }
    let Some((world, object_volume_surface)) = map_editor_brush_display_hit(&editor, objects_cursor.as_deref()) else {
        return;
    };
    let erasing = editor.domain_brush_erase
        || keyboard.pressed(KeyCode::ShiftLeft)
        || keyboard.pressed(KeyCode::ShiftRight);
    let active = mouse.pressed(MouseButton::Left);
    let volume_surface = matches!(
        editor.tool,
        EditorTool::Surface | EditorTool::Vegetation | EditorTool::Puddles
    ) && (object_volume_surface || ground_cursor.as_deref().is_some_and(|cursor| cursor.has_volume_cursor()));
    let color = if erasing {
        Color::srgb(1.0, 0.24, 0.18)
    } else if active {
        Color::srgb(1.0, 0.78, 0.12)
    } else {
        Color::srgb(0.18, 0.95, 1.0)
    };
    if editor.tool == EditorTool::Vegetation && !editor.domain_brush_active && !volume_surface // 静水使用水位平面；洞内植被圈不能再次投到室外最高高度。
        && let Some(draft) = session.draft.as_ref().filter(|draft| draft.manifest.terrain.kind == MapTerrainKind::SparseClipmap)
    // 只识别正式稀疏地形。
    {
        if let Some((map, height_revision, revision, missing_pages)) = ground_cursor
            .as_ref()
            .and_then(|cursor| cursor.accepted_preview_identity())
        {
            if let Some(clipmap) = clipmap.as_mut() {
                clipmap.brush = Some(ClipmapVegetationBrush {
                    map_id: draft.manifest.id.clone(), // 与拾取的当前草稿同图。
                    revision,                          // 使用接受命中时的高度修订，不能从当前快照冒领。
                    missing_pages,                     // 页采用变化后场景会拒绝旧圈。
                    center: world.xz(),                // Y 完全由地形真实顶点决定。
                    radius: safe_outer_radius(editor.brush_radius), // 与实际写入范围一致。
                    strength: strength_ring_radius(1.0, editor.brush_strength), // 保留原强度内圈规则。
                    color: color.to_linear(),          // GPU 使用线性颜色。
                }); // 原编辑器仍由真实地形GPU材质显示，不创建水平圈。
            }
            if let Some(path) = draft.manifest.terrain.height_path.as_deref()
                && map == &draft.map_id
                && let Some(snapshot) = ground_surface.authoring_snapshot(map, None, path, height_revision)
                && snapshot.revision == revision && snapshot.missing_pages == missing_pages
            {
                // 路径必须由同一命中GPU身份反查确认，不能拿当前草稿给旧显示补写来源。
                frame.push(AuthorTool::Brush(BrushPreview {
                    center: world.to_array(), rotation: Quat::IDENTITY.to_array(),
                    radius: safe_outer_radius(editor.brush_radius),
                    strength: Some(strength_ring_radius(1.0, editor.brush_strength)),
                    shape: BrushShape::Planar, half_height: 0.0, flatten: false, spatial: false,
                    color: color.to_linear().to_f32_array(),
                    clipmap: Some(ClipmapBrushSurface {
                        map_id: map.to_string(), terrain_id: None, height_path: path.to_owned(),
                        authoring_height_revision: height_revision,
                    }),
                }));
            }
        }
        return; // Clipmap 作者笔刷不再叠画悬空 Gizmo。
    }
    if editor.tool.is_volume_sculpt() || volume_surface {
        let radius = safe_outer_radius(editor.brush_radius);
        let plane = (editor.tool == EditorTool::VolumeFlatten)
            .then(|| volume.as_deref().and_then(|v| v.flatten_height()).unwrap_or(world.y));
        let locked_height = editor.tool.is_volume_sculpt().then(|| volume.as_deref().and_then(|v| v.brush_height())).flatten();
        let center = Vec3::new(world.x, plane.or(locked_height).unwrap_or(world.y), world.z);
        // 表面涂绘仍用原球形；只有几何工具采用规则形状，显示范围必须等于实际盖印范围。
        let shape = if volume_surface { VolumeBrushShape::Sphere } else { editor.volume_brush_shape };
        let half_height = safe_outer_radius(editor.volume_brush_half_height);
        frame.push(AuthorTool::Brush(volume_brush_preview(center, radius, half_height, shape, color, plane.is_some())));
        return;
    }
    let source_rotation = if editor.domain_brush_active { Quat::IDENTITY } else if editor.tool == EditorTool::Water {
        session.draft.as_ref().map_or(Quat::IDENTITY, |draft| crate::authoring::water_canvas::transform(draft).rotation)
    } else {
        objects_cursor.as_deref().and_then(|cursor| cursor.display_transform()).map_or(Quat::IDENTITY, |transform| transform.rotation)
    };
    // 各轴缩放已由笔刷米制空间还原；两种视口由同一renderer绘制世界米半径和原来源姿态。
    frame.push(AuthorTool::Brush(BrushPreview {
        center: world.to_array(), rotation: source_rotation.to_array(),
        radius: safe_outer_radius(editor.brush_radius),
        // 点位没有密度强度，不用内圈错误表达间距。
        strength: (!editor.domain_brush_active).then(|| strength_ring_radius(1.0, editor.brush_strength)),
        shape: BrushShape::Planar, half_height: 0.0, flatten: false, spatial: false,
        color: color.to_linear().to_f32_array(), clipmap: None,
    }));
}

/// 生产端仅发布原工具已确认的体积姿态；几何分段由场景库集中维护。
pub(super) fn volume_brush_preview(
    center: Vec3, radius: f32, half_height: f32, shape: VolumeBrushShape,
    color: Color, flatten: bool,
) -> BrushPreview {
    BrushPreview {
        center: center.to_array(), rotation: Quat::IDENTITY.to_array(), radius,
        strength: None,
        shape: match shape {
            VolumeBrushShape::Sphere => BrushShape::Sphere,
            VolumeBrushShape::Box => BrushShape::Box,
            VolumeBrushShape::Cylinder => BrushShape::Cylinder,
        },
        half_height, flatten, spatial: false, color: color.to_linear().to_f32_array(), clipmap: None,
    }
}

/// 固定空间预览沿用原独立Gizmo组的地下可见性，不能被普通跟随笔刷的深度配置替代。
pub(super) fn spatial_volume_brush_preview(
    center: Vec3, radius: f32, half_height: f32, shape: VolumeBrushShape, color: Color,
) -> BrushPreview {
    let mut preview = volume_brush_preview(center, radius, half_height, shape, color, false);
    preview.spatial = true;
    preview
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_brush_uses_shared_cursor_but_point_does_not() {
        let mut editor = MapEditorState {
            open: true,
            tool: EditorTool::Domain("test.domain"),
            view: EditorView::Top,
            brush_cursor_world: Some(Vec3::ZERO),
            domain_brush_active: true,
            ..default()
        };
        assert!(map_editor_brush_cursor_visible(
            &editor, true, false, false, false, editor.brush_cursor_world
        ));
        editor.domain_brush_active = false;
        assert!(!map_editor_brush_cursor_visible(
            &editor, true, false, false, false, editor.brush_cursor_world
        ));
    }

    #[test]
    fn only_actual_paint_tools_show_brush() {
        assert!(uses_brush(EditorTool::Vegetation));
        assert!(uses_brush(EditorTool::Surface));
        assert!(uses_brush(EditorTool::Water));
        assert!(!uses_brush(EditorTool::Inspect));
        assert!(!uses_brush(EditorTool::PlaceAsset));
    }
    #[test]
    fn moving_brush_display_keeps_the_custom_pointer_without_granting_strict_hover() {
        let editor = MapEditorState { open: true, tool: EditorTool::Raise, view: EditorView::Top, ..default() };
        assert!(editor.brush_cursor_world.is_none());
        let completed = Some(Vec3::new(2.0, 1.0, 3.0));
        assert!(map_editor_brush_cursor_visible(&editor, true, false, false, false, completed));
        for (hovered, modal, ordinary_ui) in [(false, false, false), (true, true, false), (true, false, true)] {
            assert!(!map_editor_brush_cursor_visible(&editor, hovered, modal, ordinary_ui, false, completed));
        }
        assert!(!map_editor_brush_cursor_visible(&editor, true, false, false, false, None));
        assert!(editor.brush_cursor_world.is_none(), "纯显示落点不能写回严格hover以授权作者操作");
    }
    #[test]
    fn cave_wall_paint_cursor_supports_front_and_side_views_without_exterior_projection() {
        let mut editor = MapEditorState {
            open: true,
            tool: EditorTool::Surface,
            brush_cursor_world: Some(Vec3::ZERO),
            ..default()
        };
        for view in [EditorView::Front, EditorView::Side] {
            editor.view = view;
            assert!(map_editor_brush_cursor_visible(
                &editor, true, false, false, true, editor.brush_cursor_world
            ));
            assert!(!map_editor_brush_cursor_visible(
                &editor, true, false, false, false, editor.brush_cursor_world
            ));
            assert!(!map_editor_brush_cursor_visible(
                &editor, true, true, false, true, editor.brush_cursor_world
            ));
        }
    }

    #[test] // 世界笔刷接管条件必须与实际Gizmo显示严格一致，防止再次出现双光标。
    fn custom_cursor_requires_a_visible_editable_brush_hit() {
        let mut editor = MapEditorState {
            open: true,                           // F12已经打开。
            tool: EditorTool::Vegetation,         // 当前是圆形绘制工具。
            view: EditorView::Top,                // 顶视图允许绘制。
            brush_cursor_world: Some(Vec3::ZERO), // 地图内或边缘交集产生了有效笔刷中心。
            ..default()
        };
        assert!(map_editor_brush_cursor_visible(
            &editor, true, false, false, false, editor.brush_cursor_world
        )); // 只有此时系统鼠标才应隐藏。
        assert!(!map_editor_brush_cursor_visible(
            &editor, false, false, false, false, editor.brush_cursor_world
        )); // 离开中央视口立即归还系统鼠标。
        assert!(!map_editor_brush_cursor_visible(
            &editor, true, true, false, false, editor.brush_cursor_world
        )); // 删除确认框不能被世界笔刷抢走鼠标。

        editor.brush_cursor_world = None; // 圆已经完全离开地图，Gizmo不会提交。
        assert!(!map_editor_brush_cursor_visible(
            &editor, true, false, false, false, editor.brush_cursor_world
        )); // 没有实际笔刷时系统鼠标必须可见。
    }

    #[test] // 锁定Blender式视觉方向，防止以后再次把内圈误改成柔边或反向强度。
    fn stronger_brush_always_has_a_larger_inner_ring() {
        let outer_radius = 20.0; // 固定外圈便于直接核对强度百分比对应的世界半径。
        let weak = strength_ring_radius(outer_radius, 0.05); // 编辑器正式最弱强度应产生最小内圈。
        let medium = strength_ring_radius(outer_radius, 0.5); // 中等强度应位于外圈一半。
        let strong = strength_ring_radius(outer_radius, 1.0); // 满强度应扩张到实际作用外圈。
        assert!(weak < medium && medium < strong); // 强度增加必须严格扩大内圈，禁止旧实现的反向关系。
        assert!((weak - 1.0).abs() < f32::EPSILON); // 5%强度线性对应5%外圈半径。
        assert!((medium - 10.0).abs() < f32::EPSILON); // 50%强度线性对应一半外圈半径。
        assert!((strong - outer_radius).abs() < f32::EPSILON); // 100%强度与外圈重合。
    }

    #[test] // 非法或越界状态也不能生成越过外圈的Gizmo几何。
    fn strength_ring_radius_clamps_invalid_inputs() {
        assert_eq!(strength_ring_radius(10.0, -1.0), 0.0); // 负强度钳为零。
        assert_eq!(strength_ring_radius(10.0, 2.0), 10.0); // 超满强度钳为外圈。
        assert_eq!(strength_ring_radius(10.0, f32::NAN), 0.0); // NaN强度使用安全零值。
        assert_eq!(strength_ring_radius(f32::NAN, 1.0), 0.01); // NaN半径不污染渲染输入。
    }
}
