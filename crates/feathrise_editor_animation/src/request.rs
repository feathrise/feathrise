//! 工程动作沿用当前模型中央标签，源文件与蒙皮目标分别验明身份。

use super::{data::AnimationDocument, resources::{AnimationEditorState, PendingAction, Retired, Session, TaskResult, Work}};
use feathrise_editor_animation_model::{
    media::AssetPreviewWorkspace,
    request::{DirectoryAnimationAction, PreviewRigAnimation, RigPreviewRequest, SourceAnimationNode},
};
use bevy::{prelude::*, tasks::IoTaskPool};
use std::path::{Path, PathBuf};

pub(super) fn select(state: &mut AnimationEditorState, request: PreviewRigAnimation, source: PathBuf) {
    state.weight_rest = None;
    let reuse = state.rig_preview.as_ref().is_some_and(|previous| {
        !request.recheck_source && !previous.failed && previous.source == source
            && previous.request.same_assets(&request)
            && (!previous.ready || !state.busy())
            && state.current().is_none_or(|session| {
                session.clip_from_source(request.clip_index).is_some_and(|clip| clip != session.clip)
            })
    });
    if reuse {
        if state.current().is_some_and(|session| session.pose.is_some()) {
            state.status = "请先插入或还原临时姿势".into();
            return;
        }
        // 同一份已校验文档及角色只换片段，读取中则采用最终所选片段。
        state.exit_animation_edit();
        state.rig_preview.as_mut().unwrap().request.clip_index = request.clip_index;
        if let Some(session) = state.current_mut() {
            let clip = session.clip_from_source(request.clip_index).expect("复用前已核对动作源索引");
            session.select_clip(clip);
            state.playing = true;
            if !state.select_current_clip() {
                state.changed();
            }
        }
        return;
    }
    state.reset_rig_playback();
    state.rig_preview = Some(RigPreviewRequest {
        ticket: state.source_revision,
        source,
        request,
        ready: false,
        failed: false,
    });
    state.status = "正在后台读取工程动作".into();
}

pub(super) fn launch(state: &mut AnimationEditorState) {
    let Some(request) = state.rig_preview.as_ref().filter(|request| !request.failed && !request.ready) else {
        return;
    };
    let source = request.source.clone();
    let root = request.request.asset_root.clone();
    let ticket = request.ticket;
    let existing = state.sessions.get(&source)
        .map(|session| (session.document.clone(), session.dirty()));
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
                if let Some((document, dirty)) = existing {
                    // 精确核对作者基线。磁盘改变时，只有干净文档才能替换。
                    if document.disk_unchanged(&source)? {
                        return Ok(None);
                    }
                    if dirty {
                        return Err("工程动作文件已在外部更新，原动画草稿已保留".into());
                    }
                }
                Ok(Some(Session::new(AnimationDocument::load(&source)?)))
            })();
            TaskResult::RigLoad { ticket, result }
        }),
    });
}

pub(super) fn adopt(state: &mut AnimationEditorState, ticket: u64, result: Result<Option<Session>, String>) {
    let Some(request) = state.rig_preview.as_ref().filter(|request| request.ticket == ticket) else {
        state.retired = result.ok().flatten().map(Retired::Session);
        return;
    };
    let source = request.source.clone();
    let source_clip = request.request.clip_index;
    let loaded = match result {
        Ok(loaded) => loaded,
        Err(error) => {
            reject(state, None, error);
            return;
        }
    };
    let selected = loaded.as_ref().or_else(|| state.sessions.get(&source)).and_then(|session| {
        session.clip_from_source(source_clip).map(|clip| (clip, session.clip, session.pose.is_some()))
    });
    let Some((clip, current_clip, has_pose)) = selected else {
        reject(state, loaded, "所选动画片段已不存在，请重新选择动作 GLB");
        return;
    };
    if has_pose && current_clip != clip {
        reject(state, loaded, "请先插入或还原原片段的临时姿势");
        return;
    }
    if let Some(session) = loaded {
        let retired = match state.make_room_for_session(&source) {
            Ok(retired) => retired,
            Err(error) => {
                reject(state, Some(session), error);
                return;
            }
        };
        state.retired = state.sessions.insert(source.clone(), session).or(retired).map(Retired::Session);
    }
    let session = state.sessions.get_mut(&source).expect("采用前已核对动画会话");
    if !has_pose {
        session.select_clip(clip);
    }
    session.last_used = state.revision;
    session.source_changed = false;
    // 分享作者会话，包括历史与未提交姿势。模型绑定只存在于该标签的请求中。
    state.rig_preview.as_mut().unwrap().ready = true;
    state.status = "工程动作已载入".into();
    state.model_status = "正在检查角色与动作骨架，准备重定向".into();
    state.playing = !has_pose;
    if !state.select_current_clip() {
        state.changed();
    }
}

