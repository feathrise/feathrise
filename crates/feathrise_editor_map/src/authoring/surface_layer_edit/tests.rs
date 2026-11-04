//! 地表作者层删除的四槽算法、原子撤销与迟到/失败生命周期回归。

use super::SurfaceLayerEditTask; // 确认失败不会自动重新启动整图任务。
use super::helpers::{
    // 引入地表作者层事务所需类型和纯助手。
    apply_surface_authoring_change,
    definitions_match,
    materialize_snapshot,
    remap_texel, // 回归检查当前作者定义、像素或生命周期不变量。
    remove_layer,
    snapshot, // 回归检查当前作者定义、像素或生命周期不变量。
}; // 纯算法与正式历史采用边界。
use super::systems::{adopt_result, finish_result, start_surface_materialization_if_shared}; // 完成值和失败闩锁共享生产代码。
use super::types::{SurfaceEditKind, SurfaceEditResult}; // 测试显式构造当前、迟到和失败完成值。
use crate::authoring::{
    // 引入地表作者层事务所需类型和纯助手。
    EditorAuthoringSession,
    EditorCommand,
    EditorMapDraft,
    test_heightfield_draft, // 回归检查当前作者定义、像素或生命周期不变量。
}; // 复用真实小型动态地图夹具。
use crate::resources::MapEditorState; // 引入地表作者层事务所需类型和纯助手。
use feathrise_map_content::{MAP_SURFACE_SCHEMA_VERSION, MapSurfaceAsset, MapSurfaceLayer}; // 测试继续使用正式地图作者格式。
use std::sync::Arc; // 回归明确区分浅快照和独立写侧。 // 结果采用必须同时匹配F12与地图选择。

fn draft_with_pixels(layers: u32, pixels: &[([u32; 4], [u8; 4])]) -> EditorMapDraft {
    // 地表作者操作在受检输入下执行当前步骤。
    // 小型夹具使用真实格式，不访问磁盘资产。
    let mut draft = test_heightfield_draft(Arc::new(vec![0; 121])); // 11×11合法地形仅作为作者地图上下文。
    draft.surface = Some(MapSurfaceAsset {
        // 回归检查当前作者定义、像素或生命周期不变量。
        // 构造当前地图专有地表定义。
        schema_version: MAP_SURFACE_SCHEMA_VERSION, // 与正式地表格式一致。
        map_id: draft.map_id.clone(),               // 定义严格属于当前草稿地图。
        weight_source_path: "maps/EDGE-TEST/surface/weights.rgba8".to_owned(), // 私有权重作者路径。
        weight_texture_path: "maps/EDGE-TEST/surface/weights.ktx2".to_owned(), // 私有运行时权重派生路径。
        index_source_path: "maps/EDGE-TEST/surface/indices.rgba32".to_owned(), // 私有小端局部索引路径。
        index_texture_path: "maps/EDGE-TEST/surface/indices.ktx2".to_owned(), // 私有运行时局部索引派生路径。
        resolution: [pixels.len() as u32, 1], // 每个输入元组恰好对应一个纹素。
        layers: (0..layers) // 回归检查当前作者定义、像素或生命周期不变量。
            .map(|index| MapSurfaceLayer {
                // 回归检查当前作者定义、像素或生命周期不变量。
                // 层的稳定身份与数组下标分别可验证。
                id: format!("author-{index}"), // 同预设合法独立层仍有不同地图作者ID。
                label: format!("作者层{index}"), // 中文作者名称在撤销中保持精确。
                preset_id: format!("preset-{index}"), // 每层引用其真实规范预设ID。
            }) // 当前受检分支或参数组结束。
            .collect(), // 测试内容层数不受UI显示槽位限制。
    }); // 完整地表定义结束。
    draft.surface_weights = Some(Arc::new(
        // 读取或传递同版本RGBA8权重作者源。
        pixels.iter().flat_map(|(_, weights)| *weights).collect(), // 回归检查当前作者定义、像素或生命周期不变量。
    )); // RGBA8字节与输入保持精确一致。
    draft.surface_indices = Some(Arc::new(
        // 读取或传递同版本小端RGBA32局部索引源。
        pixels // 回归检查当前作者定义、像素或生命周期不变量。
            .iter() // 回归检查当前作者定义、像素或生命周期不变量。
            .flat_map(|(indices, _)| indices.iter().flat_map(|index| index.to_le_bytes())) // 回归检查当前作者定义、像素或生命周期不变量。
            .collect(), // 回归检查当前作者定义、像素或生命周期不变量。
    )); // 按正式小端u32协议编码。
    draft // 原始字节可被每项回归独立保留和比较。
} // 地表草稿夹具结束。

