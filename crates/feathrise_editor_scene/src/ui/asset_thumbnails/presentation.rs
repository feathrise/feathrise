//! 列表/缩略图工具条、固定槽位子节点和自适应网格；不重建资产行。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::{localization::{EditorLocale, label_scene}, metrics as ui_metrics};
use super::{components::*, constants::*, resources::ThumbnailState, types::ThumbnailResult};
use crate::components::{SceneEditorAssetList, SceneEditorAssetRow, SceneEditorAssetRowText};
use crate::resources::{EditorAssetCatalog, SceneEditorState};
use crate::ui::{scene::FONT_PATH, theme as colors};
use bevy::prelude::*;
use bevy::text::FontSourceTemplate;
use bevy::ui::FocusPolicy;
use feathrise_scene_content::SceneAssetKind;
use feathrise_editor_core::EditorSettings;
use feathrise_editor_core::AssetBrowserViewMode;

fn mode_button(label: &str, mode: AssetBrowserViewMode) -> impl Scene {
    let minimum = if mode == AssetBrowserViewMode::Thumbnails { px(86) } else { px(38) };
    bsn! {
        InteractionButton ViewModeButton(mode)
        Node { height: px(26), flex_grow: 1.0, flex_basis: px(0), min_width: minimum,
            border: px(1), border_radius: BorderRadius::all(px(3)),
            align_items: AlignItems::Center, justify_content: JustifyContent::Center }
        BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            Text(label) @label_scene(label) TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
        ]
    }
}

pub(crate) fn view_mode_controls() -> impl Scene {
    bsn! {
        Node { height: px(26), flex_grow: 1.0, flex_basis: px(128), min_width: px(128), column_gap: px(4), flex_shrink: 0.0 }
        Children [
            @mode_button("列表", AssetBrowserViewMode::List)--
            @mode_button("缩略图", AssetBrowserViewMode::Thumbnails)
        ]
    }
}

