//! 植被作者图层结构与参数的原子变换。

use std::collections::BTreeMap; // 图层结构命令使用稳定ID到Arc画布的浅快照。
use std::sync::Arc; // 复制图层先共享只读像素，首次落笔时才写时复制整图。

use feathrise_map_content::{
    MapVegetationAuthoring, MapVegetationLayer, VegetationPresetRegistry,
}; // 公共格式与开发者预设目录共同负责稳定引用，UI不建立第二套内容枚举。

use crate::authoring::{EditorAuthoringSession, EditorCommand, EditorMapDraft, VegetationAuthoringChange}; // 每次结构操作以完整描述和Arc表的一条命令撤销。
use crate::resources::MapEditorState; // 修改后进入统一脏状态与后台编译修订。
use crate::types::EditorAction; // 静态BSN按钮到结构变换的稳定枚举。

use super::layer_creation::{
    add_blank_layer, add_blank_layer_with_pixels, blank_layer_pixel_count, duplicate_layer,
    make_group,
}; // 生产新层采用后台完整R8；纯变换测试仍可复用同步便捷入口。

/// 一次按钮操作完成后的选中层与用户可见说明。
pub(super) struct VegetationMutationOutcome {
    pub(super) selected: Option<usize>, // 删除最后一层时为None。
    pub(super) status: String,          // 只有真正采用或明确无操作后才返回。
}

/// 只认已经采用的当前网格画布；空作者列表合法，尚未读取的文件不能用默认值替代。
pub(crate) fn vegetation_canvas_unavailable(
    editor: &MapEditorState,
    draft: &EditorMapDraft,
) -> Option<&'static str> {
    if draft.terrain_documents.loading {
        return Some("正在载入网格作者画布；完成后可新建植被作者层。");
    }
    if draft.manifest.terrains.is_some() {
        let Some(active) = draft.terrain_documents.active.as_ref() else {
            return Some("请先在场景树选择一个网格，再新建植被作者层。");
        };
        if editor.terrain_canvas_target.as_ref() != Some(&active.definition.id) {
            return Some("请先在场景树选择一个网格，再新建植被作者层。");
        } // 切换选择而旧画布尚未停放时，也不能把新层写到旧网格。
    } else if draft.manifest.terrain.is_empty() {
        return Some("请先在场景树选择一个网格，再新建植被作者层。");
    }
    let Some(vegetation) = draft.vegetation.as_ref() else {
        return Some("当前网格植被作者数据尚未就绪；请查看底栏详情。");
    };
    if vegetation.authoring.is_none() {
        return Some("当前网格植被作者图层尚未载入；请查看底栏详情。");
    }
    None // 有效文件的layers为空时仍允许第一次后台建层，不依赖已有正式输出。
}

/// 判断动作是否必须由持有可变草稿的作者图层系统消费。
pub(super) fn is_vegetation_authoring_action(action: EditorAction) -> bool {
    matches!(
        action,
        EditorAction::VegetationAddLayer
            | EditorAction::VegetationDuplicateLayer
            | EditorAction::VegetationDeleteSelected
            | EditorAction::VegetationMoveUp
            | EditorAction::VegetationMoveDown
            | EditorAction::VegetationToggleEnabled
            | EditorAction::VegetationToggleLocked
            | EditorAction::VegetationNewGroup
            | EditorAction::VegetationNextGroup
            | EditorAction::VegetationCycleGroupMode
            | EditorAction::VegetationCycleMergeMode
    )
}

/// 在局部副本上完成一次结构变换，完整校验成功后才原子替换当前草稿。
pub(super) fn apply_vegetation_authoring_action(
    action: EditorAction,
    editor: &mut MapEditorState,
    session: &mut EditorAuthoringSession,
    preset_registry: Option<&VegetationPresetRegistry>,
) -> Result<VegetationMutationOutcome, String> {
    apply_vegetation_authoring_action_inner(action, editor, session, preset_registry, None)
}

/// 使用 single-flight 后台完成的空白 R8 执行新建层事务。
pub(super) fn apply_vegetation_authoring_action_with_blank(
    editor: &mut MapEditorState,
    session: &mut EditorAuthoringSession,
    preset_registry: Option<&VegetationPresetRegistry>,
    pixels: Vec<u8>,
) -> Result<VegetationMutationOutcome, String> {
    apply_vegetation_authoring_action_inner(
        EditorAction::VegetationAddLayer,
        editor,
        session,
        preset_registry,
        Some(pixels),
    )
}