fn context() -> (MapEditorState, EditorAuthoringSession) {
    // 地表作者操作在受检输入下执行当前步骤。
    // 两个混合像素同时覆盖保底和中间层。
    let draft = draft_with_pixels(
        // 回归检查当前作者定义、像素或生命周期不变量。
        3, // 回归检查当前作者定义、像素或生命周期不变量。
        &[
            // 回归检查当前作者定义、像素或生命周期不变量。
            ([0, 1, 2, 0], [64, 127, 64, 0]), // 回归检查当前作者定义、像素或生命周期不变量。
            ([1, 1, 0, 0], [200, 55, 0, 0]),  // 回归检查当前作者定义、像素或生命周期不变量。
        ], // 回归检查当前作者定义、像素或生命周期不变量。
    ); // 第二个像素只有待删层贡献。
    let editor = MapEditorState {
        // 回归检查当前作者定义、像素或生命周期不变量。
        open: true, // 回归检查当前作者定义、像素或生命周期不变量。
        selected_map_id: Some(draft.map_id.clone()), // 稳定地图身份必须与当前草稿一致。
        selected_surface_layer: 1, // 回归检查当前作者定义、像素或生命周期不变量。
        surface_selected_preset_id: Some("preset-1".to_owned()), // 稳定作者层和预设引用保持真实对应关系。
        ..Default::default() // 回归检查当前作者定义、像素或生命周期不变量。
    }; // 模拟已经明确选中作者层的F12界面。
    let session = EditorAuthoringSession {
        // 回归检查当前作者定义、像素或生命周期不变量。
        draft: Some(draft),   // 回归检查当前作者定义、像素或生命周期不变量。
        revision: 11,         // 回归检查当前作者定义、像素或生命周期不变量。
        surface_revision: 4,  // 地表专用修订对应当前定义和两张图。
        ..Default::default()  // 回归检查当前作者定义、像素或生命周期不变量。
    }; // 完整作者和地表修订分别冻结。
    (editor, session) // 每项生命周期回归都从同一权威身份开始。
} // 作者会话夹具结束。

fn deletion_result(session: &EditorAuthoringSession, index: usize) -> SurfaceEditResult {
    // 地表作者操作在受检输入下执行当前步骤。
    // 测试同步计算只用于小型纯夹具，生产路径使用后台池。
    let before = snapshot(session.draft.as_ref().expect("夹具草稿存在")).expect("夹具地表配对完整"); // Arc浅快照保留原始输入。
    let result = remove_layer(&before, index); // 同一正式算法生成完整候选或真实错误。
    SurfaceEditResult {
        // 回归检查当前作者定义、像素或生命周期不变量。
        ticket: 7,                                  // 任务票据必须属于当前唯一后台槽。
        revision: session.revision,                 // 完整作者修订共同防止迟到结果回写。
        surface_revision: session.surface_revision, // 地表专用修订对应当前定义和两张图。
        before,                                     // 只读输入保留操作前的完整作者版本。
        kind: SurfaceEditKind::Delete { index },    // 回归检查当前作者定义、像素或生命周期不变量。
        result,                                     // 完整输出成功后才允许作为单一版本采用。
    } // 原子采用再次检查全部身份。
} // 删除结果夹具结束。

#[test] // 删除第一项需要把所有后继层向前压缩。
fn first_layer_deletion_compacts_all_later_indices_and_normalizes() {
    // 地表作者操作在受检输入下执行当前步骤。
    // 原第一层移除后，原第二层成为新保底。
    let (indices, weights) = // 回归检查当前作者定义、像素或生命周期不变量。
        remap_texel([0, 1, 2, 3], [55, 100, 50, 50], 0, 4).expect("四槽输入有效"); // 剩余比例为2:1:1。
    assert_eq!(indices, [0, 1, 2, 0]); // 后继层依次左移，空槽回到合法索引零。
    assert_eq!(weights, [127, 64, 64, 0]); // 最大余数分配严格保持总权重255。
} // 首层删除回归结束。

#[test] // 删除中间层只改变其后的局部索引身份。
fn middle_layer_deletion_preserves_earlier_indices_and_merges_duplicates() {
    // 地表作者操作在受检输入下执行当前步骤。
    // 重复剩余索引先合并再归一化。
    let (indices, weights) = // 回归检查当前作者定义、像素或生命周期不变量。
        remap_texel([0, 2, 3, 3], [51, 51, 51, 102], 2, 4).expect("重复索引按正式四槽语义聚合"); // 原第三层被删除。
    assert_eq!(indices, [0, 2, 0, 0]); // 原零层保持，原三层压缩到二层。
    assert_eq!(weights, [64, 191, 0, 0]); // 剩余51:153按整数守恒归一化为255。
} // 中间层和重复身份回归结束。

