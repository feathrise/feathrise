//! 图片/模型/音频的中央标签、显式放大和返回；复用唯一画布、加载会话和鼠标导航。

use feathrise_editor_ui::InteractionButton;
use bevy::{
    prelude::*,
    text::FontSourceTemplate,
    ui::{FocusPolicy, RelativeCursorPosition},
};
use feathrise_editor_ui::{localization::EditorLocale, metrics as ui_metrics, workspace_tabs::WorkspaceTabButton};

use super::{components::*, resources::*, types::*};
use crate::components::SceneEditorPresetTabScroll;
use crate::resources::{SceneEditorLayoutState, SceneEditorNumericScrubMouseGuard, SceneEditorState};
use crate::ui::sidebars::{EditorRightSidebarTab, SceneEditorSidebarState};
use crate::ui::vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState};
use crate::ui::{scene::FONT_PATH, theme as colors};

pub(crate) fn center_host() -> impl Scene {
    bsn! {
        AssetPreviewCenterHost
        Node { display: Display::None, width: percent(100), flex_grow: 1.0,
            min_height: px(0), min_width: px(0), flex_basis: px(0), overflow: Overflow::clip() }
    }
}

pub(super) fn expand_button() -> impl Scene {
    bsn! {
        InteractionButton AssetPreviewExpandButton ~{AssetPreviewWorkspaceAction::ToggleExpand}
        Node { display: Display::None, height: px(24), padding: UiRect::horizontal(px(6)),
            flex_shrink: 0.0, align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            border: px(1), border_radius: BorderRadius::all(px(3)) }
        BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            AssetPreviewExpandLabel Text("放大") ~{FocusPolicy::Pass}
            TextLayout::no_wrap()
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
        ]
    }
}

/// 中央占用唯一面板时，侧栏仍留有显式更换文件/返回入口，选择文件本身不替换固定标签。
pub(crate) fn sidebar_launch(
    scenes: &feathrise_editor_documents_model::DocumentScenes,
) -> impl Scene + use<> {
    bsn! {
        AssetPreviewSidebarLaunch
        Node { display: Display::None, width: percent(100), flex_direction: FlexDirection::Column,
            flex_shrink: 0.0, row_gap: px(4), padding: UiRect { left: px(4), right: px(26), top: px(4), bottom: px(4) } }
        Children [
            @crate::ui::context_help::help_button("中央媒体预览", "媒体预览已在中央展开。打开所选媒体会追加独立文件标签；返回侧栏会保留当前观察状态。选择文档可另开中央文件页，不替换已有图片或模型。")--
            @launch_button("在中央打开所选媒体", AssetPreviewWorkspaceAction::OpenSelected)--

                AssetPreviewDocumentLaunch
                Node { display: Display::None, width: percent(100), min_width: px(0), flex_shrink: 0.0 }
                Children [@super::documents::open_selected_button(scenes)]
            -- // 文档另开自己的中央页，不把新正文覆盖到固定图片/模型画布上。
            @launch_button("返回侧栏", AssetPreviewWorkspaceAction::RestoreSidebar)
        ]
    }
}

fn launch_button(label: &str, action: AssetPreviewWorkspaceAction) -> impl Scene {
    bsn! {
        InteractionButton ~{action}
        Node { height: px(24), width: percent(100), min_width: px(0),
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            border: px(1), border_radius: BorderRadius::all(px(3)) }
        BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            Text(label) @feathrise_editor_ui::localization::label_scene(label) ~{FocusPolicy::Pass}
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
        ]
    }
}

pub(super) fn media_path(path: &str) -> bool {
    matches!(
        preview_file_kind(path),
        AssetPreviewFileKind::Image | AssetPreviewFileKind::Model | AssetPreviewFileKind::Audio
    )
}

