//! 场景树顶端的配置入口与右栏属性页，只检查当前已进入的场景。

use crate::{authoring::EditorAuthoringSession, resources::SceneEditorState, ui::theme as colors};
use bevy::{prelude::*, text::FontSourceTemplate};
use feathrise_editor_ui::{EditorButtonClick, InteractionButton, UiInteraction, metrics::FONT_CONTROL};

use super::{EditorRightSidebarTab, EditorSidebarTab, SceneConfigPage, SceneEditorSidebarPage, SceneEditorSidebarState, FONT_PATH};
use crate::ui::scene_sidebar::{TREE_BRANCH_GAP, TREE_ICON_SIZE, load_tree_branch_images, sync_tree_branch_icon, tree_branch_button, tree_branch_fill, tree_row_icon_left};

#[derive(Clone, Copy, Component)]
pub(crate) enum SceneConfigAction {
    Toggle,
    Select(SceneConfigPage),
}

#[derive(Clone, Component, Default)]
pub(crate) struct SceneConfigArrow;

#[derive(Clone, Copy, Component)]
pub(crate) enum SceneConfigRegion {
    Entries,
    Preview,
    Rules,
    Waiting,
}

#[derive(Clone, Component, Default)]
pub(crate) struct SceneConfigRulesHost;

#[derive(Clone, Component, Default)]
pub(crate) struct SceneConfigEntries;

#[derive(Clone, Component, Default)]
pub(crate) struct ScenePreviewSource;

pub(crate) fn current_scene_config_id<'a>(editor: &SceneEditorState, authoring: &'a EditorAuthoringSession) -> Option<&'a feathrise_scene_content::SceneId> {
    let draft = authoring.draft.as_ref()?;
    (editor.open && !editor.scene_transition_in_progress && editor.pending_scene_transition.is_none()
        && editor.selected_scene_id.as_ref() == Some(&draft.scene_id)).then_some(&draft.scene_id)
}

pub(crate) fn handle_scene_config_input(
    editor: Res<SceneEditorState>,
    authoring: Res<EditorAuthoringSession>,
    gate: crate::ui::terrain_objects::PanelWorkspaceGate,
    mut sidebar: ResMut<SceneEditorSidebarState>,
    clicks: Query<(&SceneConfigAction, &EditorButtonClick), Changed<EditorButtonClick>>,
) {
    if !editor.open || editor.close_requested || editor.new_scene_dialog_open
        || editor.scene_switch_confirmation.is_some()
        || !gate.creation_buttons_allowed() || !sidebar.scene_tree_visible()
    {
        return;
    }
    if let Some((action, _)) = clicks.iter().find(|(_, click)| click.is_clicked()) {
        match action {
            SceneConfigAction::Toggle => sidebar.scene_config_expanded = !sidebar.scene_config_expanded,
            SceneConfigAction::Select(page) if sidebar.scene_config_expanded && current_scene_config_id(&editor, &authoring).is_some() => {
                // 选择只改属性来源。保留选中的对象、顶部工具和中央页面。
                sidebar.reveal_scene_config(*page);
            }
            SceneConfigAction::Select(_) => {}
        }
    }
}

