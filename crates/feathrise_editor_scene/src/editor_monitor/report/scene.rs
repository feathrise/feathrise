//! F4场景摆放、传送点、区域边界与场景环境的作者数据报告。

use feathrise_atmosphere::{WeatherDebugOverride, WeatherState}; // 环境报告复用F12天气滑块的正式状态与调试覆盖来源。

use super::common::{format_count, no_draft_report, page_header}; // 场景与玩法页共用场景身份和大数格式。
use crate::authoring::EditorAuthoringSession; // 所有数据直接来自当前已加载草稿和修订。
use crate::resources::SceneEditorState; // F12与保存状态用于统一页头。

/// 报告显式GLB/GLTF场景摆放总量和少量稳定身份样本。
pub(super) fn build_placements_report(
    editor: &SceneEditorState,          // 当前F12与dirty状态。
    session: &EditorAuthoringSession, // 当前摆放文档、预览根和结构修订。
) -> String {
    let Some(draft) = session.draft.as_ref() else {
        return no_draft_report("场景摆放");
    };
    let mut lines = page_header("场景摆放", editor, session, draft);
    lines.push(format!(
        "摆放结构修订：{}  |  作者记录 {}  |  当前预览根 {}",
        session.placement_structure_revision,
        format_count(draft.placements.placements.len() as u64),
        format_count(session.placement_previews.len() as u64)
    ));
    lines.push(format!(
        "网格水绑定：{}  |  撤销 {} / 重做 {}",
        format_count(draft.placement_water_surface_count as u64),
        format_count(session.undo.len() as u64),
        format_count(session.redo.len() as u64)
    ));
    if draft.placements.placements.is_empty() {
        lines.push("当前场景没有显式场景摆放。".to_owned());
    }
    for placement in draft.placements.placements.iter().take(8) {
        lines.push(format!(
            "{}：{}  |  位置 ({:.1}, {:.1}, {:.1})  |  网格水 {}",
            placement.id,
            placement.asset_path,
            placement.translation[0],
            placement.translation[1],
            placement.translation[2],
            placement.water_surfaces.len()
        )); // 只查看固定前八条，场景摆放越多也不会扩大每次刷新工作量。
    }
    if draft.placements.placements.len() > 8 {
        lines.push(format!(
            "其余摆放：{} 项（完整数量见上方；详细层级继续在场景页查看）",
            format_count((draft.placements.placements.len() - 8) as u64)
        ));
    }
    lines.push(format!(
        "作者文件版本：{}  |  场景目录：{}",
        draft.placements.schema_version, draft.folder
    ));
    lines.push(
        "统计口径：总量读取Vec长度，详细样本固定最多八条；F4不扫描Scene0、World或全部8192项。"
            .to_owned(),
    );
    lines.join("\n")
}

/// 报告场景传送点、跨场景目标和当前预览根数量。
pub(super) fn build_portals_report(
    editor: &SceneEditorState,          // 当前F12与dirty状态。
    session: &EditorAuthoringSession, // 当前传送文档与预览根。
) -> String {
    let Some(draft) = session.draft.as_ref() else {
        return no_draft_report("传送点");
    };
    let mut lines = page_header("传送点", editor, session, draft);
    lines.push(format!(
        "传送点：{} 个  |  当前预览根 {}  |  作者文件版本 {}",
        format_count(draft.portals.portals.len() as u64),
        format_count(session.portal_previews.len() as u64),
        draft.portals.schema_version
    ));
    if draft.portals.portals.is_empty() {
        lines.push("当前场景没有数据驱动传送点。".to_owned());
    }
    for portal in draft.portals.portals.iter().take(8) {
        lines.push(format!(
            "{} [{}]：({:.1}, {:.1}, {:.1})  →  {} / {}",
            portal.label,
            portal.id,
            portal.position[0],
            portal.position[1],
            portal.position[2],
            portal.target.scene_id.as_str(),
            portal.target.portal_id.as_deref().unwrap_or("默认起始位置") // None是正式默认落点语义，监控页不得输出Option调试格式。
        )); // 固定最多八条目标身份，避免传送点数量影响稳定刷新预算。
    }
    if draft.portals.portals.len() > 8 {
        lines.push(format!(
            "其余传送点：{} 个",
            draft.portals.portals.len() - 8
        ));
    }
    lines.push(
        "统计口径：只读取传送文档长度和固定前八条记录，不查询触发器实体或目标场景文件。".to_owned(),
    );
    lines.join("\n")
}