fn apply_vegetation_authoring_action_inner(
    action: EditorAction,
    editor: &mut MapEditorState,
    session: &mut EditorAuthoringSession,
    preset_registry: Option<&VegetationPresetRegistry>,
    blank_pixels: Option<Vec<u8>>,
) -> Result<VegetationMutationOutcome, String> {
    if session.saving || session.vegetation_cross_busy {
        return Err("保存期间不能修改植被作者图层".to_owned());
    }
    let (folder, before_outputs, before, before_densities) = {
        let draft = session
            .draft
            .as_ref()
            .ok_or_else(|| "地图作者数据尚未就绪".to_owned())?;
        if let Some(message) = vegetation_canvas_unavailable(editor, draft) {
            return Err(message.to_owned());
        }
        let vegetation = draft
            .vegetation
            .as_ref()
            .ok_or_else(|| "当前地图没有正式植被层".to_owned())?;
        let authoring = vegetation
            .authoring
            .as_ref()
            .ok_or_else(|| "当前地图尚未建立植被作者图层".to_owned())?;
        (
            draft.terrain_folder().to_owned(),                      // 新层作者源继续留在本地图私有目录。
            vegetation.layers.clone(),                 // 首次使用新预设时与作者结构同次扩展。
            authoring.clone(),                         // 小型JSON描述完整复制便于失败不污染草稿。
            draft.vegetation_author_densities.clone(), // 只复制Arc句柄，不复制R8整图。
        )
    };
    let mut after_outputs = before_outputs.clone(); // 失败前只修改局部副本，正式输出不会半插入。
    let mut after = before.clone();
    let mut after_densities = before_densities.clone();
    let selected = (!after.layers.is_empty())
        .then(|| editor.selected_vegetation_layer.min(after.layers.len() - 1));
    let selected_output_id = if action == EditorAction::VegetationAddLayer {
        editor
            .vegetation_selected_preset_id
            .as_deref()
            .map(|preset_id| {
                ensure_preset_output(preset_id, &folder, &mut after_outputs, preset_registry)
            })
            .transpose()?
    } else {
        None // 非新建动作绝不借目录热更改写地图正式输出集合。
    };
    let mutation = if let Some(pixels) = blank_pixels {
        add_blank_layer_with_pixels(
            selected,
            selected_output_id.as_deref(), // 目录预设首次使用时已经在局部副本中取得正式输出身份。
            &folder,
            &after_outputs,
            &mut after,
            &mut after_densities,
            pixels,
        )?
    } else {
        mutate_authoring(
            action,
            selected,
            selected_output_id.as_deref(),
            &folder,
            &after_outputs,
            &mut after,
            &mut after_densities,
        )?
    };
    if !mutation.changed {
        return Ok(VegetationMutationOutcome {
            selected: mutation.selected,
            status: mutation.status,
        }); // 边界按钮不制造空撤销命令或无意义后台编译。
    }
    after.validate(&after_outputs)?; // 组引用、正式输出、路径和数值一次完整复核。
    validate_density_table(&after, &after_densities)?; // 描述与像素表必须同次完整存在。
    let draft = session
        .draft
        .as_mut()
        .ok_or_else(|| "地图作者数据在图层操作期间失效".to_owned())?;
    let vegetation = draft
        .vegetation
        .as_mut()
        .ok_or_else(|| "植被作者数据在图层操作期间失效".to_owned())?;
    vegetation.layers = after_outputs.clone(); // 正式输出与首张作者画布在同一主线程事务采用。
    vegetation.authoring = Some(after.clone()); // 再替换完整作者描述，任何观察者都只会见到一致结果。
    draft.vegetation_author_densities = after_densities.clone();
    session
        .undo
        .push(EditorCommand::VegetationAuthoringChanged(Box::new(
            VegetationAuthoringChange {
                before_outputs,   // 撤销首次使用预设时可移除尚未存在的正式输出。
                after_outputs,    // 重做恢复同一个稳定预设输出，不重新读取热更目录。
                before,           // 撤销恢复原组顺序、层顺序和全部参数。
                after,            // 重做恢复已经完整受检的新结构。
                before_densities, // Arc浅快照不会随任务数量复制像素。
                after_densities,
            },
        )));
    session.redo.clear(); // 新分支使旧重做历史失效。
    session.mark_vegetation_authoring_changed(editor); // single-flight后台重新编译全部正式输出。
    editor.selected_vegetation_layer = mutation.selected.unwrap_or(0);
    Ok(VegetationMutationOutcome {
        selected: mutation.selected,
        status: mutation.status,
    })
}

