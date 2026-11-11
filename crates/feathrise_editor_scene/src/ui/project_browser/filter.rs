//! 工程根与资产目录共用一个文件名输入和类型菜单触发按钮。

use super::{components::{FilenameHint, ProjectFileFilter}, input::BrowserInputGate, ProjectBrowserState};
use crate::{
    asset_catalog::{AssetTypeFilter, EditorAssetCatalog, project_files::EditorProjectFileCatalog},
    resources::SceneEditorState,
    ui::{scene::FONT_PATH, theme as colors},
};
use bevy::{
    input_focus::{InputFocus, tab_navigation::TabIndex},
    prelude::*,
    text::{EditableText, FontSourceTemplate},
    ui::FocusPolicy,
};
use feathrise_editor_ui::{InteractionButton, UiInteraction, localization::{EditorLocale, label_scene}, metrics};

pub(super) const FILTERS: [AssetTypeFilter; 9] = [
    AssetTypeFilter::All, AssetTypeFilter::Asset, AssetTypeFilter::Glb,
    AssetTypeFilter::Model, AssetTypeFilter::Animation, AssetTypeFilter::Skeleton,
    AssetTypeFilter::Image, AssetTypeFilter::Audio, AssetTypeFilter::Video,
];

const FILENAME_CHARS: usize = 128;

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
        Node { width: percent(100), min_width: px(0), height: px(26), flex_shrink: 0.0, column_gap: px(4) }
        Children [
            // 原生文本控件经 TabIndex 接受指针焦点，空值提示穿透到同一个输入实体。
            ProjectFileFilter crate::ui::text_input::EditorTextInput ~{TabIndex(0)} ~{FocusPolicy::Block}
                ~{feathrise_editor_ui::single_line_input("", FILENAME_CHARS)}
                bevy::text::EditableTextFilter::new(|character| !character.is_control())
                bevy::text::TextCursorStyle::default() TextLayout::no_wrap()
                Node { height: percent(100), min_width: px(0), flex_grow: 1.0, flex_basis: px(0),
                    padding: UiRect::horizontal(px(4)), border: px(1), overflow: Overflow::clip() }
                BackgroundColor(colors::INPUT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(metrics::FONT_CONTROL) }
                TextColor(colors::TEXT_VALUE)
                Children [
                    FilenameHint Pickable::IGNORE ~{FocusPolicy::Pass}
                    Node { position_type: PositionType::Absolute, left: px(4), right: px(4), top: px(0), bottom: px(0),
                        align_items: AlignItems::Center, overflow: Overflow::clip_x() }
                    Children [
                        Text("按文件名筛选") @label_scene("按文件名筛选") Pickable::IGNORE ~{FocusPolicy::Pass} TextLayout::no_wrap()
                        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(metrics::FONT_HINT) }
                        TextColor(colors::TEXT_HINT)
                    ]
                ]--
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
    focus: Option<Res<InputFocus>>,
    mut inputs: Query<(Entity, &mut EditableText, &mut BorderColor), With<ProjectFileFilter>>,
    mut hints: Query<(&ChildOf, &mut Node), With<FilenameHint>>,
    mut labels: Query<&mut Text, With<FilterButtonText>>,
    mut buttons: Query<(&UiInteraction, &mut BackgroundColor, &mut BorderColor),
        (With<ProjectTypeFilterButton>, Without<ProjectFileFilter>)>,
) {
    let wanted = if browser.assets_visible() { &assets.filename_filter } else { &files.filter };
    for (entity, mut input, mut border) in &mut inputs {
        if !input.is_composing() && input.pending_edits.is_empty() && input.pending_paste.is_none()
            && input.value().to_string() != *wanted
        {
            // 换目录时在原实体回显查询，保留已同步的字形样式和可见光标。
            crate::ui::text_input::replace_single_line_value(&mut input, wanted, FILENAME_CHARS);
        }
        let focused = focus.as_ref().and_then(|focus| focus.get()) == Some(entity);
        border.set_if_neq(BorderColor::all(if focused { colors::INPUT_ACTIVE_BORDER } else { colors::COPPER_BORDER_WEAK }));
        let display = if !focused && wanted.is_empty() { Display::Flex } else { Display::None };
        for (parent, mut hint) in &mut hints {
            if parent.parent() == entity && hint.display != display {
                hint.display = display;
            }
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
