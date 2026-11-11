//! 原工作台的工程页面挂载点与输入边界；宿主页不拥有或重建任何作者文档。

use bevy::prelude::*;

/// 宿主标签与原场景、预设标签同处一行，具体工程字段仍由独立宿主定义。
#[derive(Component, Default, Clone)]
pub struct WorkbenchDocumentTabs;

/// 位于原中央正文内的宿主页面；不另建窗口或第二套工作台。
#[derive(Component, Default, Clone)]
pub struct WorkbenchCentralPageHost;

/// 原“设置 → 常规”的宿主扩展位置，个人路径设置不写入游戏配置。
#[derive(Component, Default, Clone)]
pub struct WorkbenchSettingsBody;

/// 宿主的标签根用于区分工程标签点击和原作者文档点击。
#[derive(Component, Default, Clone)]
pub struct WorkbenchHostTabs;

#[derive(Resource, Default)]
pub struct WorkbenchHostPageState {
    pub active: bool,
    pub settings_open: bool,
    pub menu_open: bool,
    pub native_tab_revision: u64,
}

#[derive(Component)]
struct SuspendedPage(Display);

/// 仅正式宿主安装；原一体入口没有该插件时，原界面与输入路径不变。
pub struct WorkbenchHostPagesPlugin;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorkbenchHostPageInput;

impl Plugin for WorkbenchHostPagesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorkbenchHostPageState>()
            .init_resource::<feathrise_editor_core::EditorTextInputGuard>()
            // 原表现系统先看到自己上一有效Display，再按真实文档更新；宿主页不保存第二份文档状态。
            .add_systems(PreUpdate, restore_native_pages)
            .add_systems(
                Update,
                native_tab_input
                    .in_set(WorkbenchHostPageInput)
                    .after(feathrise_editor_ui::update_editor_button_clicks)
                    .before(crate::SceneEditorSystemSet::Input),
            )
            .add_systems(
                PostUpdate,
                (sync_settings_state, suspend_native_pages)
                    .chain()
                    .before(bevy::ui::UiSystems::Prepare),
            )
            .add_systems(
                Update,
                observe_native_navigation
                    .after(crate::SceneEditorSystemSet::Input)
                    .before(crate::SceneEditorSystemSet::Presentation),
            );

        if !app.is_plugin_added::<crate::SceneEditorPlugin>() {
            // 无工程的工作台仍使用原设置窗口与字段，不安装会写仓库settings.json的游戏保存插件。
            // 按钮改偏好后必须由唯一原设置系统写入真正UiScale；该插件只同步固定资源，没有落盘IO。
            if !app.is_plugin_added::<feathrise_editor_core::EditorSettingsPlugin>() {
                app.add_plugins(feathrise_editor_core::EditorSettingsPlugin);
            }
            app.init_resource::<feathrise_editor_core::EditorSettings>()
                .init_resource::<super::WorkbenchSettingsAccess>()
                .add_systems(
                    Update,
                    super::settings_access::prepare
                        .after(feathrise_editor_ui::update_editor_button_clicks)
                        .before(crate::SceneEditorSystemSet::Input)
                        .before(super::WorkbenchUiSet::Update),
                )
                .add_systems(
                    Update,
                    (
                        crate::ui::editor_settings::handle_settings_input,
                        crate::ui::editor_settings::setup_snap_inputs,
                        crate::ui::editor_settings::setup_mesh_lod_inputs,
                        crate::ui::editor_settings::handle_snap_inputs,
                        crate::ui::editor_settings::handle_mesh_lod_inputs,
                        crate::ui::editor_settings::scroll_settings_body,
                        crate::ui::shortcut_settings::scroll_shortcut_settings,
                    )
                        .chain()
                        .in_set(crate::SceneEditorSystemSet::Input),
                )
                .add_systems(
                    PostUpdate,
                    (
                        crate::ui::editor_settings::sync_settings_panel,
                        crate::ui::editor_settings::sync_snap_inputs,
                        crate::ui::editor_settings::sync_mesh_lod_inputs,
                        crate::ui::shortcut_settings::sync_shortcut_settings_panel,
                    )
                        .before(bevy::ui::UiSystems::Prepare),
                )
                .add_systems(
                    Update,
                    crate::ui::ime::sync_scene_editor_ime
                        .after(crate::SceneEditorSystemSet::Presentation),
                )
                .add_systems(
                    PostUpdate,
                    super::settings_access::present
                        .after(crate::ui::editor_settings::sync_settings_panel)
                        .after(crate::ui::editor_settings::sync_mesh_lod_inputs)
                        .after(crate::ui::shortcut_settings::sync_shortcut_settings_panel)
                        .before(bevy::ui::UiSystems::Prepare),
                );
        }
    }
}

