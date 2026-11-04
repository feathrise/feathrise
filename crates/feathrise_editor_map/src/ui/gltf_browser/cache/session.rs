//! GLB浏览器会话关闭、退役推进与各有界状态机的Update编排。

use bevy::prelude::*; // Res、ResMut和Assets系统参数。
use bevy::world_serialization::WorldAsset; // 浏览器只读Scene0原始World。

use super::super::EditorGltfBrowserState; // 活动会话与退役队列共用唯一资源。
use super::super::entries::{advance_visible_entries_build, request_visible_entries_rebuild}; // 完整树分帧构建与请求。
use super::super::retirement::take_browser_session_generation; // 关闭只移动所有权到预留退役槽。
use super::signature_sync::synchronize_signature_build; // 摆放结构签名按固定记录预算推进。
use super::{
    collect_invalidated_paths, invalidate_cached_paths, process_asset_demand_budget,
    process_loading_budget,
}; // 资产事件、需求和Scene0读取均由父模块提供有界步骤。
use crate::authoring::EditorAuthoringSession; // 草稿摆放表只由分帧签名任务读取。
use crate::resources::MapEditorState; // F12关闭时立即撤下公开浏览状态。

/// 当前资源是否仍拥有需要从公开会话撤下的活动数据。
fn has_live_browser_session(state: &EditorGltfBrowserState) -> bool {
    state.initialized
        || state.signature_build.is_some()
        || state.scene_build.is_some()
        || state.entries_build.is_some()
        || !state.assets.is_empty()
        || !state.entries.is_empty()
        || !state.signature.is_empty()
        || !state.expanded_placements.is_empty()
        || state.selection.is_some()
        || state.selected_placement_id.is_some()
        || !state.loading_queue.is_empty()
        || !state.status.is_empty() // 关闭等待提示也属于当前公开会话。
}

/// F12关闭同帧清空公开行、选择和活动索引，大型所有权随后按固定预算释放。
pub(super) fn release_browser_session(state: &mut EditorGltfBrowserState) {
    let generation = take_browser_session_generation(state); // 所有容器只做mem::take/移动。
    state.retirement.enqueue_close(generation); // 第二代槽专门保证关闭不会被正常换代阻塞。
}

/// 草稿结构、展开需求、相关Scene0事件或有界加载队列驱动浏览缓存更新。
pub(crate) fn refresh_gltf_browser_cache(
    session: Res<EditorAuthoringSession>, // 数值拖动Changed只做O(1)观察。
    editor: Res<MapEditorState>,          // F12关闭时立即撤下浏览公开状态。
    standalone: Option<Res<crate::standalone::StandaloneEditorMode>>,
    asset_server: Res<AssetServer>,       // 建立强Scene0句柄并解析Mesh子资产标签。
    world_assets: Res<Assets<WorldAsset>>, // 完整加载后只读一个`WorldAsset.world`。
    mut scene_events: feathrise_scene_core::residency::ResidentAssetEventReader<WorldAsset>, // 热更事件按固定预算消费。
    mut state: ResMut<EditorGltfBrowserState>, // 活动树与最多两代退役数据。
) {
    state.retirement.advance_frame(); // 无论F12开关状态，每帧只推进一次固定析构预算。

    // 嵌入试玩只隐藏作者呈现，仍属于同一作者会话；选择、展开和稳定索引不得因此退役。
    // 隐藏期间仍按原固定预算维护热更/签名，真正关闭或退出地图继续走原退役路径。
    if !editor.open && !standalone.as_ref().is_some_and(|mode| mode.embedded_playing()) {
        scene_events.clear(); // 关闭态不积累下一会话的旧资产事件游标。
        if has_live_browser_session(&state) {
            release_browser_session(&mut state); // 对外状态同帧为空，大容器进入预留关闭槽。
        }
        return;
    }

    if !state.initialized && !state.retirement.is_empty() {
        scene_events.clear();
        if state.status != "场景树：正在分帧释放上次会话，完成后自动重新打开"
        {
            state.status = "场景树：正在分帧释放上次会话，完成后自动重新打开".to_owned();
        }
        return; // 上次关闭尚未清完时不启动新会话，反复开关也不会生成第三退役代。
    }

    let invalidated = collect_invalidated_paths(&mut scene_events, &mut state);
    let mut entries_changed = invalidate_cached_paths(&mut state, invalidated);
    entries_changed |= process_asset_demand_budget(&mut state, &asset_server);
    entries_changed |= process_loading_budget(&mut state, &asset_server, &world_assets);
    if entries_changed {
        request_visible_entries_rebuild(&mut state); // 同一Update只递增一次行修订。
    }
    synchronize_signature_build(&session, session.is_changed(), &mut state); // 完成时会自行请求新结构行树。
    advance_visible_entries_build(&mut state); // 稳定时O(1)，在途时受操作、行、路径和字符四重预算。
}
