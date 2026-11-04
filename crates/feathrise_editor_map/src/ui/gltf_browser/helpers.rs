//! Scene0层级读取、glTF子资产标签解析和扁平行组合。

use std::collections::BTreeSet; // 网格去重与重复摆放ID诊断使用确定性集合。

use feathrise_map_content::{MapPlacementWaterKind, is_generated_gltf_node_name}; // UI与公共保存校验复用唯一loader生成名规则。

use super::{EditorGltfBrowserState, EditorGltfMeshSelection}; // 浏览器状态和稳定选择集中定义在模块入口。

/// 从Bevy glTF primitive子资产标签恢复父Mesh索引。
pub(crate) fn mesh_index_from_primitive_label(label: &str) -> Option<u32> {
    let remainder = label.strip_prefix("Mesh")?; // 正式标签必须以Mesh开头。
    let (mesh_index, primitive_index) = remainder.split_once("/Primitive")?; // Mesh标签本身不能冒充primitive。
    if mesh_index.is_empty()
        || primitive_index.is_empty()
        || !mesh_index.bytes().all(|byte| byte.is_ascii_digit())
        || !primitive_index.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None; // 含后缀、负号或空索引的自定义标签不是Bevy稳定格式。
    }
    mesh_index.parse().ok() // 超出u32的异常索引按不可引用处理。
}

/// 当前节点路径和网格元数据是否足够构成跨热更稳定引用。
pub(crate) fn stable_mesh_reference(
    node_path: &[String],    // 只包含Blender节点Name层级。
    mesh_name: &str,         // 直接来自GltfMeshName，缺失时为空。
    mesh_index: Option<u32>, // 直接来自Mesh3d句柄的primitive标签。
    path_names_valid: bool,  // 遍历阶段记录缺失Name，避免占位文字冒充真实名称。
) -> bool {
    path_names_valid
        && !node_path.is_empty()
        && node_path
            .iter()
            .all(|name| !name.is_empty() && !is_generated_gltf_node_name(name))
        && !mesh_name.trim().is_empty()
        && mesh_index.is_some() // 四项身份缺一不可，后续作者动作无需再猜测。
}

/// 返回路径最后一段，兼容资产统一使用的正斜杠。
pub(super) fn asset_file_name(asset_path: &str) -> &str {
    asset_path
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(asset_path) // 非法空尾路径仍保留完整文字供诊断。
}

/// 失效绑定行使用与属性面板一致的紧凑水体类别标签。
pub(super) fn water_kind_label(kind: MapPlacementWaterKind) -> &'static str {
    match kind {
        MapPlacementWaterKind::Horizontal => "水平水", // Blender水平面保留普通动态水样式。
        MapPlacementWaterKind::Sloped => "斜坡水",     // 连续降坡网格保留逐顶点高程。
        MapPlacementWaterKind::Waterfall => "瀑布",    // 陡峭水幕使用瀑布样式分支。
    }
}

/// 判断两个选择是否指向同一稳定引用，不比较随热更变化的valid位。
fn same_selection_identity(
    left: &EditorGltfMeshSelection,
    right: &EditorGltfMeshSelection,
) -> bool {
    left.placement_id == right.placement_id
        && left.node_path == right.node_path
        && left.mesh_name == right.mesh_name // mesh_index只是当前导出的诊断值，Blender重导出后变化也应继承选中态。
}

/// 在缓存或草稿改变后重新校验现有选择，不借助当前折叠可见行。
pub(super) fn revalidate_selection(
    state: &EditorGltfBrowserState, // 读取完整签名和唯一Scene0缓存。
    mut selection: EditorGltfMeshSelection, // 原选择暂时拥有，返回时写入新valid。
    duplicate_ids: &BTreeSet<String>, // 重复实例ID无法唯一定位。
) -> Option<EditorGltfMeshSelection> {
    let placement_index = state
        .placement_record_indices
        .get(&selection.placement_id)
        .copied()?; // 完整签名已同步建立唯一ID索引，热更采用帧不再扫描8192条记录。
    let placement = state.signature.get(placement_index)?;
    if placement.placement_id != selection.placement_id {
        return None; // 防御索引与签名未同批采用；绝不把选择串到另一条记录。
    }
    let cached = state.assets.get(&placement.asset_path); // 加载期允许保留身份但必须无效。
    let refreshed_mesh = cached
        .filter(|cached| cached.active_ready)
        .and_then(|cached| {
            cached.meshes.iter().find(|mesh| {
                mesh.node_path == selection.node_path && mesh.mesh_name == selection.mesh_name
            })
        }); // 最终身份只使用摆放ID、节点路径与网格名，不把易漂移索引当主键。
    if let Some(mesh) = refreshed_mesh {
        selection.mesh_index = mesh.mesh_index; // 热更后只刷新当前导出的诊断值；稳定身份匹配已经在上方完成。
        selection.valid = mesh.valid && !duplicate_ids.contains(&selection.placement_id);
        selection.orphaned_water_binding = false; // 当前Ready GLB重新命中后不再显示失效绑定诊断。
    } else if cached.is_some_and(|cached| cached.active_ready) {
        let binding_exists = placement.water_bindings.iter().any(|binding| {
            binding.node_path == selection.node_path && binding.mesh_name == selection.mesh_name
        }); // 只有作者数据仍持有相同身份时才需要保留可清除的失效选择。
        if !binding_exists {
            return None; // 普通网格从Ready GLB消失且没有作者绑定时清除旧选择，避免悬空操作。
        }
        selection.valid = false; // 失效绑定不得继续改类别或参数。
        selection.orphaned_water_binding = true; // 明确允许属性区只执行“取消水体”。
    } else {
        selection.valid = false; // 加载或失败期间保留原选择，等待下一完整Scene0重新确认。
        selection.orphaned_water_binding = false; // 资产尚未Ready不能提前宣判目标已经消失。
    }
    Some(selection) // 资源加载或失败期间保留诊断选择，但后续动作必须拒绝。
}

/// 公共点击系统用此函数匹配选中行而不重复散落四字段比较。
pub(super) fn selection_matches(
    left: Option<&EditorGltfMeshSelection>,
    right: Option<&EditorGltfMeshSelection>,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => same_selection_identity(left, right), // valid变化不应造成高亮闪烁。
        _ => false, // 根行和空选择永不高亮为网格。
    }
}

#[cfg(test)]
mod tests; // 纯helper与失效绑定回归拆出，保持生产组合逻辑低于模块行数上限。
