//! 导航按钮和收藏/访问意图采用；始终复用现有目录与文件定位事务。

use super::{components::*, helpers, messages::*, resources::AssetShortcuts, types::*};
use crate::resources::{EditorAssetCatalog, SceneEditorLayoutState, SceneEditorNumericScrubMouseGuard, SceneEditorState};
use crate::ui::sidebars::SceneEditorSidebarState;
use bevy::prelude::*;

pub(super) fn control(
    mut favorites: MessageReader<SetAssetFavorite>,
    mut visits: MessageReader<AssetVisited>,
    buttons: Query<(&feathrise_editor_ui::EditorButtonClick, &ShortcutAction, Option<&ShortcutBinding>), Changed<feathrise_editor_ui::EditorButtonClick>>,
    guard: Res<SceneEditorNumericScrubMouseGuard>,
    mut state: ResMut<AssetShortcuts>,
    mut catalog: ResMut<EditorAssetCatalog>,
    mut editor: ResMut<SceneEditorState>,
    mut sidebar: ResMut<SceneEditorSidebarState>,
    layout: Option<Res<SceneEditorLayoutState>>,
    floating: Option<Res<crate::ui::file_manager::browser::BrowserSessions>>,
    mut floating_navigation: Option<ResMut<Messages<crate::ui::file_manager::browser::BrowserNavigate>>>,
) {
    for request in favorites.read() {
        let result = helpers::target(&request.path, request.is_directory).and_then(|target| {
            helpers::change(&mut state, Change::Favorite(target, request.favorite))
        });
        editor.status = match result {
            Ok(()) => format!(
                "{} assets/{}",
                if state.is_favorite(&request.path, request.is_directory) {
                    "已收藏"
                } else {
                    "已取消收藏"
                },
                request.path
            ),
            Err(error) => error,
        };
    }
    for visit in visits.read() {
        if let Ok(target) = helpers::target(&visit.path, visit.is_directory)
            && let Err(error) = helpers::change(&mut state, Change::Visit(target))
        {
            state.feedback = error;
        }
    }
    let shortcut_window = floating.as_ref().and_then(|browser| browser.shortcut_window());
    let hosted = shortcut_window.is_some();
    let in_file_window = hosted && floating.as_ref().is_some_and(|browser| browser.accepts_service_input());
    if (guard.blocked() && !in_file_window)
        || !editor.open
        || editor.close_requested
        || editor.pending_scene_transition.is_some()
        || editor.new_scene_dialog_open
        || (!hosted && !sidebar.assets_browser_visible())
        || (!hosted && layout.as_ref().is_some_and(|layout| !layout.content_shown()))
    {
        return;
    }
    for (interaction, action, binding) in &buttons {
        if !interaction.is_clicked() {
            continue;
        }
        match action {
            ShortcutAction::Collapse(section) => {
                let index = usize::from(*section == Section::Recent);
                state.expanded[index] = !state.expanded[index];
            }
            ShortcutAction::Refresh => {
                state.check_requested = true;
                state.failed_revision = None;
            }
            ShortcutAction::ClearRecent => {
                if let Err(error) = helpers::change(&mut state, Change::ClearRecent) {
                    editor.status = error;
                }
            }
            ShortcutAction::Locate(section) | ShortcutAction::Remove(section) => {
                let Some(target) = binding.and_then(|binding| binding.0.as_ref()) else {
                    continue;
                };
                let Some(entry) = state
                    .entries(*section)
                    .iter()
                    .find(|entry| entry.target == *target)
                else {
                    continue;
                };
                if matches!(action, ShortcutAction::Remove(_)) {
                    if let Err(error) =
                        helpers::change(&mut state, Change::Remove(*section, target.clone()))
                    {
                        editor.status = error;
                    }
                } else if !entry.available {
                    editor.status =
                        format!("路径已失效：{}；可检查路径后重试或点击 × 移除", target.path);
                } else {
                    if let Some(window) = shortcut_window {
                        if let (Ok(path), Some(navigation)) = (helpers::asset_path(target), floating_navigation.as_mut()) {
                            navigation.write(crate::ui::file_manager::browser::BrowserNavigate {
                                window, path: path.to_owned(), directory: target.is_directory });
                        }
                        break; // 同一收藏在当前文件窗口定位，不改变固定左栏目录。
                    }
                    let result = helpers::asset_path(target).and_then(|path| {
                        if target.is_directory {
                            crate::asset_catalog::request_asset_folder(
                                &mut catalog,
                                &mut editor,
                                &mut sidebar,
                                path,
                            )
                        } else {
                            crate::asset_catalog::request_asset_reveal(
                                &mut catalog,
                                &mut editor,
                                &mut sidebar,
                                path,
                            )
                        }
                    });
                    if let Err(error) = result {
                        editor.status = error;
                    }
                }
            }
        }
        break; // 每次真实按下只执行一个按钮，不让子按钮连带定位父行。
    }
}