fn reject(state: &mut AnimationEditorState, loaded: Option<Session>, error: impl Into<String>) {
    state.rig_preview.as_mut().unwrap().failed = true;
    state.retired = loaded.map(Retired::Session);
    state.status = error.into();
    state.changed();
}

pub(super) fn advance_directory_pick(
    state: &mut AnimationEditorState,
    context: &super::context::AnimationContext,
    media: &AssetPreviewWorkspace,
    root: &Path,
) {
    let Some(pick) = state.directory_pick.as_ref() else { return; };
    if !context.host.open || context.host.closing
        || context.host.switching_scene
        || pick.root != root || !media.active(context.host.tab)
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
    if session.pose.is_some() {
        state.status = "请先插入或还原临时姿势".into();
        state.cancel_directory_pick();
        return;
    } else if session.source_changed {
        state.status = "动作源已更新，请先处理当前草稿".into();
        state.cancel_directory_pick();
        return;
    }
    let pick = state.directory_pick.take().expect("当前标签已核对待选动作");
    let (index, node, edit) = match pick.action {
        DirectoryAnimationAction::Preview(index) => (index, None, false),
        DirectoryAnimationAction::Edit { clip, node } => {
            let target = if let Some((index, name)) = clip {
                let Some(clip) = session.clip_from_source(index).and_then(|clip| session.document.clips.get(clip))
                    .filter(|clip| clip.name == name) else {
                        state.status = "源 GLB 动画列表已变化，请在场景树重新选择".into();
                        return;
                    };
                Some(clip.source_index)
            } else {
                session.document.clips.get(session.clip).map(|clip| clip.source_index)
            };
            let Some(index) = target else {
                state.status = "该 GLB 暂无动画，可在源模型中点击新建动画".into();
                return;
            };
            (index, node, true)
        }
        DirectoryAnimationAction::Create { node } => {
            if !source_node_current(&session.document, node.as_ref()) {
                state.status = "源网格节点已变化或不可写动画，请在场景树重新选择".into();
                return;
            }
            if !session.document.can_create_clip() {
                state.status = "当前 GLB 没有可写入动画的节点".into();
                return;
            }
            let clip_index = session.document.clips.len();
            state.directory_pick = Some(feathrise_editor_animation_model::request::DirectoryPick {
                action: DirectoryAnimationAction::EnterCreated { clip_index, node },
                ..pick
            });
            // 复用模型动画的单槽创建和撤销，不从场景菜单直接生成关键帧。
            state.pending = Some(PendingAction::Command(super::editing::Command::CreateClip));
            return;
        }
        DirectoryAnimationAction::EnterCreated { clip_index, node } => {
            // 后台失败保留已有的创建诊断，不将失败结果视为进入编辑。
            if session.clip_from_source(clip_index).is_none() { return; }
            (clip_index, node, true)
        }
    };
    let Some(clip) = session.clip_from_source(index) else {
        state.status = "所选动画片段已不存在，请重新选择动作 GLB".into();
        return;
    };
    if edit && !source_node_current(&session.document, node.as_ref()) {
        state.status = "源网格节点已变化或不可写动画，请在场景树重新选择".into();
        return;
    }
    if edit && session.document.read_only {
        state.status = "该 GLB 使用外置动画数据，仅能预览".into();
        return;
    }
    if !edit || session.clip != clip {
        session.select_clip(clip);
    }
    if let Some(node) = node {
        session.select_node(node.index);
    }
    state.playing = !edit;
    if !state.select_current_clip() {
        state.changed();
    } else if edit && state.enter_animation_edit() {
        state.status = "正在编辑共享源 GLB 动画，保存后同步引用实例".into();
    }
}

