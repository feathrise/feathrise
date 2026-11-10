//! 绑骨面板的预览请求：复用中央资产标签，在精确标签和源文档就绪后采用指定片段。

use super::super::{
    AssetPreviewNavigation, AssetPreviewState, AssetPreviewWorkspace, DocumentState,
    types::AssetPreviewModelTarget, workspace,
};
use super::resources::AnimationEditorState;
use crate::{
    resources::{MapEditorNumericScrubMouseGuard, MapEditorState},
    ui::vegetation_preset_editor::VegetationPresetEditorState,
};
use bevy::prelude::*;
use feathrise_editor_assets::files::safe_asset_path;
use std::path::{Path, PathBuf};

#[derive(Message, Clone)]
pub(in crate::ui::asset_preview) struct PreviewRigAnimation {
    pub model_path: String,
    pub animation_path: String,
    pub clip_index: usize,
}

pub(super) struct RigPreviewRequest {
    source: PathBuf,
    tab: u64,
    model_path: String,
    clip_index: usize,
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
    mut editor: ResMut<MapEditorState>,
    guard: Res<MapEditorNumericScrubMouseGuard>,
    documents: Res<DocumentState>,
    preview: Res<AssetPreviewState>,
    navigation: Res<AssetPreviewNavigation>,
    mut media: ResMut<AssetPreviewWorkspace>,
    mut workspace_state: ResMut<VegetationPresetEditorState>,
    mut animation: ResMut<AnimationEditorState>,
) {
    let Some(request) = requests.read().last() else {
        return;
    };
    if !editor.open
        || editor.close_requested
        || editor.pending_map_transition.is_some()
        || editor.new_map_dialog_open
        || editor.map_switch_confirmation.is_some()
        || guard.blocked()
        || documents.asset_root().as_os_str().is_empty()
    {
        return;
    }
    let root = documents.asset_root();
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
    if animation
        .sessions
        .get(&source)
        .is_some_and(|session| session.pose.is_some())
    {
        editor.status = "请先在原动画标签插入或还原临时姿势".into();
        return;
    }
    if let Err(error) = workspace::expand(
        &mut media,
        &mut workspace_state,
        &request.animation_path,
        &preview,
        &navigation,
    ) {
        editor.status = error;
        return;
    }
    let tab = media.expanded.as_mut().expect("中央资产预览已打开");
    tab.target = AssetPreviewModelTarget::Scene0; // 外部动作只在完整模型预览上启动。
    animation.rig_preview = Some(RigPreviewRequest {
        source,
        tab: tab.id,
        model_path: request.model_path.clone(),
        clip_index: request.clip_index,
    });
}

pub(super) fn adopt(state: &mut AnimationEditorState, active_tab: Option<u64>) {
    let Some(request) = state.rig_preview.as_ref() else {
        return;
    };
    if active_tab != Some(request.tab)
        || state.source() != Some(request.source.as_path())
        || state.failed.as_ref() == Some(&request.source)
    {
        state.rig_preview = None;
        return;
    }
    if !state.visible || state.busy() || state.current().is_none() {
        return;
    }
    let request = state.rig_preview.take().unwrap();
    let session = state.current_mut().unwrap();
    if request.clip_index >= session.document.clips.len() {
        state.status = "所选动画片段已不存在，请重新选择动作 GLB".into();
        return;
    }
    if session.pose.is_some() {
        state.status = "请先插入或还原原动画的临时姿势".into();
        return;
    }
    // 同文件的脏关键帧仍由原会话保留，预览本身只改变片段、播放与绑定状态。
    session.select_clip(request.clip_index);
    session.model_path = Some(request.model_path);
    session.retarget_model = true;
    state.model_status = "正在检查角色与动作骨架，准备重定向".into();
    state.model_binding_revision = state.model_binding_revision.wrapping_add(1);
    state.playing = true;
    state.changed();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::asset_preview::animation::{data::tests::test_document, resources::Session};

    #[test]
    fn request_adopts_clip_ordinal_only_for_the_original_media_tab() {
        let path = PathBuf::from("assets/walk.glb");
        let request = || RigPreviewRequest {
            source: path.clone(),
            tab: 5,
            model_path: "hero-rigged.glb".into(),
            clip_index: 1,
        };
        let mut state = AnimationEditorState {
            active: Some(path.clone()),
            visible: true,
            rig_preview: Some(request()),
            ..default()
        };
        state
            .sessions
            .insert(path.clone(), Session::new(test_document()));
        adopt(&mut state, Some(4));
        assert!(state.rig_preview.is_none());
        assert!(!state.retargeting());
        state.rig_preview = Some(request());
        adopt(&mut state, Some(5));
        let session = state.current().unwrap();
        assert_eq!(session.clip, 1);
        assert!(state.playing && state.retargeting() && !state.editable());
        assert!(!session.dirty());
    }
}