#[test] // 被删层独占的像素必须保持合法完整地表。
fn removed_only_texels_fall_back_to_first_remaining_layer() {
    // 地表作者操作在受检输入下执行当前步骤。
    // 删除首层和中间层都遵守删除后的局部零层回退。
    for removed in [0, 2] {
        // 依据完整作者条件选择有限处理分支。
        // 分别覆盖新保底来自原第二层和原第一层。
        assert_eq!(
            // 回归检查当前作者定义、像素或生命周期不变量。
            remap_texel([removed; 4], [100, 75, 80, 0], removed, 4).expect("纯目标像素有效"), // 前置身份和长度检查已保证该测试或作者值存在。
            ([0; 4], [255, 0, 0, 0]) // 回归检查当前作者定义、像素或生命周期不变量。
        ); // 不留下零总权重或失效索引。
    } // 两种删除位置回退回归结束。
} // 纯目标像素回归结束。

#[test] // 清空后重新绘制留下的裸地区不能因删除其他层而被填满。
fn empty_surface_texels_remain_empty_when_other_layers_are_deleted() {
    // 保持无覆盖像素的零权重语义。
    assert_eq!(
        remap_texel([0, 1, 2, 0], [0; 4], 1, 3).expect("合法无覆盖像素"),
        ([0; 4], [0; 4])
    ); // 删除中间层不制造基础层覆盖。
    assert!(remap_texel([0, 9, 0, 0], [0; 4], 1, 3).is_err()); // 原空白像素的坏索引仍需报告。
} // 裸地保持回归结束。

#[test] // 输入损坏时不得静默把它重解释为另一个合法地表。
fn invalid_indices_are_rejected_without_changing_source() {
    // 地表作者操作在受检输入下执行当前步骤。
    // 失败前后比较精确原始字节。
    assert!(remap_texel([0, 1, 9, 0], [255, 0, 0, 0], 1, 3).is_err()); // 零权重槽越界也必须拒绝。
    let draft = draft_with_pixels(1, &[([0, 0, 9, 0], [255, 0, 0, 0])]); // 唯一层也必须拒绝损坏索引，不能借清空掩盖坏数据。
    let before = snapshot(&draft).expect("像素身份交给后台逐项校验"); // 输入冻结不在主线程扫描整图。
    assert!(
        // 回归检查当前作者定义、像素或生命周期不变量。
        remove_layer(&before, 0) // 回归检查当前作者定义、像素或生命周期不变量。
            .err() // 回归检查当前作者定义、像素或生命周期不变量。
            .expect("损坏索引拒绝删除") // 即使最后一层，真实坏输入仍保留原版本。
            .contains("索引") // 错误明确指出作者源损坏。
    ); // 保护原因必须能向作者解释。
    assert!(definitions_match(
        // 回归检查当前作者定义、像素或生命周期不变量。
        draft.surface.as_ref().expect("地表定义存在"), // 只读借用当前完整作者值。
        &before.surface // 回归检查当前作者定义、像素或生命周期不变量。
    )); // 没有删除唯一层定义。
    assert_eq!(
        // 回归检查当前作者定义、像素或生命周期不变量。
        draft.surface_weights.as_deref(), // 读取或传递同版本RGBA8权重作者源。
        Some(before.weights.as_ref())     // 只读借用当前完整作者值。
    ); // 权重源字节保持原样。
    assert_eq!(
        // 回归检查当前作者定义、像素或生命周期不变量。
        draft.surface_indices.as_deref(), // 读取或传递同版本小端RGBA32局部索引源。
        Some(before.indices.as_ref())     // 只读借用当前完整作者值。
    ); // 索引源字节保持原样。
} // 失败保留输入回归结束。