fn source_node_current(document: &AnimationDocument, target: Option<&SourceAnimationNode>) -> bool {
    let Some(target) = target else { return true; };
    if !document.can_animate_node(target.index) { return false; }
    // 节点索引只在本份 GLB 内有效，后台读盘后核对父路径和 mesh，避免换代文件串改。
    let mut parent = Some(target.index);
    for name in target.path.iter().rev() {
        let Some(node) = parent.and_then(|index| document.nodes.get(index)) else { return false; };
        if &node.name != name { return false; }
        parent = node.parent;
    }
    parent.is_none() && target.mesh_index.is_none_or(|mesh| document.inspect_glb(|json, _| {
        json["nodes"].get(target.index).and_then(|node| node["mesh"].as_u64()) == Some(u64::from(mesh))
    }))
}

#[cfg(feature = "validation")]
pub fn rig_animation_identity(world: &World) -> Option<(u64, &str, &str, &str, usize)> {
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
pub fn active_project_clip(world: &World) -> Option<(&Path, usize, f32, bool)> {
    let state = world.get_resource::<AnimationEditorState>()?;
    let session = state.current()?;
    let clip = session.document.clips.get(session.clip)?;
    Some((state.source()?, clip.source_index, session.position, state.playing))
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{components::AnimationAction, control, data::tests::{test_document, test_scene}, types::PoseProperty};
    use feathrise_editor_animation_model::media::{
        AssetPreviewModelSession, AssetPreviewModelTarget, AssetPreviewPrimitiveId,
        AssetPreviewState, ExpandedAssetPreview,
    };
    use feathrise_editor_core::PresetWorkspaceTab;

    #[test]
    fn source_animation_rejects_a_stale_node_index_after_reparenting_or_reexport() {
        use serde_json::json;
        let document = test_scene(&json!({
            "asset": { "version": "2.0" },
            "scenes": [{ "nodes": [0, 2] }],
            "nodes": [
                { "name": "机关组", "children": [1] },
                { "name": "活动网格", "mesh": 3 },
                { "name": "活动网格", "mesh": 3 }
            ]
        }));
        let mut target = SourceAnimationNode {
            index: 1, path: vec!["机关组".into(), "活动网格".into()], mesh_index: Some(3),
        };
        assert!(source_node_current(&document, Some(&target)));
        target.index = 2;
        assert!(!source_node_current(&document, Some(&target)));
        target.index = 1;
        target.mesh_index = Some(4);
        assert!(!source_node_current(&document, Some(&target)));
    }

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
    fn rig_switch_retires_late_sources_and_reuses_the_author_session() {
        let author_path = PathBuf::from("assets/walk.glb");
        let model = PathBuf::from("assets/hero.glb");
        let mut author = Session::new(test_document());
        author.set_pose(0, Transform::from_xyz(3.0, 2.0, 1.0), PoseProperty::Translation);
        author.model_path = Some("previous-bound.glb".into());
        let mut state = AnimationEditorState {
            active: Some(model.clone()),
            visible: true,
            ..default()
        };
        state.sessions.insert(author_path.clone(), author);
        state.sessions.insert(model.clone(), Session::new(test_document()));
        select(&mut state, request("idle.glb", 0), "assets/idle.glb".into());
        let stale = state.rig_preview.as_ref().unwrap().ticket;
        select(&mut state, request("walk.glb", 0), author_path.clone());
        let ticket = state.rig_preview.as_ref().unwrap().ticket;
        assert_ne!(ticket, stale);
        assert!(state.in_place_rig_preview() && !state.editable());
        assert!(state.current().is_none());
        adopt(&mut state, stale, Ok(Some(Session::new(test_document()))));
        assert!(state.current().is_none());
        assert!(matches!(state.retired.take(), Some(Retired::Session(_))));
        adopt(&mut state, ticket, Ok(None));
        assert_eq!(state.author_path(), Some(author_path.as_path()));
        assert_eq!(state.preview_model_path(), Some("hero_rigged.glb"));
        assert_eq!(state.current().unwrap().model_path.as_deref(), Some("previous-bound.glb"));
        assert_eq!(state.current().unwrap().clip, 0);
        assert!(!state.playing && !state.editable());
        let pose = state.current().unwrap().pose.as_ref().unwrap().get(0).unwrap();
        assert_eq!(pose.transform.translation, Vec3::new(3.0, 2.0, 1.0));
        assert!(state.enter_animation_edit() && state.editable());
        select(&mut state, request("walk.glb", 1), author_path.clone());
        assert_eq!(state.active_clip(), 0, "未提交姿势必须留在原片段");
        control::action(&mut state, AnimationAction::ResetPose);
        let snapshot = state.document_snapshot().unwrap();
        select(&mut state, request("walk.glb", 1), author_path.clone());
        assert_eq!(state.rig_preview.as_ref().unwrap().ticket, ticket);
        assert_eq!(state.active_clip(), 1);
        assert!(!state.editable() && state.can_edit_selected());
        assert!(std::sync::Arc::ptr_eq(&snapshot, &state.document_snapshot().unwrap()));
        control::action(&mut state, AnimationAction::UnbindModel);
        assert!(!state.in_place_rig_preview());
        assert_eq!(state.source(), Some(model.as_path()));
        assert!(!state.editable(), "结束工程动作预览只回到角色会话");
        state.active = Some(author_path);
        assert!(std::sync::Arc::ptr_eq(&snapshot, &state.document_snapshot().unwrap()));
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
        adopt(&mut state, old, Ok(Some(Session::new(test_document()))));
        assert!(state.current().is_none(), "同路径旧文件的迟到结果不发布");
        assert!(state.retired.take().is_some());
        let mut replacement = test_document();
        replacement.clips[1].source_index = 5;
        adopt(&mut state, new, Ok(Some(Session::new(replacement))));
        let session = state.current().unwrap();
        assert_eq!(session.document.clips[session.clip].name, "待机");
        assert_eq!(session.document.clips[session.clip].source_index, 5);
        assert!(state.playing && !state.editable());
        assert!(state.can_edit_selected());
    }

    #[test]
    fn rig_pick_reuses_the_cached_draft_and_refuses_a_conflicting_file() {
        use super::super::data::tests::{TemporaryDirectory, write_directory_clip_fixture};

        IoTaskPool::get_or_init(bevy::tasks::TaskPool::new);
        let directory = TemporaryDirectory::new();
        let source = directory.0.join("walk.glb");
        write_directory_clip_fixture(&source, "原片段");
        let mut session = Session::new(AnimationDocument::load(&source).unwrap());
        session.saved_cursor = None;
        let draft = session.document.clone();
        let mut state = AnimationEditorState::default();
        state.sessions.insert(source.clone(), session);
        let mut choose = request("walk.glb", 1);
        choose.asset_root = directory.0.clone();
        choose.recheck_source = true;

        for changed in [false, true] {
            if changed {
                write_directory_clip_fixture(&source, "外部替换");
            }
            select(&mut state, choose.clone(), source.clone());
            assert!(state.current().is_none(), "复验期间不发布旧作者片段");
            launch(&mut state);
            let task = state.work.take().expect("使用原单槽读取").task;
            let TaskResult::RigLoad { ticket, result } = bevy::tasks::block_on(task) else {
                panic!("收到错误的动作读取回执");
            };
            adopt(&mut state, ticket, result);
            assert!(std::sync::Arc::ptr_eq(&draft, &state.sessions[&source].document));
            assert!(state.sessions[&source].dirty());
            if changed {
                assert!(state.current().is_none() && !state.can_edit_selected());
                assert!(state.status.contains("外部更新"));
            } else {
                assert!(state.playing && state.can_edit_selected() && !state.editable());
                assert_eq!(state.active_clip(), 1);
            }
        }
    }
}
