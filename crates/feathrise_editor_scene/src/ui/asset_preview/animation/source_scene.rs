//! 将场景树冻结的 GLB 和源节点交给当前窗口的模型动画编辑器。

use super::super::{
    AssetPreviewNavigation, AssetPreviewState, AssetPreviewWorkspace, DocumentState, workspace,
};
use crate::{
    authoring::EditorAuthoringSession,
    resources::{SceneEditorLayoutState, SceneEditorState},
    ui::{
        scene_context_menu::{SceneContextMenuState, SceneMenuAction, SceneObjectKey},
        sidebars::SceneEditorSidebarState,
        vegetation_preset_editor::VegetationPresetEditorState,
    },
};
use bevy::prelude::*;
use feathrise_editor_animation_model::{
    media::AssetPreviewModelTarget,
    request::{DirectoryAnimationAction, SourceAnimationNode, checked_clip_path},
    resources::AnimationEditorState,
};

pub(super) fn open(
    menu: Res<SceneContextMenuState>,
    session: Res<EditorAuthoringSession>,
    documents: Res<DocumentState>,
    preview: Res<AssetPreviewState>,
    navigation: Res<AssetPreviewNavigation>,
    mut editor: ResMut<SceneEditorState>,
    mut media: ResMut<AssetPreviewWorkspace>,
    mut workspaces: ResMut<VegetationPresetEditorState>,
    mut sidebar: ResMut<SceneEditorSidebarState>,
    mut animation: ResMut<AnimationEditorState>,
    mut layout: Option<ResMut<SceneEditorLayoutState>>,
) {
    let Some(request) = menu.ready.as_ref().filter(|request| {
        request.action.is_source_animation()
            && request.target.current(&editor, &session)
            && request.target.allows(request.action)
    }) else {
        return;
    };
    if session.saving || editor.close_requested || documents.asset_root().as_os_str().is_empty() {
        return;
    }
    let Some(path) = request.target.source_animation_path(&session) else {
        return;
    };
    if session
        .draft
        .as_ref()
        .is_some_and(|draft| draft.source_glb_edits.version(path).is_some())
    {
        editor.status = "请先保存源 GLB 的结构改动，再编辑动画".into();
        return;
    }
    let source = match checked_clip_path(documents.asset_root(), path) {
        Ok(path) => path,
        Err(reason) => {
            editor.status = reason;
            return;
        }
    };
    if documents.file_writes.blocks(&source) {
        editor.status = "源 GLB 正在执行文件操作".into();
        return;
    }
    let (node, clip) = match &request.target.key {
        SceneObjectKey::Placement { mesh, .. } => {
            let node = if let Some(mesh) = mesh {
                let Some(index) = mesh.node_index else {
                    return;
                };
                Some(SourceAnimationNode {
                    index,
                    path: mesh.node_path.clone(),
                    mesh_index: mesh.mesh_index,
                })
            } else {
                None
            };
            (node, None)
        }
        SceneObjectKey::SourceGlbAnimation { clip_index, .. } => {
            (None, Some((*clip_index, request.target.name.clone())))
        }
        _ => return,
    };
    let action = if request.action == SceneMenuAction::NewSourceGlbAnimation {
        DirectoryAnimationAction::Create { node }
    } else {
        DirectoryAnimationAction::Edit { clip, node }
    };

    if let Err(error) = workspace::expand(&mut media, &mut workspaces, path, &preview, &navigation)
    {
        editor.status = error;
        return;
    }
    // 在原窗口激活源模型标签并保留返回的场景。动画采样需要完整节点树，即便上次查看的是单 primitive。
    let tab = media.expanded.as_mut().expect("已激活源模型标签");
    tab.target = AssetPreviewModelTarget::Scene0;
    animation.queue_directory_animation(documents.asset_root().to_owned(), source, tab.id, action);
    editor.selected_asset = Some(path.to_owned());
    editor.status = "正在打开共享源 GLB 动画".into();
    sidebar.reveal_asset_properties();
    if let Some(layout) = layout.as_mut() {
        layout.inspector_visible = true;
    }
}