/// 纯解析当前新建层目标像素数；调用方据此在后台分配，不在输入帧制造整图工作。
pub(super) fn vegetation_blank_layer_pixel_count(
    editor: &MapEditorState,
    session: &EditorAuthoringSession,
    preset_registry: Option<&VegetationPresetRegistry>,
) -> Result<usize, String> {
    if session.saving {
        return Err("保存期间不能修改植被作者图层".to_owned());
    }
    let draft = session
        .draft
        .as_ref()
        .ok_or_else(|| "地图作者数据尚未就绪".to_owned())?;
    if let Some(message) = vegetation_canvas_unavailable(editor, draft) {
        return Err(message.to_owned());
    }
    let vegetation = draft
        .vegetation
        .as_ref()
        .ok_or_else(|| "当前地图没有正式植被层".to_owned())?;
    let authoring = vegetation
        .authoring
        .as_ref()
        .ok_or_else(|| "当前地图尚未建立植被作者图层".to_owned())?;
    let mut outputs = vegetation.layers.clone(); // 首次预设输出只在局部副本中解析，不提前污染草稿。
    let selected = (!authoring.layers.is_empty()).then(|| {
        editor
            .selected_vegetation_layer
            .min(authoring.layers.len() - 1)
    });
    let selected_output_id = editor
        .vegetation_selected_preset_id
        .as_deref()
        .map(|preset_id| {
            ensure_preset_output(preset_id, &draft.terrain_folder(), &mut outputs, preset_registry)
        })
        .transpose()?;
    blank_layer_pixel_count(selected, selected_output_id.as_deref(), &outputs, authoring)
}

/// 返回当前预设在地图中的正式输出；首次使用时从目录纯值描述延迟创建。
fn ensure_preset_output(
    preset_id: &str,
    folder: &str,
    outputs: &mut Vec<MapVegetationLayer>,
    preset_registry: Option<&VegetationPresetRegistry>,
) -> Result<String, String> {
    let exact_existing = outputs
        .iter()
        .find(|output| output.runtime_profile.as_deref() == Some(preset_id)); // 目录尚未就绪时，既有规范 ID 仍可继续增建作者区域。
    let Some(registry) = preset_registry else {
        return exact_existing
            .map(|output| output.id.clone())
            .ok_or_else(|| {
                format!("开发者植被预设目录尚未加载，无法创建预设{preset_id}的首次输出")
            });
    };
    let preset = registry
        .resolve(preset_id)
        .ok_or_else(|| format!("开发者植被预设{preset_id}不存在或最近热更无效"))?;
    if let Some(existing) = outputs.iter().find(|output| {
        output.runtime_profile.as_deref().is_some_and(|profile| {
            registry
                .resolve(profile)
                .is_some_and(|resolved| resolved.id == preset.id)
        })
    }) {
        return Ok(existing.id.clone()); // 历史别名输出继续复用，地图不产生同一内容的第二张正式图。
    }
    let candidate = preset.create_map_layer(folder); // 只从已完整校验的原子目录快照派生地图纯值。
    for output in outputs.iter() {
        if output.id == candidate.id {
            return Err(format!(
                "正式输出ID {} 已被预设 {:?} 占用，无法加入预设 {}",
                candidate.id, output.runtime_profile, preset.id
            ));
        }
        if output.runtime_layer_id == candidate.runtime_layer_id {
            return Err(format!(
                "运行时植被层ID {:?} 已被输出 {} 占用",
                candidate.runtime_layer_id, output.id
            ));
        }
        if candidate.prototype_id.is_some() && output.prototype_id == candidate.prototype_id {
            return Err(format!(
                "Mesh原型ID {:?} 已被输出 {} 占用",
                candidate.prototype_id, output.id
            ));
        }
        if output.density_source_path == candidate.density_source_path
            || output.density_texture_path == candidate.density_texture_path
        {
            return Err(format!(
                "预设 {} 的密度路径与现有输出 {} 冲突",
                preset.id, output.id
            ));
        }
    }
    let output_id = candidate.id.clone();
    outputs.push(candidate); // Vec 数量只受实际内存与 usize 表达范围约束，没有产品常量上限。
    Ok(output_id)
}