#[test] // 整层删除必须成为一个可精确反复撤销/重做的命令。
fn surface_layer_delete_undo_redo_restores_exact_definitions_and_both_images() {
    // 地表作者操作在受检输入下执行当前步骤。
    // 不重新运行有损重映射，直接恢复完整前后字节。
    let (mut editor, mut session) = context(); // 当前选择中间作者层。
    let before = snapshot(session.draft.as_ref().expect("草稿存在")).expect("完整地表"); // 保留输入字节与顺序供逐字节比较。
    let output = deletion_result(&session, 1); // 生成中间层删除完整候选。
    assert!(adopt_result(&mut editor, &mut session, 7, output).expect("当前结果应完整采用")); // 一次采用发布定义与两张图。
    assert_eq!(session.undo.len(), 1); // 删除不能拆成定义、索引、权重三条撤销。
    assert!(session.redo.is_empty()); // 新操作清空重做分支。
    assert_eq!(session.revision, 12); // 完整作者修订只递增一次。
    assert_eq!(session.surface_revision, 5); // 地表预览身份与作者脏状态同帧换代。
    assert!(editor.dirty && session.draft.as_ref().expect("草稿存在").surface_dirty); // 现有保存和预览链路被正确触发。
    assert_eq!(
        // 回归检查当前作者定义、像素或生命周期不变量。
        editor.surface_selected_preset_id.as_deref(), // 稳定作者层和预设引用保持真实对应关系。
        Some("preset-2")                              // 回归检查当前作者定义、像素或生命周期不变量。
    ); // 删除后笔刷同步真实剩余作者层。
    let after = snapshot(session.draft.as_ref().expect("草稿存在")).expect("删除后完整地表"); // 后像素用于精确重做比较。
    let command = session.undo.pop().expect("唯一删除命令存在"); // 模拟统一历史在校验后的转移。
    let EditorCommand::SurfaceAuthoringChanged(change) = command else {
        // 回归检查当前作者定义、像素或生命周期不变量。
        panic!("必须是完整地表结构命令"); // 回归检查当前作者定义、像素或生命周期不变量。
    }; // 拒绝假冒普通笔刷差分。
    let draft = session.draft.as_mut().expect("草稿存在"); // 两个方向均在同一草稿上应用。
    for _ in 0..3 {
        // 依据完整作者条件选择有限处理分支。
        // 多次反复操作不得累计量化误差。
        apply_surface_authoring_change(draft, &change, true).expect("删除可以精确撤销"); // 完整恢复原层及其覆盖。
        assert!(definitions_match(
            // 回归检查当前作者定义、像素或生命周期不变量。
            draft.surface.as_ref().expect("地表定义存在"), // 只读借用当前完整作者值。
            &before.surface // 回归检查当前作者定义、像素或生命周期不变量。
        )); // 原层顺序、名称、路径完全相同。
        assert_eq!(
            // 回归检查当前作者定义、像素或生命周期不变量。
            draft.surface_weights.as_deref(), // 读取或传递同版本RGBA8权重作者源。
            Some(before.weights.as_ref())     // 只读借用当前完整作者值。
        ); // 精确恢复原RGBA8字节，不重新归一化。
        assert_eq!(
            // 回归检查当前作者定义、像素或生命周期不变量。
            draft.surface_indices.as_deref(), // 读取或传递同版本小端RGBA32局部索引源。
            Some(before.indices.as_ref())     // 只读借用当前完整作者值。
        ); // 精确恢复原RGBA32全部槽。
        apply_surface_authoring_change(draft, &change, false).expect("删除可以精确重做"); // 采用已经受检的原后值。
        assert!(definitions_match(
            // 回归检查当前作者定义、像素或生命周期不变量。
            draft.surface.as_ref().expect("地表定义存在"), // 只读借用当前完整作者值。
            &after.surface // 回归检查当前作者定义、像素或生命周期不变量。
        )); // 删除后的层表精确重现。
        assert_eq!(
            // 回归检查当前作者定义、像素或生命周期不变量。
            draft.surface_weights.as_deref(), // 读取或传递同版本RGBA8权重作者源。
            Some(after.weights.as_ref())      // 只读借用当前完整作者值。
        ); // 重做权重无误差积累。
        assert_eq!(
            // 回归检查当前作者定义、像素或生命周期不变量。
            draft.surface_indices.as_deref(), // 读取或传递同版本小端RGBA32局部索引源。
            Some(after.indices.as_ref())      // 只读借用当前完整作者值。
        ); // 重做局部索引没有二次压缩。
    } // 多轮历史回归结束。
} // 原子删除历史回归结束。

#[test] // 唯一层删除必须清空整张作者画布并支持精确历史恢复。
fn last_surface_layer_delete_clears_selection_and_round_trips_exact_history() {
    // 覆盖空层采用、共享写侧与重复撤销重做。
    let (mut editor, mut session) = context(); // 复用正式会话身份和修订夹具。
    session.draft = Some(draft_with_pixels(
        1,
        &[([0; 4], [100, 75, 80, 0]), ([0; 4], [0; 4])],
    )); // 混合重复槽与原空白像素都必须被清空。
    editor.selected_surface_layer = 0; // 当前选中地图唯一作者层。
    editor.surface_selected_preset_id = Some("preset-0".to_owned()); // 模拟点击该层同步的笔刷预设。
    let output = deletion_result(&session, 0); // 使用生产后台算法产生空层候选。
    let before = output.before.clone(); // 保留精确操作前定义和字节供历史断言。
    assert!(adopt_result(&mut editor, &mut session, 7, output).expect("空层结果合法")); // 生产原子采用不得空表下溢或越界。
    assert_eq!(editor.selected_surface_layer, 0); // 空列表的选择索引饱和到零哨兵。
    assert!(editor.surface_selected_preset_id.is_none()); // 删除完成不得自动选择刚被删除的预设。
    assert!(editor.dirty && session.undo.len() == 1); // 整层删除仍是一条可保存的作者命令。
    let after = snapshot(session.draft.as_ref().expect("草稿存在")).expect("空层保留完整画布"); // 空层定义不是缺失作者源。
    assert!(after.surface.layers.is_empty()); // 不暗中制造或保留基础作者层。
    assert_eq!(after.weights.as_slice(), &[0; 8]); // 两个像素四槽覆盖全部为零。
    assert_eq!(after.indices.as_slice(), &[0; 32]); // 两个像素小端局部索引全部为零。
    assert!(materialize_snapshot(&after).is_ok()); // 重新绘制所需后台写侧允许空层画布。
    assert!(remove_layer(&after, 0).is_err()); // 空列表本身没有可再次删除的目标。
    let EditorCommand::SurfaceAuthoringChanged(change) = session.undo.pop().expect("删除历史存在")
    else {
        // 历史保存定义和两图同一事务。
        panic!("必须生成完整地表删除历史"); // 不能用不完整普通笔刷差分冒充。
    }; // 提取真实删除命令结束。
    let draft = session.draft.as_mut().expect("草稿存在"); // 在同一地图上反复切换完整版本。
    for _ in 0..3 {
        // 多次往返不能积累权重量化误差。
        apply_surface_authoring_change(draft, &change, true).expect("最后层可以撤销"); // 恢复原唯一层及精确像素。
        let restored = snapshot(draft).expect("撤销后完整画布"); // 捕获恢复后的实际作者值。
        assert!(definitions_match(&restored.surface, &before.surface)); // 原层ID、标签与路径完全恢复。
        assert_eq!(restored.weights, before.weights); // 原始非标准分槽字节保持精确。
        assert_eq!(restored.indices, before.indices); // 原始全部索引字节保持精确。
        apply_surface_authoring_change(draft, &change, false).expect("最后层可以重做"); // 空层是合法历史目标。
        let cleared = snapshot(draft).expect("重做后完整空画布"); // 重做不丢弃后续绘制所需源图。
        assert!(definitions_match(&cleared.surface, &after.surface)); // 空层定义和作者路径保持精确。
        assert_eq!(cleared.weights, after.weights); // 空覆盖字节精确恢复。
        assert_eq!(cleared.indices, after.indices); // 空索引字节精确恢复。
    } // 多轮空层历史验证结束。
} // 最后层完整生命周期回归结束。

