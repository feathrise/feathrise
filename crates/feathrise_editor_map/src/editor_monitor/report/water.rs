//! F4静水、河流、瀑布与GLB整网格水体的作者数据和预览状态报告。

use feathrise_map_content::{MapStillWaterKind, MapWaterEndpoint}; // 水域枚举转换为开发者可读中文语义。

use super::common::{format_count, no_draft_report, page_header}; // 水域各页共享地图身份和计数格式。
use crate::authoring::{EditorAuthoringSession, EditorMapDraft, WaterPreviewState}; // 只读作者水网与既有single-flight状态。
use crate::resources::MapEditorState; // F12与保存状态用于页头。

/// 报告水平湖泊/池塘的R8、水位和正式预览状态。
pub(super) fn build_still_water_report(
    editor: &MapEditorState,          // 当前F12与dirty状态。
    session: &EditorAuthoringSession, // 当前作者草稿及水域修订。
    preview: &WaterPreviewState,      // 现有水网构建与活动水面缓存。
) -> String {
    let Some(draft) = session.draft.as_ref() else {
        return no_draft_report("静水");
    };
    let mut lines = page_header("静水", editor, session, draft);
    let Some(water) = draft.water.as_ref() else {
        lines.push("当前地图没有数据驱动水域作者文件。".to_owned());
        lines.push("F4不会把场景中的普通蓝色网格猜成湖泊或池塘。".to_owned());
        return lines.join("\n");
    };
    lines.push(format!(
        "水域修订：{}  |  静水 {} 层  |  已加载遮罩 {} 层  |  待保存遮罩 {} 层",
        session.water_revision,
        water.layers.len(),
        draft.water_masks.len(),
        draft.water_dirty_layers.len()
    ));
    if water.layers.is_empty() {
        lines.push("当前地图没有水平静水体。".to_owned());
    }
    for layer in &water.layers {
        let kind = match layer.still_kind {
            MapStillWaterKind::Lake => "湖泊",
            MapStillWaterKind::Pond => "池塘",
        }; // 枚举中文化后与F12作者工具保持同一概念。
        lines.push(format!(
            "{} [{}]：{} × {} R8  |  水位 {:.2}m  |  流速 {:.2}  |  粗糙 {:.2}  |  遮罩 {}",
            layer.label,
            layer.id,
            layer.resolution[0],
            layer.resolution[1],
            layer.water_level,
            layer.flow_speed,
            layer.perceptual_roughness,
            if draft.water_masks.contains_key(&layer.id) {
                "已加载"
            } else {
                "缺失"
            }
        )); // 此诊断报告按实际层数展示，作者层数不限制。
        lines.push(format!(
            "  类型：{kind}  |  作者源：{}",
            layer.mask_source_path
        ));
    }
    append_water_preview_status(&mut lines, draft, session, preview); // 复用整批水网状态而不重建几何。
    lines.push(
        "统计口径：只读取水层元数据、遮罩缓存键和既有预览状态，不扫描R8/R16像素。".to_owned(),
    );
    lines.join("\n")
}

