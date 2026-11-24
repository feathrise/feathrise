//! 控制工程资料会话。输入读取正式文本字段，清单、图片和选择窗都通过非阻塞回执采用。

use super::{
    OpenProjectSettings, ProjectSettingsSaved, TAB,
    components::{Action, Field},
    resources::{IconPicker, Loaded, ProjectSettingsPresentation, ProjectSettingsState, Work},
};
use crate::{
    folder_picker::FolderPicker,
    native_project_close::NativeProjectCloseState,
    project::{
        Project,
        metadata::{self, Edit, IconEdit, MetadataSnapshot},
    },
};
use bevy::{
    input_focus::InputFocus,
    prelude::*,
    tasks::{IoTaskPool, futures::check_ready},
    text::EditableText,
    ui::InteractionDisabled,
    window::{PrimaryWindow, RawHandleWrapper},
};
use feathrise_editor_core::{AssetPreviewFocus, PresetWorkspaceState, PresetWorkspaceTab, WorkspaceDomainState};
use feathrise_editor_scene::game_authoring::scene_api::ui::vegetation_preset_editor::VegetationPresetEditorState;
use feathrise_editor_scene::standalone::{
    ProjectAuthorAdapter, ProjectAuthorAdapters, ProjectAuthorDocument, ProjectAuthorSnapshot,
};
use feathrise_editor_ui::EditorButtonClick;
use std::path::PathBuf;

pub(super) fn install_save_adapter(app: &mut App) {
    app.init_resource::<ProjectAuthorAdapters>();
    app.world_mut()
        .resource_mut::<ProjectAuthorAdapters>()
        .register(ProjectAuthorAdapter {
            domain: "feathrise.project-settings",
            order: 10,
            collect: |world| {
                let state = world.resource::<ProjectSettingsState>();
                let picker_open = world.get_non_send::<IconPicker>().is_some_and(|picker| picker.0.is_some());
                ProjectAuthorSnapshot {
                    pending: (state.work.is_some() || picker_open).then(|| "正在处理工程设置".into()),
                    unsaved: state
                        .dirty()
                        .then(|| ProjectAuthorDocument {
                            id: 0,
                            label: super::TITLE.into(),
                        })
                        .into_iter()
                        .collect(),
                    blocked: None,
                }
            },
            saved_play_blocker: |_| None,
            request: |world, _| {
                if world.get_non_send::<IconPicker>().is_some_and(|picker| picker.0.is_some()) {
                    return Err("请先完成工程图标选择".into());
                }
                begin_save(&mut world.resource_mut::<ProjectSettingsState>())
            },
            poll: |world, _| {
                let state = world.resource::<ProjectSettingsState>();
                if matches!(state.work, Some(Work::Save(_))) {
                    return Ok(false);
                }
                if state.dirty() {
                    return Err(state.status.clone());
                }
                Ok(true)
            },
            catalog_hash: |_, _| None,
            rebase_own_write: |_, _, _| {},
        })
        .expect("工程设置保存域只安装一次");
}

fn read_icon(project: &Project, snapshot: MetadataSnapshot) -> Loaded {
    let icon = snapshot
        .metadata
        .icon
        .as_deref()
        .map(|path| {
            metadata::saved_icon_path(project, path).and_then(|path| metadata::preview_icon(&path))
        })
        .transpose();
    Loaded { snapshot, icon }
}

pub(super) fn begin_load(mut state: ResMut<ProjectSettingsState>) {
    start_load(&mut state);
}

fn start_load(state: &mut ProjectSettingsState) {
    let project = state.project.clone();
    state.status = "正在读取工程设置…".into();
    state.allow_reload = false;
    state.work = Some(Work::Read(IoTaskPool::get().spawn(async move {
        metadata::read(&project).map(|snapshot| read_icon(&project, snapshot))
    })));
}

fn begin_save(state: &mut ProjectSettingsState) -> Result<(), String> {
    if state.work.is_some() {
        return Err("仍在处理工程设置".into());
    }
    let snapshot = state.snapshot.clone().ok_or("尚未读取工程设置")?;
    let edit = Edit {
        name: state.name.clone(),
        description: state.description.clone(),
        icon: state.icon.clone(),
    };
    let project = state.project.clone();
    state.confirm_close = false;
    state.status = "正在保存工程设置…".into();
    state.work = Some(Work::Save(IoTaskPool::get().spawn(async move {
        let (project, snapshot) = metadata::save(&project, &snapshot, edit)?;
        let loaded = read_icon(&project, snapshot);
        Ok((project, loaded))
    })));
    Ok(())
}