pub(super) fn actions(
    mut editor: ResMut<SceneEditorState>,
    mut sidebar: ResMut<SceneEditorSidebarState>,
    mut workspace: ResMut<VegetationPresetEditorState>,
    mut media: ResMut<AssetPreviewWorkspace>,
    state: Res<AssetPreviewState>,
    navigation: Res<AssetPreviewNavigation>,
    mut preview_focus: ResMut<AssetPreviewFocus>,
    guard: Res<SceneEditorNumericScrubMouseGuard>,
    buttons: Query<
        (
            &feathrise_editor_ui::EditorButtonClick,
            &AssetPreviewWorkspaceAction,
        ),
        Changed<feathrise_editor_ui::EditorButtonClick>,
    >,
    mut layout: Option<ResMut<SceneEditorLayoutState>>,
    return_tabs: (
        Option<Res<super::DocumentState>>,
        Option<Res<crate::ui::surface_preset_editor::SurfacePresetEditorState>>,
        Option<Res<crate::ui::water_preset_editor::WaterPresetEditorState>>,
        Option<Res<crate::ui::building_presets::BuildingPresetEditorState>>,
        Option<Res<feathrise_editor_core::WorkspaceDomainState>>,
    ),
    modals: (
        Option<Res<crate::resources::EditorTextInputGuard>>,
        Option<Res<crate::resources::SceneEditorSettingsState>>,
        Option<Res<crate::resources::SceneBrowserState>>,
        Option<Res<crate::resources::SceneDeletionTask>>,
        Option<Res<crate::authoring::TerrainExternalChangeState>>,
    ),
) {
    let (text, settings, browser, deletion, conflict) = modals;
    if !editor.open
        || editor.close_requested
        || editor.new_scene_dialog_open
        || editor.pending_scene_transition.is_some()
        || guard.blocked()
        || text.is_some_and(|guard| guard.block_shortcuts)
        || settings.is_some_and(|state| state.open)
        || browser.is_some_and(|state| state.dialog.is_some())
        || deletion.is_some_and(|state| state.confirmation.is_some())
        || conflict.is_some_and(|state| state.modal_open())
    {
        return;
    }
    for (interaction, action) in &buttons {
        if !interaction.is_clicked() {
            continue;
        }
        let central = media.active(workspace.active_tab);
        if let AssetPreviewWorkspaceAction::CloseTab(id) = action {
            if media.expanded.as_ref().is_none_or(|entry| entry.id != *id) {
                media.parked.retain(|entry| entry.id != *id);
                continue; // 后台标签的关闭键不改变当前文件或作者页。
            }
            if central && !media.parked.is_empty() {
                media.expanded = media.parked.pop();
                media.restore_view = true;
                workspace.touch_ui();
                continue;
            }
        }
        let sidebar_visible = sidebar.right == EditorRightSidebarTab::Assets
            && layout
                .as_ref()
                .is_none_or(|layout| layout.inspector_shown());
        match action {
            AssetPreviewWorkspaceAction::ActivateTab(id) => {
                let path = media
                    .tabs()
                    .find(|entry| entry.id == *id)
                    .map(|entry| entry.path.clone());
                if let Some(path) = path {
                    if let Err(error) =
                        expand(&mut media, &mut workspace, &path, &state, &navigation)
                    {
                        editor.status = error;
                    }
                }
            }
            AssetPreviewWorkspaceAction::ToggleExpand if !central && sidebar_visible => {
                if state.external_preview.is_some() || state.selected_path != editor.selected_asset
                {
                    continue;
                }
                if let Some(path) = state
                    .selected_path
                    .as_deref()
                    .filter(|path| media_path(path))
                {
                    if let Err(error) =
                        expand(&mut media, &mut workspace, path, &state, &navigation)
                    {
                        editor.status = error;
                    }
                }
            }
            AssetPreviewWorkspaceAction::OpenSelected if sidebar_visible => {
                if let Some(path) = editor
                    .selected_asset
                    .as_deref()
                    .filter(|path| media_path(path))
                {
                    if let Err(error) =
                        expand(&mut media, &mut workspace, path, &state, &navigation)
                    {
                        editor.status = error;
                    }
                }
            }
            AssetPreviewWorkspaceAction::RestoreSidebar
            | AssetPreviewWorkspaceAction::CloseTab(_)
            | AssetPreviewWorkspaceAction::ToggleExpand
                if central
                    || matches!(
                        action,
                        AssetPreviewWorkspaceAction::RestoreSidebar
                            | AssetPreviewWorkspaceAction::CloseTab(_)
                    ) =>
            {
                let Some(expanded) = media.expanded.take() else {
                    continue;
                };
                media.restore_view = false;
                if !central {
                    continue;
                } // 关闭后台标签不抢走当前作者页或文件选择。
                let (document, surface, water, building, domains) = &return_tabs;
                let still_open = match expanded.return_tab {
                    PresetWorkspaceTab::Scene => true,
                    PresetWorkspaceTab::AssetPreview => false,
                    PresetWorkspaceTab::Document => {
                        document.as_ref().is_some_and(|state| state.expanded())
                    }
                    PresetWorkspaceTab::Vegetation(id) => workspace.has_tab(id),
                    PresetWorkspaceTab::Surface(id) => {
                        surface.as_ref().is_some_and(|state| state.has_tab(id))
                    }
                    PresetWorkspaceTab::Water(id) => {
                        water.as_ref().is_some_and(|state| state.has_tab(id))
                    }
                    PresetWorkspaceTab::Building(id) => {
                        building.as_ref().is_some_and(|state| state.has_tab(id))
                    }
                    PresetWorkspaceTab::Domain(_, _) => domains
                        .as_ref()
                        .is_some_and(|state| state.is_open(expanded.return_tab)),
                };
                workspace.active_tab = if still_open {
                    expanded.return_tab
                } else {
                    PresetWorkspaceTab::Scene
                };
                workspace.touch_ui();
                editor.selected_asset = Some(expanded.path);
                sidebar.reveal_asset_properties();
                preview_focus.browse(workspace.active_tab); // 返回作者页后继续展示归还的文件，不被作者模型抢走。
                if let Some(layout) = layout.as_mut() {
                    if let Some((content, _)) = layout.focus_restore.take() {
                        layout.content_visible = content;
                    }
                    layout.inspector_visible = true;
                    if !layout.inspector_fits {
                        layout.content_visible = false;
                    }
                    layout.drag = None;
                    layout.owns_mouse_this_frame = true;
                }
            }
            _ => (),
        }
    }
}

