//! F4总览、地形与地表页面的动态地图作者数据报告。

use feathrise_map_content::{MapTerrainKind, MapVegetationKind}; // 总览区分GPU叶片与Mesh，地形页翻译作者源类型。

use super::common::{format_count, no_draft_report, page_header, world_extent}; // 各页共享稳定身份与计数格式。
use crate::authoring::{EditorAuthoringSession, VegetationPreviewState, WaterPreviewState}; // 只读取作者草稿和既有预览完成值。
use crate::resources::MapEditorState; // F12和dirty状态用于页头。

/// 汇总当前动态地图全部数据域，不展开任何整图像素或逐实体集合。
pub(super) fn build_overview_report(
    editor: &MapEditorState,                     // 当前F12会话状态。
    session: &EditorAuthoringSession,            // 当前作者草稿与各域修订。
    vegetation_preview: &VegetationPreviewState, // 已采用Mesh层只读计数缓存。
    water_preview: &WaterPreviewState,           // 已采用水面只读计数缓存。
) -> String {
    let Some(draft) = session.draft.as_ref() else {
        return no_draft_report("总览"); // F4独立存在时保持明确空状态。
    };
    let mut lines = page_header("总览", editor, session, draft); // 统一地图身份和保存状态。
    if let Some((width, length)) = world_extent(draft.manifest.world_min, draft.manifest.world_max)
    {
        lines.push(format!(
            "世界范围：{width:.1}m × {length:.1}m  |  默认出生：({:.1}, {:.1}, {:.1})",
            draft.manifest.default_spawn[0],
            draft.manifest.default_spawn[1],
            draft.manifest.default_spawn[2]
        )); // 只读取清单中的米制端点和出生点。
    } else {
        lines.push("世界范围：无效  |  请检查地图清单的world_min/world_max".to_owned());
    }

    let surface_layers = draft
        .surface
        .as_ref()
        .map_or(0, |surface| surface.layers.len()); // 地表层数为Vec常数时间长度。
    let (blade_layers, mesh_layers) = draft.vegetation.as_ref().map_or((0, 0), |vegetation| {
        vegetation
            .layers
            .iter()
            .fold((0, 0), |(blade, mesh), layer| {
                match layer.kind {
                    MapVegetationKind::Blade => (blade + 1, mesh), // 最多八个正式Blade层。
                    MapVegetationKind::Mesh => (blade, mesh + 1),  // 最多三十二层总预算内计数。
                }
            })
    });
    let (still_water, rivers, waterfalls) = draft.water.as_ref().map_or((0, 0, 0), |water| {
        (
            water.layers.len(),
            water.rivers.len(),
            water.waterfalls.len(),
        ) // 三个Vec长度不访问控制点内容。
    });
    lines.push(format!(
        "地图资源：地形 1  |  地表 {} 层  |  植被 Blade {} / Mesh {}  |  静水 {} / 河流 {} / 瀑布 {}",
        surface_layers, blade_layers, mesh_layers, still_water, rivers, waterfalls
    ));

    let boundary_paths = draft
        .boundaries
        .as_ref()
        .map_or(0, |asset| asset.paths.len()); // 边界路径总量有固定十六条预算。
    lines.push(format!(
        "场景与交互：摆放 {}  |  网格水绑定 {}  |  传送点 {}  |  区域边界 {} 条",
        format_count(draft.placements.placements.len() as u64),
        format_count(draft.placement_water_surface_count as u64),
        format_count(draft.portals.portals.len() as u64),
        format_count(boundary_paths as u64)
    )); // 所有数量都直接来自已加载作者文档或维护好的常数缓存。

    let active_mesh_layers = vegetation_preview
        .layers
        .values()
        .filter(|layer| layer.map_id == draft.map_id)
        .count(); // 预览层表最多受正式植被层预算约束。
    let active_mesh_instances = vegetation_preview
        .layers
        .values()
        .filter(|layer| layer.map_id == draft.map_id)
        .fold(0_u64, |total, layer| {
            total.saturating_add(layer.instance_count as u64)
        }); // 复用精确缓存计数，不逐株查询ECS。
    lines.push(format!(
        "植被缓存：正式统计 {} 层  |  非零层 {}  |  已采用Mesh预览 {} 层 / {} 个实例",
        draft.vegetation_density_stats.len(),
        draft.vegetation_nonzero_layers.len(),
        active_mesh_layers,
        format_count(active_mesh_instances)
    ));

    let water_state = if water_preview.ready_for(
        &draft.map_id,
        session.water_revision,
        session.terrain_revision,
    ) {
        "已采用当前联合修订"
    } else if water_preview.task.is_some() || water_preview.pending.is_some() {
        "后台构建或分帧安装中"
    } else {
        "尚无当前联合修订预览"
    }; // 不读取Mesh或R8，只解释既有single-flight状态机。
    lines.push(format!(
        "水域缓存：{water_state}  |  当前作者水面 {} 个",
        format_count(water_preview.spawns.len() as u64)
    ));
    lines.push(format!(
        "领域修订：地形 {}  |  地表 {}  |  植被 {}/正式 {}  |  水域 {}  |  环境 {}  |  边界 {}",
        session.terrain_revision,
        session.surface_revision,
        session.vegetation_revision,
        session.vegetation_compiled_revision,
        session.water_revision,
        session.environment_revision,
        session.boundary_revision
    ));
    lines.push(
        "统计口径：F4每次只构建当前页；稳定刷新不扫描R8/R16、不遍历逐株实体，也不启动第二份后台任务。"
            .to_owned(),
    ); // 面板自身的性能边界向开发者保持可见。
    lines.join("\n")
}

