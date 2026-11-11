//! 地表作者层选择、分页和稳定身份校验的纯逻辑。

use feathrise_scene_content::{SceneSurfaceAsset, SceneSurfaceLayer, SurfacePresetRegistry}; // 作者层名称与全局预设名称保持各自身份。

use crate::authoring::EditorAuthoringSession; // 只读访问当前地表修订和草稿。
use crate::resources::SceneEditorState; // 选择仍使用既有地表笔刷状态。

use super::constants::ROW_COUNT; // 所有分页入口使用同一步长。
use super::types::SurfaceLayerTarget; // 鼠标只可消费当前显示的真实层身份。

/// 作者名称优先保留；不同名称的同预设图层必须能在删除前明确区分。
pub(crate) fn layer_display_label(
    layer: &SceneSurfaceLayer,
    registry: Option<&SurfacePresetRegistry>,
) -> String {
    match registry.and_then(|registry| registry.resolve(&layer.preset_id)) {
        Some(preset) if preset.label != layer.label => {
            format!("{} → {}", layer.label, preset.label)
        } // 全局改名后补充当前预设名，不覆盖作者层名。
        _ => layer.label.clone(), // 名称相同或目录未就绪时保持紧凑作者名称。
    } // 仅格式化当前最多六行和一条状态，不扫描完整层表。
} // 不修改任何持久化名称。

/// 尚未被本场景引用的预设只表示下一次绘制目标，不能把旧索引误当作删除选择。
pub(crate) fn selected_index(
    // 地表状态卡与列表共用同一高亮选择规则。
    surface: &SceneSurfaceAsset,                // 当前场景的完整引用层表。
    editor: &SceneEditorState,                  // 既有笔刷索引与全局预设选择。
    registry: Option<&SurfacePresetRegistry>, // 允许通过正式别名匹配同一预设。
) -> Option<usize> {
    let index = editor.selected_surface_layer; // 不扫描全部层，只检查当前候选。
    let layer = surface.layers.get(index)?; // 过期索引不能映射成有效删除项。
    let Some(selected) = editor.surface_selected_preset_id.as_deref() else {
        return Some(index); // 刚载入场景时沿用真实初始地表层。
    }; // 已显式选择预设时继续校验引用。
    let matches = selected == layer.preset_id
        || registry.is_some_and(|registry| {
            registry
                .resolve(selected)
                .zip(registry.resolve(&layer.preset_id))
                .is_some_and(|(selected, actual)| selected.id == actual.id) // 历史别名与规范ID属于同一层预设。
        }); // 目录未加载时仍接受完全相同的稳定ID。
    matches.then_some(index) // 不匹配时列表没有高亮层，必须先明确点选。
} // 不制造任何场景引用。

/// 固定行点击必须与当前场景、地表修订、数组位置和稳定ID同时匹配。
pub(super) fn target_is_current(
    target: &SurfaceLayerTarget,
    session: &EditorAuthoringSession,
) -> bool {
    session.surface_revision == target.revision // 一次删除或笔划使之前显示的绑定过期。
        && session.draft.as_ref().is_some_and(|draft| {
            draft.scene_id == target.scene_id // 即使两个场景层名相同也不可串选。
                && draft.surface.as_ref().and_then(|surface| surface.layers.get(target.index))
                    .is_some_and(|layer| layer.id == target.layer_id) // 复核该下标仍属于显示的层。
        }) // 无草稿时自然拒绝旧行。
} // 调用方只需提示等待下一次列表刷新。

/// 翻页同时明确选择目标页首层，边界操作不会越界。
pub(super) fn page_target(total: usize, selected: usize, previous: bool) -> Option<usize> {
    let last = total.checked_sub(1)? / ROW_COUNT; // 零层没有可选页。
    let page = (selected / ROW_COUNT).min(last); // 删除后的旧选择安全钳制。
    let next = if previous {
        page.saturating_sub(1)
    } else {
        page.saturating_add(1).min(last)
    }; // 前后页均停在合法边界。
    Some(next * ROW_COUNT) // 合法页号的首层一定在层表内。
} // 不引入产品层数上限。

/// 选择场景层后同步笔刷目标，下一次落笔继续编辑该层。
pub(super) fn select_layer(
    index: usize,
    editor: &mut SceneEditorState,
    session: &EditorAuthoringSession,
) {
    let Some(layer) = session
        .draft
        .as_ref()
        .and_then(|draft| draft.surface.as_ref())
        .and_then(|surface| surface.layers.get(index))
    else {
        return;
    }; // 防御异步切换场景造成的选择失效。
    editor.selected_surface_layer = index; // 复用现有笔刷索引，页码从此值派生。
    editor.surface_selected_preset_id = Some(layer.preset_id.clone()); // 管理器与作者列表使用同一稳定预设。
    editor.tool = crate::types::EditorTool::Surface; // 点选后可直接继续绘制。
    editor.status = format!("已选择地表作者层“{}”；可继续绘制或删除所选", layer.label); // 提示只针对当前场景作者层。
} // 选择本身不改草稿或撤销历史。