/// 报告三维河流样条的数量、控制点预算和连接身份。
pub(super) fn build_river_report(
    editor: &MapEditorState,          // 当前F12与dirty状态。
    session: &EditorAuthoringSession, // 当前作者草稿及水域修订。
    preview: &WaterPreviewState,      // 整批水网预览状态。
) -> String {
    let Some(draft) = session.draft.as_ref() else {
        return no_draft_report("河流");
    };
    let mut lines = page_header("河流", editor, session, draft);
    let Some(water) = draft.water.as_ref() else {
        lines.push("当前地图没有数据驱动水域作者文件。".to_owned());
        return lines.join("\n");
    };
    let total_points = water.rivers.iter().fold(0_u64, |total, river| {
        total.saturating_add(river.points.len() as u64)
    }); // 最多三十二条河，只读取每个Vec的O(1)长度。
    let total_segments = water.rivers.iter().fold(0_u64, |total, river| {
        total.saturating_add(river.points.len().saturating_sub(1) as u64)
    }); // 不遍历控制点内容即可得到拓扑段数。
    lines.push(format!(
        "水域修订：{}  |  河流 {} 条  |  控制点 {}  |  样条段 {}",
        session.water_revision,
        water.rivers.len(),
        format_count(total_points),
        format_count(total_segments)
    ));
    if water.rivers.is_empty() {
        lines.push("当前地图没有河流样条。".to_owned());
    }
    for river in water.rivers.iter().take(8) {
        lines.push(format!(
            "{} [{}]：{} 个点  |  上游 {}  |  下游 {}  |  粗糙 {:.2}",
            river.label,
            river.id,
            river.points.len(),
            river.source_layer_id.as_deref().unwrap_or("自由端"),
            river.target_layer_id.as_deref().unwrap_or("自由端"),
            river.perceptual_roughness
        )); // 正文最多列出八条身份，避免高地图数据把面板无限拉长。
    }
    if water.rivers.len() > 8 {
        lines.push(format!(
            "其余河流：{} 条（总量已计入上方统计）",
            water.rivers.len() - 8
        ));
    }
    append_water_preview_status(&mut lines, draft, session, preview);
    lines
        .push("统计口径：控制点和段数由各河流Vec长度归约，不读取点坐标、不重采样样条。".to_owned());
    lines.join("\n")
}

/// 报告独立瀑布水幕的数量、控制点预算和端点引用。
pub(super) fn build_waterfall_report(
    editor: &MapEditorState,          // 当前F12与dirty状态。
    session: &EditorAuthoringSession, // 当前作者草稿及水域修订。
    preview: &WaterPreviewState,      // 整批水网预览状态。
) -> String {
    let Some(draft) = session.draft.as_ref() else {
        return no_draft_report("瀑布");
    };
    let mut lines = page_header("瀑布", editor, session, draft);
    let Some(water) = draft.water.as_ref() else {
        lines.push("当前地图没有数据驱动水域作者文件。".to_owned());
        return lines.join("\n");
    };
    let total_points = water.waterfalls.iter().fold(0_u64, |total, waterfall| {
        total.saturating_add(waterfall.points.len() as u64)
    }); // 最多三十二个瀑布，只读取每个控制点Vec长度。
    lines.push(format!(
        "水域修订：{}  |  瀑布 {} 个  |  控制点 {}",
        session.water_revision,
        water.waterfalls.len(),
        format_count(total_points)
    ));
    if water.waterfalls.is_empty() {
        lines.push("当前地图没有独立瀑布水幕。".to_owned());
    }
    for waterfall in water.waterfalls.iter().take(8) {
        lines.push(format!(
            "{} [{}]：{} 个点  |  {} → {}  |  泡沫 {:.2}  |  粗糙 {:.2}",
            waterfall.label,
            waterfall.id,
            waterfall.points.len(),
            endpoint_label(waterfall.source.as_ref()),
            endpoint_label(waterfall.target.as_ref()),
            waterfall.foam_strength,
            waterfall.perceptual_roughness
        )); // 最多展示八项详细身份，其余只进入总量。
    }
    if water.waterfalls.len() > 8 {
        lines.push(format!(
            "其余瀑布：{} 个（总量已计入上方统计）",
            water.waterfalls.len() - 8
        ));
    }
    append_water_preview_status(&mut lines, draft, session, preview);
    lines.push(
        "统计口径：只读取端点引用和控制点Vec长度，不遍历点坐标或重新生成水幕Mesh。".to_owned(),
    );
    lines.join("\n")
}

