//! 原状态与固定实体的地图侧栏回归；不启动地图加载、目录写入或真实回收。

use feathrise_editor_ui::UiInteraction;
use super::{components::*, resources::*, systems::sync_sidebar, types::*};
use crate::authoring::{EditorAuthoringSession, GlobalEnvironmentAuthoringState};
use crate::components::*;
use crate::resources::*;
use crate::types::*;
use crate::ui::sidebars::{EditorNavigatorTab, MapEditorSidebarState};
use bevy::prelude::*;
use feathrise_map_content::{
    MapCatalogAsset, MapContentHandles, MapContentRegistry, MapId, MapManifestAsset,
    MapTerrainDescriptor, MapTerrainKind,
};
use std::sync::Arc;

#[path = "input_tests.rs"]
mod input_tests;
#[path = "layout_tests.rs"]
mod layout_tests;
#[path = "lookup_tests.rs"]
mod lookup_tests;

pub(super) fn map_item(id: &str) -> MapBrowserItem {
    MapBrowserItem::Map(MapId::parse(id).unwrap())
}

pub(super) fn entry(item: MapBrowserItem, name: &str) -> MapBrowserEntry {
    MapBrowserEntry {
        item,
        display_name: name.into(),
        published: false,
        depth: 1,
        parent_folder_id: None,
    }
}

pub(super) fn published_index(entries: Vec<MapBrowserEntry>) -> MapBrowserIndexState {
    let signature = MapBrowserIndexSignature {
        folder_revision: 1,
        catalog_revision: 1,
    };
    MapBrowserIndexState {
        total_entries: entries.len(),
        full_entries: Arc::new(entries.clone()),
        entries: Arc::new(entries),
        applied_signature: Some(signature.clone()),
        desired_signature: Some(signature.clone()),
        applied_projection: Some((signature.clone(), 0)),
        desired_projection: Some((signature, 0)),
        ..default()
    }
}

/// 共用真实资源类型；表现夹具只持有内存中的完整目录投影。
pub(super) fn initialize(app: &mut App) {
    app.insert_resource(MapEditorState {
        open: true,
        selected_map_id: None,
        ..default()
    })
    .insert_resource(MapBrowserState {
        initial_load_resolved: true,
        write_blocked: None,
        document_revision: 1,
        ..default()
    })
    .insert_resource(published_index(vec![
        entry(map_item("MAP-1"), "第一张中文地图"),
        entry(map_item("MAP-2"), "第二张中文地图"),
    ]))
    .init_resource::<MapEditorSidebarState>()
    .init_resource::<MapBrowserDragState>()
    .init_resource::<MapFolderPersistence>()
    .init_resource::<MapCreationTask>()
    .init_resource::<MapDeletionTask>()
    .init_resource::<EditorAuthoringSession>()
    .init_resource::<GlobalEnvironmentAuthoringState>()
    .init_resource::<MapEditorNumericScrubMouseGuard>()
    .init_resource::<MapSidebarFeedback>()
    .init_resource::<Assets<MapCatalogAsset>>()
    .init_resource::<Assets<MapManifestAsset>>()
    .insert_resource(MapContentRegistry {
        catalog: Handle::default(),
        revision: 1,
        maps: default(),
        last_error: None,
    });
}

fn sidebar_app() -> App {
    let mut app = App::new();
    initialize(&mut app);
    app.add_systems(Update, sync_sidebar);
    app.world_mut()
        .spawn((MapEditorMapTreeScroll, ScrollPosition::default()));
    for field in [
        SidebarField::CurrentName,
        SidebarField::CurrentState,
        SidebarField::DirectoryFeedback,
        SidebarField::SelectedKind,
        SidebarField::SelectedName,
        SidebarField::SelectedIdentity,
        SidebarField::SelectionHint,
        SidebarField::OperationFeedback,
        SidebarField::CurrentSummary,
        SidebarField::SelectedSummary,
    ] {
        app.world_mut().spawn((
            MapSidebarText(field),
            Text::new(""),
            TextColor::default(),
            Node::default(),
        ));
    }
    for action in [
        EditorAction::NewMap,
        EditorAction::MapFolderCreate,
        EditorAction::MapFolderRename,
        EditorAction::EnterSelectedMap,
        EditorAction::DeleteMap,
    ] {
        app.world_mut().spawn((
            MapEditorActionButton(action),
            MapSidebarActionBinding::default(),
            UiInteraction::None,
            Node::default(),
            BackgroundColor::default(),
            BorderColor::default(),
        ));
        app.world_mut().spawn((
            MapSidebarActionLabel(action),
            Text::new(""),
            TextColor::default(),
        ));
    }
    app.update();
    app
}

