//! 植被预设多标签工作区的会话与异步任务状态。
//!
//! 【功能设计】场景视口是永久标签；每个新建或编辑预设拥有独立强类型草稿。
//! 中央工作区只实例化活动标签、当前 LOD 与当前部件的字段，预设和 LOD 总量不设 UI 硬上限。

mod asset_reference; // 单字段资产赋值的冻结目标、子资产游标与请求状态。
mod ui;

pub(super) use asset_reference::*; // 仅供植被预设域使用，不暴露到运行时目录协议。

pub use ui::{PresetWorkspaceTabHost, PresetWorkspaceEditorHost};
pub(crate) use ui::*;

use std::collections::{HashSet, VecDeque}; // GLTF 树按预算遍历，访问集阻止损坏层级形成循环。

use bevy::asset::AssetId; // 节点访问身份不依赖句柄内存地址。
use bevy::gltf::{Gltf, GltfMesh, GltfNode}; // 根资产与分帧节点、网格检查任务。
use bevy::prelude::*; // Component、Handle、Resource、Task 与 Transform。
use bevy::tasks::Task; // 全局目录写入始终占用唯一 IO 任务槽。
use feathrise_scene_content::{MeshPresetPart, VegetationPresetDescriptor}; // 草稿直接复用正式数据格式。

/// 新建或编辑的身份模式；编辑永远冻结原稳定 ID。
#[derive(Clone, Debug)]
pub(super) enum PresetDraftMode {
    Create,
    Edit { original_id: String },
}

/// 一个标签拥有的完整强类型草稿和局部导航状态。
#[derive(Clone, Debug)]
pub(super) struct PresetDraft {
    pub(super) mode: PresetDraftMode,
    pub(super) descriptor: VegetationPresetDescriptor,
    pub(super) baseline: VegetationPresetDescriptor,
    pub(super) section: PresetEditorSection,
    pub(super) selected_lod: usize,
    pub(super) selected_part: usize,
}

impl PresetDraft {
    /// 从正式描述创建一个独立草稿标签。
    pub(super) fn new(mode: PresetDraftMode, descriptor: VegetationPresetDescriptor) -> Self {
        Self {
            baseline: descriptor.clone(), // 脏状态只比较强类型数据，不依赖文本格式。
            descriptor,
            mode,
            section: PresetEditorSection::Basics,
            selected_lod: 0,
            selected_part: 0,
        }
    }

    /// 当前草稿是否包含尚未热采用的内容变化。
    pub(super) fn dirty(&self) -> bool {
        self.descriptor != self.baseline
    }
}

/// 一个可关闭的浏览器式预设标签。
#[derive(Clone, Debug)]
pub(crate) struct PresetEditorTabState {
    pub(crate) id: u64,
    pub(super) draft: PresetDraft,
    pub(super) close_confirmation: bool,
}

/// GLTF 检查完成后可直接写入正式描述的资产事实。
#[derive(Clone, Debug)]
pub(super) struct InspectedAsset {
    pub(super) source_path: String,
    pub(super) blade_mesh_paths: Vec<String>,
    pub(super) blade_error: Option<String>,
    pub(super) mesh_parts: Vec<MeshPresetPart>,
}

/// 一次 GLTF 检查完成后的目标；目标标签和 LOD 在请求时冻结。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum InspectionPurpose {
    CreateBlade,
    CreateMesh,
    ReplaceLod { tab_id: u64, lod: usize },
    AppendMesh { tab_id: u64 },
}

/// 根 GLTF 正在异步加载的强句柄与冻结用途。
#[derive(Clone, Debug)]
pub(super) struct PendingInspection {
    pub(super) handle: Handle<Gltf>,
    pub(super) source_path: String,
    pub(super) purpose: InspectionPurpose,
}

/// GLTF 检查的一个常数工作单元。
#[derive(Clone, Debug)]
pub(super) enum InspectionJob {
    Node {
        handle: Handle<GltfNode>,
        parent: Transform,
    },
    Primitive {
        mesh: Handle<GltfMesh>,
        transform: Transform,
        index: usize,
    },
}

/// 已加载 GLTF 的有界分帧检查状态。
pub(super) struct GltfInspectionWork {
    pub(super) source_path: String,
    pub(super) purpose: InspectionPurpose,
    pub(super) queue: VecDeque<InspectionJob>,
    pub(super) visited_nodes: HashSet<AssetId<GltfNode>>,
    pub(super) blade_mesh_paths: Vec<String>,
    pub(super) blade_error: Option<String>,
    pub(super) mesh_parts: Vec<MeshPresetPart>,
}