/// 报告地形清单、R16加载身份和米制采样规格。
pub(super) fn build_terrain_report(
    editor: &MapEditorState,          // 当前F12与dirty状态。
    session: &EditorAuthoringSession, // 当前作者草稿及地形修订。
) -> String {
    let Some(draft) = session.draft.as_ref() else {
        return no_draft_report("地形");
    };
    let terrain = &draft.manifest.terrain; // 地形权威描述始终来自地图清单。
    let mut lines = page_header("地形", editor, session, draft);
    let kind = match terrain.kind {
        MapTerrainKind::Empty => "独立地形对象容器",
        MapTerrainKind::Voxel => "独立体素密度源",
        MapTerrainKind::Gltf => "GLB/GLTF作者地形",
        MapTerrainKind::Heightfield => "R16高度场",
        MapTerrainKind::SparseClipmap => "稀疏作者块 / Geometry Clipmap", // 新图不把逻辑格点冒充完整GLB。
    }; // Rust枚举不直接泄漏到中文开发面板。
    lines.push(format!(
        "作者源：{kind}  |  修订 {}  |  {}",
        session.terrain_revision,
        if draft.terrain_dirty {
            "待保存"
        } else {
            "已落盘"
        }
    ));
    let expected_samples =
        u64::from(terrain.resolution[0]).saturating_mul(u64::from(terrain.resolution[1])); // 受损分辨率也不允许乘法回绕。
    lines.push(format!(
        "高度规格：{} × {} R16  |  期望样本 {}  |  当前内存 {}",
        terrain.resolution[0],
        terrain.resolution[1],
        format_count(expected_samples),
        draft.heights.as_ref().map_or_else(
            || "未加载".to_owned(),
            |heights| format!("{} 个样本", format_count(heights.len() as u64))
        )
    )); // Arc长度为O(1)，不读取任何高度样本。
    lines.push(format!(
        "垂直量程：{:.2}m .. {:.2}m  |  高差 {:.2}m",
        terrain.base_height,
        terrain.base_height + terrain.height_scale,
        terrain.height_scale
    ));
    if let Some((width, length)) = world_extent(draft.manifest.world_min, draft.manifest.world_max)
    {
        let spacing_x =
            (terrain.resolution[0] > 1).then(|| width / (terrain.resolution[0] - 1) as f32); // 顶点端点覆盖完整X范围。
        let spacing_z =
            (terrain.resolution[1] > 1).then(|| length / (terrain.resolution[1] - 1) as f32); // 顶点端点覆盖完整Z范围。
        lines.push(match (spacing_x, spacing_z) {
            (Some(x), Some(z)) => {
                format!("地图范围：{width:.1}m × {length:.1}m  |  顶点间距：X {x:.3}m / Z {z:.3}m")
            }
            _ => format!("地图范围：{width:.1}m × {length:.1}m  |  分辨率不足以计算顶点间距"),
        });
    } else {
        lines.push("地图范围：无效".to_owned());
    }
    lines.push(format!("派生GLB：{}", terrain.glb_path)); // 路径用于确认当前草稿绑定了哪份地形产物。
    lines.push(format!(
        "R16源：{}  |  高度纹理：{}",
        terrain.height_path.as_deref().unwrap_or("无"),
        terrain.height_texture_path.as_deref().unwrap_or("无")
    ));
    lines.push(
        "统计口径：只读取清单与Arc样本长度，不在F4刷新中求高度最小值、最大值或扫描地形网格。"
            .to_owned(),
    );
    lines.join("\n")
}

