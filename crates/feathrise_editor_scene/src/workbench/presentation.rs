//! 原工作台的宿主快照采用及真实视口测量；只更新小型状态和固定实体，不复制作者算法。
use super::{WorkbenchFloatingPanel, WorkbenchModel, WorkbenchProbe};
use crate::ui::vegetation_preset_editor::{PresetWorkspaceEditorPage, PresetWorkspaceScenePage};
use crate::{
    components::*,
    resources::{SceneBrowserState, SceneEditorState},
    types::SceneBrowserItem,
};
use bevy::{
    ecs::system::SystemParam,
    prelude::*,
    ui::{InteractionDisabled, RelativeCursorPosition},
    window::PrimaryWindow,
};

/// 只检查宿主登记的固定面板根，重叠时以实际绘制层级决定滚轮归属。
#[derive(SystemParam)]
pub struct WorkbenchFloatingPanels<'w, 's> {
    panels: Query<'w, 's,
        (Entity, &'static RelativeCursorPosition, &'static ComputedNode, &'static InheritedVisibility, &'static GlobalZIndex, Option<&'static ZIndex>),
        Or<(With<WorkbenchFloatingPanel>, With<crate::ui::status_bar::StatusPopup>, With<crate::ui::context_help::ContextHelpPopup>)>,
    >,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
}

impl WorkbenchFloatingPanels<'_, '_> {
    pub fn hovered(&self) -> Option<Entity> {
        if self.windows.single().is_ok_and(|window| !window.focused) { return None; }
        // 以已完成布局的画面为准，关闭指令发出后，同帧滚轮仍留给原面板。
        self.panels.iter()
            .filter(|(_, cursor, node, visible, _, _)| visible.get() && !node.is_empty() && cursor.cursor_over())
            .max_by_key(|(_, _, _, _, layer, order)| (layer.0, order.map_or(0, |order| order.0)))
            .map(|(entity, _, _, _, _, _)| entity)
    }
}

pub(super) fn adopt_model(
    model: Res<WorkbenchModel>,
    mut previous: Local<Option<u64>>,
    mut editor: ResMut<SceneEditorState>,
    mut browser: ResMut<SceneBrowserState>,
    mut roots: Query<&mut Node, With<SceneEditorRoot>>,
    mut statuses: Query<&mut Text, With<SceneEditorStatusText>>,
    mut current_scenes: Query<
        &mut Text,
        (With<SceneEditorCurrentSceneText>, Without<SceneEditorStatusText>),
    >,
    mut workspace: ResMut<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>,
    mut clicks: Query<
        &mut feathrise_editor_ui::EditorButtonClick,
        Or<(With<SceneEditorSceneRow>, With<SceneEditorAssetRow>)>,
    >,
) {
    if !model.is_changed() && previous.is_some() {
        return;
    }
    editor.open = model.visible;
    if editor.status != model.status {
        editor.status = model.status.clone();
    }
    editor.new_scene_dialog_open = model.modal;
    let active_changed =
        editor.selected_scene_id.as_ref().map(|id| &id.0) != model.active_scene.as_ref();
    if active_changed || *previous != Some(model.context) {
        workspace.touch_ui();
    }
    editor.selected_scene_id = model
        .active_scene
        .as_ref()
        .map(|id| feathrise_scene_content::SceneId(id.clone()));
    if *previous != Some(model.context) {
        for mut click in &mut clicks {
            click.invalidate(); // 工程换代先取消旧按下，再消费任何同名场景或资产行；不等到表现阶段才失效。
        }
        *previous = Some(model.context);
        editor.scene_list_selection_id = model
            .selected_scene
            .as_ref()
            .map(|id| feathrise_scene_content::SceneId(id.clone()));
        browser.select_only(
            editor
                .scene_list_selection_id
                .clone()
                .map(SceneBrowserItem::Scene),
        );
    }
    for mut node in &mut roots {
        node.display = if model.visible {
            Display::Flex
        } else {
            Display::None
        };
    }
    for mut text in &mut statuses {
        if text.as_str() != model.status {
            **text = model.status.clone();
        }
    }
    let current = model
        .active_scene_label
        .as_deref()
        .or(model.active_scene.as_deref());
    let label = current.map_or_else(
        || "当前场景：尚未进入".to_owned(),
        |name| format!("当前场景：{name}"),
    );
    for mut text in &mut current_scenes {
        if text.as_str() != label {
            **text = label.clone();
        }
    }
}

pub(super) fn sync_pages(
    mut commands: Commands,
    model: Res<WorkbenchModel>,
    documents: Option<Res<crate::ui::asset_preview::DocumentState>>,
    mut pages: Query<
        (
            &mut Node,
            Has<PresetWorkspaceScenePage>,
            Has<SceneEditorEmptyWorkspace>,
        ),
        Or<(
            With<PresetWorkspaceScenePage>,
            With<PresetWorkspaceEditorPage>,
            With<SceneEditorEmptyWorkspace>,
        )>,
    >,
    actions: Query<(Entity, &SceneEditorActionButton, Has<InteractionDisabled>)>,
) {
    // 工程源码与资产文本复用中央文档时，空场景提示和场景页必须让出同一块工作区。
    let document_in_center = documents.as_ref().is_some_and(|documents| documents.in_center());
    for (mut node, scene_page, empty) in &mut pages {
        let visible = if empty {
            !document_in_center && model.active_scene.is_none()
        } else {
            !document_in_center && scene_page && model.active_scene.is_some()
        };
        let display = if visible {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    for (entity, action, disabled) in &actions {
        let available = model.enabled_actions.contains(&action.0)
            || (model.asset_root.is_some()
                && crate::asset_catalog::navigation::is_navigation(action.0));
        let enabled = !model.modal && !model.busy && available;
        if enabled && disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        }
        if !enabled && !disabled {
            commands.entity(entity).insert(InteractionDisabled);
        }
    }
}

pub(super) fn measure(
    model: Res<WorkbenchModel>,
    editor: Res<SceneEditorState>,
    mut probe: ResMut<WorkbenchProbe>,
    catalog: Res<super::resources::CatalogJob>,
    menu: Res<crate::ui::scene_sidebar::SceneDirectoryMenuState>,
    status: Res<crate::ui::status_bar::EditorStatusBarState>,
    help: Res<crate::ui::context_help::EditorContextHelpState>,
    viewport: Query<
        (&ComputedNode, &UiGlobalTransform, &InheritedVisibility),
        With<SceneEditorViewport>,
    >,
    actions: Query<(
        &SceneEditorActionButton,
        &ComputedNode,
        &UiGlobalTransform,
        &InheritedVisibility,
        Has<InteractionDisabled>,
    )>,
    rows: Query<(
        &SceneEditorSceneRow,
        &ComputedNode,
        &UiGlobalTransform,
        &InheritedVisibility,
    )>,
    assets: Res<crate::asset_catalog::EditorAssetCatalog>,
    sidebar: Res<crate::ui::sidebars::SceneEditorSidebarState>,
    navigation: Query<(
        &crate::ui::sidebars::SceneEditorSidebarTabButton,
        &ComputedNode,
        &UiGlobalTransform,
        &InheritedVisibility,
    )>,
    asset_rows: Query<(
        &SceneEditorAssetRow,
        &ComputedNode,
        &UiGlobalTransform,
        &InheritedVisibility,
    )>,
) {
    let bounds = |node: &ComputedNode, transform: &UiGlobalTransform| {
        let center = transform.translation;
        let size = node.size();
        [
            center.x - size.x * 0.5,
            center.y - size.y * 0.5,
            size.x,
            size.y,
        ]
    };
    probe.overlay = menu.owns_mouse() || status.overlays_viewport() || help.overlays_viewport();
    let browser = &mut probe.asset_browser;
    browser.root = model.asset_root.clone();
    browser.folder = assets.current_folder.clone();
    browser.selected = editor.selected_asset.clone();
    browser.active = sidebar.assets_browser_visible();
    browser.ready = model.asset_root.is_some()
        && assets.applied_revision == assets.requested_revision
        && assets.feedback.as_ref().is_some_and(|feedback| {
            feedback.revision == assets.applied_revision && feedback.result.is_ok()
        });
    browser.tabs = [None; 2];
    for (tab, node, transform, visible) in &navigation {
        use crate::ui::sidebars::{EditorNavigatorTab, EditorSidebarTab};
        if !visible.get() || node.is_empty() {
            continue;
        }
        let index = match tab.0 {
            EditorSidebarTab::Navigator(EditorNavigatorTab::Scenes) => 0,
            EditorSidebarTab::Navigator(EditorNavigatorTab::Assets) => 1,
            _ => continue,
        };
        browser.tabs[index] = Some(bounds(node, transform));
    }
    browser.entries.clear();
    if browser.active {
        browser.entries.extend(
            asset_rows
                .iter()
                .filter(|(_, node, _, visible)| visible.get() && !node.is_empty())
                .filter_map(|(row, node, transform, _)| {
                    row.entry_index
                        .and_then(|index| assets.entries.get(index))
                        .map(|entry| {
                            (
                                entry.asset_path.clone(),
                                entry.is_directory,
                                bounds(node, transform),
                            )
                        })
                }),
        );
    }
    probe.viewport = (!model.modal && !probe.overlay && model.visible)
        .then(|| {
            viewport
                .iter()
                .find(|(node, _, visible)| visible.get() && node.size().min_element() > 0.)
                .map(|(node, transform, _)| bounds(node, transform))
        })
        .flatten();
    probe.context = model.context;
    probe.selected_scene = editor.scene_list_selection_id.as_ref().map(|id| id.0.clone());
    probe.selected_index = probe
        .selected_scene
        .as_ref()
        .and_then(|id| catalog.indices.get(id).copied());
    probe.actions.clear();
    probe.actions.extend(
        actions
            .iter()
            .filter(|(_, node, _, visible, _)| visible.get() && node.size().min_element() > 0.)
            .map(|(action, node, transform, _, disabled)| {
                (action.0, bounds(node, transform), !disabled)
            }),
    );
    probe.scenes.clear();
    if !catalog
        .adopted
        .as_ref()
        .is_some_and(|(context, _)| *context == model.context)
    {
        probe.selected_index = None;
        return; // 旧目录可作为加载托底显示，但不能作为新工程的可点击目录证据。
    }
    probe.scenes.extend(
        rows.iter()
            .filter(|(_, node, _, visible)| visible.get() && node.size().min_element() > 0.)
            .filter_map(|(row, node, transform, _)| match &row.item {
                Some(SceneBrowserItem::Scene(id)) => catalog
                    .indices
                    .get(&id.0)
                    .map(|index| (*index, bounds(node, transform))),
                _ => None,
            }),
    );
}
