//! F12区域边界可视线、控制点、状态文本与未保存运行时覆盖同步。

mod cache; // 后台可见段筛选，稳定帧只提交有界Gizmos。
use bevy::prelude::*; // Gizmos、Text、颜色与ECS资源。
use feathrise_map_content::{MapBoundaryCapture, MapId};
use feathrise_scene::{DynamicClipmapGroundSurface, DynamicMapBoundaryOverride}; // 未保存碰撞与基础作者高度的独立正式来源。

use super::helpers::{display_point, point_pick_radius}; // 边界线贴当前GLB派生高度并保持可点击点尺寸。
use crate::authoring::resources::{
    BoundaryEditState, BoundaryLiveOverrideState, EditorAuthoringSession,
}; // 拖动高亮、发布身份和当前草稿。
use crate::components::MapEditorBoundaryText; // 左栏边界专用固定文本节点。
use crate::resources::{MapEditorState, MapEditorViewportState}; // 工具显隐和视图缩放。
use crate::types::EditorTool; // 只在区域边界工具激活时绘制Gizmo。
use crate::viewport::MapEditorBrushGizmos; // 复用只进入F12离屏相机的独立渲染层配置。
use crate::viewport::content_selection::{LayerCandidateState, LayerTarget}; // 跨类型选择与悬停共用视觉身份。
use crate::viewport::modes::{SelectionFilter, ViewportMode, ViewportModeState}; // 选择模式显示可点击墙轮廓。

fn edited_point(edit: &BoundaryEditState, path: usize, point: usize, stored: [f32; 2]) -> [f32; 2] {
    if edit.path_index == Some(path) && edit.point_index == Some(point) {
        edit.preview_point.unwrap_or(stored)
    } else {
        stored
    }
} // 只替换当前拖动点，取消/松手无需回写未提交作者文档。

/// 按正式Cuboid厚度画中心线与两侧轮廓，并显示端点封缝延伸后的真实地表占用。
fn draw_segment_with_width(
    gizmos: &mut Gizmos<MapEditorBrushGizmos>, // 只进入F12离屏相机的线条输出。
    display: impl Fn([f32; 2]) -> Option<Vec3>, // 已接受同源高度；Clipmap缺页端点没有零高度替代。
    start: [f32; 2],                           // 作者世界X/Z起点。
    end: [f32; 2],                             // 作者世界X/Z终点。
    thickness: f32,                            // 正式静态墙完整厚度。
    center_color: Color,                       // 开放或闭合中心线颜色。
    edge_color: Color,                         // 墙体两侧较弱轮廓色。
) {
    let Some(lines) = feathrise_map_content::boundary_footprint_lines(start, end, thickness) else {
        return; // 无效中间态不产生轮廓。
    }; // F1编辑器样式与F12共用同一组三条作者线。
    for (index, (start, end)) in lines.into_iter().enumerate() {
        let (Some(start), Some(end)) = (display(start), display(end)) else {
            continue;
        };
        gizmos.line(
            start,
            end,
            if index == 0 { center_color } else { edge_color }, // 编辑状态仍保留中心/边线颜色区别。
        );
    }
}