/// 报告Blender场景实例上的整网格水绑定缓存。
pub(super) fn build_mesh_water_report(
    editor: &MapEditorState,          // 当前F12与dirty状态。
    session: &EditorAuthoringSession, // 当前作者草稿及摆放结构修订。
) -> String {
    let Some(draft) = session.draft.as_ref() else {
        return no_draft_report("网格水体");
    };
    let mut lines = page_header("网格水体", editor, session, draft);
    lines.push(format!(
        "摆放结构修订：{}  |  场景摆放 {} 项",
        session.placement_structure_revision,
        format_count(draft.placements.placements.len() as u64)
    ));
    lines.push(format!(
        "整网格水绑定：{} 项  |  绑定数量不设作者总门",
        format_count(draft.placement_water_surface_count as u64)
    )); // 该总量在加载和编辑动作中增量维护，F4无需扫描最多8192个placement。
    lines.push(format!(
        "摆放预览根：{}  |  撤销 {} / 重做 {}",
        format_count(session.placement_previews.len() as u64),
        format_count(session.undo.len() as u64),
        format_count(session.redo.len() as u64)
    ));
    if draft.placement_water_surface_count == 0 {
        lines.push("当前地图没有水平水、斜坡水或瀑布类型的GLB网格绑定。".to_owned());
    }
    lines.push(
        "统计口径：绑定总数来自草稿维护缓存；F4稳定刷新不遍历完整placements、Scene0或World。"
            .to_owned(),
    );
    lines.join("\n")
}

/// 把瀑布端点稳定引用转为紧凑中文文字。
fn endpoint_label(endpoint: Option<&MapWaterEndpoint>) -> String {
    match endpoint {
        Some(MapWaterEndpoint::StillWater { layer_id }) => format!("静水:{layer_id}"),
        Some(MapWaterEndpoint::RiverStart { river_id }) => format!("河流首端:{river_id}"),
        Some(MapWaterEndpoint::RiverEnd { river_id }) => format!("河流末端:{river_id}"),
        None => "未绑定".to_owned(),
    } // 正式瀑布应有两端；未完成草稿仍要如实显示。
}

/// 解释当前整批水网single-flight阶段；不触碰水面Mesh或像素数据。
fn append_water_preview_status(
    lines: &mut Vec<String>,          // 当前水域页面输出行。
    draft: &EditorMapDraft,           // 当前地图身份用于拒绝串图状态。
    session: &EditorAuthoringSession, // 水域与地形联合修订决定水面身份。
    preview: &WaterPreviewState,      // 既有任务、安装和活动水面缓存。
) {
    if preview.ready_for(
        &draft.map_id,
        session.water_revision,
        session.terrain_revision,
    ) {
        lines.push(format!(
            "正式预览：已采用水域修订 {} + 地形修订 {}  |  作者水面 {} 个",
            session.water_revision,
            session.terrain_revision,
            format_count(preview.spawns.len() as u64)
        ));
        return;
    }
    if preview.failed_for(
        &draft.map_id,
        session.water_revision,
        session.terrain_revision,
    ) {
        lines.push(format!(
            "正式预览：当前水域修订 {} + 地形修订 {} 构建失败；保留上一完整结果",
            session.water_revision, session.terrain_revision
        ));
        return;
    }
    if let Some(pending) = preview.pending.as_ref().filter(|pending| {
        pending.source.map_id == draft.map_id
            && pending.water_revision == session.water_revision
            && pending.terrain_revision == session.terrain_revision
    }) {
        lines.push(format!(
            "正式预览：分帧安装中  |  待处理 {}  |  已暂存 {}  |  三角形 {}",
            format_count(pending.remaining.len() as u64),
            format_count(pending.staged.len() as u64),
            format_count(pending.triangle_count as u64)
        )); // 安装队列长度和缓存三角形数均为O(1)。
        return;
    }
    let computing = preview.task.is_some()
        && preview.desired_map_id.as_ref() == Some(&draft.map_id)
        && preview.desired_water_revision == session.water_revision
        && preview.desired_terrain_revision == session.terrain_revision; // 只有联合身份一致才属于当前草稿。
    if computing {
        lines.push("正式预览：后台整批建网格中；完成前继续保留上一完整水网。".to_owned());
    } else {
        lines.push(format!(
            "正式预览：尚未采用当前水域修订 {} + 地形修订 {}  |  当前旧水面 {} 个",
            session.water_revision,
            session.terrain_revision,
            format_count(preview.spawns.len() as u64)
        ));
    }
}
