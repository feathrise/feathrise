//! 按变化更新固定搜索表单与16行虚拟池；稳定帧不扫描资产、重建节点或复制全文。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use super::{components::*, constants::*, resources::AssetSearchState, types::{ContentIdentity, ResultRow}};
use crate::{
    resources::EditorAssetCatalog,
    ui::{scene::FONT_PATH, theme as colors},
};
use bevy::{prelude::*, ui::FocusPolicy};

pub(super) fn setup(
    mut commands: Commands,
    server: Res<AssetServer>,
    hosts: Query<Entity, Added<SearchContent>>,
) {
    for host in &hosts {
        let font = feathrise_editor_core::editor_font(&server, FONT_PATH);
        commands.entity(host).with_children(|parent| {
            for slot in 0..ROW_SLOTS {
                parent
                    .spawn((
                        InteractionButton,
                        SearchRow(slot),
                        SearchAction::Inactive,
                        Node {
                            display: Display::None,
                            position_type: PositionType::Absolute,
                            width: percent(100),
                            height: px(ROW_HEIGHT),
                            left: px(0),
                            padding: UiRect::horizontal(px(4)),
                            align_items: AlignItems::Center,
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        BackgroundColor(colors::INPUT_BG),
                        BorderColor::all(colors::COPPER_BORDER_WEAK),
                    ))
                    .with_children(|row| {
                        // overflow只裁剪后代；独立容器限制长片段，防止文字绘制到两个按钮下面。
                        row.spawn((
                            Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0), overflow: Overflow::clip_x(), ..default() },
                            FocusPolicy::Pass,
                        )).with_child((
                            SearchLabel::Row(slot),
                            Text::new(""),
                            FocusPolicy::Pass,
                            TextLayout::no_wrap(),
                            Node { width: percent(100), min_width: px(0), ..default() },
                            TextFont {
                                font: font.clone().into(),
                                font_size: px(ui_metrics::FONT_CONTROL).into(),
                                ..default()
                            },
                            TextColor(colors::TEXT_PRIMARY),
                        ));
                        // 单文件与项目定位同时可见；紧凑双按钮复用固定行池，不因IDE数量增建结果行。
                        for in_project in [false, true] {
                            row.spawn((InteractionButton, SearchExternalRow(slot, in_project), SearchAction::Inactive,
                                Node { display: Display::None, width: px(48), height: px(20), flex_shrink: 0.0,
                                    margin: UiRect::left(px(3)),
                                    align_items: AlignItems::Center, justify_content: JustifyContent::Center,
                                    border: UiRect::all(px(1)), ..default() },
                                BackgroundColor(colors::SLOT_BG), BorderColor::all(colors::COPPER_BORDER_WEAK),
                                FocusPolicy::Block,
                            )).with_child((SearchLabel::External(in_project), Text::new(""), TextLayout::no_wrap(),
                                TextFont { font: font.clone().into(), font_size: ui_metrics::FONT_CONTROL.into(), ..default() },
                                TextColor(colors::TEXT_PRIMARY), FocusPolicy::Pass));
                        }
                    });
            }
        });
    }
}

