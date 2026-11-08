//! 紧凑可折叠收藏/最近访问区；固定84行池，内容少时按真实行数收缩。

use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::{localization::EditorLocale, metrics as ui_metrics};
use super::{components::*, constants::*, resources::AssetShortcuts, types::*};
use crate::ui::{scene::FONT_PATH, theme as colors};
use bevy::{
    prelude::*,
    text::FontSourceTemplate,
    ui::{FocusPolicy, RelativeCursorPosition},
};

fn button(label: &str, action: ShortcutAction) -> impl Scene {
    bsn! {
        InteractionButton ~{action}
        Node { height: px(24), padding: UiRect::horizontal(px(5)), align_items: AlignItems::Center, flex_shrink: 0.0 }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
        Children [(
            Text(label) @feathrise_editor_ui::localization::label_scene(label) ~{FocusPolicy::Pass} TextLayout::no_wrap()
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
        )]
    }
}

fn section(section: Section, initial: &str, action: ShortcutAction, label: &str) -> impl Scene {
    let (help_title, help_copy) = match section {
        Section::Favorites => ("资产收藏", "右键文件或文件夹，选择“收藏”。检查路径可更新失效标记；点击 × 仅移除导航记录。"),
        Section::Recent => ("最近访问", "浏览文件或进入目录后记录在这里。点击条目定位原路径；清空仅移除最近访问记录。"),
    };
    bsn! {
        Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, flex_shrink: 0.0, row_gap: px(2) }
        Children [(
            Node { width: percent(100), min_height: px(24), column_gap: px(3), flex_shrink: 0.0 }
            Children [(
                InteractionButton ~{ShortcutAction::Collapse(section)}
                Node { flex_grow: 1.0, min_width: px(0), padding: UiRect::horizontal(px(5)), align_items: AlignItems::Center }
                BackgroundColor(colors::SLOT_BG)
                Children [(
                    // 极窄侧栏允许计数换行，展开命中区仍覆盖完整标题。
                    ~{ShortcutHeader(section)} Text(initial) ~{FocusPolicy::Pass}
                    Node { width: percent(100), min_width: px(0) }
                    TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) } TextColor(colors::TEXT_PRIMARY)
                )]
            ), (@button(label, action)), (
                Node { width: px(18), height: px(24), flex_shrink: 0.0 }
                Children [(@crate::ui::context_help::help_button(help_title, help_copy))]
            )]
        ), (
            ~{ShortcutEmpty(section)} Text("") ~{FocusPolicy::Pass} TextLayout::no_wrap()
            Node { display: Display::None, width: percent(100), height: px(20), flex_shrink: 0.0, overflow: Overflow::clip() }
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) } TextColor(colors::TEXT_MUTED)
        ), (
            ~{ShortcutList(section)} RelativeCursorPosition::default() ScrollPosition::default()
            Node { display: Display::None, width: percent(100), min_width: px(0), height: px(0), flex_shrink: 0.0,
                flex_direction: FlexDirection::Column, row_gap: px(2), overflow: Overflow::scroll_y() }
        )]
    }
}

pub(crate) fn panel() -> impl Scene {
    bsn! {
        Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, flex_shrink: 0.0, row_gap: px(3) }
        Children [
            (@section(Section::Favorites, "收起收藏 · 0", ShortcutAction::Refresh, "检查路径")),
            (@section(Section::Recent, "展开最近访问 · 0", ShortcutAction::ClearRecent, "清空")),
            (
                ShortcutFeedback Text("") ~{FocusPolicy::Pass}
                ~{crate::ui::status_bar::PanelStatusText("资产导航记录")} feathrise_editor_ui::feedback::GlobalPanelStatus
                Node { display: Display::None, width: percent(100), min_width: px(0), flex_shrink: 0.0 }
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) } TextColor(colors::TEXT_MUTED)
            ),
        ]
    }
}

