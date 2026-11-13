//! 固定行池投影为列表或缩略卡片；只处理每窗可见项，烘焙和缓存均借用原资产缩略图服务。

use super::super::{
    components::{FileRow, FileRowIcon, RowLabel},
    resources::FileManagerState,
};
use super::{components::*, resources::BrowserSessions, types::*};
use crate::ui::{
    asset_thumbnails::{self, SharedThumbnailDemand, SharedThumbnails},
    theme as colors,
};
use bevy::prelude::*;
use feathrise_editor_core::AssetBrowserViewMode;
use feathrise_editor_project_browser_model::AssetTypeFilter;
use feathrise_editor_ui::{EditorButtonClick, UiInteraction, localization::EditorLocale};
use feathrise_scene_content::SceneAssetKind;
use std::collections::HashMap;

struct RowVisual {
    thumbnail: bool,
    image: Handle<Image>,
    ready: bool,
    title: String,
    hint: String,
    clips: bool,
}

pub(super) fn sync_rows(
    files: Res<FileManagerState>,
    browsers: Res<BrowserSessions>,
    thumbnails: SharedThumbnails,
    assets: Res<AssetServer>,
    locale: Option<Res<EditorLocale>>,
    mut nodes: ParamSet<(
        Query<(Entity, &FileRow, &mut Node)>,
        Query<(&ChildOf, &mut ImageNode, &mut Node), With<FileRowIcon>>,
        Query<
            (&ChildOf, &mut Text, &mut Node, Option<&RowLabel>),
            Or<(With<RowLabel>, With<RowHint>)>,
        >,
        Query<(&ClipExpander, &mut Node)>,
    )>,
) {
    let english = locale.as_deref().copied() == Some(EditorLocale::English);
    let folder = assets.load("icons/editor/tree_folder.png");
    let file = assets.load("icons/editor/tree_file.png");
    let mut visuals = HashMap::new();
    for (entity, row, mut node) in &mut nodes.p0() {
        let Some(window) = files.active(row.window) else {
            continue;
        };
        let Some(session) = browsers.sessions.get(&row.window) else {
            continue;
        };
        let thumbnail = session.view == AssetBrowserViewMode::Thumbnails;
        let grid = asset_thumbnails::grid_layout((window.rect.width() - 18.).max(96.), 0);
        let capacity = browsers.capacity(window);
        let entry = row.index.and_then(|index| window.entries.get(index));
        node.display = if row.slot < capacity && entry.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        node.position_type = PositionType::Absolute;
        node.width = if thumbnail {
            px(grid.width)
        } else {
            percent(100)
        };
        node.height = px(if thumbnail {
            asset_thumbnails::TILE_HEIGHT
        } else {
            26.
        });
        node.left = px(8.
            + if thumbnail {
                (row.slot % grid.columns) as f32 * (grid.width + asset_thumbnails::TILE_GAP)
            } else {
                0.
            });
        node.top = px(if thumbnail {
            (row.slot / grid.columns) as f32
                * (asset_thumbnails::TILE_HEIGHT + asset_thumbnails::TILE_GAP)
        } else {
            row.slot as f32 * 26.
        });
        node.flex_direction = if thumbnail {
            FlexDirection::Column
        } else {
            FlexDirection::Row
        };
        node.align_items = if thumbnail {
            AlignItems::Center
        } else {
            AlignItems::Center
        };
        node.justify_content = if thumbnail {
            JustifyContent::Center
        } else {
            JustifyContent::FlexStart
        };
        node.padding = UiRect::all(px(if thumbnail { 4. } else { 6. }));
        node.row_gap = px(if thumbnail { 3. } else { 0. });
        let Some(entry) = entry else {
            continue;
        };
        let asset = row.index.and_then(|index| session.entries.get(index));
        let path = asset.map(|entry| entry.asset_path.as_str());
        let cached = session
            .assets
            .then(|| path.and_then(|path| thumbnails.image(path)))
            .flatten();
        let ready = cached.is_some();
        let image = cached.filter(|_| thumbnail).unwrap_or_else(|| {
            if entry.directory {
                folder.clone()
            } else {
                file.clone()
            }
        });
        let kind = asset
            .map(|entry| entry.kind)
            .unwrap_or(SceneAssetKind::Unsupported);
        let hint = if entry.directory {
            if english { "Folder" } else { "文件夹" }
        } else if thumbnail
            && !ready
            && matches!(kind, SceneAssetKind::Scene | SceneAssetKind::Texture)
            && session.assets
        {
            if english {
                "Preparing preview…"
            } else {
                "正在生成预览…"
            }
        } else {
            match kind {
                SceneAssetKind::Scene => "GLB",
                SceneAssetKind::Texture => {
                    if english {
                        "Image"
                    } else {
                        "图片"
                    }
                }
                SceneAssetKind::Data => {
                    if english {
                        "Document"
                    } else {
                        "文档"
                    }
                }
                _ => {
                    if english {
                        "File"
                    } else {
                        "文件"
                    }
                }
            }
        };
        visuals.insert(
            entity,
            RowVisual {
                thumbnail,
                image,
                ready,
                title: entry.name.clone(),
                hint: hint.into(),
                clips: session.kind == AssetTypeFilter::Animation
                    && path.is_some_and(|path| {
                        session
                            .animations
                            .get(path)
                            .is_some_and(|clips| !clips.is_empty())
                    }),
            },
        );
    }
    for (parent, mut image, mut node) in &mut nodes.p1() {
        let Some(visual) = visuals.get(&parent.parent()) else {
            continue;
        };
        if image.image != visual.image {
            image.image = visual.image.clone();
        }
        let size = if visual.thumbnail {
            if visual.ready { 78. } else { 34. }
        } else {
            16.
        };
        node.width = px(size);
        node.height = px(if visual.thumbnail && !visual.ready {
            52.
        } else {
            size
        });
        node.align_self = AlignSelf::Center;
    }
    for (parent, mut text, mut node, label) in &mut nodes.p2() {
        let Some(visual) = visuals.get(&parent.parent()) else {
            continue;
        };
        if label.is_some() {
            if visual.thumbnail {
                text.0.clone_from(&visual.title);
            }
            node.max_width = percent(100);
            node.flex_shrink = 1.;
        } else {
            node.display = if visual.thumbnail {
                Display::Flex
            } else {
                Display::None
            };
            text.0.clone_from(&visual.hint);
        }
    }
    for (expander, mut node) in &mut nodes.p3() {
        node.display = if visuals
            .get(&expander.row)
            .is_some_and(|visual| visual.clips)
        {
            Display::Flex
        } else {
            Display::None
        };
    }
}