#[test] // 即使地表没有变化，其他作者修改也必须使旧删除失败关闭。
fn late_surface_deletion_rejects_full_author_revision_map_and_ticket_changes() {
    // 地表作者操作在受检输入下执行当前步骤。
    // 三类换代都不能发布旧候选。
    for kind in 0..4 {
        // 依据完整作者条件选择有限处理分支。
        // 覆盖完整作者修订、地图选择、票据和关闭生命周期。
        let (mut editor, mut session) = context(); // 每轮保留独立有效原始版本。
        let output = deletion_result(&session, 1); // 候选在旧身份下计算完成。
        let before = snapshot(session.draft.as_ref().expect("草稿存在")).expect("完整地表"); // 拒收前后做精确对比。
        let mut ticket = 7; // 初始票据匹配结果。
        match kind {
            // 依据完整作者条件选择有限处理分支。
            // 各种失效分别独立发生。
            0 => session.revision += 1, // 地图外的全局环境作者变化也必须取消旧删除。
            1 => editor.selected_map_id = None, // 切图开始后旧UI地图已不匹配。
            2 => ticket += 1,           // 同图同修订的旧票据也不能采用。
            _ => editor.open = false,   // 真正关闭F12后不迟到修改草稿。
        } // 失效注入结束。
        assert!(
            // 回归检查当前作者定义、像素或生命周期不变量。
            !adopt_result(&mut editor, &mut session, ticket, output).expect("迟到结果安全拒收") // 前置身份和长度检查已保证该测试或作者值存在。
        ); // 不能以旧结果覆盖新作者会话。
        let draft = session.draft.as_ref().expect("草稿仍然存在"); // 拒收不清空当前合法草稿。
        assert!(definitions_match(
            // 回归检查当前作者定义、像素或生命周期不变量。
            draft.surface.as_ref().expect("地表定义存在"), // 只读借用当前完整作者值。
            &before.surface // 回归检查当前作者定义、像素或生命周期不变量。
        )); // 层定义保持完整原状态。
        assert!(Arc::ptr_eq(
            // 回归检查当前作者定义、像素或生命周期不变量。
            draft.surface_weights.as_ref().expect("权重存在"), // 读取或传递同版本RGBA8权重作者源。
            &before.weights // 回归检查当前作者定义、像素或生命周期不变量。
        )); // 拒收甚至不更换权重指针。
        assert!(Arc::ptr_eq(
            // 回归检查当前作者定义、像素或生命周期不变量。
            draft.surface_indices.as_ref().expect("索引存在"), // 读取或传递同版本小端RGBA32局部索引源。
            &before.indices // 回归检查当前作者定义、像素或生命周期不变量。
        )); // 索引也没有任何副作用。
        assert!(session.undo.is_empty() && session.redo.is_empty() && !editor.dirty); // 旧结果不能制造历史或脏责任。
    } // 所有失效场景结束。
} // 完整身份迟到回归结束。

