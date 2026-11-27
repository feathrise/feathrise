//! 环境页签切换、环境来源与方案库作者事务。

use bevy::{input_focus::InputFocus, prelude::*, text::{EditableText, FontSourceTemplate}};
use feathrise_editor_ui::{EditorButtonClick, InteractionButton, UiInteraction, localization::EditorLocale};
use feathrise_scene_content::{SceneEnvironmentSource, SceneSkyMode};

use crate::authoring::{
    EditorAuthoringSession, GlobalEnvironmentAuthoringState,
    environment_library::EnvironmentLibraryEditRequest as Edit,
};
use crate::resources::SceneEditorState;
use crate::ui::{
    placement_transform::{property_button, property_button_visual},
    scene::FONT_PATH,
    sidebars::{EditorRightSidebarTab, SceneEditorSidebarState},
    text_input::{EditorTextInput, EditorTextInputGuard, replace_single_line_value},
    theme as colors,
};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum EnvironmentPage {
    #[default]
    Scene,
    Library,
}

#[derive(Resource, Default)]
pub(crate) struct EnvironmentPageState {
    pub(crate) page: EnvironmentPage,
    choosing_shared: bool,
}

#[derive(Component, Clone, Copy)]
pub(crate) enum LibraryPanel {
    Page(EnvironmentPage),
    SceneOptions,
    SharedPicker,
    SelectedProfile,
    LegacyOverride,
    Simulation,
}

#[derive(Component, Clone)]
pub(crate) enum LibraryButton {
    Page(EnvironmentPage),
    ChooseShared,
    ProjectDefault,
    MakeLocal,
    UseShared(String),
    Select(String),
    Create,
    Duplicate,
    Rename,
    SetDefault,
    GlobalSkyMode(SceneSkyMode),
    SetGlobalSkybox,
    ClearGlobalSkybox,
}

#[derive(Component, Clone, Copy)]
pub(crate) struct ProfileList(pub(super) EnvironmentPage);

#[derive(Component, Clone, Copy)]
pub(crate) enum ProfileText {
    Impact,
    Skybox,
}

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct ProfileName;

pub(super) fn column(display: Display) -> Node {
    Node {
        display, width: percent(100), min_width: px(0), flex_shrink: 0.0,
        flex_direction: FlexDirection::Column, row_gap: px(3), ..default()
    }
}

pub(super) fn button_pair(first: (&str, LibraryButton), second: (&str, LibraryButton)) -> impl Scene {
    bsn! {
        Node { width: percent(100), min_width: px(0), column_gap: px(3), flex_shrink: 0.0 }
        Children [@property_button(first.0, first.1, 0.0)-- @property_button(second.0, second.1, 0.0)]
    }
}

pub(super) fn tabs() -> impl Scene {
    button_pair(("场景环境", LibraryButton::Page(EnvironmentPage::Scene)),
        ("全局环境库", LibraryButton::Page(EnvironmentPage::Library)))
}

pub(super) fn scene_options() -> impl Scene {
    bsn! { ~{LibraryPanel::SceneOptions} ~{column(Display::None)}
        Children [
            @button_pair(("继承工程默认", LibraryButton::ProjectDefault), ("选择全局方案", LibraryButton::ChooseShared))--
            ~{LibraryPanel::SharedPicker} ~{ProfileList(EnvironmentPage::Scene)} ~{column(Display::None)}--
            @property_button("创建场景独立副本", LibraryButton::MakeLocal, 0.0)
        ]
    }
}

pub(super) fn library_list() -> impl Scene {
    bsn! { ~{column(Display::Flex)} Children [
        ~{ProfileList(EnvironmentPage::Library)} ~{column(Display::Flex)}--
        @button_pair(("新建方案", LibraryButton::Create), ("复制选中方案", LibraryButton::Duplicate))
    ] }
}

pub(super) fn profile_name() -> impl Scene {
    bsn! {
        Node { width: percent(100), min_width: px(0), column_gap: px(3), height: px(26), flex_shrink: 0.0 }
        Children [
            ProfileName EditorTextInput ~{bevy::ui::FocusPolicy::Block}
            ~{bevy::input_focus::tab_navigation::TabIndex(0)}
            ~{EditableText { allow_newlines: false, ..default() }} bevy::text::TextCursorStyle::default()
            TextLayout::no_wrap()
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(feathrise_editor_ui::metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_VALUE) BackgroundColor(colors::INPUT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
            Node { flex_grow: 2.0, min_width: px(0), border: px(1), padding: UiRect::horizontal(px(4)), overflow: Overflow::clip() }
        --  @property_button("改名", LibraryButton::Rename, 0.0)
        ]
    }
}

pub(super) fn profile_text(field: ProfileText) -> impl Scene {
    bsn! {
        ~{field} Text("") TextColor(colors::TEXT_HINT)
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(feathrise_editor_ui::metrics::FONT_HINT) }
        TextLayout { linebreak: bevy::text::LineBreak::AnyCharacter }
        Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
    }
}

