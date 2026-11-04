//! 独立模式标记及固定一笔待试玩请求；不拥有任何作者源或历史。

use bevy::{ecs::system::SystemParam, prelude::*};

#[derive(Resource, Default)]
pub struct StandaloneEditorMode {
    pub(super) pending: bool,
    pub(super) waiting_save_sequence: Option<u64>,
    pub(super) external_play_active: bool,
    pub(super) embedded_playing: bool,
}

impl StandaloneEditorMode {
    /// 宿主只发布唯一工作槽是否占用，不把进程句柄或作者文档带进共享编辑器。
    pub fn set_external_play_active(&mut self, active: bool) {
        self.external_play_active = active;
    }

    /// 只表示工程已确认进入独立预览；不再收起作者工作台或租用其焦点。
    pub fn embedded_playing(&self) -> bool {
        self.embedded_playing
    }
}

/// 仅常驻游戏模式交接占有作者输入；稳定独立预览和独立World后台运行不设置此门。
/// 该事实和 external_play_active 分离，发送模式请求不等于游戏已确认进入试玩。
#[derive(Resource, Default)]
pub struct EmbeddedAuthorInput {
    pub(super) blocked: bool,
}

impl EmbeddedAuthorInput {
    pub fn set_blocked(&mut self, blocked: bool) {
        self.blocked = blocked;
    }

    pub fn blocked(&self) -> bool {
        self.blocked
    }
}

/// 可选系统参数让原一体调度不依赖独立模式消息，避免未安装插件时缺资源。
#[derive(SystemParam)]
pub(crate) struct StandalonePlayRequest<'w> {
    mode: Option<ResMut<'w, StandaloneEditorMode>>,
    launch: Option<Res<'w, feathrise_editor_core::ProjectLaunchSettings>>,
}

impl StandalonePlayRequest<'_> {
    pub(crate) fn legacy_shortcut_allowed(&self) -> bool {
        // 工程3D预览由明确按钮进入/返回；原编辑器3D和一体编辑器继续保留F12对照入口。
        !self.launch.as_ref().is_some_and(|settings| settings.preview)
    }

    pub(crate) fn embedded_playing(&self) -> bool {
        self.mode.as_deref().is_some_and(StandaloneEditorMode::embedded_playing)
    }

    pub(crate) fn request(&mut self, editor: &mut crate::resources::MapEditorState) -> bool {
        let Some(mode) = self.mode.as_deref_mut() else {
            return false;
        };
        if self.launch.as_ref().is_some_and(|settings| settings.switching_profile) {
            editor.status = "正在更换Debug/Release并收口当前工程实例，请稍后再预览；作者现场保留".into();
            return true;
        }
        mode.pending = !mode.pending;
        mode.waiting_save_sequence = None;
        // 启动外部游戏不能触发旧关闭门，否则会移走本地草稿、暂停或视口相机。
        editor.close_requested = false;
        // 常驻预览的返回只发宿主请求；原工作台始终保留，不借此重开作者会话。
        if !mode.embedded_playing() {
            editor.open = true;
        }
        editor.status = if mode.pending {
            if self.launch.as_ref().is_some_and(|settings| settings.preview) {
                "准备预览当前工程草稿；点击预览悬浮工具条的“返回”继续编辑，不会隐式保存".into()
            } else {
                "准备运行已保存工程；未保存草稿保留在编辑器，不会隐式保存".into()
            }
        } else {
            "已取消等待预览，作者现场保留".into()
        };
        true
    }
}