/// 每个原按钮增加固定图片与类别占位，不改变鼠标命中身份。
pub(crate) fn spawn_thumbnail_nodes(
    parent: &mut ChildSpawnerCommands,
    slot: usize,
    font: &Handle<Font>,
) {
    parent.spawn((
        ThumbnailImage(slot),
        ImageNode::default(),
        FocusPolicy::Pass,
        Node {
            display: Display::None,
            width: px(78),
            height: px(78),
            align_self: AlignSelf::Center,
            flex_shrink: 0.0,
            ..default()
        }, // RTT保持方形，不能被宽卡片拉伸模型/原图。
    ));
    parent
        .spawn((
            ThumbnailPlaceholder(slot),
            FocusPolicy::Pass,
            Node {
                display: Display::None,
                width: percent(100),
                height: px(78),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(3),
                ..default()
            },
        ))
        .with_children(|placeholder| {
            placeholder
                .spawn((
                    FocusPolicy::Pass,
                    Node {
                        width: px(34),
                        height: px(30),
                        ..default()
                    },
                ))
                .with_children(|icon| {
                    // 铜线文件/文件夹图标用原生Node构造，不依赖字体符号或外部图标资产。
                    for part in 0..4 {
                        icon.spawn((
                            ThumbnailIcon { slot, part },
                            FocusPolicy::Pass,
                            Node {
                                position_type: PositionType::Absolute,
                                border: UiRect::all(px(1)),
                                ..default()
                            },
                            BorderColor::all(colors::COPPER_BORDER_WEAK),
                        ));
                    }
                });
            placeholder.spawn((
                ThumbnailStatus(slot),
                Text::new("等待预览"),
                FocusPolicy::Pass,
                TextFont {
                    font: font.clone().into(),
                    font_size: ui_metrics::FONT_CONTROL.into(),
                    ..default()
                },
                TextColor(colors::TEXT_HINT),
                Node {
                    max_width: percent(100),
                    height: px(15),
                    overflow: Overflow::clip(),
                    ..default()
                },
            ));
        });
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GridLayout {
    pub columns: usize,
    pub width: f32,
    pub height: f32,
}

pub(crate) fn grid_layout(width: f32, count: usize) -> GridLayout {
    let width = if width.is_finite() {
        width.max(96.0)
    } else {
        260.0
    };
    // 左栏默认232px扣除内边距后仍可排两列；100px单格能容纳原78px快照及卡片边距。
    let columns = (((width + TILE_GAP) / (100.0 + TILE_GAP)).floor() as usize).clamp(1, 4);
    let rows = count.div_ceil(columns);
    GridLayout {
        columns,
        width: (width - TILE_GAP * (columns - 1) as f32) / columns as f32,
        height: rows as f32 * TILE_HEIGHT + rows.saturating_sub(1) as f32 * TILE_GAP,
    }
}

#[allow(clippy::type_complexity)]
pub(super) fn sync_presentation(
    settings: Res<EditorSettings>,
    locale: Option<Res<EditorLocale>>,
    catalog: Res<EditorAssetCatalog>,
    editor: Res<SceneEditorState>,
    state: Res<ThumbnailState>,
    mut pools: Query<
        (&mut Node, &ComputedNode),
        (
            With<SceneEditorAssetList>,
            Without<SceneEditorAssetRow>,
            Without<SceneEditorAssetRowText>,
        ),
    >,
    mut rows: Query<
        (
            &SceneEditorAssetRow,
            &mut Node,
            &mut BorderColor,
            &mut BackgroundColor,
            &Children,
            &UiInteraction,
        ),
        (Without<SceneEditorAssetList>, Without<SceneEditorAssetRowText>),
    >,
    mut texts: Query<
        (&mut Node, &mut TextLayout),
        (
            With<SceneEditorAssetRowText>,
            Without<SceneEditorAssetRow>,
            Without<SceneEditorAssetList>,
        ),
    >,
    mut images: Query<
        (&ThumbnailImage, &mut ImageNode, &mut Node),
        (
            Without<SceneEditorAssetList>,
            Without<SceneEditorAssetRow>,
            Without<SceneEditorAssetRowText>,
            Without<ThumbnailPlaceholder>,
            Without<ThumbnailIcon>,
            Without<ThumbnailStatus>,
        ),
    >,
    mut placeholders: Query<
        (&ThumbnailPlaceholder, &mut Node),
        (
            Without<SceneEditorAssetList>,
            Without<SceneEditorAssetRow>,
            Without<SceneEditorAssetRowText>,
            Without<ThumbnailImage>,
            Without<ThumbnailIcon>,
            Without<ThumbnailStatus>,
        ),
    >,
    mut icons: Query<
        (&ThumbnailIcon, &mut Node),
        (
            Without<SceneEditorAssetList>,
            Without<SceneEditorAssetRow>,
            Without<SceneEditorAssetRowText>,
            Without<ThumbnailImage>,
            Without<ThumbnailPlaceholder>,
            Without<ThumbnailStatus>,
        ),
    >,
    mut statuses: Query<(&ThumbnailStatus, &mut Text), Without<SceneEditorAssetRowText>>,
    mut buttons: Query<
        (
            &ViewModeButton,
            &UiInteraction,
            &mut BorderColor,
            &mut BackgroundColor,
        ),
        Without<SceneEditorAssetRow>,
    >,
) {
    if !editor.open {
        return;
    }
    let thumbnails = settings.asset_browser_view_mode == AssetBrowserViewMode::Thumbnails;
    let visible = catalog.entries.len();
    let Ok((mut pool, computed)) = pools.single_mut() else {
        return;
    };
    let layout = grid_layout(computed.size().x * computed.inverse_scale_factor(), visible);
    let height = if thumbnails {
        layout.height
    } else {
        visible as f32 * ui_metrics::ROW_HEIGHT + visible.saturating_sub(1) as f32 * 2.0
    };
    let display = if visible == 0 {
        Display::None
    } else {
        Display::Flex
    };
    // 仅差异字段写入，任务非阻塞轮询不会导致稳定UI每帧重新布局。
    if pool.height != px(height) {
        pool.height = px(height);
    }
    if pool.display != display {
        pool.display = display;
    }
    for (button, interaction, mut border, mut background) in &mut buttons {
        let selected = button.0 == settings.asset_browser_view_mode;
        let target_border = BorderColor::all(if selected || *interaction != UiInteraction::None {
            colors::COPPER_BORDER
        } else {
            colors::COPPER_BORDER_WEAK
        });
        let target_background = BackgroundColor(match (*interaction, selected) {
            (UiInteraction::Pressed, _) => colors::SELECT_BG_PRESSED,
            (UiInteraction::Hovered, _) => colors::SELECT_BG_HOVER,
            (UiInteraction::None, true) => colors::SELECT_BG,
            (UiInteraction::None, false) => colors::SLOT_BG,
        });
        if *border != target_border {
            *border = target_border;
        }
        if *background != target_background {
            *background = target_background;
        }
    }
    for (row, mut node, mut border, mut background, children, interaction) in &mut rows {
        let Some(entry) = row.entry_index.and_then(|index| catalog.entries.get(index)) else {
            continue;
        };
        let mut target = node.clone();
        target.position_type = PositionType::Absolute;
        target.left = if thumbnails {
            px((row.entry_index.unwrap_or_default() % layout.columns) as f32 * (layout.width + TILE_GAP))
        } else {
            px(0)
        };
        target.top = if thumbnails {
            px((row.entry_index.unwrap_or_default() / layout.columns) as f32 * (TILE_HEIGHT + TILE_GAP))
        } else {
            px(row.entry_index.unwrap_or_default() as f32 * (ui_metrics::ROW_HEIGHT + 2.))
        };
        target.width = if thumbnails {
            px(layout.width)
        } else {
            percent(100)
        };
        target.height = px(if thumbnails { TILE_HEIGHT } else { ui_metrics::ROW_HEIGHT });
        target.border = if thumbnails { UiRect::all(px(1)) } else { UiRect::ZERO };
        target.flex_direction = if thumbnails {
            FlexDirection::Column
        } else {
            FlexDirection::Row
        };
        target.padding = if thumbnails {
            UiRect::all(px(4))
        } else {
            UiRect::horizontal(px(5))
        };
        if *node != target {
            *node = target;
        }
        let selected = editor.selected_asset.as_deref() == Some(&entry.asset_path);
        let target_border = BorderColor::all(if selected || *interaction != UiInteraction::None {
            colors::COPPER_BORDER
        } else {
            colors::COPPER_BORDER_WEAK
        });
        let target_background = BackgroundColor(match (*interaction, selected) {
            (UiInteraction::Pressed, _) => colors::SELECT_BG_PRESSED,
            (UiInteraction::Hovered, _) => colors::SELECT_BG_HOVER,
            (UiInteraction::None, true) => colors::SELECT_BG,
            (UiInteraction::None, false) => colors::PANEL_BG,
        });
        if *border != target_border {
            *border = target_border;
        }
        if *background != target_background {
            *background = target_background;
        }
        for child in children.iter() {
            if let Ok((mut text_node, mut text_layout)) = texts.get_mut(child) {
                let height = if thumbnails { px(28) } else { Val::Auto };
                if text_node.height != height {
                    text_node.height = height;
                }
                let width = if thumbnails { percent(100) } else { px(0) };
                if text_node.width != width {
                    text_node.width = width;
                }
                let grow = if thumbnails { 0.0 } else { 1.0 };
                let basis = if thumbnails { Val::Auto } else { px(0) };
                if text_node.flex_grow != grow { text_node.flex_grow = grow; }
                if text_node.flex_basis != basis { text_node.flex_basis = basis; }
                if text_node.min_width != px(0) {
                    text_node.min_width = px(0);
                }
                if text_node.overflow != Overflow::clip() {
                    text_node.overflow = Overflow::clip();
                }
                let linebreak = if thumbnails {
                    bevy::text::LineBreak::AnyCharacter
                } else {
                    bevy::text::LineBreak::NoWrap
                };
                if text_layout.linebreak != linebreak {
                    text_layout.linebreak = linebreak;
                }
                // 列表单行裁切、卡片两行名称共享固定实体，不随长文件名撑开行池。
            }
        }
    }
    let source_for_slot = |slot: usize| (slot < catalog.visible_count).then(|| catalog.entries.get(catalog.visible_start + slot)).flatten();
    for (marker, mut image, mut node) in &mut images {
        let ready = source_for_slot(marker.0)
            .and_then(|entry| state.cache.get(&entry.asset_path))
            .and_then(|entry| match &entry.result {
                ThumbnailResult::Ready(image) => Some(image.clone()),
                _ => None,
            });
        let display = if thumbnails && ready.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        let handle = ready.filter(|_| thumbnails).unwrap_or_default();
        if image.image != handle {
            image.image = handle;
        } // 翻页和切回列表立即释放旧槽位图片引用。
    }
    for (marker, mut node) in &mut placeholders {
        let ready = source_for_slot(marker.0)
            .and_then(|entry| state.cache.get(&entry.asset_path))
            .is_some_and(|entry| matches!(entry.result, ThumbnailResult::Ready(_)));
        let display = if thumbnails && !ready {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    let locale = locale.as_deref().copied().unwrap_or_default();
    for (marker, mut text) in &mut statuses {
        let label = source_for_slot(marker.0)
            .map(|entry| {
                if entry.is_directory {
                    return locale.text("asset.thumbnail.folder").into();
                }
                if let Some(cached) = state.cache.get(&entry.asset_path) {
                    return match &cached.result {
                        ThumbnailResult::Failed(error) => error.clone(),
                        ThumbnailResult::Queued
                            if state
                                .active
                                .as_ref()
                                .is_some_and(|job| job.path == entry.asset_path) =>
                        {
                            locale.text("asset.thumbnail.generating").into()
                        }
                        ThumbnailResult::Queued => locale.text("label.waiting-for-preview").into(),
                        ThumbnailResult::Ready(_) => String::new(),
                    };
                }
                if let Some(format) = crate::asset_catalog::document_format(&entry.asset_path) {
                    return if format == crate::asset_catalog::DocumentFormat::Markdown {
                        locale.text("asset.thumbnail.markdown").into()
                    } else {
                        locale.format("asset.thumbnail.document", &[("format", locale.editor_label(format.label()).into())])
                    };
                }
                if super::super::asset_preview::preview_file_kind(&entry.asset_path)
                    == super::super::asset_preview::AssetPreviewFileKind::Audio
                {
                    return locale.text("asset.thumbnail.audio").into(); // 不为音频启动图片/模型缩略图任务。
                }
                locale.text(match entry.kind {
                    SceneAssetKind::Scene | SceneAssetKind::Texture => "asset.thumbnail.inspecting",
                    SceneAssetKind::Data => "asset.thumbnail.data",
                    SceneAssetKind::Unsupported => "asset.thumbnail.read_only",
                }).into()
            })
            .unwrap_or_default();
        if text.0 != label {
            text.0 = label;
        }
    }
    for (marker, mut node) in &mut icons {
        let directory = source_for_slot(marker.slot).is_some_and(|entry| entry.is_directory);
        let mut target = node.clone();
        target.display = if marker.part == 1 && !directory || marker.part >= 2 && directory {
            Display::None
        } else {
            Display::Flex
        };
        let (left, top, width, height) = match marker.part {
            0 if directory => (1.0, 8.0, 32.0, 22.0),
            0 => (5.0, 0.0, 24.0, 30.0),
            1 => (2.0, 3.0, 14.0, 6.0),
            2 => (9.0, 9.0, 15.0, 1.0),
            _ => (9.0, 17.0, 12.0, 1.0),
        };
        target.left = px(left);
        target.top = px(top);
        target.width = px(width);
        target.height = px(height);
        if *node != target {
            *node = target;
        }
    }
}