/// 全局目录写入成功后的热采用门。
#[derive(Debug)]
pub(super) struct CatalogWriteResult {
    pub(super) baseline_revision: u64,
    pub(super) preset_id: String,
    pub(super) deleted: bool,
    pub(super) tab_id: Option<u64>,
    pub(super) message: String,
}

/// 文件已原子替换、等待 AssetServer 采用完整新目录的状态。
#[derive(Debug)]
pub(super) struct CatalogAdoption {
    pub(super) baseline_revision: u64,
    pub(super) preset_id: String,
    pub(super) deleted: bool,
    pub(super) tab_id: Option<u64>,
    pub(super) message: String,
}

/// F12 全局预设管理的唯一会话资源。
#[derive(Resource)]
pub struct VegetationPresetEditorState {
    pub(crate) tabs: Vec<PresetEditorTabState>,
    pub workspace: feathrise_editor_core::PresetWorkspaceState, // 原会话持有唯一工作区；其他功能只借用共享契约。
    pub(super) next_tab_id: u64,
    pub(super) delete_confirmation_id: Option<String>,
    pub(super) active_field: Option<PresetField>,
    pub(super) input_buffer: String,
    pub(super) pending_inspection: Option<PendingInspection>,
    pub(super) inspection_work: Option<GltfInspectionWork>,
    pub(super) asset_reference: Option<AssetReferenceRequest>, // 同一时刻仅一个字段资产选择/验证，取消不改草稿。
    pub(super) next_reference_ticket: u64,                     // 旧选择按钮不能操作后续新请求。
    pub(super) write_task: Option<Task<Result<CatalogWriteResult, String>>>,
    pub(super) adoption: Option<CatalogAdoption>,
    pub(super) save_feedback_target: Option<crate::types::EditorStatusTarget>,
    pub(super) save_feedback_error: Option<std::sync::Arc<str>>,
    pub(super) status: String,
}

impl Default for VegetationPresetEditorState {
    fn default() -> Self {
        Self {
            tabs: Vec::new(),
            workspace: feathrise_editor_core::PresetWorkspaceState::default(),
            next_tab_id: 1,
            delete_confirmation_id: None,
            active_field: None,
            input_buffer: String::new(),
            pending_inspection: None,
            inspection_work: None,
            asset_reference: None,    // 首次打开没有待赋值目标。
            next_reference_ticket: 1, // 零号保留为未初始化身份。
            write_task: None,
            adoption: None,
            save_feedback_target: None,
            save_feedback_error: None,
            status: String::new(), // 初始教程归入分组帮助，底栏只接收实际发生的操作反馈。
        }
    }
}

impl VegetationPresetEditorState {
    /// 已保存试玩只等待实际文件提交/采用，不因草稿脏位、只读模型检查或字段焦点阻止启动。
    pub(crate) fn saved_play_pending(&self) -> bool {
        self.write_task.is_some() || self.adoption.is_some()
    }