/// 报告区域边界路径、控制点与可派生碰撞段总量。
pub(super) fn build_boundaries_report(
    editor: &SceneEditorState,          // 当前F12与dirty状态。
    session: &EditorAuthoringSession, // 当前边界文档及修订。
) -> String {
    let Some(draft) = session.draft.as_ref() else {
        return no_draft_report("区域边界");
    };
    let mut lines = page_header("区域边界", editor, session, draft);
    let Some(boundaries) = draft.boundaries.as_ref() else {
        lines.push("当前场景没有数据驱动区域边界文件。".to_owned());
        lines.push("动态场景发布前仍需要至少一条闭合安全边界。".to_owned());
        return lines.join("\n");
    };
    let total_points = boundaries.paths.iter().fold(0_u64, |total, path| {
        total.saturating_add(path.points.len() as u64)
    }); // 最多十六条路径，只读取每条Vec长度。
    let total_segments = boundaries.paths.iter().fold(0_u64, |total, path| {
        let open_segments = path.points.len().saturating_sub(1) as u64; // 相邻控制点构成主体段。
        let closing_segment = u64::from(path.closed && path.points.len() >= 3); // 闭合路径多一段回首点。
        total.saturating_add(open_segments.saturating_add(closing_segment))
    });
    lines.push(format!(
        "边界修订：{}  |  {}  |  路径 {}  |  控制点 {}  |  碰撞段 {}",
        session.boundary_revision,
        if draft.boundaries_dirty {
            "待保存"
        } else {
            "已落盘"
        },
        boundaries.paths.len(),
        format_count(total_points),
        format_count(total_segments)
    ));
    for path in &boundaries.paths {
        lines.push(format!(
            "{} [{}]：{} 个点  |  {}  |  Y {:.1}..{:.1}m  |  厚 {:.2}m",
            path.label,
            path.id,
            path.points.len(),
            if path.closed { "闭合" } else { "开放" },
            path.bottom_y,
            path.top_y,
            path.thickness
        )); // 正式边界最多十六条，逐路径元数据仍是固定小预算。
    }
    lines.push(
        "统计口径：点数与段数只由路径Vec长度计算，不遍历坐标、不派生Collider或查询物理World。"
            .to_owned(),
    );
    lines.join("\n")
}

/// 报告当前场景天空IBL、光照作者值和正式天气配置。
pub(super) fn build_environment_report(
    editor: &SceneEditorState,                 // 当前F12与dirty状态。
    session: &EditorAuthoringSession,        // 当前清单环境与修订。
    weather_state: &WeatherState,            // 自动天气的当前连续过渡进度。
    weather_override: &WeatherDebugOverride, // F12强制天气或手动预览覆盖。
) -> String {
    let Some(draft) = session.draft.as_ref() else {
        return no_draft_report("环境");
    };
    let mut lines = page_header("环境", editor, session, draft);
    let environment = &draft.manifest.environment; // 环境权威值与场景清单同生共存。
    lines.push(format!(
        "环境修订：{}  |  {}",
        session.environment_revision,
        if draft.environment_dirty {
            "待保存"
        } else {
            "已落盘"
        }
    ));
    lines.push(format!(
        "天空模式：{}  |  天空资源：{}",
        environment.sky_mode.display_name(),
        environment
            .skybox_path
            .as_deref()
            .unwrap_or("无（动态天空仍正常显示）")
    ));
    lines.push(format!(
        "环境参数：天空盒亮度 {:.1}（仅天空盒）  |  漫反射补光 {:.1}  |  镜面反射 {:.1}  |  天空/IBL Y旋转 {:.1}°",
        environment.skybox_brightness,
        environment.environment_diffuse_intensity,
        environment.environment_specular_intensity,
        environment.normalized_skybox_yaw_degrees()
    ));
    lines.push(format!(
        "天气配置：{}  |  气候：{}",
        environment.weather.region.display_name(),
        environment.weather.climate.display_name()
    ));
    let automatic_weather_progress = if weather_state.is_transitioning() {
        weather_state.transition_progress() // 过渡中显示状态机真实连续比例。
    } else {
        1.0 // 稳定天气在F12滑块中统一显示完整生效。
    };
    let weather_preview_progress =
        if weather_override.forced().is_some() || weather_override.preview_override_active() {
            weather_override.preview() // 强制模式和手动拖动均采用调试覆盖比例。
        } else {
            automatic_weather_progress // 自动模式未手动接管时保持与正式天气状态一致。
        };
    lines.push(format!(
        "场景资产根：{} 个  |  天气预览进度 {:.1}%",
        draft.manifest.asset_roots.len(),
        weather_preview_progress * 100.0
    ));
    for root in draft.manifest.asset_roots.iter().take(8) {
        lines.push(format!("资产根：{root}")); // 清单根数量很小，正文仍固定最多展示八项。
    }
    if draft.manifest.asset_roots.len() > 8 {
        lines.push(format!(
            "其余资产根：{} 个",
            draft.manifest.asset_roots.len() - 8
        ));
    }
    lines.push(
        "统计口径：显示清单中的作者基准；当前实时昼夜/天气乘数仍由运行时天气系统维护。".to_owned(),
    );
    lines.join("\n")
}