pub(crate) fn handle_environment_library_buttons(
    editor: Res<SceneEditorState>,
    sidebar: Res<SceneEditorSidebarState>,
    guard: Option<Res<EditorTextInputGuard>>,
    global: Res<GlobalEnvironmentAuthoringState>,
    mut state: ResMut<EnvironmentPageState>,
    mut focus: Option<ResMut<InputFocus>>,
    name: Query<(Entity, &EditableText), With<ProfileName>>,
    buttons: Query<(&LibraryButton, &EditorButtonClick), Changed<EditorButtonClick>>,
    mut requests: MessageWriter<Edit>,
) {
    let name = name.single().ok();
    let mut clear_name_focus = || {
        if let Some(focus) = focus.as_mut() && name.is_some_and(|(entity, _)| focus.get() == Some(entity)) {
            focus.clear();
        }
    };
    if !editor.open || sidebar.right != EditorRightSidebarTab::Environment {
        clear_name_focus();
        return;
    }
    if guard.is_some_and(|guard| guard.block_shortcuts) {
        return; // 候选词确认帧只由官方文本输入消费。
    }
    let Some(button) = buttons.iter().find_map(|(button, click)| click.is_clicked().then_some(button)) else { return };
    let selected = || global.selected_environment_id.clone();
    let request = match button {
        LibraryButton::Page(page) => {
            if state.page != *page {
                state.page = *page;
                state.choosing_shared = false;
                clear_name_focus();
            }
            return;
        }
        LibraryButton::ChooseShared => {
            state.choosing_shared = !state.choosing_shared;
            return;
        }
        LibraryButton::ProjectDefault => Some(Edit::UseProjectDefault),
        LibraryButton::MakeLocal => Some(Edit::MakeLocal),
        LibraryButton::UseShared(id) => Some(Edit::UseShared(id.clone())),
        LibraryButton::Select(id) => Some(Edit::Select(id.clone())),
        LibraryButton::Create => Some(Edit::Create),
        LibraryButton::Duplicate => selected().map(Edit::Duplicate),
        LibraryButton::Rename => selected().zip(name).map(|(id, (_, text))| Edit::Rename(id, text.value().to_string())),
        LibraryButton::SetDefault => selected().map(Edit::SetDefault),
        LibraryButton::GlobalSkyMode(mode) => Some(Edit::SetGlobalSkyMode(*mode)),
        LibraryButton::SetGlobalSkybox => Some(Edit::SetGlobalSelectedSkybox),
        LibraryButton::ClearGlobalSkybox => Some(Edit::ClearGlobalSkybox),
    };
    if let Some(request) = request {
        if matches!(button, LibraryButton::UseShared(_) | LibraryButton::ProjectDefault | LibraryButton::MakeLocal) {
            state.choosing_shared = false;
        }
        if matches!(button, LibraryButton::Select(_) | LibraryButton::Create | LibraryButton::Duplicate | LibraryButton::Rename) {
            clear_name_focus();
        }
        requests.write(request);
    }
}

