//! 工程关闭的轻量只读摘要和保存调度合同；不持有任何草稿正文。

use bevy::prelude::*;
use feathrise_scene_content::SceneId;
use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectCloseSnapshot {
    /// 后台事务仍有真实所有者，允许继续非阻塞等待。
    pub pending: Option<String>,
    /// 包含后台标签和已关闭标签仍保留的草稿。
    pub unsaved: Vec<String>,
    /// 需要返回原编辑器处理的输入/失败，不能用模态等待代替作者决定。
    pub blocked: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ProjectSaveProgress {
    #[default]
    Idle,
    Running {
        current: String,
    },
    Succeeded,
    Failed {
        detail: String,
    },
}

/// 外部作者只发布轻量文档身份，不把领域模型、草稿正文或历史带入场景库。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectAuthorDocument {
    pub id: u64,
    pub label: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectAuthorSnapshot {
    pub pending: Option<String>,
    pub blocked: Option<String>,
    pub unsaved: Vec<ProjectAuthorDocument>,
}

/// 仅装配时登记已有作者事务入口；不负责插件发现、加载或领域文件格式。
#[derive(Clone, Copy)]
pub struct ProjectAuthorAdapter {
    pub domain: &'static str,
    pub order: u8,
    pub collect: fn(&World) -> ProjectAuthorSnapshot,
    pub saved_play_blocker: fn(&World) -> Option<String>,
    pub request: fn(&mut World, u64) -> Result<(), String>,
    pub poll: fn(&World, u64) -> Result<bool, String>,
    pub catalog_hash: fn(&World, u64) -> Option<String>,
    pub rebase_own_write: fn(&mut World, u64, Option<&str>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SaveTarget {
    Documents,
    Animation(PathBuf),
    Weights(PathBuf),
    Domain(&'static str, u64),
    Surface(u64),
    Vegetation(u64),
    Water(u64),
    Building(u64),
    Scene(SceneId),
    GlobalEnvironment,
}

#[derive(Clone, Debug)]
pub(crate) struct SaveItem {
    pub target: SaveTarget,
    pub label: String,
}

impl ProjectCloseSnapshot {
    pub(crate) fn pending(&mut self, detail: impl Into<String>) {
        self.pending.get_or_insert_with(|| detail.into());
    }

    pub(crate) fn blocked(&mut self, detail: impl Into<String>) {
        self.blocked.get_or_insert_with(|| detail.into());
    }

    pub(crate) fn draft(
        &mut self,
        items: &mut Vec<SaveItem>,
        target: SaveTarget,
        label: impl Into<String>,
    ) {
        let label = label.into();
        self.unsaved.push(label.clone());
        items.push(SaveItem { target, label });
    }
}
