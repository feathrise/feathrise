//! 根应用并列安装场景和动画，两边只通过原调度阶段、场景工厂和显式操作交接。

use crate::{request::PreviewRigAnimation, resources::AnimationEditorState};
use bevy::prelude::*;
use std::path::{Path, PathBuf};

/// 动画实现构造 BSN，宿主只拿已装箱的布局并挂到原有实体。
#[derive(Resource, Clone, Copy)]
pub struct AnimationScenes {
    pub pose_toolbar: fn() -> Box<dyn Scene>,
    pub inline_panel: fn() -> Box<dyn Scene>,
    pub timeline_dock: fn() -> Box<dyn Scene>,
    pub properties_host: fn() -> Box<dyn Scene>,
    pub binding_properties_host: fn() -> Box<dyn Scene>,
}

/// 仅在同帧确认动作来源和关工程保存时调用正式实现。
#[derive(Resource, Clone, Copy)]
pub struct AnimationOperations {
    pub select: fn(&mut AnimationEditorState, PreviewRigAnimation, PathBuf),
    pub save: fn(&mut AnimationEditorState, &Path) -> Result<(), String>,
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnimationSystems {
    Input,
    Drive,
    Preview,
    Presentation,
}