fn field(app: &mut App, wanted: SidebarField) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &MapSidebarText)>()
        .iter(world)
        .find(|(_, field)| field.0 == wanted)
        .unwrap()
        .0
}

fn text(app: &mut App, wanted: SidebarField) -> String {
    let entity = field(app, wanted);
    app.world().get::<Text>(entity).unwrap().0.clone()
}

fn button(app: &mut App, wanted: EditorAction) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &MapEditorActionButton)>()
        .iter(world)
        .find(|(_, action)| action.0 == wanted)
        .unwrap()
        .0
}

fn select(app: &mut App, item: Option<MapBrowserItem>) {
    app.world_mut()
        .resource_mut::<MapBrowserState>()
        .selected_item = item;
    app.update();
}

pub(super) fn manifest(id: &str, name: &str) -> MapManifestAsset {
    MapManifestAsset {
        vegetation_paint_groups: Default::default(),
        terrains: None,
        sub_maps: Default::default(),
        schema_version: 1,
        id: MapId::parse(id).unwrap(),
        display_name: name.into(),
        notes: "真实备注".into(),
        runtime_scene: "DynamicMapScene".into(),
        world_min: [0.0; 2],
        world_max: [10.0; 2],
        default_spawn: [5.0, 0.0, 5.0],
        environment: default(),
        puddles: None,
        asset_roots: vec![],
        terrain: MapTerrainDescriptor {resolved_stitches: Default::default(),
            kind: MapTerrainKind::Heightfield,
            glb_path: "maps/fixture/terrain.glb".into(),
            resolution: [11; 2],
            base_height: 0.0,
            height_scale: 1.0,
            collision_lods: vec![],
            display_lods: None,
            collision_mode: default(),
            player_collision_preload_radius_meters: 16.0,
            role_collision_preload_radius_meters: None,
            player_collision_chunk_cells: None,
            role_collision_chunk_cells: None,
            player_collision_span_meters: None,
            role_collision_span_meters: None,
            height_path: None,
            height_texture_path: None,
        },
    }
}

pub(super) fn register_manifest(app: &mut App, manifest: MapManifestAsset) {
    let id = manifest.id.clone();
    let handle = app
        .world_mut()
        .resource_mut::<Assets<MapManifestAsset>>()
        .add(manifest);
    app.world_mut()
        .resource_mut::<MapContentRegistry>()
        .maps
        .insert(
            id,
            MapContentHandles {
                manifest: handle,
                portals: Handle::default(),
                placements: Handle::default(),
                characters_path: None,
                surface: None,
                vegetation: None,
                water: None,
                boundaries: None,
            },
        );
}

#[test]
fn loaded_map_and_selected_map_are_separate_and_use_full_identity() {
    let mut app = sidebar_app();
    register_manifest(&mut app, manifest("MAP-1", "已经进入的地图"));
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .selected_map_id = Some(MapId::parse("MAP-1").unwrap());
    select(&mut app, Some(map_item("MAP-2")));
    assert_eq!(text(&mut app, SidebarField::CurrentName), "已经进入的地图");
    assert_eq!(text(&mut app, SidebarField::SelectedName), "第二张中文地图");
    assert_eq!(
        text(&mut app, SidebarField::SelectedIdentity),
        "地图ID：MAP-2"
    );
    assert!(text(&mut app, SidebarField::SelectedKind).contains("尚未进入"));
    assert!(
        app.world()
            .resource::<MapEditorState>()
            .pending_map_transition
            .is_none()
    );
    select(&mut app, Some(map_item("MAP-1")));
    assert_eq!(text(&mut app, SidebarField::SelectedKind), "当前地图");
    app.world_mut().resource_mut::<MapEditorState>().dirty = true;
    app.update();
    assert!(text(&mut app, SidebarField::CurrentState).contains("未保存"));
    let delete = button(&mut app, EditorAction::DeleteMap);
    assert!(
        !app.world()
            .get::<MapSidebarActionBinding>(delete)
            .unwrap()
            .enabled
    );
}