/// 每帧按固定控制点预算绘制主边界线、控制点和开放路径下一段预览。
pub(crate) fn draw_map_boundaries(
    editor: Res<MapEditorState>,          // 当前工具、光标与F12显隐。
    session: Res<EditorAuthoringSession>, // 当前小型边界草稿与GLB派生高度。
    edit: Res<BoundaryEditState>,         // 当前拖动点使用独立高亮色。
    candidates: Res<LayerCandidateState>, // 只读临时预高亮，不改变控制点。
    modes: Res<ViewportModeState>,        // 仅全部/玩法筛选显示普通边界轮廓。
    viewport: Option<Res<MapEditorViewportState>>, // 控制点球体保持固定像素可见尺寸。
    mut gizmos: Gizmos<MapEditorBrushGizmos>, // 线条只渲染到编辑器RTT。
    mut cache: Local<cache::PreviewCache>, // 视口可见段缓存不改变正式墙数据。
    ground: Option<Res<DynamicClipmapGroundSurface>>, // 只共享正式基础页，不建立完整高度数组。
) {
    let selecting = modes.mode == ViewportMode::Select;
    if !editor.open
        || (editor.tool != EditorTool::Boundary
            && !(selecting
                && matches!(
                    modes.filter,
                    SelectionFilter::All | SelectionFilter::Gameplay
                )))
    {
        cache.invalidate(); // 隐藏工具后不能复用同图重进的旧数组索引。
        return; // 非边界工具不增加视口线条干扰。
    }
    let Some(draft) = session.draft.as_ref() else {
        cache.invalidate();
        return; // 作者数据尚未就绪时没有可显示边界。
    };
    let Some(document) = draft.boundaries.as_ref() else {
        cache.invalidate();
        return; // 专用静态地图没有数据驱动边界。
    };
    let point_radius = viewport
        .as_deref()
        .map(|viewport| point_pick_radius(viewport.extent, viewport.render_size) * 0.34)
        .unwrap_or(0.8); // RTT建立前仍提供保守可见点尺寸。
    let editable_path = (!selecting)
        .then(|| super::helpers::selected_path_index(&session))
        .flatten(); // 只在编辑时解析一次所选路径，其他路径不显示拖动点。
    let preferred = editable_path.or_else(|| {
        let target = candidates.hover.as_ref().or(candidates.selected.as_ref());
        let Some(LayerTarget::Boundary(id)) = target else {
            return None;
        };
        document.paths.iter().position(|path| &path.id == id)
    });
    let center = viewport
        .as_deref()
        .map_or(Vec2::ZERO, |view| view.center.xz());
    let radius = viewport.as_deref().map_or(10_000.0, |view| {
        view.extent * 4.0 * (view.render_size.x as f32 / view.render_size.y.max(1) as f32).max(1.0)
    }); // 保守包含当前轨道视野，后台剔除远处墙段。
    cache.update(
        document,
        session.boundary_revision,
        center,
        radius,
        preferred,
        ground
            .as_ref()
            .and_then(|ground| ground.snapshot.as_ref())
            .filter(|ground| {
                ground.ready
                    && ground.map_id == draft.map_id
                    && ground.authoring_height_revision() == Some(session.terrain_revision)
            }),
        edit.path_index
            .zip(edit.point_index)
            .zip(edit.preview_point)
            .map(|((path, point), value)| (path, point, value)),
    );
    let clipmap = draft.manifest.terrain.kind == feathrise_map_content::MapTerrainKind::SparseClipmap;
    let display = |point| {
        if clipmap {
            cache.display_point(point)
        } else {
            Some(display_point(draft, point))
        }
    };
    for (&path_index, segments) in cache.paths() {
        let Some(path) = document.paths.get(path_index).filter(|path| path.visible) else {
            continue;
        };
        let highlighted = matches!(candidates.hover.as_ref().or(candidates.selected.as_ref()), Some(LayerTarget::Boundary(id)) if *id == path.id); // 悬停其他类型时旧边界不保留高亮。
        let line_color = if selecting && highlighted {
            Color::srgb(1.0, 0.78, 0.24) // 当前路径铜金色。
        } else if selecting {
            Color::srgba(0.45, 0.68, 0.67, 0.55) // 未选空气墙仍有淡轮廓供点击。
        } else if path.closed {
            Color::srgb(0.10, 0.95, 1.0) // 闭合安全边界使用青色。
        } else {
            Color::srgb(1.0, 0.72, 0.10) // 尚未闭合路径使用警示黄色。
        };
        let edge_color = if selecting {
            line_color
        } else if path.closed {
            Color::srgba(0.10, 0.95, 1.0, 0.42) // 青色半透明边线表达实际墙体宽度。
        } else {
            Color::srgba(1.0, 0.72, 0.10, 0.42) // 开放路径仍以黄色宽度轮廓警示。
        };
        for &segment in segments {
            let Some(start) = path.points.get(segment) else {
                continue;
            };
            let Some(end) = path.points.get((segment + 1) % path.points.len()) else {
                continue;
            };
            draw_segment_with_width(
                &mut gizmos,                                      // 输出中心线和两侧真实轮廓。
                display, // 全部端点贴当前未保存地形；Clipmap在后台派生同源Mip 0高度。
                edited_point(&edit, path_index, segment, *start), // 当前线段起点，包含未提交拖点。
                edited_point(&edit, path_index, (segment + 1) % path.points.len(), *end),
                path.thickness, // 与正式Avian Cuboid使用同一完整墙厚。
                line_color,     // 开放与闭合状态肉眼可辨。
                edge_color,     // 两侧轮廓显示实际占用。
            );
        }
        for &point_index in cache
            .points(path_index)
            .iter()
            .filter(|_| editable_path == Some(path_index))
        {
            let Some(point) = path.points.get(point_index) else {
                continue;
            };
            let selected =
                edit.path_index == Some(path_index) && edit.point_index == Some(point_index); // 当前拖动点独立高亮。
            let point_color = if selected {
                Color::srgb(1.0, 0.18, 0.12) // 拖动中的点使用红色。
            } else {
                Color::srgb(0.98, 0.98, 0.92) // 普通控制点使用米白色。
            };
            let Some(world) = display(edited_point(&edit, path_index, point_index, *point)) else {
                continue;
            };
            gizmos
                .sphere(
                    Isometry3d::from_translation(world), // 球心贴地并略微抬高。
                    point_radius,                        // 随视图缩放保持约定像素大小。
                    point_color,                         // 选择状态无需猜测。
                )
                .resolution(12); // 固定低段数足够显示控制点且预算明确。
        }
    }
    if !selecting
        && let Some(path_index) = editable_path
        && let Some(path) = document.paths.get(path_index)
        && !path.closed
        && path.visible
        && let (Some(last), Some(cursor)) = (path.points.last(), editor.cursor_world)
        && let Some(last) = display(edited_point(
            &edit,
            path_index,
            path.points.len() - 1,
            *last,
        ))
    {
        gizmos.line(
            last,                         // 开放主路径当前末点的同源贴地高度。
            cursor + Vec3::Y * 0.35,      // 下一落点跟随当前地形命中。
            Color::srgb(1.0, 0.38, 0.12), // 橙色虚拟下一段与已落黄色段区分。
        );
    }
}

