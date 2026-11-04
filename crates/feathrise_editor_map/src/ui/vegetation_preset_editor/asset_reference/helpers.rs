//! 资产引用字段分类、冻结目标和单字段原子赋值；不进行IO或完整目录扫描。

use super::super::fields::{commit_field, field_value}; // 复用现有强类型字段读写，不维护旁路值。
use super::super::types::{
    BladeLodField, MeshPartField, PresetDraft, PresetField, PresetWorkspaceTab, ReferenceKind,
    ReferenceTarget, SubAssetChoice, VegetationPresetEditorState,
};
use feathrise_map_content::VegetationPresetRuntime; // 直接校验真实LOD和部件身份。 // 每个请求只指向一个明确字段。

/// 普通文本与数值字段没有资产动作；四类引用统一由这里声明。
pub(in super::super) fn reference_kind(field: PresetField) -> Option<ReferenceKind> {
    match field {
        PresetField::BladeBaseColorPath => Some(ReferenceKind::Image), // 可选二维图集。
        PresetField::BladeLod {
            field: BladeLodField::MeshPath,
            ..
        } => Some(ReferenceKind::BladeMesh), // 单个Blade网格。
        PresetField::MeshPart {
            field: MeshPartField::MeshPath,
            ..
        } => Some(ReferenceKind::Mesh), // 仅当前部件网格。
        PresetField::MeshPart {
            field: MeshPartField::MaterialPath,
            ..
        } => Some(ReferenceKind::Material), // 仅当前部件材质。
        _ => None, // 名称、距离、尺寸等继续使用普通字段。
    }
}

/// 从真实详细页拥有者建立目标，而不是记住会漂移的最后焦点。
pub(super) fn freeze_target(
    state: &VegetationPresetEditorState, // 只读当前工作区。
    tab_id: u64,                         // 点击按钮所属详细页的稳定会话身份。
    field: PresetField,                  // 点击行明确声明的字段。
) -> Result<ReferenceTarget, String> {
    if state.active_tab != PresetWorkspaceTab::Vegetation(tab_id) {
        return Err("资产引用所在标签已切换，请在当前字段重新操作".to_owned()); // 旧UI动作失败关闭。
    }
    reference_kind(field).ok_or_else(|| "这个字段不是资产引用".to_owned())?; // 不允许用资产字符串写入普通参数。
    let (tab_index, tab) = state
        .tabs
        .iter()
        .enumerate()
        .find(|(_, tab)| tab.id == tab_id)
        .ok_or_else(|| "目标预设标签已关闭".to_owned())?; // 只在显式点击时定位标签一次，后续轮询按索引和ID双重检查。
    let draft = &tab.draft; // 不借用其他草稿。
    let (lod_level, part_id) = target_ids(draft, field)?; // 冻结下标对应的真实稳定身份。
    let field_name = match field {
        PresetField::BladeBaseColorPath => "基础色纹理".to_owned(), // 与模型路径分开。
        PresetField::BladeLod { .. } => format!("LOD {} · 网格", lod_level.unwrap()), // 清楚显示这一档。
        PresetField::MeshPart { field, .. } => format!(
            "LOD {} · 部件 {} · {}",
            lod_level.unwrap(),
            part_id.unwrap(),
            if field == MeshPartField::MaterialPath {
                "材质"
            } else {
                "网格"
            }
        ), // 单个部件而非整层。
        _ => return Err("这个字段不是资产引用".to_owned()), // 穷尽类型保护。
    };
    Ok(ReferenceTarget {
        tab_index,              // 当前标签位置只作快速访问，稳定ID仍是授权依据。
        tab_id,                 // 标签拥有者身份。
        field,                  // 精确字段路径。
        section: draft.section, // 离开该分区即取消。
        lod_level,              // 数据重排后不能静默命中同一下标。
        part_id,                // 部件更换后不能静默覆盖。
        original_path: field_value(&draft.descriptor, field)?, // 原值用于最后一步乐观检查。
        description: format!("{} · {field_name}", draft.descriptor.label), // 始终向用户回显真正目标。
    })
}