#[test] // 当前失败和迟到失败都不能发布半套定义或像素。
fn failed_surface_deletion_and_late_failure_preserve_authoring_state() {
    // 地表作者操作在受检输入下执行当前步骤。
    // 明确注入资源不足的后台错误。
    for stale in [false, true] {
        // 依据完整作者条件选择有限处理分支。
        // 当前失败可显示原因，旧失败只能丢弃。
        let (mut editor, mut session) = context(); // 原作者图和层表保持有效。
        let mut output = deletion_result(&session, 1); // 保留完整失败身份。
        output.result = Err("测试注入：地表后台画布分配失败".to_owned()); // 模拟CPU任务申请结果图失败。
        let before = snapshot(session.draft.as_ref().expect("草稿存在")).expect("完整地表"); // 记录权威输入。
        if stale {
            // 依据完整作者条件选择有限处理分支。
            session.revision += 1; // 完整作者修订共同防止迟到结果回写。
        } // 迟到错误不能覆盖新作者操作的状态。
        let result = adopt_result(&mut editor, &mut session, 7, output); // 正式采用先查身份再查结果。
        if stale {
            // 依据完整作者条件选择有限处理分支。
            assert_eq!(result, Ok(false)); // 回归检查当前作者定义、像素或生命周期不变量。
        } else {
            // 当前受检分支或参数组结束。
            assert!(result.is_err()); // 回归检查当前作者定义、像素或生命周期不变量。
        } // 区分当前失败与安全拒收。
        let draft = session.draft.as_ref().expect("草稿仍然存在"); // 原有效地表必须继续可编辑。
        assert!(definitions_match(
            // 回归检查当前作者定义、像素或生命周期不变量。
            draft.surface.as_ref().expect("地表定义存在"), // 只读借用当前完整作者值。
            &before.surface // 回归检查当前作者定义、像素或生命周期不变量。
        )); // 坏候选不能先删除定义。
        assert_eq!(
            // 回归检查当前作者定义、像素或生命周期不变量。
            draft.surface_weights.as_deref(), // 读取或传递同版本RGBA8权重作者源。
            Some(before.weights.as_ref())     // 只读借用当前完整作者值。
        ); // 权重源完整不变。
        assert_eq!(
            // 回归检查当前作者定义、像素或生命周期不变量。
            draft.surface_indices.as_deref(), // 读取或传递同版本小端RGBA32局部索引源。
            Some(before.indices.as_ref())     // 只读借用当前完整作者值。
        ); // 索引源完整不变。
        assert!(session.undo.is_empty() && !editor.dirty); // 没有失败历史或假dirty。
    } // 两种失败生命周期回归结束。
} // 失败原子性回归结束。

#[test] // 关闭或保存请求本身应等待删除完成，不能使已受理操作失效。
fn pending_close_and_save_requests_allow_complete_surface_deletion() {
    // 地表作者操作在受检输入下执行当前步骤。
    // F12仍打开时允许完成作者事务。
    let (mut editor, mut session) = context(); // 当前完整作者身份未改变。
    let output = deletion_result(&session, 1); // 用户已经明确要求删除。
    editor.close_requested = true; // 关闭门会等待本操作完成。
    editor.save_requested = true; // 保存门保留请求直到新完整地表就绪。
    assert!(
        // 回归检查当前作者定义、像素或生命周期不变量。
        adopt_result(&mut editor, &mut session, 7, output) // 回归检查当前作者定义、像素或生命周期不变量。
            .expect("请求关闭和保存不能取消已经受理的删层") // 前置身份和长度检查已保证该测试或作者值存在。
    ); // 全部像素完成后一次采用。
    assert!(editor.close_requested && editor.save_requested); // 上层生命周期请求保持原样继续执行。
} // 保存关闭衔接回归结束。

#[test] // 写侧物化完成不改变任何作者语义或修订。
fn surface_materialization_is_atomic_and_read_only_until_adopted() {
    // 地表作者操作在受检输入下执行当前步骤。
    // 复制输出在采用前不会改动当前原Arc。
    let (mut editor, mut session) = context(); // 当前两张作者源仍是权威。
    let before = snapshot(session.draft.as_ref().expect("草稿存在")).expect("完整地表"); // 持有原输入以模拟历史共享。
    let after = materialize_snapshot(&before).expect("小型后台写侧分配成功"); // 纯测试同步执行正式后台复制助手。
    assert_eq!(after.weights, before.weights); // 物化不能量化或重映射像素。
    assert!(!Arc::ptr_eq(&after.weights, &before.weights)); // 后台结果是独立权重写侧。
    assert!(!Arc::ptr_eq(&after.indices, &before.indices)); // 索引同样独立物化。
    let output = SurfaceEditResult {
        // 回归检查当前作者定义、像素或生命周期不变量。
        ticket: 7,                                  // 任务票据必须属于当前唯一后台槽。
        revision: session.revision,                 // 完整作者修订共同防止迟到结果回写。
        surface_revision: session.surface_revision, // 地表专用修订对应当前定义和两张图。
        before,                                     // 只读输入保留操作前的完整作者版本。
        kind: SurfaceEditKind::Materialize,         // 回归检查当前作者定义、像素或生命周期不变量。
        result: Ok(after),                          // 回归检查当前作者定义、像素或生命周期不变量。
    }; // 两张图共同绑定完整原输入身份。
    assert!(adopt_result(&mut editor, &mut session, 7, output).expect("当前写侧完整采用")); // 采用只移动两个Arc。
    assert_eq!((session.revision, session.surface_revision), (11, 4)); // 无语义复制不推进作者和预览修订。
    assert!(!editor.dirty && session.undo.is_empty()); // 不制造任何历史或保存责任。
    let draft = session.draft.as_mut().expect("草稿存在"); // 当前写侧应该可供局部笔刷直接使用。
    assert!(Arc::get_mut(draft.surface_weights.as_mut().expect("权重存在")).is_some()); // 权重拥有者唯一，无需输入帧整图COW。
    assert!(Arc::get_mut(draft.surface_indices.as_mut().expect("索引存在")).is_some()); // 索引拥有者同样唯一。
} // 无语义原子物化回归结束。