#[cfg(test)]
#[path = "host_pages_scale_tests.rs"]
mod scale_tests;

#[cfg(test)]
#[path = "host_layout_tests.rs"]
mod layout_tests;

fn restore_native_pages(
    mut commands: Commands,
    mut pages: Query<(Entity, &mut Node, &SuspendedPage)>,
) {
    for (entity, mut node, previous) in &mut pages {
        node.display = previous.0;
        commands.entity(entity).remove::<SuspendedPage>();
    }
}

fn native_tab_input(
    clicks: Query<
        (Entity, &feathrise_editor_ui::EditorButtonClick),
        Changed<feathrise_editor_ui::EditorButtonClick>,
    >,
    parents: Query<&ChildOf>,
    tabs: Query<(), With<WorkbenchDocumentTabs>>,
    host_tabs: Query<(), With<WorkbenchHostTabs>>,
    mut state: ResMut<WorkbenchHostPageState>,
) {
    if !state.active {
        return;
    }
    for (entity, click) in &clicks {
        if !click.is_clicked() {
            continue;
        }
        let mut original = false;
        let mut host = false;
        for ancestor in parents.iter_ancestors(entity) {
            original |= tabs.contains(ancestor);
            host |= host_tabs.contains(ancestor);
        }
        if original && !host {
            state.active = false;
            state.native_tab_revision = state.native_tab_revision.wrapping_add(1);
            break;
        }
    }
}

fn sync_settings_state(
    settings: Res<crate::resources::SceneEditorSettingsState>,
    mut state: ResMut<WorkbenchHostPageState>,
) {
    state.settings_open = settings.open;
}

fn observe_native_navigation(
    workspace: Res<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>,
    editor: Res<crate::resources::SceneEditorState>,
    mut previous: Local<Option<(feathrise_editor_core::PresetWorkspaceTab, bool)>>,
    mut state: ResMut<WorkbenchHostPageState>,
) {
    let current = (workspace.active_tab, editor.scene_transition_in_progress);
    if let Some(before) = *previous {
        // 原侧栏打开预设或发起切换场景，同样激活原文档；纯后台标签摘要刷新不抢走开始页。
        if state.active && (before.0 != current.0 || (!before.1 && current.1)) {
            state.active = false;
            state.native_tab_revision = state.native_tab_revision.wrapping_add(1);
        }
    }
    *previous = Some(current);
}

fn suspend_native_pages(
    mut commands: Commands,
    state: Res<WorkbenchHostPageState>,
    host: Query<(Entity, &ChildOf), With<WorkbenchCentralPageHost>>,
    tabs: Query<(), With<WorkbenchDocumentTabs>>,
    mut nodes: Query<(Entity, &ChildOf, &mut Node)>,
) {
    if !state.active {
        return;
    }
    for (host, parent) in &host {
        for (entity, owner, mut node) in &mut nodes {
            if owner.parent() != parent.parent() || entity == host || tabs.contains(entity) {
                continue;
            }
            if node.display != Display::None {
                commands.entity(entity).insert(SuspendedPage(node.display));
                node.display = Display::None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_page_keeps_the_original_document_entity_and_display() {
        let mut app = App::new();
        app.insert_resource(WorkbenchHostPageState {
            active: true,
            ..default()
        })
        .add_systems(PreUpdate, restore_native_pages)
        .add_systems(PostUpdate, suspend_native_pages);
        let parent = app.world_mut().spawn_empty().id();
        let original = app
            .world_mut()
            .spawn((Node::default(), ChildOf(parent)))
            .id();
        app.world_mut()
            .spawn((WorkbenchCentralPageHost, Node::default(), ChildOf(parent)));
        app.update();
        assert_eq!(
            app.world().get::<Node>(original).unwrap().display,
            Display::None
        );
        app.world_mut()
            .resource_mut::<WorkbenchHostPageState>()
            .active = false;
        app.update();
        assert_eq!(
            app.world().get::<Node>(original).unwrap().display,
            Display::Flex
        );
    }
}
