//! 唯一资产画布在侧栏、中央媒体页和应用提交的实际宿主间迁移；不包含任何游戏预设类型。

use bevy::prelude::*;
use crate::{game_authoring::ExternalAssetPreviewRequest, resources::SceneEditorState};
use super::{types::AssetPreviewPanel, components::AssetPreviewCenterHost, AssetPreviewWorkspace};
use crate::ui::vegetation_preset_editor::VegetationPresetEditorState;

pub(super) fn mount_asset_preview_panel(
    mut commands: Commands,
    editor: Res<SceneEditorState>,
    workspace: Res<VegetationPresetEditorState>,
    media: Option<Res<AssetPreviewWorkspace>>,
    external: Res<ExternalAssetPreviewRequest>,
    panels: Query<(Entity, &ChildOf), With<AssetPreviewPanel>>,
    mut centers: Query<(Entity, &mut Node), With<AssetPreviewCenterHost>>,
    mut original: Local<Option<Entity>>,
) {
    let Ok((panel, parent)) = panels.single() else { return; };
    let center = centers.single_mut().ok();
    let central = editor.open && media.as_ref().is_some_and(|media| media.active(workspace.active_tab));
    if original.is_none() && center.as_ref().is_none_or(|(id, _)| *id != parent.parent())
        && external.host != Some(parent.parent()) {
        *original = Some(parent.parent());
    }
    let target = if central { center.as_ref().map(|(id, _)| *id) }
        else if editor.open && external.active { external.host.or(*original) }
        else { *original };
    if let Some(target) = target && target != parent.parent() {
        commands.entity(panel).insert(ChildOf(target));
    }
    if let Some((_, mut node)) = center {
        let display = if central { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
    }
}