#[test] // 真实分配失败不得让待补笔划每帧自动重试并永久占住关闭/保存门。
fn surface_materialization_failure_is_consumed_once_without_automatic_retry() {
    // 地表作者操作在受检输入下执行当前步骤。
    // 不需要启动真实线程池即可验证故障门。
    let (_, session) = context(); // 当前作者身份与失败记录匹配。
    let map_id = session.draft.as_ref().expect("草稿存在").map_id.clone(); // 故障不能跨地图传播。
    let mut task = SurfaceLayerEditTask::default(); // 尚未存在任何后台任务。
    task.materialize_failure = Some((
        // 回归检查当前作者定义、像素或生命周期不变量。
        map_id,                    // 稳定地图身份必须与当前草稿一致。
        session.revision,          // 完整作者修订共同防止迟到结果回写。
        session.surface_revision,  // 地表专用修订对应当前定义和两张图。
        "测试分配失败".to_owned(), // 回归检查当前作者定义、像素或生命周期不变量。
    )); // 模拟轮询记录的真实失败。
    assert!(start_surface_materialization_if_shared(&session, &mut task).is_err()); // 上层收到Err后会结束当前待补笔划或清理撤销请求。
    assert!(!task.is_pending()); // 错误消费不能偷偷再启动整图任务。
    assert!(task.materialize_failure.is_none()); // 只消费一次，后续明确的新用户动作仍可重试。
} // 写侧失败停止重试回归结束。

#[test] // 删除未完成时应留在F12展示失败，不能保存旧地表并把错误提示覆盖为成功。
fn failed_or_stale_surface_delete_cancels_waiting_close_and_save() {
    // 地表作者操作在受检输入下执行当前步骤。
    // 正式完成值处理同时负责生命周期门。
    for stale in [false, true] {
        // 依据完整作者条件选择有限处理分支。
        // 当前失败和身份变化取消均须保留可见反馈。
        let (mut editor, mut session) = context(); // 原作者地表有效且尚未修改。
        let mut output = deletion_result(&session, 1); // 删除已经被受理并完成后台计算。
        let mut task = SurfaceLayerEditTask {
            // 回归检查当前作者定义、像素或生命周期不变量。
            ticket: 7,            // 任务票据必须属于当前唯一后台槽。
            deleting: true,       // 回归检查当前作者定义、像素或生命周期不变量。
            ..Default::default()  // 回归检查当前作者定义、像素或生命周期不变量。
        }; // 模拟轮询刚取出真实完成值后的唯一槽状态。
        editor.close_requested = true; // 用户要求完成删除后关闭。
        editor.save_requested = true; // 用户要求保存最终删除结果。
        if stale {
            // 依据完整作者条件选择有限处理分支。
            session.revision += 1; // 完整作者修订共同防止迟到结果回写。
        } else {
            // 当前受检分支或参数组结束。
            output.result = Err("测试删层分配失败".to_owned()); // 回归检查当前作者定义、像素或生命周期不变量。
        } // 分别注入取消或真实后台失败。
        finish_result(&mut editor, &mut session, &mut task, output); // 调用与非阻塞轮询完全相同的完成状态处理。
        assert!(editor.open && !editor.close_requested); // 错误留在可操作F12中，不会在同帧自动关闭。
        assert!(!editor.save_requested); // 不能继续保存旧作者地表并掩盖未完成删除。
        assert!(!task.deleting && session.undo.is_empty()); // 在途身份归还，原历史和图层不产生假删除。
        assert!(editor.status.contains(if stale { "取消" } else { "失败" })); // 作者可见的最终状态说明实际未完成原因。
    } // 两类未完成结果的生命周期门回归结束。
} // 关闭与保存失败反馈回归结束。