fn restore_fields(state: &mut ProjectSettingsState, icon: Option<Handle<Image>>) {
    if let Some(saved) = &state.snapshot {
        state.name.clone_from(&saved.metadata.name);
        state.description.clone_from(&saved.metadata.description);
    }
    state.icon = IconEdit::Keep;
    state.preview = icon;
    state.confirm_close = false;
    state.fields_revision = state.fields_revision.wrapping_add(1);
}

fn adopt(
    state: &mut ProjectSettingsState,
    presentation: &mut ProjectSettingsPresentation,
    images: &mut Assets<Image>,
    loaded: Loaded,
    success: &'static str,
) {
    state.snapshot = Some(loaded.snapshot);
    state.status = success.into();
    state.allow_reload = false;
    presentation.icon = match loaded.icon {
        Ok(image) => image.map(|image| images.add(image)),
        Err(error) => {
            state.status = error;
            None
        }
    };
    restore_fields(state, presentation.icon.clone());
}

enum Ready {
    Read(Result<Loaded, String>),
    Preview(Result<Image, String>, PathBuf),
    Save(Result<(Project, Loaded), String>),
}

pub(super) fn poll(
    mut state: ResMut<ProjectSettingsState>,
    mut presentation: ResMut<ProjectSettingsPresentation>,
    mut picker: NonSendMut<IconPicker>,
    mut images: ResMut<Assets<Image>>,
    mut saved: MessageWriter<ProjectSettingsSaved>,
) {
    if state.work.is_none()
        && let Some(result) = picker.0.as_mut().and_then(FolderPicker::poll)
    {
        picker.0 = None;
        match result {
            Ok(Some(path)) => {
                let path = PathBuf::from(path);
                let input = path.clone();
                state.status = "正在读取图标…".into();
                state.work = Some(Work::Preview(
                    IoTaskPool::get().spawn(async move { metadata::preview_icon(&input) }),
                    path,
                ));
            }
            Ok(None) => state.status.clear(),
            Err(error) => state.status = error,
        }
    }
    let reply = match state.work.as_mut() {
        Some(Work::Read(task)) => check_ready(task).map(Ready::Read),
        Some(Work::Preview(task, path)) => {
            check_ready(task).map(|result| Ready::Preview(result, path.clone()))
        }
        Some(Work::Save(task)) => check_ready(task).map(Ready::Save),
        None => return,
    };
    let Some(reply) = reply else {
        return;
    };
    state.work = None;
    match reply {
        Ready::Read(Ok(loaded)) => adopt(&mut state, &mut presentation, &mut images, loaded, ""),
        Ready::Save(Ok((project, loaded))) => {
            state.project = project.clone();
            adopt(
                &mut state,
                &mut presentation,
                &mut images,
                loaded,
                "工程设置已保存",
            );
            saved.write(ProjectSettingsSaved { project });
        }
        Ready::Read(Err(error)) | Ready::Save(Err(error)) => {
            // 保留原草稿供用户重试。重新读取须由用户明确选择。
            state.allow_reload = true;
            state.status = error;
        }
        Ready::Preview(Ok(image), path) => {
            state.preview = Some(images.add(image));
            state.icon = IconEdit::Import(path);
            state.confirm_close = false;
            state.status.clear();
        }
        Ready::Preview(Err(error), _) => state.status = error,
    }
}

pub(super) fn capture_fields(
    mut state: ResMut<ProjectSettingsState>,
    picker: NonSend<IconPicker>,
    mut fields: Query<(&Field, &mut EditableText)>,
    mut adopted: Local<u64>,
) {
    if *adopted != state.fields_revision {
        for (field, mut text) in &mut fields {
            let value = match field {
                Field::Name => &state.name,
                Field::Description => &state.description,
            };
            if text.value() != value.as_str() {
                text.clear();
                text.editor.set_text(value);
            }
        }
        *adopted = state.fields_revision;
        return;
    }
    if !state.open || state.snapshot.is_none() || state.busy(&picker) {
        return;
    }
    for (field, text) in &mut fields {
        if !text.is_changed() {
            continue;
        }
        let target = match field {
            Field::Name => &mut state.name,
            Field::Description => &mut state.description,
        };
        if text.value() != target.as_str() {
            *target = text.value().to_string();
            state.confirm_close = false;
        }
    }
}

