//! 水体预设草稿、字段路径、按钮和后台事务类型。

use bevy::prelude::*;
use bevy::tasks::Task;
use feathrise_map_content::WaterPresetDescriptor;

use crate::ui::vegetation_preset_editor::PresetWorkspaceTab;

#[derive(Clone, Debug)]
pub(super) enum WaterDraftMode {
    Create,
    Edit { original_id: String },
}

#[derive(Clone, Debug)]
pub(super) struct WaterDraft {
    pub(super) mode: WaterDraftMode,
    pub(super) descriptor: WaterPresetDescriptor,
    pub(super) baseline: WaterPresetDescriptor,
}

impl WaterDraft {
    pub(super) fn new(mode: WaterDraftMode, descriptor: WaterPresetDescriptor) -> Self {
        Self {
            baseline: descriptor.clone(),
            descriptor,
            mode,
        }
    }

    pub(super) fn dirty(&self) -> bool {
        self.descriptor != self.baseline
    }
}

#[derive(Clone, Debug)]
pub(super) struct WaterTab {
    pub(super) id: u64,
    pub(super) draft: WaterDraft,
    pub(super) section: WaterSection, // 每份文档独立记忆分区，不进入作者描述或脏标记。
    pub(super) close_confirmation: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum WaterSection {
    #[default]
    Appearance,
    Waves,
    Normals,
    Material,
}

/// 中央表单中的一个可编辑标量或名称。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WaterPresetField {
    Label,
    BaseColor(usize),
    Roughness,
    Reflectance,
    TroughDarkening,
    ShoreWaveAttenuation,
    WaveDirection { wave: usize, component: usize },
    WaveAmplitude(usize),
    WaveWavelength(usize),
    WaveSpeed(usize),
    WaveSteepness(usize),
    NormalTile(usize),
    NormalVelocity { layer: usize, component: usize },
    NormalStrength(usize),
}

