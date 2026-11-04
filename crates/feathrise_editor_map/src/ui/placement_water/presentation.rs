//! 整网格水属性文字与类别按钮的只读Presentation回显。

use bevy::prelude::*; // 文本、交互颜色、资源票据与查询系统参数。
use feathrise_map_content::MapPlacementWaterKind; // 类别按钮按正式水类型保持选中态。
use feathrise_scene::MapPlacementRuntimeStatus; // 活动批次切换票据唤醒当前选择回显。
use crate::ui::theme as colors; // 延续F12水墨、铜边与文字颜色。

use super::presentation_gate::PlacementWaterTextRefreshState; // 稳定帧阻断无语义Resource Changed。
use super::{
    MapEditorPlacementWaterActionButton, MapEditorPlacementWaterText, PlacementWaterAction,
    PlacementWaterRuntimeSelectionCache, PlacementWaterSelectionCache, kind_label,
}; // 复用父模块既有标记、派生缓存和类别显示口径。
use crate::resources::MapEditorState; // 关闭编辑器时不刷新隐藏属性树。
use crate::ui::gltf_browser::EditorGltfBrowserState; // 当前场景树稳定选择身份。

/// 选择、草稿或撤销变化时增量刷新紧凑属性文字。
pub(crate) fn sync_placement_water_text(
    editor: Res<MapEditorState>,
    browser: Res<EditorGltfBrowserState>,
    selection_cache: Res<PlacementWaterSelectionCache>, // 变化帧预先定位，本文字系统不再扫描最多8192条摆放记录。
    runtime_cache: Res<PlacementWaterRuntimeSelectionCache>, // 当前选择状态已由生命周期索引定向聚合。
    placement_runtime: Res<MapPlacementRuntimeStatus>, // 活动批次O(1)切换时组件本身不产生Changed，由公开批次状态唤醒回显。
    mut refresh_state: Local<PlacementWaterTextRefreshState>, // 浏览进度、底栏状态和候选detail变化不能唤醒整根汇总。
    mut texts: Query<&mut Text, With<MapEditorPlacementWaterText>>,
) {
    let presentation_identity_changed = refresh_state.observe(
        editor.open,
        browser.selection.as_ref(),
        &placement_runtime,
        runtime_cache.presentation_revision(),
    ); // O(1)正式批次、选择与定向状态票据比较；只在真实语义边沿克隆小身份。
    if !editor.open || !(presentation_identity_changed || selection_cache.is_changed()) {
        return;
    } // 稳定帧不触及全部根；活动根状态由生命周期索引定向唤醒。
    let value = match browser.selection.as_ref() {
        None => "请选择场景树中的Blender网格\n资产浏览器只负责选择源文件和摆放".to_owned(),
        Some(selection) if selection.orphaned_water_binding => format!(
            "{} / {}\n失效水绑定：当前GLB已删除或改名该目标\n可点击“取消水体”清除作者数据",
            selection.node_path.join(" / "),
            selection.mesh_name,
        ),
        Some(selection) if !selection.valid => format!(
            "{} / {}\n名称不稳定：请在Blender中显式命名对象与网格后重新导出",
            selection.node_path.join(" / "),
            selection.mesh_name,
        ),
        Some(selection) => {
            let surface = selection_cache.surface(); // 同一Presentation帧与按钮、NumericInput共用权威派生值。
            if let Some(surface) = surface {
                format!(
                    "{} / {}\n{} · 流速 {:.2} · 高度 {:+.2}m\n{}",
                    selection.node_path.join(" / "),
                    selection.mesh_name,
                    kind_label(surface.kind),
                    surface.flow_speed,
                    surface.height_offset,
                    runtime_cache.label(),
                )
            } else {
                format!(
                    "{} / {}\n普通Blender网格（尚未赋水）\n{}",
                    selection.node_path.join(" / "),
                    selection.mesh_name,
                    runtime_cache.label(),
                )
            }
        }
    };
    for mut text in &mut texts {
        **text = value.clone(); // 静态BSN通常只有一个目标；测试重复节点仍保持一致。
    }
}

/// 属性按钮持续回显当前类别，并保留普通悬停/按下反馈。
pub(crate) fn sync_placement_water_button_styles(
    selection_cache: Res<PlacementWaterSelectionCache>, // 稳定帧O(1)读取，结构/作者变化只由刷新系统定位一次。
    mut buttons: Query<(
        &Interaction,
        &MapEditorPlacementWaterActionButton,
        &mut BackgroundColor,
    )>,
) {
    let selected_kind = selection_cache.surface().map(|surface| surface.kind);
    for (interaction, button, mut background) in &mut buttons {
        let active = matches!(
            (button.0, selected_kind),
            (
                PlacementWaterAction::SetHorizontal,
                Some(MapPlacementWaterKind::Horizontal)
            ) | (
                PlacementWaterAction::SetSloped,
                Some(MapPlacementWaterKind::Sloped)
            ) | (
                PlacementWaterAction::SetWaterfall,
                Some(MapPlacementWaterKind::Waterfall)
            )
        );
        background.0 = if active {
            colors::SELECT_BG
        } else {
            match interaction {
                Interaction::Pressed => colors::BUTTON_DEV_SMALL_PRESSED,
                Interaction::Hovered => colors::BUTTON_DEV_SMALL_HOVERED,
                Interaction::None => colors::BUTTON_DEV_SMALL_NORMAL,
            }
        };
    }
}