fn activate(
    state: &mut ProjectSettingsState,
    workspace: &mut PresetWorkspaceState,
    domains: &mut WorkspaceDomainState,
) {
    if !domains.is_open(TAB) {
        domains.open_tabs.push(TAB);
    }
    state.open = true;
    state.confirm_close = false;
    if workspace.active_tab != TAB {
        state.return_tab = workspace.active_tab;
        workspace.active_tab = TAB;
        workspace.touch_ui();
    }
}

pub(super) fn input(
    mut opens: MessageReader<OpenProjectSettings>,
    mut state: ResMut<ProjectSettingsState>,
    mut workspace: ResMut<VegetationPresetEditorState>,
    mut domains: ResMut<WorkspaceDomainState>,
    mut preview_focus: ResMut<AssetPreviewFocus>,
    mut picker: NonSendMut<IconPicker>,
    presentation: Res<ProjectSettingsPresentation>,
    mut focus: ResMut<InputFocus>,
    close: Option<Res<NativeProjectCloseState>>,
    windows: Query<&RawHandleWrapper, With<PrimaryWindow>>,
    buttons: Query<
        (&Action, &EditorButtonClick),
        (Changed<EditorButtonClick>, Without<InteractionDisabled>),
    >,
    fields: Query<&EditableText, With<Field>>,
) {
    let requested = opens.read().count() > 0;
    if close.is_some_and(|close| close.active()) {
        if let Some(picker) = &picker.0 {
            picker.cancel();
        }
        return;
    }
    if requested {
        activate(&mut state, &mut workspace, &mut domains);
        preview_focus.show_workspace();
        focus.clear();
    }
    let Some(action) = buttons
        .iter()
        .find_map(|(action, click)| click.is_clicked().then_some(*action))
    else {
        return;
    };
    if !state.open {
        return;
    }
    if action == Action::Activate {
        activate(&mut state, &mut workspace, &mut domains);
        preview_focus.show_workspace();
        focus.clear();
        return;
    }
    if state.busy(&picker) || fields.iter().any(EditableText::is_composing) {
        return;
    }
    if action == Action::Close {
        if state.dirty() && !state.confirm_close {
            state.confirm_close = true;
            state.status = "再次点击关闭可放弃未保存的工程设置".into();
            return;
        }
        restore_fields(&mut state, presentation.icon.clone());
        state.open = false;
        domains.open_tabs.retain(|tab| *tab != TAB);
        if workspace.active_tab == TAB {
            workspace.active_tab = match state.return_tab {
                PresetWorkspaceTab::Domain(..) if !domains.is_open(state.return_tab) => {
                    PresetWorkspaceTab::Scene
                }
                tab => tab,
            };
        }
        workspace.touch_ui();
        focus.clear();
        return;
    }
    if workspace.active_tab != TAB {
        return;
    }
    match action {
        Action::ChooseIcon => {
            let Some(owner) = windows
                .iter()
                .next()
                .and_then(crate::workspace_window::primary_hwnd)
            else {
                state.status = "当前窗口尚未准备好，无法选择图标".into();
                return;
            };
            match FolderPicker::start_project_icon(owner, state.project.root.clone()) {
                Ok(dialog) => {
                    picker.0 = Some(dialog);
                    focus.clear();
                }
                Err(error) => state.status = error,
            }
        }
        Action::ResetIcon => {
            state.icon = if state
                .snapshot
                .as_ref()
                .is_some_and(|saved| saved.metadata.icon.is_some())
            {
                IconEdit::Remove
            } else {
                IconEdit::Keep
            };
            state.preview = None;
            state.confirm_close = false;
            state.status.clear();
        }
        Action::Save if state.dirty() => {
            if let Err(error) = begin_save(&mut state) {
                state.status = error;
            }
            focus.clear();
        }
        Action::Reload => {
            start_load(&mut state);
            focus.clear();
        }
        _ => {}
    }
}