/// 报告RGBA地表权重、层参数和纹理绑定，不扫描权重像素。
pub(super) fn build_surface_report(
    editor: &MapEditorState,          // 当前F12与dirty状态。
    session: &EditorAuthoringSession, // 当前作者草稿及地表修订。
) -> String {
    let Some(draft) = session.draft.as_ref() else {
        return no_draft_report("地表");
    };
    let mut lines = page_header("地表", editor, session, draft);
    let Some(surface) = draft.surface.as_ref() else {
        lines.push("当前地图没有数据驱动地表作者文件。".to_owned());
        lines.push("旧专用地图可继续使用自身地表适配器；F4不会猜测或伪造层数据。".to_owned());
        return lines.join("\n");
    };
    lines.push(format!(
        "地表修订：{}  |  {}  |  图层 {} 个",
        session.surface_revision,
        if draft.surface_dirty {
            "待保存"
        } else {
            "已落盘"
        },
        surface.layers.len()
    ));
    let texel_count =
        u64::from(surface.resolution[0]).saturating_mul(u64::from(surface.resolution[1]));
    let expected_weight_bytes = texel_count.saturating_mul(4); // RGBA8每像素四字节。
    let expected_index_bytes = texel_count.saturating_mul(16); // 四个小端u32局部索引。
    lines.push(format!(
        "权重规格：{} × {} RGBA8  |  期望 {} 字节  |  当前内存 {}",
        surface.resolution[0],
        surface.resolution[1],
        format_count(expected_weight_bytes),
        draft.surface_weights.as_ref().map_or_else(
            || "未加载".to_owned(),
            |weights| format!("{} 字节", format_count(weights.len() as u64))
        )
    )); // Vec长度为O(1)，不读取综合色像素。
    lines.push(format!(
        "索引规格：RGBA32_UINT  |  期望 {} 字节  |  当前内存 {}  |  每纹素固定取贡献最高4层",
        format_count(expected_index_bytes),
        draft.surface_indices.as_ref().map_or_else(
            || "未加载".to_owned(),
            |indices| format!("{} 字节", format_count(indices.len() as u64))
        )
    ));
    for (index, layer) in surface.layers.iter().enumerate() {
        lines.push(format!(
            "层 {}：{} [{}]  |  全局预设 {}",
            index, layer.label, layer.id, layer.preset_id,
        )); // 只遍历作者明确登记的地图层，不读取整张索引或权重图。
    }
    lines.push(format!(
        "权重：{} → {}",
        surface.weight_source_path, surface.weight_texture_path,
    ));
    lines.push(format!(
        "索引：{} → {}  |  PBR数组由全局地表预设目录统一提供",
        surface.index_source_path, surface.index_texture_path,
    ));
    lines.push("统计口径：F4只显示层引用与缓冲长度，不扫描索引/权重或重建综合色。".to_owned());
    lines.join("\n")
}