/// 只在边界草稿或工具状态变化时更新左栏点数、闭合、墙厚与操作提示。
pub(crate) fn sync_boundary_text(
    editor: Res<MapEditorState>,          // 工具选择变化需要更新提示。
    session: Res<EditorAuthoringSession>, // 边界文档变化需要更新规格。
    candidates: Res<LayerCandidateState>, // 选中整路径时摘要显示该路径，不固定首条。
    mut texts: Query<&mut Text, With<MapEditorBoundaryText>>, // 固定BSN文本节点原地更新。
) {
    if !editor.is_changed() && !session.is_changed() && !candidates.is_changed() {
        return; // 稳定帧不分配状态字符串。
    }
    let value = session
        .draft
        .as_ref()
        .and_then(|draft| draft.boundaries.as_ref())
        .and_then(|document| document.paths.get(super::helpers::selected_path_index(&session)?)) // 只读正式选择，悬停不切换属性。
        .map_or_else(
            || "空气墙：请新建或选择一面墙".to_owned(), // HN-01/HN-02专用实现不会被本工具替代。
            |path| {
                format!(
                    "空气墙：{} · {}点 · {}\n墙厚 {:.2}m · 墙高 {:.1}..{:.1}m\n单击加点，拖动控制点；Ctrl插点，Delete删选点\n内部挡墙至少2点即可；地图外框须闭合",
                    path.label, // 当前主路径作者显示名。
                    path.points.len(), // 控制点数量。
                    if path.closed { "已闭合" } else { "未闭合" }, // 安全状态直白回显。
                    path.thickness, // Avian Cuboid完整厚度。
                    path.bottom_y, // 空气墙最低世界Y。
                    path.top_y, // 空气墙最高世界Y。
                )
            },
        );
    for mut text in &mut texts {
        **text = value.clone(); // 固定节点通常唯一，防御重复节点时保持一致。
    }
}