pub(super) fn demand(
    files: Res<FileManagerState>,
    browsers: Res<BrowserSessions>,
    mut demand: ResMut<SharedThumbnailDemand>,
) {
    let mut items = Vec::new();
    if !files.suspended {
        for window in files
            .windows
            .iter()
            .rev()
            .filter(|window| !window.minimized)
        {
            let Some(session) = browsers.sessions.get(&window.id).filter(|session| {
                session.assets
                    && session.applied == window.revision
                    && session.view == AssetBrowserViewMode::Thumbnails
                    && !window.loading
                    && !browsers.service_open(window.id)
            }) else {
                continue;
            };
            for entry in session
                .entries
                .iter()
                .skip(window.first)
                .take(browsers.capacity(window))
            {
                if !entry.is_directory
                    && matches!(entry.kind, SceneAssetKind::Scene | SceneAssetKind::Texture)
                    && !items.iter().any(|(path, _)| path == &entry.asset_path)
                {
                    items.push((entry.asset_path.clone(), entry.kind));
                }
            }
        }
    }
    demand.replace(items);
}

pub(super) fn sync_toolbar(
    browsers: Res<BrowserSessions>,
    assets: Res<AssetServer>,
    locale: Option<Res<EditorLocale>>,
    buttons: Query<&BrowserButton>,
    mut labels: Query<(&ChildOf, &BrowserButtonLabel, &mut Text)>,
    mut icons: Query<(&BrowserToolIcon, &mut ImageNode)>,
    mut appearance: Query<(
        &BrowserButton,
        &UiInteraction,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
) {
    let english = locale.as_deref().copied() == Some(EditorLocale::English);
    for (parent, marker, mut text) in &mut labels {
        if marker.0 != BrowserAction::Filter {
            continue;
        }
        if let Ok(button) = buttons.get(parent.parent()) {
            let kind = browsers
                .sessions
                .get(&button.window)
                .map_or(Default::default(), |session| session.kind);
            text.0 = format!("{} ▼", type_label(kind, english));
        }
    }
    for (icon, mut image) in &mut icons {
        image.image = assets.load(icon.0);
    }
    for (button, interaction, mut background, mut border) in &mut appearance {
        let selected = match button.action {
            BrowserAction::Filter => browsers
                .sessions
                .get(&button.window)
                .is_some_and(|session| session.kind != AssetTypeFilter::All),
            BrowserAction::View => browsers
                .sessions
                .get(&button.window)
                .is_some_and(|session| session.view == AssetBrowserViewMode::Thumbnails),
            BrowserAction::Search | BrowserAction::Missing => browsers
                .service
                .as_ref()
                .is_some_and(|host| host.window == button.window && host.search),
            BrowserAction::Favorites | BrowserAction::Recent => browsers
                .service
                .as_ref()
                .is_some_and(|host| host.shows_shortcuts(button.window, button.action == BrowserAction::Recent)),
            _ => false,
        };
        background.0 = if *interaction == UiInteraction::Pressed {
            colors::SELECT_BG_PRESSED
        } else if *interaction == UiInteraction::Hovered {
            colors::SELECT_BG_HOVER
        } else if selected {
            colors::SELECT_BG
        } else {
            colors::BUTTON_DEV_SMALL_NORMAL
        };
        border.set_if_neq(BorderColor::all(if selected {
            colors::COPPER_BORDER
        } else {
            colors::COPPER_BORDER_WEAK
        }));
    }
}

pub(super) fn sync_popup(
    files: Res<FileManagerState>,
    browsers: Res<BrowserSessions>,
    locale: Option<Res<EditorLocale>>,
    mut overlays: Query<(&BrowserOverlay, &mut Node), Without<BrowserChoice>>,
    mut choices: Query<
        (
            &mut BrowserChoice,
            &mut Node,
            &Children,
            &mut EditorButtonClick,
        ),
        Without<BrowserOverlay>,
    >,
    mut labels: Query<&mut Text, With<ChoiceLabel>>,
    anchors: Query<(&UiGlobalTransform, &ComputedNode)>,
) {
    let english = locale.as_deref().copied() == Some(EditorLocale::English);
    for (overlay, mut node) in &mut overlays {
        let open = if overlay.service {
            browsers
                .service
                .as_ref()
                .is_some_and(|host| host.window == overlay.window)
        } else {
            browsers
                .popup
                .as_ref()
                .is_some_and(|popup| popup.window == overlay.window)
        };
        node.display = if open { Display::Flex } else { Display::None };
        if !overlay.service {
            if let Some(window) = files.active(overlay.window) {
                node.max_height = px((window.rect.height() - 142.).max(24.));
                if let Some(popup) = browsers
                    .popup
                    .as_ref()
                    .filter(|popup| popup.window == overlay.window)
                    && let Ok((transform, computed)) = anchors.get(popup.anchor)
                {
                    let point =
                        transform.translation * computed.inverse_scale_factor - window.rect.min;
                    let height = (window.rect.height() - 150.).clamp(24., 260.);
                    node.right = auto();
                    node.left =
                        px((point.x - 118.).clamp(8., (window.rect.width() - 244.).max(8.)));
                    node.top = px((point.y
                        + computed.size().y * computed.inverse_scale_factor * 0.5)
                        .clamp(108., (window.rect.height() - height - 30.).max(108.)));
                }
            }
        }
    }
    for (mut choice, mut node, children, mut click) in &mut choices {
        let open = browsers
            .popup
            .as_ref()
            .filter(|open| open.window == choice.window);
        let capacity = files.active(choice.window).map_or(1, |window| {
            (((window.rect.height() - 150.).max(24.) / 26.).floor() as usize).clamp(1, 10)
        });
        let value = open
            .filter(|_| choice.slot < capacity)
            .and_then(|open| match &open.kind {
                PopupKind::Filters => FILTERS
                    .get(open.first + choice.slot)
                    .map(|filter| (type_label(*filter, english).to_owned(), Some(*filter), None)),
                PopupKind::Clips { path, .. } => browsers
                    .sessions
                    .get(&open.window)
                    .and_then(|session| session.animations.get(path))
                    .and_then(|clips| clips.get(open.first + choice.slot))
                    .map(|clip| (clip.label.clone(), None, Some(clip.index))),
            });
        node.display = if value.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        let (label, filter, clip) = value.unwrap_or_default();
        if choice.filter != filter || choice.clip != clip {
            click.invalidate();
            choice.filter = filter;
            choice.clip = clip;
        }
        for child in children {
            if let Ok(mut text) = labels.get_mut(*child) {
                text.0.clone_from(&label);
            }
        }
    }
}
