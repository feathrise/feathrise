//! 地表预设选择只定位作者层；独立预设实现不能直接写地图草稿或历史。
use crate::{authoring::EditorAuthoringSession, resources::MapEditorState, types::EditorTool};
use feathrise_map_content::{SurfacePresetDescriptor, SurfacePresetRegistry};
pub(super) fn select_preset(
    editor: &mut MapEditorState,
    session: &EditorAuthoringSession,
    preset: &SurfacePresetDescriptor,
    registry: &SurfacePresetRegistry,
) -> Result<String, String> {
    if session.saving || editor.close_requested {
        // 保存快照冻结期间不能追加新层。
        return Err("地图正在保存或关闭，请稍候再选择地表预设".to_owned()); // 不产生看似创建成功的错误反馈。
    } // 仅在可编辑的地图上下文创建层。
    if session
        .draft
        .as_ref()
        .is_some_and(|draft| Some(&draft.map_id) != editor.selected_map_id.as_ref())
    {
        // 切图过渡时不得写入旧草稿。
        return Err("地图作者数据正在切换，请稍候再选择地表预设".to_owned()); // 等待新图完整加载。
    } // 稳定地图身份检查结束。
    editor.surface_selected_preset_id = Some(preset.id.clone());
    editor.tool = EditorTool::Surface;
    let Some(draft) = session.draft.as_ref() else {
        return Ok(format!("已选择“{}”；进入可编辑地图后可绘制", preset.label));
    };
    let Some(surface) = draft.surface.as_ref() else {
        return Err("当前地图没有索引式地表作者文件".to_owned());
    };
    let matches = |layer: &feathrise_map_content::MapSurfaceLayer| {
        registry
            .resolve(&layer.preset_id)
            .is_some_and(|resolved| resolved.id == preset.id)
    }; // 用规范预设身份匹配已有作者层。
    let existing = surface
        .layers
        .get(editor.selected_surface_layer)
        .filter(|layer| matches(layer))
        .map(|_| editor.selected_surface_layer) // 重选同预设保留精确独立层。
        .or_else(|| surface.layers.iter().position(matches)); // 没有当前匹配时复用第一层。
    let status = if let Some(existing) = existing {
        // 选择已有层不创建重复记录。
        editor.selected_surface_layer = existing; // 笔刷与作者行同步目标。
        format!(
            "已选中“{}”；点击新建层可创建独立层",
            surface.layers[existing].label
        ) // 明确独立创建入口。
    } else {
        // 新预设只设置目标，等待显式按钮。
        format!("新建目标：“{}”；请点击下方新建层", preset.label) // 不再要求首次落笔隐式创建。
    }; // 预设选择不写作者历史或像素。
    Ok(status)
}
