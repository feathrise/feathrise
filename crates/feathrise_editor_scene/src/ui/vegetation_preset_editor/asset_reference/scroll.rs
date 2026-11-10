//! 中央内嵌选择区的滚动交接；只有开始、换组和结束时调整，不占用稳定帧控制权。

use super::super::types::{
    PresetWorkspaceEditorPage, ReferencePhase, ReferenceTarget, VegetationPresetEditorState,
}; // 选择只属于原字段页面。
use super::helpers::target_navigation_is_current; // UI回位与字段写入共享相同标签/分区身份规则。
use crate::resources::SceneEditorState;
use bevy::prelude::*; // 直接复用编辑正文的逻辑像素滚动，不操作相机。 // 关闭或切换场景时不抢回滚动。

/// 每次只保留一个目标及一个原滚动位置，不复制草稿或模型。
#[derive(Default)]
pub(in super::super) struct PickerScrollCache {
    saved: Option<(ReferenceTarget, Vec2)>, // 原字段所属页面和发起前逻辑滚动位置。
    visible_key: Option<(u64, bool, usize)>, // 请求票据、是否已可选、当前组决定是否需要露出候选。
}

/// 选择区位于正文头部，发起时暂时显示；成功/取消仍在原分区时恢复原位置。
pub(in super::super) fn sync_picker_scroll(
    state: Res<VegetationPresetEditorState>, // 不改变预设、输入缓冲或请求阶段。
    editor: Res<SceneEditorState>,             // 作者离开F12后不操作隐藏页面。
    mut cache: Local<PickerScrollCache>,     // 同一阶段重复帧保持作者手动滚动。
    mut pages: Query<&mut ScrollPosition, With<PresetWorkspaceEditorPage>>, // 中央唯一编辑正文，不触碰左右栏或场景视口。
) {
    if !editor.open || editor.close_requested || editor.pending_scene_transition.is_some() {
        cache.saved = None; // 作者已经离开会话，不在下一次打开F12时恢复旧位置。
        cache.visible_key = None; // 清理一次性滚动意图，不修改隐藏UI。
        return;
    }
    let Ok(mut scroll) = pages.single_mut() else {
        return;
    }; // 静态UI尚未挂载时不消费交接。
    if let Some(request) = state.asset_reference.as_ref() {
        if !target_navigation_is_current(&state, &request.target) {
            return;
        } // 旧请求不能把新标签卷回顶部。
        if cache.saved.is_none() {
            cache.saved = Some((request.target.clone(), scroll.0)); // 只在本轮发起时记住原字段位置。
        }
        let key = (
            request.ticket,
            matches!(request.phase, ReferencePhase::Choosing),
            request.page,
        ); // 候选准备完成和换组时再次露出列表。
        if cache.visible_key != Some(key) {
            scroll.y = 0.0; // 内嵌选择区位于正文最上方，矮窗口或长表单下方点击也能立即看见。
            cache.visible_key = Some(key); // 后续稳定帧不能追着重置手动滚动。
        }
    } else {
        if let Some((target, original)) = cache.saved.take()
            && editor.open
            && !editor.close_requested
            && editor.pending_scene_transition.is_none()
            && target_navigation_is_current(&state, &target)
        {
            scroll.0 = original; // 面板移除时恢复原字段位置；Bevy布局负责窗口改变后的合法范围钳制。
        }
        cache.visible_key = None; // 本轮已结束，下次独立记录新的发起位置。
    }
}