pub(super) fn setup_rows(
    mut commands: Commands,
    lists: Query<(Entity, &ShortcutList), Added<ShortcutList>>,
    assets: Res<AssetServer>,
) {
    if lists.is_empty() {
        return;
    }
    let font: bevy::text::FontSource = feathrise_editor_core::editor_font(&assets, FONT_PATH).into();
    for (entity, list) in &lists {
        let section = list.0;
        let count = if section == Section::Favorites {
            FAVORITES
        } else {
            RECENT
        };
        commands.entity(entity).with_children(|parent| {
            for slot in 0..count {
                parent
                    .spawn((
                        ShortcutRow { section, slot },
                        Node {
                            display: Display::None,
                            width: percent(100),
                            height: px(ROW_HEIGHT),
                            min_width: px(0),
                            flex_shrink: 0.0,
                            column_gap: px(2),
                            ..default()
                        },
                    ))
                    .with_children(|row| {
                        row.spawn((
                            InteractionButton,
                            ShortcutAction::Locate(section),
                            ShortcutBinding::default(),
                            Node {
                                height: px(ROW_HEIGHT),
                                flex_grow: 1.0,
                                flex_basis: px(0),
                                min_width: px(0),
                                align_items: AlignItems::Center,
                                padding: UiRect::horizontal(px(5)),
                                overflow: Overflow::clip(),
                                ..default()
                            },
                            BackgroundColor(colors::SLOT_BG),
                            FocusPolicy::Block,
                        ))
                        .with_child((
                            ShortcutLabel,
                            Text::new(""),
                            TextLayout::no_wrap(),
                            TextFont {
                                font: font.clone(),
                                font_size: ui_metrics::FONT_CONTROL.into(),
                                ..default()
                            },
                            TextColor(colors::TEXT_PRIMARY),
                            FocusPolicy::Pass,
                        ));
                        row.spawn((
                            InteractionButton,
                            ShortcutAction::Remove(section),
                            ShortcutBinding::default(),
                            Node {
                                width: px(24),
                                height: px(ROW_HEIGHT),
                                flex_shrink: 0.0,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                ..default()
                            },
                            BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL),
                            FocusPolicy::Block,
                        ))
                        .with_child((
                            Text::new("×"),
                            TextLayout::no_wrap(),
                            TextFont {
                                font: font.clone(),
                                font_size: 12.0.into(),
                                ..default()
                            },
                            TextColor(colors::TEXT_MUTED),
                            FocusPolicy::Pass,
                        ));
                    });
            }
        });
    }
}

pub(super) fn list_height(count: usize) -> f32 {
    (count as f32 * ROW_HEIGHT + count.saturating_sub(1) as f32 * ROW_GAP).min(LIST_HEIGHT)
}