pub(super) struct LocalMutation {
    selected: Option<usize>,
    status: String,
    changed: bool,
}

#[allow(clippy::too_many_arguments)] // 每个入参都是一次纯结构事务的显式边界，不读取ECS或文件系统。
fn mutate_authoring(
    action: EditorAction,
    selected: Option<usize>,
    new_layer_output_id: Option<&str>,
    folder: &str,
    outputs: &[MapVegetationLayer],
    authoring: &mut MapVegetationAuthoring,
    densities: &mut BTreeMap<String, Arc<Vec<u8>>>,
) -> Result<LocalMutation, String> {
    match action {
        EditorAction::VegetationAddLayer => add_blank_layer(
            selected,
            new_layer_output_id,
            folder,
            outputs,
            authoring,
            densities,
        ),
        EditorAction::VegetationDuplicateLayer => {
            duplicate_layer(require_selected(selected)?, folder, authoring, densities)
        }
        EditorAction::VegetationDeleteSelected => {
            delete_layer(require_selected(selected)?, authoring, densities)
        }
        EditorAction::VegetationMoveUp => move_layer(require_selected(selected)?, -1, authoring),
        EditorAction::VegetationMoveDown => move_layer(require_selected(selected)?, 1, authoring),
        EditorAction::VegetationToggleEnabled => {
            toggle_layer_enabled(require_selected(selected)?, authoring)
        }
        EditorAction::VegetationToggleLocked => {
            toggle_layer_locked(require_selected(selected)?, authoring)
        }
        EditorAction::VegetationNewGroup => new_group(selected, authoring),
        EditorAction::VegetationNextGroup => {
            move_to_next_group(require_selected(selected)?, authoring)
        }
        EditorAction::VegetationCycleGroupMode => {
            change_group(require_selected(selected)?, authoring, GroupChange::Mode)
        }
        EditorAction::VegetationCycleMergeMode => {
            change_group(require_selected(selected)?, authoring, GroupChange::Merge)
        }
        _ => Err("当前按钮不是植被作者图层操作".to_owned()),
    }
}

fn delete_layer(
    index: usize,
    authoring: &mut MapVegetationAuthoring,
    densities: &mut BTreeMap<String, Arc<Vec<u8>>>,
) -> Result<LocalMutation, String> {
    require_unlocked(authoring, index)?;
    let removed = authoring.layers.remove(index);
    densities.remove(&removed.id); // 文件系统旧源不在交互帧删除，撤销仍可从命令Arc恢复。
    let selected = (!authoring.layers.is_empty()).then(|| index.min(authoring.layers.len() - 1));
    Ok(changed(
        selected,
        format!(
            "已删除作者图层“{}”；旧私有源不会在交互帧破坏性删除",
            removed.label
        ),
    ))
}

fn move_layer(
    index: usize,
    direction: isize,
    authoring: &mut MapVegetationAuthoring,
) -> Result<LocalMutation, String> {
    require_unlocked(authoring, index)?;
    let target = index as isize + direction;
    if target < 0 || target >= authoring.layers.len() as isize {
        return Ok(unchanged(Some(index), "作者图层已经位于该方向边界"));
    }
    let target = target as usize;
    authoring.layers.swap(index, target);
    Ok(changed(Some(target), "已调整作者图层顺序"))
}

fn toggle_layer_enabled(
    index: usize,
    authoring: &mut MapVegetationAuthoring,
) -> Result<LocalMutation, String> {
    let layer = authoring
        .layers
        .get_mut(index)
        .ok_or_else(|| "选择的作者图层不存在".to_owned())?;
    layer.enabled = !layer.enabled; // 显隐允许在锁定状态切换，像素与规则仍受锁保护。
    Ok(changed(
        Some(index),
        format!(
            "作者图层“{}”已{}",
            layer.label,
            if layer.enabled { "显示" } else { "隐藏" }
        ),
    ))
}

fn toggle_layer_locked(
    index: usize,
    authoring: &mut MapVegetationAuthoring,
) -> Result<LocalMutation, String> {
    let layer = authoring
        .layers
        .get_mut(index)
        .ok_or_else(|| "选择的作者图层不存在".to_owned())?;
    layer.locked = !layer.locked;
    Ok(changed(
        Some(index),
        format!(
            "作者图层“{}”已{}",
            layer.label,
            if layer.locked { "锁定" } else { "解锁" }
        ),
    ))
}