pub(super) fn sync(
    state: Res<AssetSearchState>,
    catalog: Res<EditorAssetCatalog>,
    locale: Option<Res<feathrise_editor_ui::localization::EditorLocale>>,
    available: Res<crate::ui::external_files::ExternalEditors>,
    scrolls: Query<&ScrollPosition, With<SearchResults>>,
    mut nodes: Query<
        (
            &mut Node,
            Option<&SearchBody>,
            Option<&SearchResults>,
            Option<&SearchContent>,
            Option<&SearchRow>,
            Option<&SearchExternalRow>,
            Option<&SearchTextControls>,
            Option<&SearchReferenceControls>,
            Option<&SearchEditorControls>,
            Option<&SearchEditorOption>,
        ),
        Or<(
            With<SearchBody>,
            With<SearchResults>,
            With<SearchContent>,
            With<SearchRow>,
            With<SearchExternalRow>,
            With<SearchTextControls>,
            With<SearchReferenceControls>,
            With<SearchEditorControls>,
            With<SearchEditorOption>,
        )>,
    >,
    mut labels: Query<(&SearchLabel, &mut Text, &mut TextColor)>,
    mut actions: Query<(
        &mut SearchAction,
        Option<&SearchRow>,
        Option<&SearchExternalRow>,
        &UiInteraction,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
    changed: Query<(), (With<SearchAction>, Changed<UiInteraction>)>,
    mut previous: Local<Option<(u64, usize, bool, u64)>>,
) {
    let scroll = scrolls.single().map_or(0.0, |scroll| scroll.y);
    let start = (scroll.max(0.0) / ROW_HEIGHT).floor() as usize;
    let key = (state.ticket, start, state.open, catalog.requested_revision);
    if !state.is_changed()
        && !catalog.is_changed()
        && !locale.as_ref().is_some_and(|locale| locale.is_changed())
        && !available.is_changed()
        && previous.as_ref() == Some(&key)
        && changed.is_empty()
    {
        return;
    }
    *previous = Some(key);
    let locale = locale.as_deref().copied().unwrap_or_default();
    let result = state.current();
    let references = state.path_query();
    let ide = state.selected_editor(&available); // 两种打开方式共享所选IDE；切工具不改变按钮宽度或结果布局。
    let count = result.map_or(0, |result| result.rows.len());
    for (mut node, body, results, content, row, external, text_controls, reference_controls, editor_controls, editor_option) in &mut nodes {
        if text_controls.is_some() { node.display = if references { Display::None } else { Display::Flex }; }
        if reference_controls.is_some() { node.display = if references { Display::Flex } else { Display::None }; }
        if editor_controls.is_some() { node.display = if ide.is_some() { Display::Flex } else { Display::None }; }
        if let Some(option) = editor_option { node.display = if available.installed(option.0) { Display::Flex } else { Display::None }; }
        if body.is_some() {
            node.display = if state.open {
                Display::Flex
            } else {
                Display::None
            };
        }
        if results.is_some() {
            // 文件分组与命中均占一行；少量结果只占实际高度，多量结果沿用滚动与虚拟行池。
            node.height = px((count as f32 * ROW_HEIGHT).min(VIEW_HEIGHT));
            node.display = if state.open && count > 0 {
                Display::Flex
            } else {
                Display::None
            };
        }
        if content.is_some() {
            node.height = px(count as f32 * ROW_HEIGHT);
        }
        if let Some(row) = row {
            let index = start + row.0;
            node.display = if index < count {
                Display::Flex
            } else {
                Display::None
            };
            node.top = px(index as f32 * ROW_HEIGHT);
        }
        if let Some(slot) = external {
            node.display = if ide.is_some()
                && result.is_some_and(|result| matches!(result.rows.get(start + slot.0), Some(ResultRow::Hit(..)))) {
                Display::Flex
            } else { Display::None };
        }
    }
    for (kind, mut text, mut color) in &mut labels {
        let value = match kind {
            SearchLabel::Toggle => locale.editor_label(if state.missing_assets && state.open {
                "收起资产缺失检查"
            } else if state.missing_assets {
                "资产缺失检查"
            } else if references && state.open {
                "收起资产引用"
            } else if references {
                "资产引用"
            } else if state.open {
                "收起内容搜索 Ctrl+Shift+F"
            } else {
                "内容搜索 Ctrl+Shift+F"
            }).into(),
            SearchLabel::Status => locale.editor_notice(&state.hint).into_owned(),
            SearchLabel::ReferenceTarget => if state.missing_assets { locale.text("asset.search.missing_scope").into() }
                else { locale.format("asset.search.reference_target", &[("path", state.reference_target.as_deref().unwrap_or("").into())]) },
            SearchLabel::PathSubmit => locale.editor_label(if state.missing_assets { "重新检查" } else { "重新查找" }).into(),
            SearchLabel::External(in_project) => locale.editor_label(if *in_project { "打开工程" } else { "打开文件" }).into(),
            SearchLabel::Scope if state.missing_assets => locale.text("asset.search.scope.missing").into(),
            SearchLabel::Scope if references => locale.text("asset.search.scope.references").into(),
            SearchLabel::Scope => locale.format(
                "asset.search.scope.recursive",
                &[("folder", if state.current_folder {
                    catalog.current_folder.as_str()
                } else {
                    ""
                }.into())],
            ),
            SearchLabel::Row(slot) => match result
                .and_then(|result| result.rows.get(start + *slot).map(|row| (result, row)))
            {
                Some((result, ResultRow::File(file))) => {
                    let file = &result.files[*file];
                    color.0 = colors::TEXT_ACCENT;
                    locale.format("asset.search.result.file", &[
                        ("source", if matches!(file.identity, ContentIdentity::ProjectSource(_)) {
                            format!("{} · ", locale.text("asset.search.result.source"))
                        } else { String::new() }),
                        ("file", file.path.to_string()),
                        ("dirty", if file.dirty { " *" } else { "" }.into()),
                        ("count", file.hits.len().to_string()),
                    ])
                }
                Some((result, ResultRow::Hit(file, hit))) => {
                    color.0 = colors::TEXT_PRIMARY;
                    let hit = &result.files[*file].hits[*hit];
                    format!("  {}:{}  {}", hit.line, hit.column, hit.snippet)
                }
                None => String::new(),
            },
        };
        if text.0 != value {
            text.0 = value;
        }
    }
    for (mut action, row, external, interaction, mut background, mut border) in &mut actions {
        if let Some(slot) = row.map(|row| row.0).or_else(|| external.map(|row| row.0)) {
            let index = start + slot;
            *action = if result
                .is_some_and(|result| matches!(result.rows.get(index), Some(ResultRow::Hit(..))))
            {
                if let Some(external) = external {
                    ide.map_or(SearchAction::Inactive, |editor| SearchAction::External { ticket: state.ticket, index, editor, in_project: external.1 })
                } else { SearchAction::Row { ticket: state.ticket, index } }
            } else {
                SearchAction::Inactive
            };
        }
        let selected = match *action {
            SearchAction::AllAssets => !state.current_folder,
            SearchAction::CurrentFolder => state.current_folder,
            SearchAction::CaseSensitive => state.case_sensitive,
            SearchAction::WholeWord => state.whole_word,
            SearchAction::ChooseEditor(editor) => ide == Some(editor),
            SearchAction::MissingAssets => state.missing_assets,
            _ => false,
        };
        background.0 = if selected {
            colors::SELECT_BG
        } else if *interaction != UiInteraction::None && *action != SearchAction::Inactive {
            colors::SELECT_BG_HOVER
        } else {
            colors::SLOT_BG
        };
        *border = BorderColor::all(if selected {
            colors::INPUT_ACTIVE_BORDER
        } else {
            colors::COPPER_BORDER_WEAK
        });
    }
}