/// 把当前未保存边界专用修订发布给正式动态地图碰撞系统；保存后归还文件权威。
#[derive(Default)]
pub(crate) struct OverrideCapture {
    identity: Option<(MapId, u64)>, // 捕获中途地图/修订变化时整体重启。
    capture: MapBoundaryCapture,
}

/// 完整分帧快照才交给运行时，普通稳定帧不复制作者数组。
pub(crate) fn sync_live_boundary_override(
    session: Res<EditorAuthoringSession>, // 当前草稿、边界专用修订和脏状态。
    mut live: ResMut<BoundaryLiveOverrideState>, // 防止稳定帧重复写覆盖资源。
    mut boundary_override: ResMut<DynamicMapBoundaryOverride>, // 正式场景唯一未保存覆盖入口。
    edit: Res<BoundaryEditState>,         // 控制点拖动尚未提交时不捕获混合文档。
    numeric: Option<Res<super::BoundaryThicknessInputState>>, // 数值scrub期间修订不变，需等事务结束。
    mut capture: Local<OverrideCapture>,                      // 固定预算保存当前最新待发布草稿。
) {
    let current_map_id = session.draft.as_ref().map(|draft| draft.map_id.clone()); // 轻量当前地图身份。
    if live.map_id != current_map_id {
        if live.active
            && let Some(old_map_id) = live.map_id.as_ref()
        {
            boundary_override.clear_for_map(old_map_id); // 换图先撤销旧地图未保存权威。
        }
        live.map_id = current_map_id.clone(); // 保存新草稿身份。
        live.revision = 0; // 新地图必须独立比较边界修订。
        live.active = false; // 尚未判断新地图是否需要内存接管。
    }
    let Some(draft) = session.draft.as_ref() else {
        return; // 空作者会话没有新的覆盖可发布。
    };
    let requires_override = draft.manifest.is_data_driven()
        && draft.boundaries.is_some()
        && (draft.boundaries_dirty || draft.boundary_catalog_registration_required); // 只有未保存边界变化取得文件之前的权威。
    if requires_override {
        if edit.before.is_some() || numeric.as_ref().is_some_and(|numeric| numeric.busy()) {
            capture.identity = None;
            capture.capture = default();
            return; // 松手后的单一作者修订才可以触发正式碰撞替换。
        }
        if live.active && live.revision == session.boundary_revision {
            return; // 拖动过程不推进修订，因此不会逐帧重建物理。
        }
        let Some(source) = draft.boundaries.as_ref() else {
            return; // requires_override已经保证存在，此处保持防御。
        };
        let identity = (draft.map_id.clone(), session.boundary_revision);
        if capture.identity.as_ref() != Some(&identity) {
            capture.identity = Some(identity);
            capture.capture = default();
        }
        if !capture.capture.step(source, 4096, 64) {
            return;
        }
        let Some(document) = capture.capture.take() else {
            return;
        }; // 只有完整点集可以移交给Arc发布。
        boundary_override.publish(
            draft.map_id.clone(),      // 覆盖严格绑定当前地图。
            session.boundary_revision, // 松手、按钮或撤销后才变化。
            document,                  // 小型完整文档由正式场景一次性派生碰撞。
        );
        live.revision = session.boundary_revision; // 保存最近发布修订。
        live.active = true; // 当前正式运行时由内存草稿接管。
    } else if live.active {
        boundary_override.clear_for_map(&draft.map_id); // 保存成功后正式文件重新取得权威。
        live.revision = session.boundary_revision; // 保存清除时的作者修订用于诊断。
        live.active = false; // 下一次真实修改可以重新发布。
    }
}
