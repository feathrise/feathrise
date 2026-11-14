//! 同一窗口中的紧凑文件名/类型/资产工具和可收起正文，不增加预览相机或独立搜索表单。

use super::super::components::{FileRow, FileSearchHint, FileSearchInput};
use super::{components::*, types::*};
use crate::ui::{
    scene::FONT_PATH,
    theme as colors,
    tooltips::{EditorTooltipTarget, TooltipAction},
};
use bevy::{
    input_focus::tab_navigation::TabIndex,
    prelude::*,
    text::EditableTextFilter,
    ui::{FocusPolicy, RelativeCursorPosition},
};
use feathrise_editor_ui::{InteractionButton, metrics};

fn label(value: &str, font: &Handle<Font>) -> impl Bundle {
    (
        Text::new(value),
        TextLayout::no_wrap(),
        TextFont {
            font: font.clone().into(),
            font_size: metrics::FONT_CONTROL.into(),
            ..default()
        },
        TextColor(colors::TEXT_PRIMARY),
        FocusPolicy::Pass,
        Pickable::IGNORE,
    )
}

fn button(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    id: u64,
    action: BrowserAction,
    value: &str,
    icon: Option<(&'static str, TooltipAction)>,
) {
    let mut entity = parent.spawn((
        BrowserButton { window: id, action },
        InteractionButton,
        Node {
            min_width: px(24),
            height: px(26),
            flex_shrink: 0.,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(px(4)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(3)),
            ..default()
        },
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL),
        BorderColor::all(colors::COPPER_BORDER_WEAK),
    ));
    if action == BrowserAction::Search {
        entity.insert(feathrise_editor_documents_model::search::components::SearchAction::Toggle);
    }
    if action == BrowserAction::Missing {
        entity.insert(
            feathrise_editor_documents_model::search::components::SearchAction::MissingAssets,
        );
    }
    if let Some((path, tooltip)) = icon {
        entity.insert(EditorTooltipTarget(tooltip)).with_child((
            BrowserToolIcon(path),
            ImageNode::default(),
            FocusPolicy::Pass,
            Pickable::IGNORE,
            Node {
                width: px(16),
                height: px(16),
                flex_shrink: 0.,
                ..default()
            },
        ));
    } else {
        entity.with_child((BrowserButtonLabel(action), label(value, font)));
    }
}

pub(in crate::ui::file_manager) fn spawn_toolbar(parent: &mut ChildSpawnerCommands, font: &Handle<Font>, id: u64) {
    parent
        .spawn((Node {
            width: percent(100),
            height: px(34),
            flex_shrink: 0.,
            column_gap: px(3),
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(8)),
            ..default()
        },))
        .with_children(|row| {
            row.spawn((
                FileSearchInput { window: id },
                crate::ui::text_input::EditorTextInput,
                TabIndex(0),
                FocusPolicy::Block,
                feathrise_editor_ui::single_line_input("", 128),
                EditableTextFilter::new(|character| !character.is_control()),
                bevy::text::TextCursorStyle::default(),
                TextLayout::no_wrap(),
                Node {
                    height: px(26),
                    min_width: px(30),
                    flex_grow: 1.,
                    flex_basis: px(0),
                    padding: UiRect::horizontal(px(4)),
                    border: UiRect::all(px(1)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(colors::INPUT_BG),
                BorderColor::all(colors::COPPER_BORDER_WEAK),
                TextFont {
                    font: font.clone().into(),
                    font_size: metrics::FONT_CONTROL.into(),
                    ..default()
                },
                TextColor(colors::TEXT_VALUE),
            ))
            .with_child((
                FileSearchHint { window: id },
                label("按文件名筛选", font),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(4),
                    right: px(4),
                    top: px(2),
                    overflow: Overflow::clip_x(),
                    ..default()
                },
            ));
            button(row, font, id, BrowserAction::Filter, "全部 ▼", None);
            for (action, path, tooltip) in [
                (
                    BrowserAction::Search,
                    "icons/editor/tool_search.png",
                    TooltipAction::AssetSearch,
                ),
                (
                    BrowserAction::Missing,
                    "icons/editor/tool_missing_assets.png",
                    TooltipAction::AssetMissingCheck,
                ),
                (
                    BrowserAction::Favorites,
                    "icons/editor/tool_favorite.png",
                    TooltipAction::AssetFavorites,
                ),
                (
                    BrowserAction::Recent,
                    "icons/editor/tool_recent.png",
                    TooltipAction::AssetRecent,
                ),
                (
                    BrowserAction::View,
                    "icons/editor/tree_image.png",
                    TooltipAction::AssetThumbnails,
                ),
            ] {
                button(row, font, id, action, "", Some((path, tooltip)));
            }
        });
    // 覆盖正文而非扩大工具栏：窄窗仍可滚动原服务，目录原行不会在其背后接受鼠标。
    parent
        .spawn((
            BrowserOverlay {
                window: id,
                service: true,
            },
            RelativeCursorPosition::default(),
            FocusPolicy::Block,
            feathrise_editor_ui::UiInteraction::default(),
            ZIndex(4),
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                top: px(108),
                bottom: px(28),
                left: px(8),
                right: px(8),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(7)),
                row_gap: px(5),
                overflow: Overflow::scroll_y(),
                border: UiRect::all(px(1)),
                ..default()
            },
            ScrollPosition::default(),
            BackgroundColor(colors::PANEL_BG_STRONG),
            BorderColor::all(colors::COPPER_BORDER),
        ))
        .with_children(|body| {
            button(body, font, id, BrowserAction::Dismiss, "收起 ×", None);
        });
    parent
        .spawn((
            BrowserOverlay {
                window: id,
                service: false,
            },
            RelativeCursorPosition::default(),
            FocusPolicy::Block,
            feathrise_editor_ui::UiInteraction::default(),
            ZIndex(5),
            ScrollPosition::default(),
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                top: px(108),
                right: px(8),
                width: px(236),
                max_height: px(286),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(4)),
                row_gap: px(2),
                overflow: Overflow::clip(),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(colors::PANEL_BG_STRONG),
            BorderColor::all(colors::COPPER_BORDER),
        ))
        .with_children(|menu| {
            for slot in 0..16 {
                menu.spawn((
                    BrowserChoice {
                        window: id,
                        slot,
                        filter: None,
                        clip: None,
                    },
                    InteractionButton,
                    Node {
                        display: Display::None,
                        width: percent(100),
                        height: px(24),
                        flex_shrink: 0.,
                        padding: UiRect::horizontal(px(6)),
                        align_items: AlignItems::Center,
                        overflow: Overflow::clip_x(),
                        ..default()
                    },
                    BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL),
                ))
                .with_child((ChoiceLabel, label("", font)));
            }
        });
}

pub(super) fn row_extras(
    mut commands: Commands,
    rows: Query<Entity, Added<FileRow>>,
    assets: Res<AssetServer>,
) {
    let font = feathrise_editor_core::editor_font(&assets, FONT_PATH);
    for entity in &rows {
        commands.entity(entity).with_children(|row| {
            row.spawn((
                RowHint,
                label("", &font),
                Node {
                    display: Display::None,
                    height: px(16),
                    max_width: percent(100),
                    overflow: Overflow::clip_x(),
                    ..default()
                },
            ));
            row.spawn((
                ClipExpander { row: entity },
                InteractionButton,
                Node {
                    display: Display::None,
                    position_type: PositionType::Absolute,
                    right: px(3),
                    top: px(2),
                    width: px(24),
                    height: px(22),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL),
            ))
                .with_child(label("▼", &font));
        });
    }
}
