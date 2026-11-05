//! 多文件文档标签与唯一IO/解析槽；只有活动文件占用正文，草稿随F12关闭保留。

use super::types::*;
use bevy::{prelude::*, tasks::Task};
use std::path::PathBuf;

#[derive(Resource)]
pub(crate) struct DocumentState {
    pub(super) root: PathBuf,
    pub(super) project_root: Option<PathBuf>, // 显式工程来源终生固定；不能从assets父目录猜测。
    pub(super) active: Option<String>,
    pub(super) expanded: Option<ExpandedDocument>,
    pub(super) parked: Vec<ExpandedDocument>, // 后台标签只固定身份，不能被会话缓存淘汰。
    pub(super) next_tab: u64,
    pub(super) center_activation: u64, // 显式打开同一文件也换激活票据，中央工具预览须让位给这次操作。
    pub(super) central_active: bool,
    pub(super) documents: Vec<Document>,
    pub(super) pending_load: Option<LoadRequest>,
    pub(super) loading: Option<LoadRequest>, // 唯一IO槽中真实读取的目标；保存和外部核对不能误禁编辑。
    pub(super) load_ticket: u64,
    pub(super) io: Option<Task<IoResult>>,
    pub(super) saving: Option<String>,
    pub(super) saving_batch: Vec<SourceKey>, // 保存期间撤销到干净状态也不能淘汰本批会话。
    pub(super) parser: Option<Task<PreviewResult>>,
    pub(super) json: super::json::JsonTools,
    pub(super) visible: bool,
    pub(super) wheel_owned: bool,
    pub(super) error: String,
    pub(super) external: super::external::ExternalCheckState,
}

impl Default for DocumentState {
    fn default() -> Self {
        Self {
            root: feathrise_editor_core::workspace_asset_root(), // 全文草稿、搜索和保存终生绑定当前工程。
            project_root: feathrise_editor_core::editor_project_paths().map(|paths| paths.project_root().to_path_buf()),
            active: None,
            expanded: None,
            parked: Vec::new(),
            next_tab: 0,
            center_activation: 0,
            central_active: false,
            documents: Vec::new(),
            pending_load: None,
            loading: None,
            load_ticket: 0,
            io: None,
            saving: None,
            saving_batch: Vec::new(),
            parser: None,
            json: default(),
            visible: false,
            wheel_owned: false,
            error: String::new(),
            external: default(),
        }
    }
}

impl DocumentState {
    /// 只冻结仍可能采用的重载目标；切标签失效的旧票据不阻止其他会话继续编辑。
    pub(super) fn is_loading(&self, path: &str) -> bool {
        self.pending_load.iter().chain(self.loading.iter())
            .any(|request| request.ticket == self.load_ticket && request.path == path)
    }

    /// 只等待显式写入和保存前校验，不把只读加载/语法诊断或未保存草稿当作保存。
    pub(crate) fn saved_play_pending(&self) -> bool {
        self.saving.is_some() || !self.saving_batch.is_empty() || self.json.saved_play_pending()
    }

    /// 关闭中央标签不会删除缓存的脏正文；退出必须检查全部原会话而非仅可见标签。
    pub(crate) fn exit_blocker(&self) -> Option<&'static str> {
        if self.io.is_some() || self.saving.is_some() || !self.saving_batch.is_empty() || self.json.exit_pending() {
            return Some("文件文档保存或格式化事务尚未完成");
        }
        self.documents.iter().any(|document| document.dirty())
            .then_some("存在未保存的文件文档（包括已关闭标签的草稿），请用原文档全部保存或丢弃操作收口")
    }
}

/// 非活动文档的原生编辑状态移出ECS暂存；切标签只移动所有权，不复制全文或新增输入节点。
#[derive(Resource, Default)]
pub(super) struct DocumentInputs {
    pub sessions: Vec<NativeDocumentInput>,
}

/// 完整预览只有一个采用树、一个构造树与一个退役树，快速切文件不会无限排队。
#[derive(Resource, Default)]
pub(super) struct PreviewRenderState {
    pub active: Option<RenderedPreview>,
    pub building: Option<PreviewBuild>,
    pub retiring: Option<Entity>,
}

/// 唯一代码小地图：一个有界快照/后台槽和当前纹理；失效任务完成前仍占槽。
#[derive(Resource, Default)]
pub(crate) struct DocumentMinimapState {
    pub(super) picture: Option<MinimapPicture>,
    pub(super) collecting: Option<MinimapCollection>,
    pub(super) task: Option<Task<MinimapResult>>,
    pub(super) rejected: Option<MinimapKey>,
    pub(super) shown: bool,
    pub(super) drag: Option<MinimapDrag>,
    pub(super) mouse_owned: bool,
    pub(super) wheel_owned: bool,
}

impl DocumentMinimapState {
    pub(crate) fn owns_mouse(&self) -> bool {
        self.mouse_owned || self.drag.is_some()
    }
    pub(crate) fn owns_wheel(&self) -> bool {
        self.wheel_owned
    }
}