/// 读取当前LOD和部件稳定ID；下标及当前导航必须同时匹配。
fn target_ids(
    draft: &PresetDraft,
    field: PresetField,
) -> Result<(Option<u32>, Option<u32>), String> {
    match (field, &draft.descriptor.runtime) {
        (PresetField::BladeLod { lod, .. }, VegetationPresetRuntime::Blade { lods, .. })
            if draft.selected_lod == lod =>
        {
            let lod = lods.get(lod).ok_or_else(|| "目标LOD已不存在".to_owned())?; // 不能向钳制后的邻档写入。
            Ok((Some(lod.level), None)) // Blade没有部件层。
        }
        (PresetField::MeshPart { lod, part, .. }, VegetationPresetRuntime::Mesh { lods, .. })
            if draft.selected_lod == lod && draft.selected_part == part =>
        {
            let lod = lods.get(lod).ok_or_else(|| "目标LOD已不存在".to_owned())?; // 当前真实距离档。
            let part = lod
                .parts
                .get(part)
                .ok_or_else(|| "目标部件已不存在".to_owned())?; // 当前真实部件。
            Ok((Some(u32::from(lod.level)), Some(part.part_id))) // 两级稳定身份一并校验。
        }
        (PresetField::BladeBaseColorPath, _) => Ok((None, None)), // 只有基础色纹理属于不绑定LOD的全预设引用，备注不是资产。
        _ => Err("目标LOD、部件或后端已切换，请重新选择资产".to_owned()), // 导航变化取消，不把旧结果转交新目标。
    }
}

/// 真正写入前复验会话、导航、稳定ID和原值；不允许迟到覆盖。
pub(super) fn target_is_current(
    state: &VegetationPresetEditorState,
    target: &ReferenceTarget,
) -> bool {
    target_navigation_is_current(state, target)
        && field_value(&state.tabs[target.tab_index].draft.descriptor, target.field)
            .is_ok_and(|path| path == target.original_path) // 导航身份先验证，成功赋值前再拒绝覆盖后来值。
}

/// 完成选择后的UI回位只检查导航，不把已经成功更新的路径误认为换了页面。
pub(super) fn target_navigation_is_current(
    state: &VegetationPresetEditorState,
    target: &ReferenceTarget,
) -> bool {
    if state.active_tab != PresetWorkspaceTab::Vegetation(target.tab_id) {
        return false;
    } // 不再构造每帧目标说明字符串。
    let Some(tab) = state.tabs.get(target.tab_index) else {
        return false;
    }; // 标签关闭后拒绝迟到结果。
    tab.id == target.tab_id // 相同下标不代表同一个标签。
        && tab.draft.section == target.section // 选择面板属于原分区。
        && target_ids(&tab.draft, target.field).is_ok_and(|ids| ids == (target.lod_level, target.part_id)) // 原LOD与部件身份必须仍在原位置。
}

/// 最终采用只修改这一字段，其余LOD距离、半径、变换、材质和作者源均不连带变化。
pub(super) fn apply_value(
    state: &mut VegetationPresetEditorState,
    target: &ReferenceTarget,
    path: &str,
) -> Result<(), String> {
    if !target_is_current(state, target) {
        return Err("目标字段已变化，本次资产选择已取消，原值保留".to_owned()); // 任何失败均在写入前结束。
    }
    let draft = &mut state.tabs[target.tab_index].draft; // 上方已完整验证索引和稳定ID，采用不再扫描标签集合。
    commit_field(&mut draft.descriptor, target.field, path)?; // 唯一强类型落值，不复制整份描述。
    state.status = format!(
        "已设置 {}：{path}；仅修改当前引用，保存预设后生效",
        target.description
    ); // 不谎称已写入磁盘。
    state.touch_ui(); // 只在实际结果采用时重建有限活动表单。
    Ok(())
}

/// 从真实枚举得到的数字身份生成Bevy子资产路径，绝不默认Mesh0。
pub(super) fn choice_path(source: &str, choice: SubAssetChoice) -> String {
    match choice {
        SubAssetChoice::Mesh { mesh, primitive } => {
            format!("{source}#Mesh{mesh}/Primitive{primitive}")
        } // 原始Mesh叶子。
        SubAssetChoice::Material(index) => format!("{source}#Material{index}/std"), // Bevy0.19派生StandardMaterial。
        SubAssetChoice::DefaultMaterial => format!("{source}#DefaultMaterial/std"), // 无显式材质primitive的真实默认材质。
    }
}
