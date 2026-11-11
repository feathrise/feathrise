//! 绑骨后的只读动作沿用当前模型中央标签，源文件和蒙皮目标分别验明身份。

use super::super::{
    AssetPreviewState, AssetPreviewWorkspace, DocumentState, types::AssetPreviewModelTarget,
};
use super::{data::AnimationDocument, resources::{AnimationEditorState, Retired, Session, TaskResult, Work}};
use crate::{
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState},
    ui::vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState},
};
use bevy::{prelude::*, tasks::IoTaskPool};
use feathrise_editor_assets::files::safe_asset_path;
use std::path::{Path, PathBuf};

#[derive(Message, Clone)]
pub(in crate::ui::asset_preview) struct PreviewRigAnimation {
    pub model_path: String,
    pub animation_path: String,
    pub clip_index: usize,
    pub host_path: String,
    pub host_generation: u64,
    pub host_tab: u64,
    pub asset_root: PathBuf,
    pub recheck_source: bool,
}

impl PreviewRigAnimation {
    pub(super) fn matches(
        &self,
        preview: &AssetPreviewState,
        media: &AssetPreviewWorkspace,
        tab: PresetWorkspaceTab,
        asset_root: &Path,
    ) -> bool {
        self.asset_root == asset_root
            && media.active(tab)
            && media.expanded.as_ref().is_some_and(|entry| {
                entry.id == self.host_tab && entry.path == self.host_path
            })
            && preview.selected_path.as_deref() == Some(self.host_path.as_str())
            && preview.external_preview.is_none()
            && preview.model.as_ref().is_some_and(|model| {
                model.generation == self.host_generation && model.target == AssetPreviewModelTarget::Scene0
            })
    }

    fn same_assets(&self, other: &Self) -> bool {
        self.host_tab == other.host_tab
            && self.host_path == other.host_path
            && self.host_generation == other.host_generation
            && self.asset_root == other.asset_root
            && self.model_path == other.model_path
            && self.animation_path == other.animation_path
    }
}

pub(super) struct RigPreviewRequest {
    pub ticket: u64,
    pub source: PathBuf,
    pub request: PreviewRigAnimation,
    failed: bool,
}

pub(in crate::ui::asset_preview) fn checked_clip_path(root: &Path, path: &str) -> Result<PathBuf, String> {
    let path = safe_asset_path(root, path)?;
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("glb"))
    {
        return Err("动作预览需要工程资产中的 GLB 文件".into());
    }
    Ok(path)
}

pub(in crate::ui::asset_preview) fn receive(
    mut requests: MessageReader<PreviewRigAnimation>,
    mut editor: ResMut<SceneEditorState>,
    guard: Res<SceneEditorNumericScrubMouseGuard>,
    documents: Res<DocumentState>,
    preview: Res<AssetPreviewState>,
    media: Res<AssetPreviewWorkspace>,
    workspace: Res<VegetationPresetEditorState>,
    mut animation: ResMut<AnimationEditorState>,
) {
    let Some(request) = requests.read().last() else {
        return;
    };
    let root = documents.asset_root();
    if !editor.open
        || editor.close_requested
        || editor.pending_scene_transition.is_some()
        || editor.new_scene_dialog_open
        || editor.scene_switch_confirmation.is_some()
        || guard.blocked()
        || root.as_os_str().is_empty()
        || !request.matches(&preview, &media, workspace.active_tab, root)
    {
        return;
    }
    let source = match checked_clip_path(root, &request.animation_path).and_then(|source| {
        checked_clip_path(root, &request.model_path)?;
        Ok(source)
    }) {
        Ok(source) => source,
        Err(error) => {
            editor.status = error;
            return;
        }
    };
    select(&mut animation, request.clone(), source);
}

fn select(state: &mut AnimationEditorState, request: PreviewRigAnimation, source: PathBuf) {
    state.weight_rest = None;
    let reuse = state.rig_preview.as_ref().is_some_and(|previous| {
        !request.recheck_source && !previous.failed && previous.request.same_assets(&request)
            && state.current().is_none_or(|session| {
                session.clip_from_source(request.clip_index).is_some_and(|clip| clip != session.clip)
            })
    });
    if reuse {
        // 同一份已校验文档及角色只换片段，读取中则采用最终所选片段。
        state.rig_preview.as_mut().unwrap().request.clip_index = request.clip_index;
        if let Some(session) = state.current_mut() {
            let clip = session.clip_from_source(request.clip_index).expect("复用前已核对动作源索引");
            session.select_clip(clip);
            state.playing = true;
            state.changed();
        }
        return;
    }
    state.reset_rig_playback();
    state.rig_preview = Some(RigPreviewRequest {
        ticket: state.source_revision,
        source,
        request,
        failed: false,
    });
    state.status = "正在后台读取工程动作".into();
}

