//! 工程根与资产目录共用一个文件名输入和类型菜单触发按钮。

use super::{components::ProjectFileFilter, input::BrowserInputGate, ProjectBrowserState};
use crate::{
    asset_catalog::{AssetTypeFilter, EditorAssetCatalog, project_files::EditorProjectFileCatalog},
    resources::SceneEditorState,
    ui::{scene::FONT_PATH, theme as colors},
};
use bevy::{prelude::*, text::{EditableText, FontSourceTemplate}, ui::FocusPolicy};
use feathrise_editor_ui::{InteractionButton, UiInteraction, localization::{EditorLocale, label_scene}, metrics};

pub(super) const FILTERS: [AssetTypeFilter; 9] = [
    AssetTypeFilter::All, AssetTypeFilter::Asset, AssetTypeFilter::Glb,
    AssetTypeFilter::Model, AssetTypeFilter::Animation, AssetTypeFilter::Skeleton,
    AssetTypeFilter::Image, AssetTypeFilter::Audio, AssetTypeFilter::Video,
];

#[derive(Component, Clone, Default)]
pub(crate) struct ProjectTypeFilterButton;

#[derive(Component, Clone, Default)]
pub(super) struct FilterButtonText;

pub(super) fn type_label(filter: AssetTypeFilter, locale: EditorLocale) -> &'static str {
    locale.text(match filter {
        AssetTypeFilter::All => "project.filter.all",
        AssetTypeFilter::Asset => "project.filter.assets",
        AssetTypeFilter::Glb => "project.filter.glb",
        AssetTypeFilter::Model => "project.filter.models",
        AssetTypeFilter::Animation => "project.filter.animations",
        AssetTypeFilter::Skeleton => "project.filter.skeletons",
        AssetTypeFilter::Image => "project.filter.images",
        AssetTypeFilter::Audio => "project.filter.audio",
        AssetTypeFilter::Video => "project.filter.video",
    })
}

pub(crate) fn filter_toolbar() -> impl Scene {
    bsn! {
        Node { width: percent(100), min_width: px(0), flex_shrink: 0.0,
            flex_direction: FlexDirection::Column, row_gap: px(3) }
        Children [
            Text("筛选当前目录文件名") @label_scene("筛选当前目录文件名") ~{FocusPolicy::Pass}
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(metrics::FONT_HINT) }
                TextColor(colors::TEXT_MUTED)--
            Node { width: percent(100), min_width: px(0), height: px(26), column_gap: px(4) }
            Children [
                // 输入框消化宽度变化，类型入口使用文案的自然宽度，窄栏不裁掉按钮。
                ProjectFileFilter crate::ui::text_input::EditorTextInput
                    ~{feathrise_editor_ui::single_line_input("", 128)}
                    bevy::text::EditableTextFilter::new(|character| !character.is_control())
                    bevy::text::TextCursorStyle::default() TextLayout::no_wrap()
                    Node { height: percent(100), min_width: px(0), flex_grow: 1.0, flex_basis: px(0),
                        padding: UiRect::horizontal(px(4)), border: px(1), overflow: Overflow::clip() }
                    BackgroundColor(colors::INPUT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
                    TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(metrics::FONT_CONTROL) }
                    TextColor(colors::TEXT_VALUE)--
                InteractionButton ProjectTypeFilterButton
                    Node { height: percent(100), flex_shrink: 0.0, align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center, padding: UiRect::horizontal(px(6)),
                        border: px(1), border_radius: BorderRadius::all(px(3)) }
                    BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL) BorderColor::all(colors::COPPER_BORDER_WEAK)
                    Children [
                        FilterButtonText Text("筛选 ▼") ~{FocusPolicy::Pass} ~{Pickable::IGNORE} TextLayout::no_wrap()
                        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(metrics::FONT_CONTROL) }
                        TextColor(colors::TEXT_PRIMARY)
                    ]
            ]
        ]
    }
}

pub(super) fn edit_name_filter(
    gate: BrowserInputGate,
    editor: Res<SceneEditorState>,
    browser: Res<ProjectBrowserState>,
    mut files: ResMut<EditorProjectFileCatalog>,
    mut assets: ResMut<EditorAssetCatalog>,
    inputs: Query<Ref<EditableText>, With<ProjectFileFilter>>,
) {
    if !gate.available(&editor) { return; }
    for input in &inputs {
        if !input.is_changed() || input.is_composing() || !input.pending_edits.is_empty() || input.pending_paste.is_some() {
            continue;
        }
        let value = input.value().to_string();
        if browser.assets_visible() {
            assets.set_filename_filter(&value);
        } else if value != files.filter {
            assets.cancel_pending_reveal(); // 新名称查询优先于尚未完成的引用定位。
            files.set_filter(&value);
        }
    }
}

pub(super) fn sync_toolbar(
    browser: Res<ProjectBrowserState>,
    files: Res<EditorProjectFileCatalog>,
    assets: Res<EditorAssetCatalog>,
    locale: Option<Res<EditorLocale>>,
    mut inputs: Query<&mut EditableText, With<ProjectFileFilter>>,
    mut labels: Query<&mut Text, With<FilterButtonText>>,
    mut buttons: Query<(&UiInteraction, &mut BackgroundColor, &mut BorderColor), With<ProjectTypeFilterButton>>,
) {
    let wanted = if browser.assets_visible() { &assets.filename_filter } else { &files.filter };
    for mut input in &mut inputs {
        if !input.is_composing() && input.pending_edits.is_empty() && input.pending_paste.is_none()
            && input.value().to_string() != *wanted
        {
            *input = feathrise_editor_ui::single_line_input(wanted, 128);
        }
    }
    let locale = locale.as_deref().copied().unwrap_or_default();
    let selected = browser.type_filter != AssetTypeFilter::All;
    let label = if selected { type_label(browser.type_filter, locale) } else { locale.text("project.filter.trigger") };
    let title = format!("{label} ▼");
    for mut text in &mut labels {
        if text.0 != title { text.0.clone_from(&title); }
    }
    for (interaction, mut background, mut border) in &mut buttons {
        background.set_if_neq(BackgroundColor(match (*interaction, selected) {
            (UiInteraction::Pressed, _) => colors::SELECT_BG_PRESSED,
            (UiInteraction::Hovered, _) => colors::SELECT_BG_HOVER,
            (UiInteraction::None, true) => colors::SELECT_BG,
            (UiInteraction::None, false) => colors::BUTTON_DEV_SMALL_NORMAL,
        }));
        border.set_if_neq(BorderColor::all(if selected { colors::COPPER_BORDER } else { colors::COPPER_BORDER_WEAK }));
    }
}
