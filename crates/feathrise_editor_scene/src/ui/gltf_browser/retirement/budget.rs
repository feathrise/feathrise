//! GLB浏览器旧代清理共享的元素、路径段和字符串三维帧预算。

use super::super::{CachedGltfMesh, EditorGltfBrowserEntry, EditorGltfBrowserEntryKind}; // 行和网格使用相同深路径成本模型。

/// 每帧最多释放的逻辑元素数；容器壳在清空后才O(1)析构。
const MAX_RETIREMENT_ELEMENTS_PER_FRAME: usize = 128;
/// 每帧最多释放的拥有型节点路径段近似数。
const MAX_RETIREMENT_PATH_SEGMENTS_PER_FRAME: usize = 512;
/// 每帧最多释放的字符串字节近似数；字符串析构本身O(1)，该预算约束分配器峰值。
const MAX_RETIREMENT_STRING_BYTES_PER_FRAME: usize = 32 * 1024;

/// 一帧共享的三维退役预算。
pub(in crate::ui::gltf_browser) struct BrowserRetirementBudget {
    elements_left: usize,      // 所有Vec/Map/Set条目共享逻辑元素额度。
    path_segments_left: usize, // 深节点路径另有段数额度。
    string_bytes_left: usize,  // 拥有型String容量按当前长度近似计费。
}

impl Default for BrowserRetirementBudget {
    fn default() -> Self {
        Self {
            elements_left: MAX_RETIREMENT_ELEMENTS_PER_FRAME, // 每个Update重新取得固定元素额度。
            path_segments_left: MAX_RETIREMENT_PATH_SEGMENTS_PER_FRAME, // 深路径不能借元素额度隐藏成本。
            string_bytes_left: MAX_RETIREMENT_STRING_BYTES_PER_FRAME, // 名称密集资产同样分帧释放。
        }
    }
}

impl BrowserRetirementBudget {
    /// 只有三项剩余额度都足够时才允许真正pop/drop，避免先移出后被迫整项析构。
    pub(in crate::ui::gltf_browser) fn try_charge(
        &mut self,
        elements: usize,
        path_segments: usize,
        string_bytes: usize,
    ) -> bool {
        let fresh_frame = self.elements_left == MAX_RETIREMENT_ELEMENTS_PER_FRAME
            && self.path_segments_left == MAX_RETIREMENT_PATH_SEGMENTS_PER_FRAME
            && self.string_bytes_left == MAX_RETIREMENT_STRING_BYTES_PER_FRAME; // 尚未释放任何元素时允许一个合法大项独占本帧。
        if elements > self.elements_left
            || path_segments > self.path_segments_left
            || string_bytes > self.string_bytes_left
        {
            if !fresh_frame {
                return false; // 已有工作后余额不足，完整留给下一Update。
            }
            self.elements_left = 0; // 单个受公共硬门保护的大项独占本帧，防止永久饿死。
            self.path_segments_left = 0; // 即使近似成本略超标也不再释放第二项。
            self.string_bytes_left = 0; // String析构本身O(1)，这里只限制allocator调用聚集。
            return true;
        }
        self.elements_left -= elements; // 本帧剩余逻辑条目额度精确递减。
        self.path_segments_left -= path_segments; // 节点路径段复制/释放额度独立递减。
        self.string_bytes_left -= string_bytes; // UTF-8字节是无需扫描字符的稳定近似量。
        true
    }

    /// 一个普通拥有字符串按一个元素和其当前字节长度计费。
    pub(in crate::ui::gltf_browser) fn try_charge_string(&mut self, value: &str) -> bool {
        self.try_charge(1, 0, value.len()) // String释放是O(1)，但大量allocator调用仍受元素预算。
    }

    /// 一条节点路径和网格名按段数及全部字符串字节计费。
    pub(in crate::ui::gltf_browser) fn try_charge_mesh_identity(
        &mut self,
        path: &[String],
        mesh: &str,
    ) -> bool {
        let path_bytes = path.iter().map(String::len).sum::<usize>(); // 正式路径至多64段/4096字符。
        self.try_charge(
            1,                                     // 整个稳定身份是一个逻辑元素。
            path.len(),                            // 每段拥有独立String分配。
            path_bytes.saturating_add(mesh.len()), // 网格名与路径共同进入字符预算。
        )
    }

    /// 一条公开行同时计入显示文本、类型ID和可选稳定选择中的全部拥有字符串。
    pub(super) fn try_charge_entry(&mut self, entry: &EditorGltfBrowserEntry) -> bool {
        let mut bytes = entry.display_name.len(); // 行显示文本始终拥有一份完整副本。
        let mut segments = 0usize; // 根行没有节点路径，网格行至多公共64段。
        if let EditorGltfBrowserEntryKind::Placement { placement_id, .. } = &entry.kind {
            bytes = bytes.saturating_add(placement_id.len()); // 根点击身份是第二份拥有字符串。
        }
        if let Some(selection) = &entry.selection {
            bytes = bytes
                .saturating_add(selection.placement_id.len())
                .saturating_add(selection.mesh_name.len())
                .saturating_add(selection.node_path.iter().map(String::len).sum::<usize>());
            segments = selection.node_path.len(); // 选择的Vec<String>按真实段数计费。
        }
        self.try_charge(1, segments, bytes) // 一次只在预算足够时pop整条行。
    }

    /// Scene0缓存网格使用相同的稳定路径成本模型。
    pub(super) fn try_charge_cached_mesh(&mut self, mesh: &CachedGltfMesh) -> bool {
        self.try_charge_mesh_identity(&mesh.node_path, &mesh.mesh_name) // 索引与布尔字段为固定小成本。
    }
}

/// 单步结果区分“完成”“确实推进”和“预算不足”，外层据此停止本帧循环。
pub(in crate::ui::gltf_browser) enum BrowserRetirementStep {
    Complete,   // 当前工作项已经只剩空容器壳。
    Progressed, // 本帧成功释放至少一个有界元素，可以继续尝试下一项。
    Blocked,    // 当前元素超过本帧剩余额度，必须留到下一Update。
}
