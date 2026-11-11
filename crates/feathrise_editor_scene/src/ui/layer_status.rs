//! 地形与植被页面各自独立的作者状态回显。

use bevy::prelude::*; // 资源变更检测与标记文本查询。
use feathrise_scene_content::{SurfacePresetRegistry, VegetationPresetRegistry}; // 未引用预设也需显示真实绘制目标，不能冒充旧作者层。

use crate::authoring::{EditorAuthoringSession, selected_vegetation_layer}; // 作者层必须映射到组和固定正式输出。
use crate::components::{SceneEditorSurfaceText, SceneEditorVegetationStatusText}; // 两个页面状态文本从当前作者草稿回显。
use crate::resources::SceneEditorState; // 当前地表与植被作者选择及覆盖预览模式。

/// 只在选择或草稿变化时重建短文本，不让主编辑器壳继续承担作者规则格式化职责。
pub(crate) fn sync_editor_layer_status(
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    preset_registry: Option<Res<VegetationPresetRegistry>>,
    surface_registry: Option<Res<SurfacePresetRegistry>>, // 全局地表改名或新预设选择后同步本页状态。
    mut texts: Query<(
        &mut Text,
        Option<&SceneEditorSurfaceText>,
        Option<&SceneEditorVegetationStatusText>,
    )>,
) {
    if !(editor.is_changed()
        || session.is_changed()
        || preset_registry
            .as_ref()
            .is_some_and(|registry| registry.is_changed())
        || surface_registry
            .as_ref()
            .is_some_and(|registry| registry.is_changed()))
    // 地表目录修订独立于植被目录。
    {
        return;
    }
    let surface_label = session
        .draft
        .as_ref()
        .and_then(|draft| draft.surface.as_ref())
        .and_then(|surface| {
            super::surface_layers::selected_index(surface, &editor, surface_registry.as_deref())
                .and_then(|index| surface.layers.get(index))
        }) // 与实际高亮和删除入口使用同一有效选择。
        .map(|layer| {
            super::surface_layers::layer_display_label(layer, surface_registry.as_deref())
        }); // 与列表保持同一个作者层名称和预设说明。
    let vegetation_layer = session
        .draft
        .as_ref()
        .and_then(|draft| selected_vegetation_layer(draft, editor.selected_vegetation_layer)); // UI索引指作者画布，不误取正式层数组同下标。
    let vegetation_label = vegetation_layer
        .as_ref()
        .map_or("无", |selected| selected.author.label.as_str());
    // 新建目标统一由上方预设管理摘要显示，所选层卡不再重复解析目录。
    let vegetation_group = vegetation_layer.as_ref().map_or_else(
        || "无组".to_owned(),
        |selected| {
            format!(
                "{}\n组内：{} · 跨组：{}\n状态：{}{}", // 一行一个阅读主题，参数由下方数值框统一回显。
                selected.group.label,
                selected.group.mode.label(),
                selected.group.output_merge.label(),
                if selected.author.enabled {
                    "显示"
                } else {
                    "隐藏"
                },
                if selected.author.locked {
                    " · 已锁定" // 明确当前不可绘制的状态。
                } else {
                    " · 可编辑" // 不要求作者从缺失图标推断锁定状态。
                },
            )
        },
    ); // 明确写出当前画布如何进入最终运行时结果。
    let vegetation_value = format!(
        "所选层：{vegetation_label}\n分组：{vegetation_group}\n预览：{}", // 去除profile和重复新建目标，属性卡只说明当前层。
        editor.vegetation_preview_mode.label(),
    );
    let surface_value = if let Some(label) = surface_label {
        format!("已选地表层：{label}\n删除清除该层全部涂绘；可删至空地表") // 最后一层也可删除，空画布仍能继续绘制。
    } else if let Some(id) = editor.surface_selected_preset_id.as_deref() {
        let label = surface_registry
            .as_deref()
            .and_then(|registry| registry.resolve(id))
            .map_or(id, |preset| preset.label.as_str()); // 未使用目录项同样显示可读名称。
        format!("新建目标：{label}\n点击“新建层”创建地表作者层") // 与植被保持显式建层操作。
    } else {
        "地表：当前场景没有可用作者层".to_owned()
    }; // 无场景时不显示伪造的输出。
    for (mut text, surface_text, vegetation_text) in &mut texts {
        if surface_text.is_some() {
            **text = surface_value.clone(); // 地形页不再混入植被或水域状态。
        } else if vegetation_text.is_some() {
            **text = vegetation_value.clone(); // 植被页只回显作者层、组和编译目标。
        }
    }
}