pub(super) fn retire(state: &mut AnimationEditorState) {
    if state.work.is_none() && state.retired.is_none()
        && state.rig_session.as_ref().is_some_and(|(ticket, _)| {
            state.rig_preview.as_ref().is_none_or(|request| request.ticket != *ticket)
        })
    {
        // 过期文档与原作者文件释放共用单槽。连续选择只保留最新意图。
        state.retired = state.rig_session.take().map(|(_, session)| Retired::Session(session));
    }
}

pub(super) fn launch(state: &mut AnimationEditorState) {
    let Some(request) = state.rig_preview.as_ref().filter(|request| !request.failed) else {
        return;
    };
    if state.current().is_some() {
        return;
    }
    let source = request.source.clone();
    let root = request.request.asset_root.clone();
    let ticket = request.ticket;
    state.work = Some(Work {
        path: source.clone(),
        generation: Some(request.request.host_generation),
        saving: false,
        task: IoTaskPool::get().spawn(async move {
            let result = (|| {
                let root = root.canonicalize().map_err(|error| format!("无法打开工程资产根：{error}"))?;
                let source = source.canonicalize().map_err(|error| format!("无法打开动作资产：{error}"))?;
                if !source.starts_with(&root) {
                    return Err("动作资产已离开当前工程资产目录".into());
                }
                let mut document = AnimationDocument::load(&source)?;
                // 仅读取磁盘版本。原动作标签的草稿、历史及临时姿势仍由原会话保留。
                document.read_only = true;
                Ok(Session::new(document))
            })();
            TaskResult::RigLoad { ticket, result }
        }),
    });
}

pub(super) fn adopt(state: &mut AnimationEditorState, ticket: u64, result: Result<Session, String>) {
    let Some(request) = state.rig_preview.as_mut().filter(|request| request.ticket == ticket) else {
        state.retired = Some(Retired::Result(TaskResult::RigLoad { ticket, result }));
        return;
    };
    let mut session = match result {
        Ok(session) => session,
        Err(error) => {
            request.failed = true;
            state.status = error;
            return;
        }
    };
    let Some(clip) = session.clip_from_source(request.request.clip_index) else {
        request.failed = true;
        state.status = "所选动画片段已不存在，请重新选择动作 GLB".into();
        state.retired = Some(Retired::Session(session));
        return;
    };
    session.select_clip(clip);
    session.model_path = Some(request.request.model_path.clone());
    session.retarget_model = true;
    debug_assert!(state.rig_session.is_none());
    state.rig_session = Some((ticket, session));
    state.status = "工程动作已载入 · 预览只读".into();
    state.model_status = "正在检查角色与动作骨架，准备重定向".into();
    state.playing = true;
    state.changed();
}

pub(super) struct DirectoryPick {
    root: PathBuf,
    source: PathBuf,
    tab: u64,
    clip_index: usize,
    recheck_source: bool,
}

impl AnimationEditorState {
    pub(in crate::ui::asset_preview) fn queue_directory_pick(
        &mut self, root: PathBuf, source: PathBuf, tab: u64, clip_index: usize,
    ) {
        self.stop_rig_preview();
        let recheck_source = self.sessions.contains_key(&source)
            || self.work.as_ref().is_some_and(|work| work.path == source);
        if self.failed.as_ref() == Some(&source) {
            self.failed = None;
        }
        self.directory_pick = Some(DirectoryPick { root, source, tab, clip_index, recheck_source });
    }

    pub(in crate::ui::asset_preview) fn cancel_directory_pick(&mut self) {
        self.directory_pick = None;
    }
}

