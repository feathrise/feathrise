//! 每个工程只有一份资料草稿和一个后台请求。标签换页不会丢弃这份草稿。

use crate::{
    folder_picker::FolderPicker,
    project::{
        Project,
        metadata::{IconEdit, MetadataSnapshot},
    },
};
use bevy::{prelude::*, tasks::Task};
use feathrise_editor_core::PresetWorkspaceTab;
use std::path::PathBuf;

#[derive(Resource, Default)]
pub(crate) struct ProjectSettingsPresentation {
    pub(crate) icon: Option<Handle<Image>>,
}

#[derive(Default)]
pub(super) struct IconPicker(pub Option<FolderPicker>);

pub(super) struct Loaded {
    pub snapshot: MetadataSnapshot,
    pub icon: Result<Option<Image>, String>,
}

pub(super) enum Work {
    Read(Task<Result<Loaded, String>>),
    Preview(Task<Result<Image, String>>, PathBuf),
    Save(Task<Result<(Project, Loaded), String>>),
}

#[derive(Resource)]
pub(super) struct ProjectSettingsState {
    pub project: Project,
    pub snapshot: Option<MetadataSnapshot>,
    pub name: String,
    pub description: String,
    pub icon: IconEdit,
    pub preview: Option<Handle<Image>>,
    pub work: Option<Work>,
    pub open: bool,
    pub return_tab: PresetWorkspaceTab,
    pub confirm_close: bool,
    pub allow_reload: bool,
    pub status: String,
    pub fields_revision: u64,
}

impl ProjectSettingsState {
    pub fn new(project: Project) -> Self {
        Self {
            project,
            snapshot: None,
            name: String::new(),
            description: String::new(),
            icon: IconEdit::Keep,
            preview: None,
            work: None,
            open: false,
            return_tab: PresetWorkspaceTab::Scene,
            confirm_close: false,
            allow_reload: false,
            status: String::new(),
            fields_revision: 0,
        }
    }

    pub fn dirty(&self) -> bool {
        self.snapshot.as_ref().is_some_and(|snapshot| {
            self.name != snapshot.metadata.name
                || self.description != snapshot.metadata.description
                || !matches!(self.icon, IconEdit::Keep)
        })
    }

    pub fn busy(&self, picker: &IconPicker) -> bool {
        self.work.is_some() || picker.0.is_some()
    }
}