impl WaterPresetField {
    pub(super) const fn accepts_text(self) -> bool {
        matches!(self, Self::Label)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WaterPresetAction {
    SelectPreset(usize),
    Create,
    EditSelected,
    DeleteSelected,
    ApplyToSelectedStillWater,
    ActivateTab(u64),
    CloseTab(u64),
    Section(WaterSection),
    Focus(WaterPresetField),
    ApplySelectedAsset(usize),
    ClearTexture(usize),
    Save,
    Cancel,
}

#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct WaterPresetButton(pub(crate) WaterPresetAction);

impl Default for WaterPresetButton {
    fn default() -> Self {
        Self(WaterPresetAction::Create)
    }
}

#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct WaterPresetList;

#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct WaterPresetListTitle;

#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct WaterPresetDynamicListRoot;

#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct WaterPresetSummaryText;

#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct WaterPresetStatusText;

#[derive(Debug)]
pub(super) struct WaterCatalogWriteResult {
    pub(super) baseline_revision: u64,
    pub(super) preset_id: String,
    pub(super) deleted: bool,
    pub(super) tab_id: Option<u64>,
    pub(super) message: String,
}

#[derive(Debug)]
pub(super) struct WaterCatalogAdoption {
    pub(super) baseline_revision: u64,
    pub(super) preset_id: String,
    pub(super) deleted: bool,
    pub(super) tab_id: Option<u64>,
    pub(super) message: String,
}

#[derive(Resource)]
pub(crate) struct WaterPresetEditorState {
    pub(super) tabs: Vec<WaterTab>,
    pub(super) next_tab_id: u64,
    pub(super) ui_revision: u64,
    pub(super) delete_confirmation_id: Option<String>,
    pub(super) active_field: Option<WaterPresetField>,
    pub(super) input_buffer: String,
    pub(super) delete_preflight: Option<super::persistence::WaterDeletePreflight>,
    pub(super) write_task: Option<Task<Result<WaterCatalogWriteResult, String>>>,
    pub(super) adoption: Option<WaterCatalogAdoption>,
    pub(super) save_feedback_target: Option<crate::types::EditorStatusTarget>,
    pub(super) save_feedback_error: Option<std::sync::Arc<str>>,
    pub(super) status: String,
}

impl Default for WaterPresetEditorState {
    fn default() -> Self {
        Self {
            tabs: Vec::new(),
            next_tab_id: 1,
            ui_revision: 1,
            delete_confirmation_id: None,
            active_field: None,
            input_buffer: String::new(),
            delete_preflight: None,
            write_task: None,
            adoption: None,
            save_feedback_target: None,
            save_feedback_error: None,
            status: String::new(), // 预设用途归入分组帮助，初始状态不制造底栏操作通知。
        }
    }
}

impl WaterPresetEditorState {
    /// 原文件事务与完整目录采用仍在进行时，试玩不能读到上次保存的数据。
    pub(crate) fn saved_play_pending(&self) -> bool {
        self.busy()
    }

    /// 检查全部原标签，而不是只检查当前中央页；不提交字段、不重写保存流程。
    pub(crate) fn exit_blocker(&self) -> Option<&'static str> {
        if self.busy() { return Some("水体预设写入或热采用尚未完成"); }
        if self.active_field.is_some() { return Some("水体预设字段尚未完成，请先Enter提交或Esc取消"); }
        self.tabs.iter().any(|tab| matches!(tab.draft.mode, WaterDraftMode::Create) || tab.draft.dirty())
            .then_some("存在未保存的水体预设，请先保存或使用原标签关闭确认丢弃")
    }

    /// 保存的反馈对象与原single-flight槽同寿命，不借用当前活动标签猜测对象。
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
            "水体预设“{}” [{}] · {phase}",
            target.label, target.id
        ))
    }

    pub(super) fn record_save_warning(
        &self,
        editor: &mut crate::resources::MapEditorState,
        active: PresetWorkspaceTab,
    ) {
        if let Some(draft) = self.active_draft(active) {
            let target = crate::types::EditorStatusTarget::new(
                &draft.descriptor.id,
                &draft.descriptor.label,
            );
            editor.record_operation_result(
                crate::types::EditorStatusOperation::WaterPresetSave,
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
        editor: &mut crate::resources::MapEditorState,
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
                crate::types::EditorStatusOperation::WaterPresetSave,
                target,
                crate::types::EditorStatusKind::Error,
                std::sync::Arc::from(format!(
                    "文件已发布，但运行时目录采用失败：{error}；原采用门仍在等待"
                )),
            );
        }
    }

    /// 中央文档还原前只检查打开标签，不读取或修改预设内容。
    pub(crate) fn has_tab(&self, id: u64) -> bool {
        self.tabs.iter().any(|tab| tab.id == id)
    }

    pub(crate) fn text_input_active(&self) -> bool {
        self.active_field.is_some()
    }

    pub(super) fn busy(&self) -> bool {
        self.delete_preflight.is_some() || self.write_task.is_some() || self.adoption.is_some()
    }

    pub(super) fn active_draft(&self, active: PresetWorkspaceTab) -> Option<&WaterDraft> {
        let PresetWorkspaceTab::Water(id) = active else {
            return None;
        };
        self.tabs
            .iter()
            .find(|tab| tab.id == id)
            .map(|tab| &tab.draft)
    }

    pub(super) fn active_draft_mut(
        &mut self,
        active: PresetWorkspaceTab,
    ) -> Option<&mut WaterDraft> {
        let PresetWorkspaceTab::Water(id) = active else {
            return None;
        };
        self.tabs
            .iter_mut()
            .find(|tab| tab.id == id)
            .map(|tab| &mut tab.draft)
    }

    pub(super) fn draft_mut(&mut self, tab_id: u64) -> Option<&mut WaterDraft> {
        self.tabs
            .iter_mut()
            .find(|tab| tab.id == tab_id)
            .map(|tab| &mut tab.draft)
    }

    pub(super) fn open_draft(&mut self, draft: WaterDraft) -> u64 {
        if let WaterDraftMode::Edit { original_id } = &draft.mode
            && let Some(id) = self
                .tabs
                .iter()
                .find(|tab| {
                    matches!(&tab.draft.mode, WaterDraftMode::Edit { original_id: current } if current == original_id)
                })
                .map(|tab| tab.id)
        {
            return id;
        }
        let id = self.next_tab_id;
        self.next_tab_id = self.next_tab_id.wrapping_add(1).max(1);
        self.tabs.push(WaterTab {
            id,
            draft,
            section: WaterSection::default(),
            close_confirmation: false,
        });
        self.touch_ui();
        id
    }

    pub(super) fn touch_ui(&mut self) {
        self.ui_revision = self.ui_revision.wrapping_add(1).max(1);
    }
}
