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

fn glb_path(root: &Path, path: &str) -> Result<PathBuf, String> {
    let path = safe_asset_path(root, path)?;
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("glb"))
    {
        return Err("人形动作预览需要工程资产中的 GLB 文件".into());
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
    let source = match glb_path(root, &request.animation_path).and_then(|source| {
        glb_path(root, &request.model_path)?;
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
        !previous.failed && previous.request.same_assets(&request)
            && state.current().is_none_or(|session| {
                session.clip != request.clip_index
                    && request.clip_index < session.document.clips.len()
            })
    });
    if reuse {
        // 同一份已校验文档及角色只换片段，读取中则采用最终所选片段。
        state.rig_preview.as_mut().unwrap().request.clip_index = request.clip_index;
        if let Some(session) = state.current_mut() {
            session.select_clip(request.clip_index);
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
    if request.request.clip_index >= session.document.clips.len() {
        request.failed = true;
        state.status = "所选动画片段已不存在，请重新选择动作 GLB".into();
        state.retired = Some(Retired::Session(session));
        return;
    }
    session.select_clip(request.request.clip_index);
    session.model_path = Some(request.request.model_path.clone());
    session.retarget_model = true;
    debug_assert!(state.rig_session.is_none());
    state.rig_session = Some((ticket, session));
    state.status = "工程动作已载入 · 预览只读".into();
    state.model_status = "正在检查角色与动作骨架，准备重定向".into();
    state.playing = true;
    state.changed();
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
        state.current().map_or(request.clip_index, |session| session.clip),
    ))
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
}
