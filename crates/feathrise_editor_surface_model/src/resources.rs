//! 地表预设唯一会话、任务槽及原采用回执；宿主与实现借用同一资源。
use super::types::*;
use bevy::{prelude::*, tasks::Task};
use feathrise_editor_core::PresetWorkspaceTab;

#[derive(Resource)]
pub struct SurfacePresetEditorState {
    pub tabs: Vec<SurfaceTab>,
    pub next_tab_id: u64,
    pub ui_revision: u64,
    pub delete_confirmation_id: Option<String>,
    pub active_field: Option<SurfacePresetField>,
    pub input_buffer: String,
    pub write_task: Option<Task<Result<SurfaceCatalogWriteResult, String>>>,
    pub adoption: Option<SurfaceCatalogAdoption>,
    pub save_feedback_target: Option<feathrise_editor_core::EditorStatusTarget>,
    pub save_feedback_error: Option<std::sync::Arc<str>>,
    pub status: String,
}

impl Default for SurfacePresetEditorState {
    fn default() -> Self {
        Self {
            tabs: Vec::new(),
            next_tab_id: 1,
            ui_revision: 1,
            delete_confirmation_id: None,
            active_field: None,
            input_buffer: String::new(),
            write_task: None,
            adoption: None,
            save_feedback_target: None,
            save_feedback_error: None,
            status: "选择预设后显式新建地图层；新建或编辑预设在中央标签完成".to_owned(),
        }
    }
}

impl SurfacePresetEditorState {
    /// 只读原任务和采用门；冻结对象不随标签切换变化，也不把落盘当作最终成功。
    pub fn save_feedback_task(&self) -> Option<String> {
        let target = self.save_feedback_target.as_ref()?;
        let phase = if self.write_task.is_some() {
            "后台处理中"
        } else if self.adoption.is_some() {
            "等待运行时目录采用"
        } else {
            return None;
        };
        Some(format!(
            "地表预设“{}” [{}] · {phase}",
            target.label, target.id
        ))
    }

    pub fn record_save_warning(
        &self,
        editor: &mut dyn crate::host::SurfaceEditorHost,
        active: PresetWorkspaceTab,
    ) {
        if let Some(draft) = self.active_draft(active) {
            let target = feathrise_editor_core::EditorStatusTarget::new(
                &draft.descriptor.id,
                &draft.descriptor.label,
            );
            editor.record_operation_result(
                feathrise_editor_core::EditorStatusOperation::SurfacePresetSave,
                &target,
                feathrise_editor_core::EditorStatusKind::Warning,
            );
        }
    }

    pub fn save_feedback_failure(&self) -> Option<(&feathrise_editor_core::EditorStatusTarget, &str)> {
        self.adoption.as_ref()?;
        Some((
            self.save_feedback_target.as_ref()?,
            self.save_feedback_error.as_deref()?,
        ))
    }

    pub fn observe_save_adoption_error(
        &mut self,
        editor: &mut dyn crate::host::SurfaceEditorHost,
        error: Option<&str>,
    ) {
        let Some(error) = error else {
            self.save_feedback_error = None; // 只更新仍在等待的故障证据，不清除未读回执或解除采用门。
            return;
        };
        if let Some(target) = self.save_feedback_target.as_ref()
            && self.save_feedback_error.as_deref() != Some(error)
        {
            self.save_feedback_error = Some(std::sync::Arc::from(error));
            editor.record_operation_feedback(
                feathrise_editor_core::EditorStatusOperation::SurfacePresetSave,
                target,
                feathrise_editor_core::EditorStatusKind::Error,
                std::sync::Arc::from(format!(
                    "文件已发布，但运行时目录采用失败：{error}；原采用门仍在等待"
                )),
            );
        }
    }

    /// 中央文档还原前只检查打开标签，不读取或修改预设内容。
    pub fn has_tab(&self, id: u64) -> bool {
        self.tabs.iter().any(|tab| tab.id == id)
    }

    pub fn text_input_active(&self) -> bool {
        self.active_field.is_some()
    }

    pub fn busy(&self) -> bool {
        self.write_task.is_some() || self.adoption.is_some()
    }

    pub fn active_draft(&self, active: PresetWorkspaceTab) -> Option<&SurfaceDraft> {
        let PresetWorkspaceTab::Surface(id) = active else {
            return None;
        };
        self.tabs
            .iter()
            .find(|tab| tab.id == id)
            .map(|tab| &tab.draft)
    }

    pub fn active_draft_mut(&mut self, active: PresetWorkspaceTab) -> Option<&mut SurfaceDraft> {
        let PresetWorkspaceTab::Surface(id) = active else {
            return None;
        };
        self.tabs
            .iter_mut()
            .find(|tab| tab.id == id)
            .map(|tab| &mut tab.draft)
    }

    pub fn draft_mut(&mut self, tab_id: u64) -> Option<&mut SurfaceDraft> {
        self.tabs
            .iter_mut()
            .find(|tab| tab.id == tab_id)
            .map(|tab| &mut tab.draft)
    }

    /// 编辑同一稳定 ID 时聚焦已有标签；新建标签身份仅在本次 F12 会话内使用。
    pub fn open_draft(&mut self, draft: SurfaceDraft) -> u64 {
        if let SurfaceDraftMode::Edit { original_id } = &draft.mode
            && let Some(id) = self
                .tabs
                .iter()
                .find(|tab| {
                    matches!(&tab.draft.mode, SurfaceDraftMode::Edit { original_id: current } if current == original_id)
                })
                .map(|tab| tab.id)
        {
            return id;
        }
        let id = self.next_tab_id;
        self.next_tab_id = self.next_tab_id.wrapping_add(1).max(1);
        self.tabs.push(SurfaceTab {
            id,
            draft,
            section: SurfaceSection::default(),
            close_confirmation: false,
        });
        self.touch_ui();
        id
    }

    pub fn touch_ui(&mut self) {
        self.ui_revision = self.ui_revision.wrapping_add(1).max(1);
    }
}
