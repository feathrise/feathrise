//! 作者图层覆盖输入选择；视口悬停只改变预览，不写回正式选层。
use super::*; // 共用覆盖输入与编辑器类型。

pub(super) fn selected_overlay<'a>(
    editor: &MapEditorState,
    session: &'a EditorAuthoringSession,
    viewport_selection: &LayerCandidateState, // 独立于侧栏正式索引的悬停目标。
) -> Option<OverlaySelection<'a>> {
    let draft = session.draft.as_ref()?; // 覆盖必须属于当前完整作者草稿。
    if !super::super::terrain_documents::canvas_visible(session, editor) {
        return None;
    }
    let terrain_id = draft
        .terrain_documents
        .active
        .as_ref()
        .map(|active| &active.definition.id);
    if editor.tool == EditorTool::Water {
        let layer = draft
            .water
            .as_ref()?
            .layers
            .get(editor.selected_water_layer)?;
        return Some(OverlaySelection {
            map_id: &draft.map_id,
            terrain_id,
            id: layer.id.as_str(),
            revision: session.water_revision,
            resolution: layer.resolution,
            density: draft.water_masks.get(&layer.id)?,
            color: [36, 150, 255], // 静水覆盖使用蓝色，和植被类别颜色明确区分。
        });
    } // Clipmap静水复用同一个有界R8降采样任务和正式地形投影。
    if editor.tool == EditorTool::Inspect {
        if !viewport_selection.author_canvas_matches(draft) { return None; }
        // 选择模式只读取精确作者源，不解析合并输出。
        let LayerTarget::Vegetation(id) = viewport_selection
            .hover
            .as_ref()
            .or(viewport_selection.selected.as_ref())?
        else {
            return None;
        }; // 跨类别悬停隐藏旧植被高亮。
        if editor.selected_map_id.as_ref() != Some(&draft.map_id) {
            return None;
        } // 禁止旧地图身份泄漏。
        let index = viewport_selection.vegetation_index(id)?; // 使用后台候选索引，不逐帧扫描作者层。
        let author = draft
            .vegetation
            .as_ref()?
            .authoring
            .as_ref()?
            .layers
            .get(index)?; // 防御删除换位。
        if author.id != *id || !author.enabled {
            return None;
        } // 隐藏作者不能被旧候选恢复高亮。
        return Some(OverlaySelection {
            // 同预设各层仍读取各自画布。
            map_id: &draft.map_id, // 完整地图身份。
            terrain_id,
            id: &author.id,                                      // 作者稳定ID。
            revision: session.vegetation_revision,               // 仅作者修订控制覆盖更新。
            resolution: author.resolution,                       // 独立画布尺寸。
            density: draft.vegetation_author_densities.get(id)?, // 共享作者源，不使用合并Compiled输出。
            color: [24, 220, 255],                               // 选择模式统一青色高亮。
        }); // 不修改正式侧栏选层与预览模式。
    } // 编辑模式继续保留原来的源图、最终与冲突预览。
    let selected = selected_vegetation_layer(draft, editor.selected_vegetation_layer)?; // UI索引指作者层。
    match editor.vegetation_preview_mode {
        VegetationPreviewMode::Source => Some(OverlaySelection {
            map_id: &draft.map_id, // 当前地图身份。
            terrain_id,
            id: selected.author.id.as_str(), // 原图身份使用作者层ID。
            revision: session.vegetation_revision, // 笔刷当帧推进作者修订。
            resolution: selected.author.resolution, // 每张作者图可以拥有独立分辨率。
            density: draft.vegetation_author_densities.get(&selected.author.id)?, // 笔刷真正写入的R8画布。
            color: layer_color(selected.output), // 颜色仍表达最终profile类别。
        }),
        VegetationPreviewMode::Compiled => Some(OverlaySelection {
            map_id: &draft.map_id, // 当前地图身份。
            terrain_id,
            id: selected.output.id.as_str(), // 最终图身份使用固定正式层ID。
            revision: session.vegetation_compiled_revision, // 只显示完整原子编译修订。
            resolution: selected.output.resolution, // 正式运行时纹理分辨率。
            density: draft.vegetation_densities.get(&selected.output.id)?, // 与四级LOD和保存派生完全相同的最终R8。
            color: layer_color(selected.output),                           // 保持profile综合色。
        }),
        VegetationPreviewMode::Conflict => {
            let conflict = draft.vegetation_conflicts.get(&selected.group.id)?; // 冲突图与当前组完整编译结果同修订。
            Some(OverlaySelection {
                map_id: &draft.map_id, // 当前地图身份。
                terrain_id,
                id: selected.group.id.as_str(), // 冲突身份使用作者组ID。
                revision: session.vegetation_compiled_revision, // 不能用新作者源配旧冲突图。
                resolution: conflict.resolution, // 当前组最高作者分辨率。
                density: &conflict.pixels,      // 同组至少两层重叠的强度覆盖。
                color: [255, 48, 132],          // 洋红色明确区别任何具体草木profile。
            })
        }
    }
}

/// 植被工具显示当前画布；选择模式显示悬停优先的精确作者图。
pub(super) fn overlay_context_active(
    editor: &MapEditorState,
    session: &EditorAuthoringSession,
    viewport_selection: &LayerCandidateState, // 统一候选状态。
) -> bool {
    if !super::super::terrain_documents::canvas_visible(session, editor) {
        return false;
    }
    if matches!(editor.tool, EditorTool::Vegetation | EditorTool::Water) {
        return true; // 正常笔刷、参数和图层行操作保持既有整层覆盖语义。
    }
    if editor.tool != EditorTool::Inspect {
        return false; // 其他作者工具不能被旧植被选择覆盖视口。
    }
    selected_overlay(editor, session, viewport_selection).is_some() // 不接受其他类别或已失效作者目标。
}

pub(super) fn overlay_mode(editor: &MapEditorState) -> VegetationPreviewMode {
    // 请求和显示共用模式身份。
    if editor.tool == EditorTool::Inspect || editor.tool == EditorTool::Water {
        VegetationPreviewMode::Source
    } else {
        editor.vegetation_preview_mode
    } // 选择不能合并同预设不同作者层。
}