pub(super) fn expand(
    media: &mut AssetPreviewWorkspace,
    workspace: &mut VegetationPresetEditorState,
    path: &str,
    state: &AssetPreviewState,
    navigation: &AssetPreviewNavigation,
) -> Result<(), String> {
    if !media.tabs().any(|entry| entry.path == path) && media.tabs().count() >= MAX_MEDIA_TABS {
        return Err(
            "媒体预览标签已占满，请先关闭一个预览标签；其他文件的观察状态和试听位置仍会保留".into(),
        );
    }
    let return_tab = if workspace.active_tab == PresetWorkspaceTab::AssetPreview {
        media
            .expanded
            .as_ref()
            .map_or(PresetWorkspaceTab::Scene, |entry| entry.return_tab)
    } else {
        workspace.active_tab
    };
    let current = state.selected_path.as_deref() == Some(path) && state.external_preview.is_none();
    if media
        .expanded
        .as_ref()
        .is_none_or(|entry| entry.path != path)
    {
        let cached = media
            .parked
            .iter()
            .position(|entry| entry.path == path)
            .map(|index| media.parked.remove(index));
        if let Some(mut previous) = media.expanded.take() {
            if media.central_active
                && !media.restore_view
                && state.external_preview.is_none()
                && state.selected_path.as_deref() == Some(previous.path.as_str())
            {
                previous.view = navigation.view;
                previous.target = state.model.as_ref().map_or(default(), |model| model.target);
                previous.restore_cursor = 0;
            }
            media.parked.push(previous);
        }
        media.expanded = Some(cached.unwrap_or_else(|| {
            media.next_tab = media.next_tab.wrapping_add(1).max(1);
            ExpandedAssetPreview {
                id: media.next_tab,
                path: path.to_owned(),
                return_tab,
                view: if current { navigation.view } else { default() },
                target: if current {
                    state.model.as_ref().map_or(default(), |model| model.target)
                } else {
                    default()
                },
                restore_cursor: 0,
                audio_position: 0.0, // 新音频不继承上一文件进度，已有标签从轻量会话恢复。
            }
        }));
        if current && !media.central_active {
            let entry = media.expanded.as_mut().unwrap();
            entry.view = navigation.view;
            entry.target = state.model.as_ref().map_or(default(), |model| model.target);
            entry.restore_cursor = 0; // 从侧栏打开已有标签，采用作者刚在侧栏调整的观察状态。
        }
    } else {
        let entry = media.expanded.as_mut().unwrap();
        if current && (!media.central_active || !media.restore_view) {
            entry.view = navigation.view;
            entry.target = state.model.as_ref().map_or(default(), |model| model.target);
            entry.restore_cursor = 0; // 从侧栏再次放大同一文件采用作者刚调整的视角。
        }
    }
    media.expanded.as_mut().unwrap().return_tab = return_tab; // 从其他作者页激活旧标签也更新返回目标。
    media.restore_view = true;
    workspace.active_tab = PresetWorkspaceTab::AssetPreview;
    workspace.touch_ui();
    Ok(())
}