    /// 原生窗口退出只读全部标签；切到场景页不能遗漏停放的预设草稿。
    pub(crate) fn exit_blocker(&self) -> Option<&'static str> {
        if self.busy() { return Some("植被预设的检查、写入或热采用尚未完成"); }
        if self.active_field.is_some() { return Some("植被预设字段尚未完成，请先Enter提交或Esc取消"); }
        self.tabs.iter().any(|tab| matches!(tab.draft.mode, PresetDraftMode::Create) || tab.draft.dirty())
            .then_some("存在未保存的植被预设，请先保存或使用原标签关闭确认丢弃")
    }

    /// 仅描述目录保存的原任务/采用阶段，模型检查和预设删除不冒充保存。
    pub(crate) fn save_feedback_task(&self) -> Option<String> {
        let target = self.save_feedback_target.as_ref()?;
        let phase = if self.write_task.is_some() {
            "后台处理中"
        } else if self.adoption.is_some() {
            "等待运行时目录采用"
        } else {
            return None;
        };
        Some(format!(
            "植被预设“{}” [{}] · {phase}",
            target.label, target.id
        ))
    }

    pub(super) fn record_save_warning(&self, editor: &mut crate::resources::SceneEditorState) {
        if let Some(draft) = self.active_draft() {
            let target = crate::types::EditorStatusTarget::new(
                &draft.descriptor.id,
                &draft.descriptor.label,
            );
            editor.record_operation_result(
                crate::types::EditorStatusOperation::VegetationPresetSave,
                &target,
                crate::types::EditorStatusKind::Warning,
            );
        }
    }

    pub(crate) fn save_feedback_failure(
        &self,
    ) -> Option<(&crate::types::EditorStatusTarget, &str)> {
        self.adoption.as_ref()?;
        Some((
            self.save_feedback_target.as_ref()?,
            self.save_feedback_error.as_deref()?,
        ))
    }

    pub(super) fn observe_save_adoption_error(
        &mut self,
        editor: &mut crate::resources::SceneEditorState,
        error: Option<&str>,
    ) {
        let Some(error) = error else {
            self.save_feedback_error = None;
            return;
        };
        if let Some(target) = self.save_feedback_target.as_ref()
            && self.save_feedback_error.as_deref() != Some(error)
        {
            self.save_feedback_error = Some(std::sync::Arc::from(error));
            editor.record_operation_feedback(
                crate::types::EditorStatusOperation::VegetationPresetSave,
                target,
                crate::types::EditorStatusKind::Error,
                std::sync::Arc::from(format!(
                    "文件已发布，但运行时目录采用失败：{error}；原采用门仍在等待"
                )),
            );
        }
    }

    /// 快捷键系统用此判断字符是否属于预设输入框。
    pub(crate) fn text_input_active(&self) -> bool {
        self.active_field.is_some()
    }

    /// 目录写入、热采用和 GLTF 检查期间拒绝覆盖另一个请求。
    pub(super) fn busy(&self) -> bool {
        self.pending_inspection.is_some()
            || self.inspection_work.is_some()
            || self.asset_reference.is_some() // 选择期间冻结草稿结构，防止LOD和部件下标漂移。
            || self.write_task.is_some()
            || self.adoption.is_some()
    }

    /// 中央文档还原前只检查打开标签，不读取或修改预设内容。
    pub(crate) fn has_tab(&self, id: u64) -> bool {
        self.tabs.iter().any(|tab| tab.id == id)
    }

    /// 返回活动预设标签 ID。
    pub(super) const fn active_preset_tab_id(&self) -> Option<u64> {
        match self.workspace.active_tab {
            PresetWorkspaceTab::Scene | PresetWorkspaceTab::Document | PresetWorkspaceTab::AssetPreview => None,
            PresetWorkspaceTab::Vegetation(id) => Some(id),
            PresetWorkspaceTab::Surface(_)
            | PresetWorkspaceTab::Water(_)
            | PresetWorkspaceTab::Building(_)
            | PresetWorkspaceTab::Domain(_, _) => None, // 应用作者域拥有独立会话。
        }
    }

    /// 返回活动预设草稿。
    pub(super) fn active_draft(&self) -> Option<&PresetDraft> {
        let id = self.active_preset_tab_id()?;
        self.tabs
            .iter()
            .find(|tab| tab.id == id)
            .map(|tab| &tab.draft)
    }

    /// 返回活动预设草稿的可变引用。
    pub(super) fn active_draft_mut(&mut self) -> Option<&mut PresetDraft> {
        let id = self.active_preset_tab_id()?;
        self.tabs
            .iter_mut()
            .find(|tab| tab.id == id)
            .map(|tab| &mut tab.draft)
    }

    /// 按标签 ID 返回草稿。
    pub(super) fn draft_mut(&mut self, tab_id: u64) -> Option<&mut PresetDraft> {
        self.tabs
            .iter_mut()
            .find(|tab| tab.id == tab_id)
            .map(|tab| &mut tab.draft)
    }

    /// 打开一个新草稿；编辑同一稳定预设时直接聚焦已有标签。
    pub(super) fn open_draft(&mut self, draft: PresetDraft) -> u64 {
        if let PresetDraftMode::Edit { original_id } = &draft.mode
            && let Some(tab_id) = self
                .tabs
                .iter()
                .find(|tab| {
                    matches!(&tab.draft.mode, PresetDraftMode::Edit { original_id: current } if current == original_id)
                })
                .map(|tab| tab.id)
        {
            self.active_tab = PresetWorkspaceTab::Vegetation(tab_id);
            self.touch_ui();
            return tab_id;
        }
        let id = self.next_tab_id;
        self.next_tab_id = self.next_tab_id.wrapping_add(1).max(1);
        self.tabs.push(PresetEditorTabState {
            id,
            draft,
            close_confirmation: false,
        });
        self.active_tab = PresetWorkspaceTab::Vegetation(id);
        self.touch_ui();
        id
    }
}

// 保留旧会话的字段访问路径与 Bevy 变更检测；共享状态仅在此处存一份。
impl std::ops::Deref for VegetationPresetEditorState {
    type Target = feathrise_editor_core::PresetWorkspaceState;
    fn deref(&self) -> &Self::Target {
        &self.workspace
    }
}

impl std::ops::DerefMut for VegetationPresetEditorState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.workspace
    }
}