pub(super) fn sync(
    state: Res<AssetShortcuts>,
    locale: Option<Res<EditorLocale>>,
    new_rows: Query<(), Added<ShortcutRow>>,
    mut lists: Query<
        (&ShortcutList, &mut Node, &mut ScrollPosition),
        (
            Without<ShortcutRow>,
            Without<ShortcutEmpty>,
            Without<ShortcutFeedback>,
        ),
    >,
    mut rows: Query<
        (&ShortcutRow, &mut Node, &Children),
        (
            Without<ShortcutList>,
            Without<ShortcutEmpty>,
            Without<ShortcutFeedback>,
        ),
    >,
    mut bindings: Query<(&mut ShortcutBinding, &Children, Option<&mut feathrise_editor_ui::EditorButtonClick>)>,
    mut text: Query<&mut Text>,
    headers: Query<(Entity, &ShortcutHeader)>,
    mut empty: Query<
        (Entity, &ShortcutEmpty, &mut Node),
        (
            Without<ShortcutList>,
            Without<ShortcutRow>,
            Without<ShortcutFeedback>,
        ),
    >,
    mut feedback: Query<
        (Entity, &mut Node),
        (
            With<ShortcutFeedback>,
            Without<ShortcutList>,
            Without<ShortcutRow>,
            Without<ShortcutEmpty>,
        ),
    >,
    labels: Query<(), With<ShortcutLabel>>,
    mut colors_query: Query<&mut TextColor, With<ShortcutLabel>>,
    mut buttons: Query<
        (&Interaction, &mut BackgroundColor),
        (With<ShortcutAction>, Changed<Interaction>),
    >,
) {
    // 交互回显不依赖记录变更；稳定帧不重写文字或整个行池。
    for (interaction, mut color) in &mut buttons {
        color.0 = match interaction {
            Interaction::Pressed => colors::BUTTON_DEV_SMALL_PRESSED,
            Interaction::Hovered => colors::BUTTON_DEV_SMALL_HOVERED,
            _ => colors::BUTTON_DEV_SMALL_NORMAL,
        };
    }
    if !state.is_changed() && new_rows.is_empty()
        && !locale.as_ref().is_some_and(|locale| locale.is_changed()) {
        return;
    }
    let locale = locale.as_deref().copied().unwrap_or_default();
    for (list, mut node, mut scroll) in &mut lists {
        let count = state.entries(list.0).len();
        node.display = if state.expanded(list.0) && count > 0 {
            Display::Flex
        } else {
            Display::None
        };
        node.height = px(list_height(count));
        let maximum = (count as f32 * ROW_HEIGHT + count.saturating_sub(1) as f32 * ROW_GAP
            - LIST_HEIGHT)
            .max(0.0);
        scroll.y = scroll.y.clamp(0.0, maximum); // 删除或收缩后不能留下只有底部空白的旧滚动偏移。
    }
    for (row, mut node, children) in &mut rows {
        let entry = state.entries(row.section).get(row.slot);
        node.display = if entry.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        for child in children {
            let Ok((mut binding, descendants, click)) = bindings.get_mut(*child) else {
                continue;
            };
            let target = entry.map(|entry| entry.target.clone());
            if binding.0 != target {
                if let Some(mut click) = click { click.invalidate(); } // 收藏/最近条目重排不能改派仍按住的按钮。
                binding.0 = target;
            }
            for label in descendants {
                if !labels.contains(*label) {
                    continue;
                }
                let value = entry.map_or(String::new(), |entry| {
                    format!(
                        "{}{}{} {}",
                        if entry.available { "" } else { locale.text("asset.shortcuts.invalid") },
                        if entry.available { "" } else { " " },
                        if entry.target.is_directory {
                            locale.text("asset.shortcuts.folder")
                        } else {
                            locale.text("asset.shortcuts.file")
                        },
                        entry
                            .target
                            .path
                            .strip_prefix("assets/")
                            .unwrap_or(&entry.target.path)
                    )
                });
                if let Ok(mut label_text) = text.get_mut(*label)
                    && label_text.0 != value
                {
                    label_text.0 = value;
                }
                if let Ok(mut color) = colors_query.get_mut(*label) {
                    let value = if entry.is_some_and(|entry| entry.available) {
                        colors::TEXT_PRIMARY
                    } else {
                        colors::TEXT_MUTED
                    };
                    if color.0 != value {
                        color.0 = value;
                    }
                }
            }
        }
    }
    for (entity, header) in &headers {
        // 状态决定整句文案，避免分别翻译动词和类别导致英文词序或截断问题。
        let key = match (header.0, state.expanded(header.0)) {
            (Section::Favorites, true) => "asset.shortcuts.favorites.hide",
            (Section::Favorites, false) => "asset.shortcuts.favorites.show",
            (Section::Recent, true) => "asset.shortcuts.recent.hide",
            (Section::Recent, false) => "asset.shortcuts.recent.show",
        };
        let value = locale.format(key, &[("count", state.entries(header.0).len().to_string())]);
        if let Ok(mut label) = text.get_mut(entity)
            && label.0 != value
        {
            label.0 = value;
        }
    }
    for (entity, section, mut node) in &mut empty {
        node.display = if state.loaded && state.expanded(section.0) && state.entries(section.0).is_empty() {
            Display::Flex
        } else {
            Display::None
        };
        let value = if section.0 == Section::Favorites {
            locale.text("asset.shortcuts.favorites.empty")
        } else {
            locale.text("asset.shortcuts.recent.empty")
        };
        if let Ok(mut label) = text.get_mut(entity)
            && label.0 != value
        {
            label.0 = value.into();
        }
    }
    for (entity, mut node) in &mut feedback {
        // 操作说明放标题问号；原唯一反馈源承接加载或故障，由底栏统一收集。
        let source = if state.feedback.is_empty() && !state.loaded {
            "正在载入导航记录…"
        } else {
            state.feedback.as_str()
        };
        let value = locale.editor_notice(source);
        node.display = if value.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
        if let Ok(mut label) = text.get_mut(entity)
            && label.as_str() != value.as_ref()
        {
            label.0 = value.into_owned();
        }
    }
}