#[test]
fn root_none_folder_and_missing_object_have_contextual_actions() {
    let mut app = sidebar_app();
    let folder = MapBrowserItem::Folder("FOLDER-1".into());
    let long_name = "完整中文文件夹名称".repeat(14);
    app.insert_resource(published_index(vec![entry(folder.clone(), &long_name)]));
    select(&mut app, Some(folder));
    assert_eq!(text(&mut app, SidebarField::SelectedName), long_name);
    assert!(text(&mut app, SidebarField::SelectedIdentity).is_empty());
    let enter = button(&mut app, EditorAction::EnterSelectedMap);
    let edit = button(&mut app, EditorAction::MapFolderRename);
    assert_eq!(
        app.world().get::<Node>(enter).unwrap().display,
        Display::None
    );
    assert!(
        app.world()
            .get::<MapSidebarActionBinding>(edit)
            .unwrap()
            .enabled
    );
    let world = app.world_mut();
    assert!(
        world
            .query::<(&MapSidebarActionLabel, &Text)>()
            .iter(world)
            .any(|(label, text)| label.0 == EditorAction::MapFolderRename
                && text.as_str() == "重命名")
    );
    for selection in [Some(MapBrowserItem::Root), None] {
        select(&mut app, selection);
        assert_eq!(
            app.world().get::<Node>(edit).unwrap().display,
            Display::None
        );
    }
    select(&mut app, Some(map_item("MAP-REMOVED")));
    assert_eq!(
        text(&mut app, SidebarField::SelectedName),
        "所选对象已不在目录中"
    );
    assert!(
        !app.world()
            .get::<MapSidebarActionBinding>(enter)
            .unwrap()
            .enabled
    );
}

#[test]
fn directory_feedback_requires_adoption_and_keeps_real_errors() {
    let mut app = sidebar_app();
    app.world_mut()
        .resource_mut::<MapContentRegistry>()
        .revision = 0;
    app.update();
    assert_eq!(
        text(&mut app, SidebarField::DirectoryFeedback),
        "正在加载地图目录"
    );
    app.world_mut()
        .resource_mut::<MapContentRegistry>()
        .revision = 1;
    app.world_mut()
        .resource_mut::<MapBrowserState>()
        .initial_load_resolved = false;
    app.update();
    assert_eq!(
        text(&mut app, SidebarField::DirectoryFeedback),
        "正在加载地图归类"
    );
    app.world_mut()
        .resource_mut::<MapBrowserState>()
        .initial_load_resolved = true;
    app.insert_resource(MapBrowserIndexState::default());
    app.update();
    assert!(text(&mut app, SidebarField::DirectoryFeedback).contains("整理"));
    app.insert_resource(published_index(vec![]));
    app.update();
    assert_eq!(
        text(&mut app, SidebarField::DirectoryFeedback),
        "当前没有地图或文件夹"
    );
    app.world_mut()
        .resource_mut::<MapContentRegistry>()
        .last_error = Some("正式地图目录校验失败".into());
    app.update();
    app.world_mut().resource_mut::<MapEditorState>().status = "其他工具已经完成".into();
    app.update();
    assert_eq!(
        text(&mut app, SidebarField::DirectoryFeedback),
        "正式地图目录校验失败"
    );
    app.world_mut()
        .resource_mut::<MapContentRegistry>()
        .last_error = None;
    app.world_mut()
        .resource_mut::<MapBrowserState>()
        .write_blocked = Some("原归类文件损坏，禁止覆盖".into());
    app.update();
    assert_eq!(
        text(&mut app, SidebarField::DirectoryFeedback),
        "原归类文件损坏，禁止覆盖"
    );
}