/// 只保存一份轻量导航快照，后台标签不持有模型树；原加载流程可以继续服务作者预览。
pub(super) fn sync_activity(
    editor: Res<SceneEditorState>,
    workspace: Res<VegetationPresetEditorState>,
    mut media: ResMut<AssetPreviewWorkspace>,
    state: Res<AssetPreviewState>,
    navigation: Res<AssetPreviewNavigation>,
) {
    if workspace.active_tab == PresetWorkspaceTab::AssetPreview
        && media.expanded.is_none()
        && !media.parked.is_empty()
    {
        media.expanded = media.parked.pop();
        media.restore_view = true;
    }
    let active = editor.open && media.active(workspace.active_tab);
    if active == media.central_active {
        return;
    }
    if media.central_active
        && !media.restore_view
        && let Some(entry) = media.expanded.as_mut()
        && state.selected_path.as_deref() == Some(entry.path.as_str())
    {
        entry.view = navigation.view;
        entry.target = state.model.as_ref().map_or(default(), |model| model.target);
        entry.restore_cursor = 0;
    }
    media.central_active = active;
    if active {
        media.restore_view = true;
    }
}

/// 重新借回唯一视图时最多核对64项目录；完整加载后才恢复倍率、轨道和两种平移。
pub(super) fn restore_navigation(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut media: ResMut<AssetPreviewWorkspace>,
    mut state: ResMut<AssetPreviewState>,
    mut navigation: ResMut<AssetPreviewNavigation>,
    mut roots: Query<&mut Visibility, With<AssetPreviewModelRoot>>,
) {
    if !media.central_active || !media.restore_view {
        return;
    }
    let Some(entry) = media.expanded.as_mut() else {
        return;
    };
    if state.selected_path.as_deref() != Some(entry.path.as_str()) {
        return;
    }
    if state.external_preview.is_some() {
        return; // 同路径的作者模型不能冒充中央文件会话，等待加载系统正式移交来源。
    }
    if matches!(
        state.content,
        AssetPreviewContent::Audio {
            phase: AssetPreviewPhase::Ready | AssetPreviewPhase::Failed,
        }
    ) {
        media.restore_view = false; // 音频不需要恢复相机；完整准备或明确失败都交给专用播放器回显。
        return;
    }
    if let AssetPreviewModelTarget::Primitive(id) = entry.target
        && let Some(model) = state.model.as_mut()
        && model.target != entry.target
    {
        if model.catalog.phase == AssetPreviewCatalogPhase::Ready {
            let start = entry.restore_cursor.min(model.catalog.choices.len()); // 热更新缩短目录也不能让恢复游标越界。
            let end = start.saturating_add(64).min(model.catalog.choices.len());
            let found = model.catalog.choices[start..end]
                .iter()
                .position(|choice| choice.id == id)
                .map(|offset| start + offset);
            entry.restore_cursor = end;
            if let Some(index) = found {
                model.catalog.page = index / super::subassets::PRIMITIVES_PER_PAGE;
                if let Err(error) = super::interaction::show_primitive(
                    index % super::subassets::PRIMITIVES_PER_PAGE,
                    &mut commands,
                    &server,
                    &mut state,
                    &mut navigation,
                    &mut roots,
                ) {
                    state.status = error;
                    entry.target = AssetPreviewModelTarget::Scene0;
                    entry.view = default();
                }
            } else if end == model.catalog.choices.len() {
                entry.target = AssetPreviewModelTarget::Scene0;
                entry.view = default(); // 源文件删掉旧目标后采用新Scene0，不能把旧单体视角应用到新几何。
            }
        } else if model.catalog.phase == AssetPreviewCatalogPhase::Failed {
            entry.target = AssetPreviewModelTarget::Scene0;
            entry.view = default();
        }
        return;
    }
    if matches!(
        state.content,
        AssetPreviewContent::Image {
            phase: AssetPreviewPhase::Ready,
            ..
        } | AssetPreviewContent::Model {
            phase: AssetPreviewPhase::Ready,
            ..
        }
    ) {
        navigation.view = entry.view;
        media.restore_view = false;
    }
}