#[test] // 真实写侧失败应经轮询闩锁交给待补意图，并停止本次自动保存关闭。
fn materialization_failure_latches_current_input_and_preserves_original_images() {
    // 地表作者操作在受检输入下执行当前步骤。
    // 覆盖失败任务到上层取消的完整纯状态边界。
    let (mut editor, mut session) = context(); // 当前权威两图完整有效。
    let before = snapshot(session.draft.as_ref().expect("草稿存在")).expect("完整地表"); // 模拟后台任务已经浅冻结输入。
    let output = SurfaceEditResult {
        // 回归检查当前作者定义、像素或生命周期不变量。
        ticket: 7,                                  // 任务票据必须属于当前唯一后台槽。
        revision: session.revision,                 // 完整作者修订共同防止迟到结果回写。
        surface_revision: session.surface_revision, // 地表专用修订对应当前定义和两张图。
        before: before.clone(),                     // 只读输入保留操作前的完整作者版本。
        kind: SurfaceEditKind::Materialize,         // 回归检查当前作者定义、像素或生命周期不变量。
        result: Err("测试写侧分配失败".to_owned()), // 回归检查当前作者定义、像素或生命周期不变量。
    }; // 输出没有任何部分候选图。
    let mut task = SurfaceLayerEditTask {
        // 回归检查当前作者定义、像素或生命周期不变量。
        ticket: 7,            // 任务票据必须属于当前唯一后台槽。
        ..Default::default()  // 回归检查当前作者定义、像素或生命周期不变量。
    }; // 唯一任务已经真实完成，准备消费错误。
    editor.close_requested = true; // 关闭正在等待共享写侧和原冻结落点。
    editor.save_requested = true; // 保存同样等待待补样本完成。
    finish_result(&mut editor, &mut session, &mut task, output); // 正式轮询结果处理登记同输入错误。
    assert!(task.materialize_failure.is_some()); // 下一次笔刷/撤销准备将收到具体失败。
    assert!(start_surface_materialization_if_shared(&session, &mut task).is_err()); // 上层收到Err后收口或清请求，不再自动重试。
    assert!(!task.is_pending() && !editor.close_requested && !editor.save_requested); // 不残留隐藏CPU任务或永久等待的生命周期门。
    let draft = session.draft.as_ref().expect("草稿存在"); // 失败后仍保留上一完整权威地表。
    assert!(Arc::ptr_eq(
        // 回归检查当前作者定义、像素或生命周期不变量。
        draft.surface_weights.as_ref().expect("权重存在"), // 读取或传递同版本RGBA8权重作者源。
        &before.weights // 回归检查当前作者定义、像素或生命周期不变量。
    )); // 权重保持原指针与精确字节。
    assert!(Arc::ptr_eq(
        // 回归检查当前作者定义、像素或生命周期不变量。
        draft.surface_indices.as_ref().expect("索引存在"), // 读取或传递同版本小端RGBA32局部索引源。
        &before.indices // 回归检查当前作者定义、像素或生命周期不变量。
    )); // 索引同样完全未改变。
} // 物化失败完整状态回归结束。

#[test] // 旧地图任务释放槽后不能污染新图状态或消费新图保存关闭请求。
fn late_other_map_delete_silently_preserves_new_map_requests_and_status() {
    // 同图换代和跨图迟到必须采用不同的用户反馈边界。
    let (mut editor, mut session) = context(); // 开始时任务属于原地图。
    let output = deletion_result(&session, 1); // 后台保留原地图完整输入快照。
    let mut task = SurfaceLayerEditTask {
        ticket: 7,
        deleting: true,
        ..Default::default()
    }; // 模拟旧CPU任务已经真正完成。
    let new_map =
        feathrise_map_content::MapId::parse("MAP-next-surface").expect("另一地图ID有效"); // 切入具有自己生命周期请求的新地图。
    editor.selected_map_id = Some(new_map.clone()); // UI开始归属新地图。
    let draft = session.draft.as_mut().expect("草稿存在"); // 模拟新图完整作者草稿已经采用。
    draft.map_id = new_map.clone(); // 新草稿不再属于原删除任务。
    draft.surface.as_mut().expect("地表存在").map_id = new_map; // 地表定义同样归属于新图。
    editor.status = "新地图正在等待自己的保存".to_owned(); // 新会话状态不得被旧任务错误覆盖。
    editor.save_requested = true; // 新图独立保存请求已经受理。
    editor.close_requested = true; // 新图独立关闭请求已经受理。
    finish_result(&mut editor, &mut session, &mut task, output); // 正式轮询后处理也要守住跨图边界。
    assert!(editor.save_requested && editor.close_requested); // 旧删除取消不能清理新地图的请求。
    assert_eq!(editor.status, "新地图正在等待自己的保存"); // 新地图提示保持原样。
    assert!(!task.deleting && session.undo.is_empty()); // 只归还已完成CPU槽，不能制造任何旧图历史。
} // 跨图迟到后处理回归结束。