#[test]
fn explicit_feedback_is_bound_to_real_context_not_global_status() {
    let mut editor = MapEditorState {
        open: true,
        status: "原进入动作失败".into(),
        ..default()
    };
    let mut browser = MapBrowserState {
        selected_item: Some(map_item("MAP-2")),
        ..default()
    };
    let mut feedback = MapSidebarFeedback::default();
    feedback.record(&editor, &browser, None, false);
    editor.status = "其他工具的新提示".into();
    assert_eq!(
        feedback.current_message(&editor, &browser, None, false),
        "原进入动作失败"
    );
    browser.selected_item = Some(map_item("MAP-1"));
    assert!(
        feedback
            .current_message(&editor, &browser, None, false)
            .is_empty()
    );
    browser.selected_item = Some(map_item("MAP-2"));
    editor.pending_map_transition = Some(MapId::parse("MAP-2").unwrap());
    assert!(
        feedback
            .current_message(&editor, &browser, None, false)
            .is_empty()
    );
}

#[test]
fn real_operation_phases_and_folder_diagnostics_are_not_fake_ready() {
    let mut app = sidebar_app();
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .new_map_dialog_open = true;
    app.update();
    assert!(text(&mut app, SidebarField::OperationFeedback).contains("新地图"));
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .new_map_dialog_open = false;
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .pending_map_transition = Some(MapId::parse("MAP-2").unwrap());
    app.update();
    assert!(text(&mut app, SidebarField::OperationFeedback).contains("完整准备"));
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .map_transition_in_progress = true;
    app.update();
    assert!(text(&mut app, SidebarField::OperationFeedback).contains("切换地图"));
    app.world_mut().resource_mut::<MapBrowserState>().last_error =
        Some("无法保存原归类文件".into());
    app.update();
    assert_eq!(
        text(&mut app, SidebarField::OperationFeedback),
        "无法保存原归类文件"
    );
}

#[derive(Resource, Default)]
struct Writes(usize);

#[test]
fn pending_batch_shows_saving_feedback_and_keeps_structure_buttons_disabled() {
    bevy::tasks::AsyncComputeTaskPool::get_or_init(Default::default);
    let mut app = sidebar_app();
    select(&mut app, Some(map_item("MAP-1")));
    app.world_mut().resource_mut::<MapFolderPersistence>().batch =
        Some(crate::types::MapFolderBatchMove {
            ticket: 0,
            source: MapBrowserIndexSignature {
                folder_revision: 1,
                catalog_revision: 1,
            },
            target: crate::types::MapBrowserDropTarget::Root,
            expected_bytes: None,
            asset_root: default(),
            cancelled: false,
            phase: crate::types::MapFolderBatchPhase::Preparing(
                bevy::tasks::AsyncComputeTaskPool::get().spawn(async { Ok(None) }),
            ),
        });
    app.update(); // 等待松键和后台准备都是在途事务，不能显示空闲或假成功。
    assert!(text(&mut app, SidebarField::OperationFeedback).contains("正在保存地图归类"));
    for action in [
        EditorAction::NewMap,
        EditorAction::MapFolderCreate,
        EditorAction::DeleteMap,
    ] {
        let entity = button(&mut app, action);
        assert!(
            !app.world()
                .get::<MapSidebarActionBinding>(entity)
                .unwrap()
                .enabled
        );
    }
}

#[test]
fn offscreen_name_wait_disables_target_actions_until_matching_record_is_found() {
    let mut app = sidebar_app();
    app.insert_resource(published_index(
        (0..140)
            .map(|number| entry(map_item(&format!("MAP-{number}")), "真实离屏名称"))
            .collect(),
    ));
    select(&mut app, Some(map_item("MAP-139")));
    let enter = button(&mut app, EditorAction::EnterSelectedMap);
    assert_eq!(
        text(&mut app, SidebarField::SelectedName),
        "正在同步所选名称"
    );
    assert!(
        !app.world()
            .get::<MapSidebarActionBinding>(enter)
            .unwrap()
            .enabled
    );
    app.update();
    assert!(
        !app.world()
            .get::<MapSidebarActionBinding>(enter)
            .unwrap()
            .enabled
    );
    app.update();
    assert_eq!(text(&mut app, SidebarField::SelectedName), "真实离屏名称");
    assert!(
        app.world()
            .get::<MapSidebarActionBinding>(enter)
            .unwrap()
            .enabled
    );
}

