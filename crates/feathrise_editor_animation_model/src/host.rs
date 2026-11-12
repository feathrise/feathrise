//! 动画在输入、任务和表现阶段读取同一工程的轻量宿主事实。

use bevy::prelude::*;
use feathrise_editor_core::PresetWorkspaceTab;
use std::path::{Path, PathBuf};

/// 源资源仍归宿主。仅输入门、选中项与资产根在各原有阶段发布给独立动画插件。
#[derive(Resource, Debug, Default)]
pub struct AnimationHost {
    pub asset_root: PathBuf,
    pub selected_asset: Option<String>,
    pub tab: PresetWorkspaceTab,
    pub open: bool,
    pub closing: bool,
    pub switching_scene: bool,
    pub modal_open: bool,
    pub sidebar_assets_visible: bool,
    pub shading_hovered: bool,
    pub layout_dragging: bool,
    pub scene_dragging: bool,
}

impl AnimationHost {
    pub fn asset_root(&self) -> &Path {
        &self.asset_root
    }
}

#[derive(Resource, Debug, Default)]
pub struct SceneEditorNumericScrubMouseGuard {
    blocked: bool, // `true`表示本帧除已持有数值/预览拖动者外的作者鼠标入口都必须丢弃输入。
    pub floating_panel_hovered: bool, // 悬停挡住滚轮；只有在浮窗内按下才保留拖动所有权。
}

impl SceneEditorNumericScrubMouseGuard {
    /// 共享 UI 控件拖动态与本帧鼠标边沿共同推进唯一所有权状态机。
    pub fn advance(
        &mut self,
        scrub_active: bool,        // 当前是否仍有公共数值框持有左右拖动身份。
        any_button_pressed: bool,  // 左、右、中任一作者鼠标键是否仍按住。
        any_button_released: bool, // 本帧是否刚松开左、右、中任一键；release帧仍不得穿透。
    ) {
        if scrub_active {
            self.blocked = true; // 只要拖动实体存在，就立即取得场景编辑器级鼠标所有权。
            return;
        }
        if !self.blocked {
            return; // 从未开始数值拖动时，普通鼠标按键不能反向激活该门。
        }
        self.blocked = any_button_pressed || any_button_released; // 全松后的下一静默帧才归还鼠标。
    }

    /// 当前帧是否必须丢弃全部非数值场景编辑器鼠标输入。
    pub fn blocked(&self) -> bool {
        self.blocked || self.floating_panel_hovered // 鼠标离开浮窗时，单纯悬停不会扣住下一次外部点击。
    }
}