pub(crate) fn sync_scene_config_presentation(
    editor: Res<SceneEditorState>,
    authoring: Res<EditorAuthoringSession>,
    sidebar: Res<SceneEditorSidebarState>,
    server: Option<Res<AssetServer>>,
    mut images: Local<Option<[Handle<Image>; 2]>>,
    mut regions: Query<(&SceneConfigRegion, &mut Node)>,
    mut buttons: Query<(&SceneConfigAction, &UiInteraction, &mut BackgroundColor, Has<SceneConfigArrow>)>,
    arrows: Query<&Children, With<SceneConfigArrow>>,
    mut icons: Query<&mut ImageNode>,
    mut sources: Query<(&mut Text, &mut Node), (With<ScenePreviewSource>, Without<SceneConfigRegion>)>,
) {
    let ready = current_scene_config_id(&editor, &authoring).is_some();
    let page = sidebar.scene_config_page();
    let expanded = sidebar.scene_config_expanded;
    for (region, mut node) in &mut regions {
        let shown = match region {
            SceneConfigRegion::Entries => expanded,
            SceneConfigRegion::Preview => ready && page == Some(SceneConfigPage::Preview),
            SceneConfigRegion::Rules => ready && matches!(page, Some(SceneConfigPage::Variables | SceneConfigPage::Triggers)),
            // 交替进入场景时不投影上一次的场景表单。
            SceneConfigRegion::Waiting => !ready && page.is_some(),
        };
        let display = if shown { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
    }
    if images.is_none() && let Some(server) = server.as_deref() {
        *images = Some(load_tree_branch_images(server));
    }
    for children in &arrows {
        if let Some(images) = &*images && let Some(child) = children.first()
            && let Ok(mut icon) = icons.get_mut(*child)
        {
            sync_tree_branch_icon(!expanded, true, images, &mut icon);
        }
    }
    for (action, interaction, mut fill, arrow) in &mut buttons {
        let color = match action {
            SceneConfigAction::Toggle if arrow => tree_branch_fill(true, *interaction),
            SceneConfigAction::Select(_) if !ready => BackgroundColor(colors::SLOT_BG),
            SceneConfigAction::Select(item) => BackgroundColor(super::sidebar_tab_background(*interaction, page == Some(*item))),
            SceneConfigAction::Toggle => BackgroundColor(super::sidebar_tab_background(*interaction, false)),
        };
        fill.set_if_neq(color);
    }
    // 旧地图的基础网格来源沿用当前草稿，只在预览页实有来源时占一行。
    let source = authoring.draft.as_ref().filter(|_| ready).and_then(|draft| {
        let terrain = &draft.manifest.terrain;
        if draft.manifest.terrains.is_some() || terrain.is_empty() { return None; }
        if terrain.kind == feathrise_scene_content::SceneTerrainKind::SparseClipmap {
            Some(terrain.height_path.as_deref().unwrap_or("尚未指定稀疏高度块"))
        } else {
            Some(terrain.glb_path.as_str())
        }
    });
    for (mut text, mut node) in &mut sources {
        let display = if source.is_some() { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
        if let Some(source) = source && text.strip_prefix("基础网格来源  ") != Some(source) {
            **text = format!("基础网格来源  {source}");
        }
    }
}

pub(crate) fn navigation() -> impl Scene {
    bsn! {
        Node { width: percent(100), min_width: px(0), min_height: px(24), flex_direction: FlexDirection::Column, row_gap: px(2) }
        Children [
            InteractionButton ~{Name::new("场景配置")} ~{SceneConfigAction::Toggle}
            ~{crate::ui::scene_sidebar::tree_section_header()}
            BackgroundColor(colors::PANEL_BG_STRONG)
            Children [@tree_branch_button() SceneConfigArrow ~{SceneConfigAction::Toggle}-- @caption("场景配置")]--

            ~{SceneConfigRegion::Entries} SceneConfigEntries bevy::ui::RelativeCursorPosition::default() ScrollPosition::default()
            // 矮窗口只压缩这三项的滚动区，主场景根和对象列表仍可浏览。
            Node { display: Display::None, width: percent(100), min_width: px(0), min_height: px(0),
                flex_direction: FlexDirection::Column, row_gap: px(1), overflow: Overflow::scroll_y() }
            Children [
                @entry("场景预览", SceneConfigPage::Preview)--
                @entry("场景变量", SceneConfigPage::Variables)--
                @entry("场景触发", SceneConfigPage::Triggers)
            ]
        ]
    }
}

fn caption(label: &'static str) -> impl Scene {
    bsn! {
        Text(label) @feathrise_editor_ui::localization::label_scene(label) Pickable::IGNORE TextLayout::no_wrap()
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(FONT_CONTROL) }
        TextColor(colors::TEXT_PRIMARY)
    }
}

fn entry(label: &'static str, page: SceneConfigPage) -> impl Scene {
    bsn! {
        InteractionButton ~{Name::new(label)} ~{SceneConfigAction::Select(page)}
        Node { width: percent(100), min_width: px(0), height: px(24), flex_shrink: 0.0, align_items: AlignItems::Center,
            padding: UiRect { left: tree_row_icon_left(1, TREE_ICON_SIZE), right: px(4), ..UiRect::ZERO },
            column_gap: px(TREE_BRANCH_GAP), border_radius: BorderRadius::all(px(3)), overflow: Overflow::clip_x() }
        BackgroundColor(colors::PANEL_BG_STRONG)
        Children [@caption(label)]
    }
}

pub(super) fn properties() -> impl Scene {
    bsn! {
        SceneEditorSidebarPage(EditorSidebarTab::Right(EditorRightSidebarTab::SceneConfig))
        Node { display: Display::None, width: percent(100), min_width: px(0), min_height: px(0), flex_grow: 1.0, flex_direction: FlexDirection::Column, overflow: Overflow::clip() }
        Children [
            ~{SceneConfigRegion::Preview} ~{Name::new("场景预览属性")}
            Node { display: Display::None, width: percent(100), min_width: px(0), min_height: px(0), flex_grow: 1.0, flex_direction: FlexDirection::Column }
            Children [@crate::ui::terrain_shading::terrain_shading_controls()]--

            ~{SceneConfigRegion::Rules} SceneConfigRulesHost ~{Name::new("场景规则属性")}
            crate::components::SceneEditorRightPanelScroll bevy::ui::RelativeCursorPosition::default() ScrollPosition::default()
            Node { display: Display::None, width: percent(100), min_width: px(0), min_height: px(0),
                flex_grow: 1.0, flex_basis: px(0), flex_direction: FlexDirection::Column, overflow: Overflow::scroll_y() }--

            ~{SceneConfigRegion::Waiting}
            Node { display: Display::None, padding: UiRect::all(px(6)) }
            Children [@caption("等待当前场景就绪")]
        ]
    }
}