#[test]
fn pending_folder_projection_keeps_directory_feedback_hidden() {
    let mut app = sidebar_app();
    let entries = Arc::clone(&app.world().resource::<MapBrowserIndexState>().full_entries);
    app.update();
    let feedback = field(&mut app, SidebarField::DirectoryFeedback);
    for revision in 1..=3 {
        {
            let mut index = app.world_mut().resource_mut::<MapBrowserIndexState>();
            index.desired_projection.as_mut().unwrap().1 = revision;
            index.rebuilding = true;
        }
        app.update();
        assert!(text(&mut app, SidebarField::DirectoryFeedback).is_empty());
        assert_eq!(app.world().get::<Node>(feedback).unwrap().display, Display::None);
        {
            let mut index = app.world_mut().resource_mut::<MapBrowserIndexState>();
            index.applied_projection = index.desired_projection.clone();
            index.rebuilding = false;
        }
        app.update();
        assert!(text(&mut app, SidebarField::DirectoryFeedback).is_empty());
    }
    assert!(Arc::ptr_eq(
        &entries,
        &app.world().resource::<MapBrowserIndexState>().full_entries
    ));
}

#[test]
fn pending_source_projection_keeps_directory_message_without_touching_the_old_tree() {
    let mut app = sidebar_app();
    let entries = Arc::clone(&app.world().resource::<MapBrowserIndexState>().full_entries);
    app.world_mut()
        .resource_mut::<MapBrowserIndexState>()
        .desired_projection
        .as_mut()
        .unwrap()
        .0.folder_revision += 1;
    app.update();
    assert!(text(&mut app, SidebarField::DirectoryFeedback).contains("整理目录"));
    assert!(Arc::ptr_eq(
        &entries,
        &app.world().resource::<MapBrowserIndexState>().full_entries
    ));
}

#[test]
fn cursor_and_other_tool_status_do_not_rewrite_sidebar_text() {
    fn observe(
        texts: Query<(), (With<MapSidebarText>, Changed<Text>)>,
        mut writes: ResMut<Writes>,
    ) {
        writes.0 += texts.iter().count();
    }
    let mut app = sidebar_app();
    select(&mut app, Some(map_item("MAP-2")));
    app.init_resource::<Writes>()
        .add_systems(Update, observe.after(sync_sidebar));
    app.update();
    app.world_mut().resource_mut::<Writes>().0 = 0;
    for index in 0..6 {
        {
            let mut editor = app.world_mut().resource_mut::<MapEditorState>();
            editor.status = format!("笔刷新提示{index}");
            editor.cursor_world = Some(Vec3::new(index as f32, 0.0, 0.0));
            editor.brush_radius = 10.0 + index as f32;
        }
        app.update();
    }
    assert_eq!(app.world().resource::<Writes>().0, 0);
    let entity = field(&mut app, SidebarField::SelectedName);
    app.world_mut().resource_mut::<MapEditorSidebarState>().navigator = EditorNavigatorTab::Scene;
    app.update();
    app.world_mut()
        .resource_mut::<MapBrowserState>()
        .selected_item = Some(MapBrowserItem::Root);
    app.update();
    assert_eq!(
        app.world().get::<Text>(entity).unwrap().as_str(),
        "第二张中文地图"
    );
    app.world_mut().resource_mut::<MapEditorSidebarState>().navigator = EditorNavigatorTab::Maps;
    app.update();
    assert_eq!(field(&mut app, SidebarField::SelectedName), entity);
    assert_eq!(text(&mut app, SidebarField::SelectedName), "根目录");
}