fn new_group(
    selected: Option<usize>,
    authoring: &mut MapVegetationAuthoring,
) -> Result<LocalMutation, String> {
    if let Some(index) = selected {
        require_unlocked(authoring, index)?;
    }
    let group = make_group(authoring.groups.len());
    let label = group.label.clone();
    let id = group.id.clone();
    authoring.groups.push(group);
    if let Some(index) = selected {
        authoring.layers[index].group_id = id; // 新组直接接管当前层，避免创建不可见的空组。
    }
    Ok(changed(
        selected,
        format!("已新建作者组“{label}”并接入当前图层"),
    ))
}

fn move_to_next_group(
    index: usize,
    authoring: &mut MapVegetationAuthoring,
) -> Result<LocalMutation, String> {
    require_unlocked(authoring, index)?;
    if authoring.groups.len() <= 1 {
        return Ok(unchanged(Some(index), "当前只有一个作者组；请先新建分组"));
    }
    let current_id = authoring.layers[index].group_id.clone();
    let current = authoring
        .groups
        .iter()
        .position(|group| group.id == current_id)
        .ok_or_else(|| "当前作者图层引用的组不存在".to_owned())?;
    let next = (current + 1) % authoring.groups.len();
    authoring.layers[index].group_id = authoring.groups[next].id.clone();
    Ok(changed(
        Some(index),
        format!("当前作者图层已移到组“{}”", authoring.groups[next].label),
    ))
}

enum GroupChange {
    Mode,
    Merge,
}

fn change_group(
    index: usize,
    authoring: &mut MapVegetationAuthoring,
    change: GroupChange,
) -> Result<LocalMutation, String> {
    require_unlocked(authoring, index)?;
    let group_id = authoring.layers[index].group_id.clone();
    let group = authoring
        .groups
        .iter_mut()
        .find(|group| group.id == group_id)
        .ok_or_else(|| "当前作者图层引用的组不存在".to_owned())?;
    match change {
        GroupChange::Mode => group.mode = group.mode.next(),
        GroupChange::Merge => group.output_merge = group.output_merge.next(),
    }
    Ok(changed(
        Some(index),
        format!(
            "组“{}”：{}，跨组{}，容量{:.0}%，覆盖阈值{:.0}%",
            group.label,
            group.mode.label(),
            group.output_merge.label(),
            group.capacity * 100.0,
            group.claim_threshold * 100.0
        ),
    ))
}

fn require_selected(selected: Option<usize>) -> Result<usize, String> {
    selected.ok_or_else(|| "当前没有可操作的植被作者图层".to_owned())
}

fn require_unlocked(authoring: &MapVegetationAuthoring, index: usize) -> Result<(), String> {
    let layer = authoring
        .layers
        .get(index)
        .ok_or_else(|| "选择的作者图层不存在".to_owned())?;
    if layer.locked {
        return Err(format!("作者图层“{}”已锁定，请先解锁", layer.label));
    }
    Ok(())
}

fn pixel_count(resolution: [u32; 2]) -> Result<usize, String> {
    (resolution[0] as usize)
        .checked_mul(resolution[1] as usize)
        .ok_or_else(|| "植被作者层分辨率乘法溢出".to_owned())
}

pub(super) fn validate_density_table(
    authoring: &MapVegetationAuthoring,
    densities: &BTreeMap<String, Arc<Vec<u8>>>,
) -> Result<(), String> {
    for layer in &authoring.layers {
        let expected = pixel_count(layer.resolution)?;
        let actual = densities
            .get(&layer.id)
            .ok_or_else(|| format!("作者图层{}缺少R8画布", layer.id))?
            .len();
        if actual != expected {
            return Err(format!(
                "作者图层{}为{actual}字节，期望{expected}字节",
                layer.id
            ));
        }
    }
    Ok(())
}

pub(super) fn changed(selected: Option<usize>, status: impl Into<String>) -> LocalMutation {
    LocalMutation {
        selected,
        status: status.into(),
        changed: true,
    }
}

fn unchanged(selected: Option<usize>, status: impl Into<String>) -> LocalMutation {
    LocalMutation {
        selected,
        status: status.into(),
        changed: false,
    }
}

#[cfg(test)]
#[path = "operations_tests.rs"]
mod tests;