pub(super) fn sync_presentation(
    mut commands: Commands,
    media: Res<AssetPreviewWorkspace>,
    state: Res<AssetPreviewState>,
    editor: Res<SceneEditorState>,
    server: Res<AssetServer>,
    locale: Option<Res<EditorLocale>>,
    drafts: (
        Option<Res<super::AnimationEditorState>>,
        Option<Res<super::DocumentState>>,
    ),
    mut nodes: Query<
        (
            &mut Node,
            Option<&AssetPreviewSidebarLaunch>,
            Option<&AssetPreviewExpandButton>,
            Option<&AssetPreviewDocumentLaunch>,
        ),
        Or<(
            With<AssetPreviewSidebarLaunch>,
            With<AssetPreviewExpandButton>,
            With<AssetPreviewDocumentLaunch>,
        )>,
    >,
    mut labels: Query<&mut Text>,
    expand_labels: Query<Entity, With<AssetPreviewExpandLabel>>,
    tab_buttons: Query<(&AssetPreviewWorkspaceAction, &Children), With<WorkspaceTabButton>>,
    hosts: Query<Entity, With<SceneEditorPresetTabScroll>>,
    roots: Query<Entity, With<AssetPreviewTabRoot>>,
    added: Query<
        (),
        Or<(
            Added<AssetPreviewExpandButton>,
            Added<AssetPreviewSidebarLaunch>,
            Added<SceneEditorPresetTabScroll>,
        )>,
    >,
    mut painted: Local<Option<Vec<(u64, String, bool, bool)>>>,
) {
    let locale_changed = locale.as_ref().is_some_and(|locale| locale.is_changed());
    if !media.is_changed()
        && !state.is_changed()
        && !editor.is_changed()
        && !locale_changed
        && added.is_empty()
        && !drafts.0.as_ref().is_some_and(|state| state.is_changed())
    {
        return;
    } // 稳定帧不复制路径或遍历入口。
    let file_media =
        state.selected_path.as_deref().is_some_and(media_path) && state.external_preview.is_none();
    for (mut node, launch, _, document) in &mut nodes {
        let shown = if document.is_some() {
            media.expanded.is_some()
                && editor
                    .selected_asset
                    .as_deref()
                    .is_some_and(|path| crate::asset_catalog::document_format(path).is_some())
        } else if launch.is_some() {
            media.expanded.is_some()
        } else {
            file_media
        };
        let display = if shown { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
    let locale = locale.as_deref().copied().unwrap_or_default();
    let label = if media.central_active {
        locale.text("label.return-to-sidebar")
    } else {
        locale.text("label.zoom-in")
    };
    for entity in &expand_labels {
        if let Ok(mut text) = labels.get_mut(entity) && text.0 != label {
            text.0 = label.into();
        }
    }
    let Ok(host) = hosts.single() else {
        return;
    };
    let mut key: Vec<_> =
        media
            .tabs()
            .map(|entry| {
                (
                    entry.id,
                    entry.path.clone(),
                    media.central_active
                        && media
                            .expanded
                            .as_ref()
                            .is_some_and(|current| current.id == entry.id),
                    drafts.0.as_ref().zip(drafts.1.as_ref()).is_some_and(
                        |(animation, documents)| {
                            animation.has_unsaved_draft(documents.asset_root(), &entry.path)
                        },
                    ),
                )
            })
            .collect();
    key.sort_by_key(|entry| entry.0);
    if painted.as_ref() == Some(&key) && (key.is_empty() || !roots.is_empty()) {
        // 媒体标签切换语言时仅更新标题，保留画布、活动标签和当前点击身份。
        if locale_changed {
            for (action, children) in &tab_buttons {
                let AssetPreviewWorkspaceAction::ActivateTab(id) = action else { continue };
                let Some((_, path, _, dirty)) = key.iter().find(|tab| tab.0 == *id) else { continue };
                for child in children.iter() {
                    if let Ok(mut text) = labels.get_mut(child) {
                        text.set_if_neq(Text::new(media_tab_caption(locale, path, *dirty)));
                    }
                }
            }
        }
        return;
    }
    *painted = Some(key.clone());
    for root in &roots {
        commands.entity(root).despawn();
    }
    if key.is_empty() {
        return;
    }
    let font = feathrise_editor_core::editor_font(&server, FONT_PATH);
    commands.entity(host).with_children(|parent| {
        parent
            .spawn((
                AssetPreviewTabRoot,
                RelativeCursorPosition::default(), // 文件菜单沿真实可见标签范围命中。
                crate::ui::workspace_tabs::tab_list_node(),
            ))
            .with_children(|row| {
                for (id, path, active, dirty) in key {
                    let label = media_tab_caption(locale, &path, dirty);
                    crate::ui::workspace_tabs::spawn_preset_tab(
                        row,
                        &font,
                        &label,
                        (
                            AssetPreviewWorkspaceAction::ActivateTab(id),
                            crate::ui::external_files::ExternalFileTab(path.clone()),
                            RelativeCursorPosition::default(),
                        ),
                        (
                            AssetPreviewWorkspaceAction::CloseTab(id),
                            crate::ui::external_files::ExternalFileTab(path),
                            RelativeCursorPosition::default(),
                        ),
                        active,
                        false,
                    );
                }
            });
    });
}

fn media_tab_caption(locale: EditorLocale, path: &str, dirty: bool) -> String {
    let file = path.rsplit('/').next().unwrap_or(path);
    locale.format("asset.preview.tab", &[("file", file.into()), ("dirty", if dirty { " *" } else { "" }.into())])
}

#[cfg(test)]
#[path = "workspace_tests.rs"]
mod tests;