pub(super) fn advance_directory_pick(
    state: &mut AnimationEditorState,
    context: &super::context::AnimationContext,
    media: &AssetPreviewWorkspace,
    root: &Path,
) {
    let Some(pick) = state.directory_pick.as_ref() else { return; };
    if !context.editor.open || context.editor.close_requested
        || context.editor.pending_scene_transition.is_some()
        || context.editor.new_scene_dialog_open || context.editor.scene_switch_confirmation.is_some()
        || pick.root != root || !media.active(context.workspace.active_tab)
        || media.expanded.as_ref().is_none_or(|tab| tab.id != pick.tab || root.join(&tab.path) != pick.source)
    {
        // 原打开的标签或工程已离开。已在途的 GLB 读取可以完成，但不得补播放。
        state.cancel_directory_pick();
        return;
    }
    if !state.visible || state.busy() || state.active.as_ref() != Some(&pick.source) {
        return;
    }
    if state.failed.as_ref() == Some(&pick.source) {
        state.cancel_directory_pick();
        return;
    }
    let Some(session) = state.sessions.get_mut(&pick.source) else { return; };
    if pick.recheck_source && session.pose.is_none() {
        // 停放的草稿可能没有收到模型热更新。解释目录原索引前核对磁盘基线，
        // 干净文档可沿原重载分支更新，脏文档只核对字节并保留全部草稿。
        let path = pick.source.clone();
        state.directory_pick.as_mut().expect("待选动作仍有效").recheck_source = false;
        if session.dirty() {
            let source = path.clone();
            let generation = state.model_generation.as_ref()
                .filter(|(current, _)| current == &path)
                .map(|(_, generation)| *generation);
            let document = session.document.clone();
            state.work = Some(Work {
                path,
                generation,
                saving: false,
                task: IoTaskPool::get().spawn(async move {
                    match document.disk_unchanged(&source) {
                        Ok(true) => TaskResult::Unchanged,
                        Ok(false) => TaskResult::Load(Err("动作文件已在外部更新，请先处理当前动画草稿再选择目录动作".into())),
                        Err(error) => TaskResult::Load(Err(error)),
                    }
                }),
            });
        } else {
            session.source_changed = true;
        }
        return;
    }
    if session.source_changed && !session.dirty() {
        return; // 先等热更新的完整新文档采用，再解释目录提供的原片段索引。
    }
    let clip = session.clip_from_source(pick.clip_index);
    let can_select = if session.pose.is_some() {
        state.status = "请先插入或还原临时姿势".into();
        false
    } else if session.source_changed {
        state.status = "动作源已更新，请先处理当前草稿".into();
        false
    } else if let Some(clip) = clip {
        session.select_clip(clip);
        state.playing = true;
        true
    } else {
        state.status = "所选动画片段已不存在，请重新选择动作 GLB".into();
        false
    };
    state.cancel_directory_pick();
    if can_select {
        state.changed();
    }
}

#[cfg(feature = "validation")]
pub(crate) fn rig_animation_identity(world: &World) -> Option<(u64, &str, &str, &str, usize)> {
    let state = world.get_resource::<AnimationEditorState>()?;
    let request = &state.rig_preview.as_ref()?.request;
    Some((
        request.host_tab,
        request.host_path.as_str(),
        request.model_path.as_str(),
        request.animation_path.as_str(),
        state.current().map_or(request.clip_index, |session| {
            session.document.clips[session.clip].source_index
        }),
    ))
}