#[derive(Default)]
pub(crate) struct LibraryPresentation {
    list_versions: [Option<(Entity, u64, EditorLocale)>; 2],
    name: Option<(String, String)>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn sync_environment_library(
    mut commands: Commands,
    editor: Res<SceneEditorState>,
    sidebar: Res<SceneEditorSidebarState>,
    session: Res<EditorAuthoringSession>,
    global: Res<GlobalEnvironmentAuthoringState>,
    state: Res<EnvironmentPageState>,
    locale: Option<Res<EditorLocale>>,
    assets: Res<AssetServer>,
    mut previous: Local<LibraryPresentation>,
    mut panels: Query<(&LibraryPanel, &mut Node)>,
    lists: Query<(Entity, &ProfileList, Option<&Children>)>,
    mut buttons: Query<(&LibraryButton, &UiInteraction, &mut BackgroundColor, &mut BorderColor)>,
    mut texts: Query<(&ProfileText, &mut Text)>,
    mut name: Query<&mut EditableText, With<ProfileName>>,
) {
    if !editor.open || sidebar.right != EditorRightSidebarTab::Environment { return }
    let library = &global.draft.library;
    let scene_source = session.draft.as_ref()
        .filter(|draft| !editor.scene_transition_in_progress
            && editor.selected_scene_id.as_ref() == Some(&draft.scene_id)
            && draft.manifest.is_data_driven())
        .map(|draft| &draft.manifest.environment.source);
    let selected_id = global.selected_environment_id.as_deref();
    let selected = global.selected_environment();
    let locale = locale.as_deref().copied().unwrap_or_default();
    let scene_uses = match scene_source {
        Some(SceneEnvironmentSource::InheritDefault) => Some(library.default_profile_id.as_str()),
        Some(SceneEnvironmentSource::Shared(id)) => Some(id.as_str()),
        _ => None,
    };
    for (panel, mut node) in &mut panels {
        let visible = match *panel {
            LibraryPanel::Page(page) => state.page == page,
            LibraryPanel::SceneOptions => scene_source.is_some(),
            LibraryPanel::SharedPicker => state.choosing_shared && scene_source.is_some(),
            LibraryPanel::SelectedProfile => selected.is_some(),
            LibraryPanel::LegacyOverride => matches!(scene_source, Some(SceneEnvironmentSource::Legacy)),
            LibraryPanel::Simulation => match state.page {
                EnvironmentPage::Scene => matches!(scene_source, Some(SceneEnvironmentSource::Local)),
                EnvironmentPage::Library => selected.is_some(),
            },
        };
        let display = if visible { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
    }
    for (button, interaction, mut background, mut border) in &mut buttons {
        let highlighted = match button {
            LibraryButton::Page(page) => state.page == *page,
            LibraryButton::ChooseShared => matches!(scene_source, Some(SceneEnvironmentSource::Shared(_))), // 候选打开由列表显隐表示，按钮高亮只确认当前来源。
            LibraryButton::ProjectDefault => matches!(scene_source, Some(SceneEnvironmentSource::InheritDefault)),
            LibraryButton::MakeLocal => matches!(scene_source, Some(SceneEnvironmentSource::Local)),
            LibraryButton::Select(id) => selected_id == Some(id.as_str()),
            LibraryButton::UseShared(id) => matches!(scene_source, Some(SceneEnvironmentSource::Shared(active)) if active == id),
            LibraryButton::SetDefault => selected_id == Some(library.default_profile_id.as_str()),
            LibraryButton::GlobalSkyMode(mode) => selected.is_some_and(|profile| profile.appearance.sky_mode == *mode),
            _ => false,
        };
        let (new_background, new_border) = property_button_visual(*interaction, highlighted);
        background.set_if_neq(new_background);
        border.set_if_neq(new_border);
    }
    for (host, list, children) in &lists {
        if state.page != list.0 || (list.0 == EnvironmentPage::Scene && !state.choosing_shared) { continue }
        let version = (host, global.library_listing_revision, locale);
        let is_library = list.0 == EnvironmentPage::Library;
        let index = usize::from(is_library);
        if previous.list_versions[index] == Some(version) { continue }
        previous.list_versions[index] = Some(version);
        // 只有库的目录版本改变才维护行。拖动天空或天体数值不重建方案列表。
        for child in children.into_iter().flat_map(|children| children.iter()) { commands.entity(child).despawn(); }
        let font = feathrise_editor_core::editor_font(&assets, FONT_PATH);
        for (id, profile) in library.profiles.iter() {
            let label = if *id == library.default_profile_id {
                format!("{} · {}", profile.name, locale.editor_label("工程默认"))
            } else { profile.name.clone() };
            let button = if is_library {
                LibraryButton::Select(id.clone())
            } else {
                LibraryButton::UseShared(id.clone())
            };
            commands.spawn((button, InteractionButton, ChildOf(host),
                Node { width: percent(100), min_width: px(0), min_height: px(26), padding: UiRect::all(px(4)),
                    flex_shrink: 0.0, border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(3)), ..default() },
                BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL), BorderColor::all(colors::COPPER_BORDER_WEAK),
            )).with_child((Text::new(label), TextColor(colors::TEXT_PRIMARY),
                TextFont::from_font_size(feathrise_editor_ui::metrics::FONT_CONTROL).with_font(font.clone()),
                TextLayout { linebreak: bevy::text::LineBreak::AnyCharacter, ..default() }, Pickable::IGNORE));
        }
    }
    if state.page != EnvironmentPage::Library { return }
    let selected_name = selected_id.zip(selected.map(|profile| profile.name.as_str()));
    if previous.name.as_ref().map(|(id, name)| (id.as_str(), name.as_str())) != selected_name
        && let Ok(mut input) = name.single_mut()
    {
        replace_single_line_value(&mut input, selected_name.map_or("", |(_, name)| name), None);
        previous.name = selected_name.map(|(id, name)| (id.to_owned(), name.to_owned()));
    }
    for (field, mut text) in &mut texts {
        if !global.is_changed() && !session.is_changed() && !state.is_changed() && !text.is_added() {
            continue; // 延续已显示的说明和路径，稳定帧不重新分配文案。
        }
        let message = match field {
            ProfileText::Impact if scene_uses.is_some() && scene_uses == selected_id =>
                "当前场景正在使用此方案。修改将影响所有引用此方案的场景。".to_owned(),
            ProfileText::Impact if selected_id == Some(library.default_profile_id.as_str()) =>
                "工程默认方案影响继承工程默认的场景，也影响直接引用它的场景。".to_owned(),
            ProfileText::Impact => "修改将影响引用此方案的场景。".to_owned(),
            ProfileText::Skybox => format!("天空资源  {}", selected.and_then(|profile| profile.appearance.skybox_path.as_deref()).unwrap_or("未设置")),
        };
        if text.as_str() != message { **text = message; }
    }
}