#[cfg(feature = "validation")]
pub(crate) fn active_project_clip(world: &World) -> Option<(&Path, usize, f32, bool)> {
    let state = world.get_resource::<AnimationEditorState>()?;
    let session = state.current()?;
    let clip = session.document.clips.get(session.clip)?;
    Some((state.source()?, clip.source_index, session.position, state.playing))
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{components::AnimationAction, control, data::tests::test_document};
    use crate::ui::asset_preview::types::{
        AssetPreviewModelSession, AssetPreviewPrimitiveId, ExpandedAssetPreview,
    };

    fn request(path: &str, clip_index: usize) -> PreviewRigAnimation {
        PreviewRigAnimation {
            model_path: "hero_rigged.glb".into(),
            animation_path: path.into(),
            clip_index,
            host_path: "hero.glb".into(),
            host_generation: 8,
            host_tab: 6,
            asset_root: "assets".into(),
            recheck_source: false,
        }
    }

    #[test]
    fn rig_request_requires_its_model_tab_project_generation_and_complete_scene() {
        let request = request("walk.glb", 0);
        let mut media = AssetPreviewWorkspace {
            expanded: Some(ExpandedAssetPreview {
                id: request.host_tab,
                path: request.host_path.clone(),
                return_tab: PresetWorkspaceTab::Scene,
                view: default(),
                target: AssetPreviewModelTarget::Scene0,
                audio_position: 0.0,
                restore_cursor: 0,
            }),
            ..default()
        };
        let mut preview = AssetPreviewState {
            selected_path: Some(request.host_path.clone()),
            model: Some(AssetPreviewModelSession::new(request.host_generation, Handle::default())),
            ..default()
        };
        let tab = PresetWorkspaceTab::AssetPreview;
        assert!(request.matches(&preview, &media, tab, &request.asset_root));
        assert!(!request.matches(&preview, &media, tab, Path::new("other/assets")));
        assert!(!request.matches(&preview, &media, PresetWorkspaceTab::Scene, &request.asset_root));
        media.expanded.as_mut().unwrap().id += 1;
        assert!(!request.matches(&preview, &media, tab, &request.asset_root));
        media.expanded.as_mut().unwrap().id = request.host_tab;
        preview.model.as_mut().unwrap().generation += 1;
        assert!(!request.matches(&preview, &media, tab, &request.asset_root));
        let model = preview.model.as_mut().unwrap();
        model.generation = request.host_generation;
        model.target = AssetPreviewModelTarget::Primitive(AssetPreviewPrimitiveId { mesh: 0, primitive: 0 });
        assert!(!request.matches(&preview, &media, tab, &request.asset_root));
    }

    #[test]
    fn rig_switch_retires_late_sources_and_keeps_editable_source_session() {
        let author_path = PathBuf::from("assets/walk.glb");
        let mut author = Session::new(test_document());
        author.pose = Some((0, Transform::from_xyz(3.0, 2.0, 1.0)));
        let mut state = AnimationEditorState {
            active: Some(author_path.clone()),
            visible: true,
            ..default()
        };
        state.sessions.insert(author_path.clone(), author);
        select(&mut state, request("idle.glb", 0), "assets/idle.glb".into());
        let stale = state.rig_preview.as_ref().unwrap().ticket;
        select(&mut state, request("walk.glb", 0), author_path.clone());
        let ticket = state.rig_preview.as_ref().unwrap().ticket;
        assert_ne!(ticket, stale);
        assert!(state.in_place_rig_preview() && !state.editable());
        assert!(state.current().is_none());
        adopt(&mut state, stale, Ok(Session::new(test_document())));
        assert!(state.current().is_none());
        assert!(matches!(state.retired.take(), Some(Retired::Result(_))));
        adopt(&mut state, ticket, Ok(Session::new(test_document())));
        assert_eq!(state.current().unwrap().model_path.as_deref(), Some("hero_rigged.glb"));
        assert_eq!(state.current().unwrap().clip, 0);
        assert!(state.playing && !state.editable());
        assert!(state.current().unwrap().pose.is_none());
        let snapshot = state.document_snapshot().unwrap();
        select(&mut state, request("walk.glb", 1), author_path.clone());
        assert_eq!(state.rig_preview.as_ref().unwrap().ticket, ticket);
        assert_eq!(state.active_clip(), 1);
        assert!(std::sync::Arc::ptr_eq(&snapshot, &state.document_snapshot().unwrap()));
        control::action(&mut state, AnimationAction::UnbindModel);
        retire(&mut state);
        assert!(!state.in_place_rig_preview());
        assert!(matches!(state.retired.as_ref(), Some(Retired::Session(_))));
        assert_eq!(state.source(), Some(author_path.as_path()));
        assert_eq!(state.current().unwrap().pose.unwrap().1.translation, Vec3::new(3.0, 2.0, 1.0));
        assert!(state.editable());
    }

    #[test]
    fn directory_rig_pick_uses_source_index_and_invalidates_a_previous_disk_load() {
        let mut state = AnimationEditorState::default();
        let source = PathBuf::from("assets/walk.glb");
        select(&mut state, request("walk.glb", 0), source.clone());
        let old = state.rig_preview.as_ref().unwrap().ticket;
        let mut pick = request("walk.glb", 5);
        pick.recheck_source = true;
        select(&mut state, pick, source);
        let new = state.rig_preview.as_ref().unwrap().ticket;
        assert_ne!(old, new);
        adopt(&mut state, old, Ok(Session::new(test_document())));
        assert!(state.current().is_none(), "同路径旧文件的迟到结果不发布");
        assert!(state.retired.take().is_some());
        let mut replacement = test_document();
        replacement.clips[1].source_index = 5;
        adopt(&mut state, new, Ok(Session::new(replacement)));
        let session = state.current().unwrap();
        assert_eq!(session.document.clips[session.clip].name, "待机");
        assert_eq!(session.document.clips[session.clip].source_index, 5);
        assert!(state.playing && !state.editable());
    }
}
