//! 管理模态的原实体、冻结目标、真实反馈、滚轮和危险样式回归；不运行文件事务。

use feathrise_editor_ui::UiInteraction;
use super::DialogKind;
use super::components::*;
use crate::authoring::{TerrainConflictResolution, TerrainExternalChangeState};
use crate::components::*;
use crate::resources::{SceneBrowserState, SceneDeletionTask, SceneEditorState};
use crate::types::{
    EditorAction, SceneBrowserItem, SceneDeletionRequest, SceneFolderDialogMode, SceneFolderDialogState,
};
use crate::ui::theme as colors;
use bevy::ecs::system::RunSystemOnce;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::{FocusPolicy, RelativeCursorPosition};
use feathrise_editor_ui::EditorButtonClick;
use feathrise_scene_content::SceneId;

pub(super) fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin {
            file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets").into(),
            watch_for_changes_override: Some(false),
            ..default()
        },
        bevy::scene::ScenePlugin,
    ))
    .init_asset::<Font>()
    .init_resource::<SceneEditorState>()
    .init_resource::<SceneBrowserState>()
    .init_resource::<SceneDeletionTask>()
    .init_resource::<TerrainExternalChangeState>()
    .add_message::<MouseWheel>()
    .add_systems(Startup, |mut commands: Commands| {
        commands.spawn_scene(crate::ui::delete_scene_dialog::delete_scene_dialog());
        commands.spawn_scene(crate::scene_browser::scene_folder_dialog());
        commands.spawn_scene(crate::ui::terrain_conflict_dialog::terrain_conflict_dialog());
    })
    .add_systems(
        Update,
        (
            (
                crate::ui::delete_scene_dialog::configure_delete_scene_dialog_focus,
                crate::ui::terrain_conflict_dialog::configure_terrain_conflict_dialog_focus,
            ),
            (
                crate::ui::delete_scene_dialog::sync_delete_scene_dialog,
                crate::scene_browser::sync_scene_folder_dialog,
                crate::ui::terrain_conflict_dialog::sync_terrain_conflict_dialog,
            ),
            super::systems::scroll_dialogs,
            super::systems::sync_button_styles,
        )
            .chain(),
    );
    app
}

pub(super) fn entity<Marker: Component>(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<Marker>>()
        .single(world)
        .unwrap()
}

pub(super) fn deletion(id: &str) -> SceneDeletionRequest {
    SceneDeletionRequest {
        scene_id: SceneId::parse(id).unwrap(),
        display_name: "完整中文待删除场景".into(),
        folder: format!("scenes/{id}"),
    }
}

pub(super) fn folder(mode: SceneFolderDialogMode) -> SceneFolderDialogState {
    SceneFolderDialogState {
        mode,
        target_folder_id: Some("folder-frozen".into()),
        name: "冻结中文文件夹".into(),
    }
}

fn value<Marker: Component>(app: &mut App) -> String {
    let item = entity::<Marker>(app);
    app.world().get::<Text>(item).unwrap().0.clone()
}

#[test]
fn original_roots_and_actions_remain_and_terrain_has_no_fake_cancel() {
    let mut app = app();
    app.update();
    let name = entity::<SceneFolderDialogNameText>(&mut app);
    assert!(app.world().get::<bevy::text::EditableText>(name).is_some());
    for root in [
        entity::<SceneDeletionDialog>(&mut app),
        entity::<SceneFolderDialog>(&mut app),
        entity::<TerrainConflictDialog>(&mut app),
    ] {
        assert_eq!(
            app.world().get::<Node>(root).unwrap().display,
            Display::None
        );
        assert_eq!(
            app.world().get::<FocusPolicy>(root),
            Some(&FocusPolicy::Pass)
        );
    }
    let world = app.world_mut();
    let mut actions: Vec<_> = world
        .query_filtered::<&SceneEditorActionButton, With<ManagementButton>>()
        .iter(world)
        .map(|button| button.0)
        .collect();
    assert_eq!(actions.len(), 7);
    for expected in [
        EditorAction::CancelDeleteScene,
        EditorAction::ConfirmDeleteScene,
        EditorAction::ConfirmDeleteScenePermanently,
        EditorAction::CancelSceneFolderDialog,
        EditorAction::ConfirmSceneFolderDialog,
        EditorAction::AdoptBlenderTerrain,
        EditorAction::OverwriteBlenderTerrain,
    ] {
        let index = actions
            .iter()
            .position(|action| *action == expected)
            .unwrap();
        actions.remove(index);
    }
    assert!(actions.is_empty());
}

#[test]
fn frozen_target_and_consequences_survive_full_error_and_unrelated_selection() {
    let mut app = app();
    app.world_mut().resource_mut::<SceneEditorState>().open = true;
    app.world_mut()
        .resource_mut::<SceneDeletionTask>()
        .confirmation = Some(deletion("SCENE-frozen"));
    app.world_mut().resource_mut::<SceneBrowserState>().dialog =
        Some(folder(SceneFolderDialogMode::Delete));
    app.update();
    let target = value::<FolderTargetText>(&mut app);
    let consequences = value::<FolderConsequencesText>(&mut app);
    let error = format!("真实保存失败：{}", "完整错误路径/".repeat(160));
    {
        let mut browser = app.world_mut().resource_mut::<SceneBrowserState>();
        browser.selected_item = Some(SceneBrowserItem::Folder("folder-unrelated".into()));
        browser.last_error = Some(error.clone());
    }
    app.world_mut().resource_mut::<SceneEditorState>().status = "其他模块最近操作".into();
    app.update();
    assert_eq!(value::<SceneFolderDialogStatusText>(&mut app), error);
    assert_eq!(value::<FolderTargetText>(&mut app), target);
    assert_eq!(value::<FolderConsequencesText>(&mut app), consequences);
    assert!(
        value::<SceneDeletionDialogText>(&mut app)
            .contains("SceneId：SCENE-frozen\n资产目录：assets/scenes/SCENE-frozen")
    );
    assert!(!target.contains("folder-unrelated"));
    assert!(consequences.contains("不删除任何场景资产"));
}

#[test]
fn folder_modes_keep_the_official_name_entity_and_buffer() {
    let mut app = app();
    app.world_mut().resource_mut::<SceneEditorState>().open = true;
    app.update();
    let name = entity::<SceneFolderDialogNameText>(&mut app);
    app.world_mut()
        .get_mut::<bevy::text::EditableText>(name)
        .unwrap()
        .editor
        .set_text("已确认中文缓冲");
    for (mode, expected) in [
        (SceneFolderDialogMode::Create, "新建文件夹"),
        (SceneFolderDialogMode::Rename, "保存名称"),
        (SceneFolderDialogMode::Delete, "删除空文件夹"),
    ] {
        app.world_mut().resource_mut::<SceneBrowserState>().dialog = Some(folder(mode));
        app.update();
        assert_eq!(entity::<SceneFolderDialogNameText>(&mut app), name);
        assert_eq!(
            app.world()
                .get::<bevy::text::EditableText>(name)
                .unwrap()
                .value(),
            "已确认中文缓冲"
        );
        assert_eq!(value::<FolderConfirmText>(&mut app), expected);
        let group = entity::<SceneFolderDialogNameGroup>(&mut app);
        assert_eq!(
            app.world().get::<Node>(group).unwrap().display,
            if mode == SceneFolderDialogMode::Delete {
                Display::None
            } else {
                Display::Flex
            }
        );
    }
    app.world_mut().resource_mut::<SceneBrowserState>().dialog = Some(SceneFolderDialogState {
        target_folder_id: None,
        ..folder(SceneFolderDialogMode::Create)
    });
    app.update();
    assert_eq!(value::<FolderTargetText>(&mut app), "创建位置：虚拟根目录");
    assert!(
        app.world()
            .get::<crate::ui::text_input::EditorTextInput>(name)
            .is_some()
    );
}

#[test]
fn terrain_choice_stays_a_request_and_tracks_the_new_digest() {
    let mut app = app();
    app.world_mut().resource_mut::<SceneEditorState>().open = true;
    app.world_mut()
        .resource_mut::<TerrainExternalChangeState>()
        .register_save_conflict(SceneId::parse("SCENE-terrain").unwrap(), [7; 32]);
    app.add_systems(
        Update,
        crate::ui::terrain_conflict_dialog::handle_terrain_conflict_buttons
            .before(crate::ui::terrain_conflict_dialog::sync_terrain_conflict_dialog),
    );
    app.update();
    assert!(value::<TerrainConflictDialogText>(&mut app).contains("070707070707"));
    let button = {
        let world = app.world_mut();
        world
            .query::<(Entity, &SceneEditorActionButton)>()
            .iter(world)
            .find(|(_, action)| action.0 == EditorAction::AdoptBlenderTerrain)
            .unwrap()
            .0
    };
    // 本例只验证冲突决策和摘要，直接注入已由公共协议确认的释放脉冲。
    app.world_mut()
        .entity_mut(button)
        .insert(EditorButtonClick::triggered());
    app.update();
    assert_eq!(
        app.world()
            .resource::<TerrainExternalChangeState>()
            .resolution,
        Some(TerrainConflictResolution::AdoptBlender)
    );
    assert!(value::<TerrainConflictFeedbackText>(&mut app).contains("尚未完成采用"));
    app.world_mut()
        .get_mut::<EditorButtonClick>(button)
        .unwrap()
        .invalidate();
    app.world_mut()
        .resource_mut::<TerrainExternalChangeState>()
        .register_save_conflict(SceneId::parse("SCENE-terrain").unwrap(), [9; 32]);
    app.update();
    assert!(value::<TerrainConflictDialogText>(&mut app).contains("090909090909"));
    assert_eq!(
        value::<TerrainConflictFeedbackText>(&mut app),
        "地形草稿尚未保存，请选择保留哪一版。"
    );
    assert_eq!(
        app.world()
            .resource::<TerrainExternalChangeState>()
            .resolution,
        None
    );
}

#[test]
fn closing_releases_all_overlays_and_pressed_descendants() {
    let mut app = app();
    app.world_mut().resource_mut::<SceneEditorState>().open = true;
    app.world_mut()
        .resource_mut::<SceneDeletionTask>()
        .confirmation = Some(deletion("SCENE-close"));
    app.world_mut().resource_mut::<SceneBrowserState>().dialog =
        Some(folder(SceneFolderDialogMode::Rename));
    app.world_mut()
        .resource_mut::<TerrainExternalChangeState>()
        .register_save_conflict(SceneId::parse("SCENE-close").unwrap(), [3; 32]);
    app.update();
    let roots = [
        entity::<SceneDeletionDialog>(&mut app),
        entity::<SceneFolderDialog>(&mut app),
        entity::<TerrainConflictDialog>(&mut app),
    ];
    for root in roots {
        assert_eq!(
            app.world().get::<FocusPolicy>(root),
            Some(&FocusPolicy::Block)
        );
    }
    let world = app.world_mut();
    for mut interaction in world.query::<&mut UiInteraction>().iter_mut(world) {
        *interaction = UiInteraction::Pressed;
    }
    app.world_mut().resource_mut::<SceneEditorState>().open = false;
    app.update();
    for root in roots {
        assert_eq!(
            app.world().get::<Node>(root).unwrap().display,
            Display::None
        );
        assert_eq!(
            app.world().get::<FocusPolicy>(root),
            Some(&FocusPolicy::Pass)
        );
    }
    let world = app.world_mut();
    assert!(
        world
            .query::<&UiInteraction>()
            .iter(world)
            .all(|interaction| *interaction == UiInteraction::None)
    );
    app.world_mut().resource_mut::<SceneEditorState>().open = true;
    app.update();
    for root in roots {
        assert_eq!(
            app.world().get::<FocusPolicy>(root),
            Some(&FocusPolicy::Block)
        );
    }
}

#[test]
fn destructive_style_survives_generic_style_and_changes_with_folder_mode() {
    let mut app = app();
    app.world_mut().resource_mut::<SceneEditorState>().open = true;
    app.init_resource::<crate::authoring::EditorAuthoringSession>();
    app.world_mut().resource_mut::<SceneBrowserState>().dialog =
        Some(folder(SceneFolderDialogMode::Delete));
    app.update();
    let buttons: Vec<_> = {
        let world = app.world_mut();
        world
            .query_filtered::<(Entity, &SceneEditorActionButton), With<ManagementButton>>()
            .iter(world)
            .map(|(entity, action)| (entity, action.0))
            .collect()
    };
    app.world_mut()
        .run_system_once(crate::ui::actions::sync_editor_action_button_styles)
        .unwrap();
    for (button, action) in &buttons {
        let danger = matches!(
            action,
            EditorAction::ConfirmDeleteScene
                | EditorAction::ConfirmDeleteScenePermanently
                | EditorAction::ConfirmSceneFolderDialog
                | EditorAction::OverwriteBlenderTerrain
        );
        assert!(
            app.world()
                .get::<SceneEditorCustomActionStyle>(*button)
                .is_some()
        );
        assert_eq!(
            app.world().get::<BackgroundColor>(*button).unwrap().0,
            if danger {
                colors::DANGER_BG
            } else {
                colors::BUTTON_DEV_SMALL_NORMAL
            }
        );
    }
    let button = buttons
        .iter()
        .find(|(_, action)| *action == EditorAction::ConfirmSceneFolderDialog)
        .unwrap()
        .0;
    *app.world_mut().get_mut::<UiInteraction>(button).unwrap() = UiInteraction::Hovered;
    app.update();
    assert_eq!(
        *app.world().get::<BorderColor>(button).unwrap(),
        BorderColor::all(colors::DANGER_TEXT)
    );
    app.world_mut().resource_mut::<SceneBrowserState>().dialog =
        Some(folder(SceneFolderDialogMode::Create));
    app.update();
    assert_eq!(
        app.world().get::<BackgroundColor>(button).unwrap().0,
        colors::BUTTON_DEV_SMALL_HOVERED
    );
}

#[test]
fn destructive_style_restores_the_border_when_a_pressed_pointer_leaves() {
    let mut app = app();
    app.world_mut().resource_mut::<SceneEditorState>().open = true;
    app.world_mut().resource_mut::<SceneBrowserState>().dialog =
        Some(folder(SceneFolderDialogMode::Delete));
    app.update();
    let button = {
        let world = app.world_mut();
        world
            .query_filtered::<(Entity, &SceneEditorActionButton), With<ManagementButton>>()
            .iter(world)
            .find(|(_, action)| action.0 == EditorAction::ConfirmSceneFolderDialog)
            .unwrap()
            .0
    };
    app.world_mut().entity_mut(button).insert((
        UiInteraction::Pressed,
        RelativeCursorPosition {
            cursor_over: true,
            normalized: Some(Vec2::ZERO),
        },
    ));
    app.update();
    assert_eq!(
        *app.world().get::<BorderColor>(button).unwrap(),
        BorderColor::all(colors::DANGER_TEXT)
    );
    // 官方命中在拖出时可能仍保留Pressed；只改变真实落点也必须熄灭危险边框。
    app.world_mut()
        .get_mut::<RelativeCursorPosition>(button)
        .unwrap()
        .cursor_over = false;
    app.update();
    assert_eq!(
        *app.world().get::<UiInteraction>(button).unwrap(),
        UiInteraction::Pressed
    );
    assert_eq!(
        *app.world().get::<BorderColor>(button).unwrap(),
        BorderColor::all(colors::DANGER_BORDER)
    );
    assert_eq!(
        app.world().get::<BackgroundColor>(button).unwrap().0,
        colors::DANGER_BG
    );
    // 返回原按钮可继续显示按压反馈；危险语义底色始终由业务样式保留。
    app.world_mut()
        .get_mut::<RelativeCursorPosition>(button)
        .unwrap()
        .cursor_over = true;
    app.update();
    assert_eq!(
        *app.world().get::<BorderColor>(button).unwrap(),
        BorderColor::all(colors::DANGER_TEXT)
    );
    assert_eq!(
        app.world().get::<BackgroundColor>(button).unwrap().0,
        colors::DANGER_BG
    );
}

#[test]
fn unrelated_feedback_does_not_rewrite_target_consequences_or_button_color() {
    let mut app = app();
    app.world_mut().resource_mut::<SceneEditorState>().open = true;
    app.world_mut().resource_mut::<SceneBrowserState>().dialog =
        Some(folder(SceneFolderDialogMode::Rename));
    app.update();
    let target = entity::<FolderTargetText>(&mut app);
    let consequences = entity::<FolderConsequencesText>(&mut app);
    let button = {
        let world = app.world_mut();
        world
            .query_filtered::<(Entity, &SceneEditorActionButton), With<ManagementButton>>()
            .iter(world)
            .find(|(_, action)| action.0 == EditorAction::ConfirmSceneFolderDialog)
            .unwrap()
            .0
    };
    let before = (
        app.world()
            .entity(target)
            .get_ref::<Text>()
            .unwrap()
            .last_changed(),
        app.world()
            .entity(consequences)
            .get_ref::<Text>()
            .unwrap()
            .last_changed(),
        app.world()
            .entity(button)
            .get_ref::<BackgroundColor>()
            .unwrap()
            .last_changed(),
    );
    for _ in 0..3 {
        app.world_mut().resource_mut::<SceneEditorState>().status = "其他模块变化".into();
        app.update();
    }
    assert_eq!(
        before,
        (
            app.world()
                .entity(target)
                .get_ref::<Text>()
                .unwrap()
                .last_changed(),
            app.world()
                .entity(consequences)
                .get_ref::<Text>()
                .unwrap()
                .last_changed(),
            app.world()
                .entity(button)
                .get_ref::<BackgroundColor>()
                .unwrap()
                .last_changed()
        )
    );
}

fn fake_scroll(app: &mut App, kind: DialogKind) -> Entity {
    app.world_mut()
        .spawn((
            ManagementScroll(kind),
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::splat(0.5)),
            },
            ComputedNode {
                size: Vec2::new(100.0, 100.0),
                content_size: Vec2::new(100.0, 300.0),
                inverse_scale_factor: 1.0,
                ..default()
            },
            ScrollPosition::default(),
        ))
        .id()
}

fn wheel(app: &mut App, amount: f32, unit: MouseScrollUnit) {
    let window = app.world_mut().spawn_empty().id();
    app.world_mut().write_message(MouseWheel {
        x: 0.0,
        y: amount,
        unit,
        window,
        phase: bevy::input::touch::TouchPhase::Moved,
    });
    app.update();
}

#[test]
fn scroll_is_bounded_independent_and_never_replays_hidden_wheel() {
    let mut app = app();
    app.world_mut().resource_mut::<SceneEditorState>().open = true;
    app.world_mut().resource_mut::<SceneBrowserState>().dialog =
        Some(folder(SceneFolderDialogMode::Rename));
    let body = fake_scroll(&mut app, DialogKind::Folder);
    let feedback = fake_scroll(&mut app, DialogKind::Folder);
    app.update();
    app.world_mut()
        .get_mut::<RelativeCursorPosition>(feedback)
        .unwrap()
        .cursor_over = false;
    wheel(&mut app, -1.0, MouseScrollUnit::Line);
    assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 24.0);
    assert_eq!(app.world().get::<ScrollPosition>(feedback).unwrap().y, 0.0);
    app.world_mut()
        .get_mut::<RelativeCursorPosition>(body)
        .unwrap()
        .cursor_over = false;
    app.world_mut()
        .get_mut::<RelativeCursorPosition>(feedback)
        .unwrap()
        .cursor_over = true;
    wheel(&mut app, -1000.0, MouseScrollUnit::Pixel);
    assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 24.0);
    assert_eq!(
        app.world().get::<ScrollPosition>(feedback).unwrap().y,
        200.0
    );
    app.world_mut().resource_mut::<SceneEditorState>().open = false;
    wheel(&mut app, -10.0, MouseScrollUnit::Line);
    app.world_mut().resource_mut::<SceneEditorState>().open = true;
    app.update();
    app.update();
    assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 0.0);
    assert_eq!(app.world().get::<ScrollPosition>(feedback).unwrap().y, 0.0);
    app.world_mut()
        .get_mut::<ComputedNode>(feedback)
        .unwrap()
        .size = Vec2::ZERO;
    wheel(&mut app, -1.0, MouseScrollUnit::Line);
    assert_eq!(app.world().get::<ScrollPosition>(feedback).unwrap().y, 0.0);
}

#[test]
fn owner_changes_reset_scroll_but_name_and_error_edits_preserve_it() {
    let mut app = app();
    app.world_mut().resource_mut::<SceneEditorState>().open = true;
    app.world_mut().resource_mut::<SceneBrowserState>().dialog =
        Some(folder(SceneFolderDialogMode::Rename));
    app.world_mut()
        .resource_mut::<SceneDeletionTask>()
        .confirmation = Some(deletion("SCENE-owner"));
    app.world_mut()
        .resource_mut::<TerrainExternalChangeState>()
        .register_save_conflict(SceneId::parse("SCENE-owner").unwrap(), [1; 32]);
    let folder_scroll = fake_scroll(&mut app, DialogKind::Folder);
    let deletion_scroll = fake_scroll(&mut app, DialogKind::SceneDeletion);
    let terrain_scroll = fake_scroll(&mut app, DialogKind::TerrainConflict);
    app.update();
    for item in [folder_scroll, deletion_scroll, terrain_scroll] {
        app.world_mut().get_mut::<ScrollPosition>(item).unwrap().y = 72.0;
    }
    {
        let mut browser = app.world_mut().resource_mut::<SceneBrowserState>();
        browser.dialog.as_mut().unwrap().name = "新输入中文".into();
        browser.last_error = Some("真实校验错误".into());
    }
    app.update();
    assert_eq!(
        app.world().get::<ScrollPosition>(folder_scroll).unwrap().y,
        72.0
    );
    app.world_mut()
        .resource_mut::<SceneBrowserState>()
        .dialog
        .as_mut()
        .unwrap()
        .target_folder_id = Some("new-owner".into());
    app.world_mut()
        .resource_mut::<SceneDeletionTask>()
        .confirmation = Some(deletion("SCENE-new-owner"));
    app.world_mut()
        .resource_mut::<TerrainExternalChangeState>()
        .register_save_conflict(SceneId::parse("SCENE-owner").unwrap(), [2; 32]);
    app.update();
    for item in [folder_scroll, deletion_scroll, terrain_scroll] {
        assert_eq!(app.world().get::<ScrollPosition>(item).unwrap().y, 0.0);
    }
}

#[test]
fn management_wheel_never_moves_the_background_sidebar() {
    let mut app = app();
    app.init_resource::<crate::resources::SceneEditorNumericScrubMouseGuard>()
        .init_resource::<crate::resources::SceneBrowserDragState>()
        .add_systems(Update, crate::ui::systems::scroll_editor_side_panels);
    app.world_mut().resource_mut::<SceneEditorState>().open = true;
    let background = app
        .world_mut()
        .spawn((
            SceneEditorLeftPanelScroll,
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::splat(0.5)),
            },
            ComputedNode {
                size: Vec2::new(100.0, 100.0),
                content_size: Vec2::new(100.0, 300.0),
                inverse_scale_factor: 1.0,
                ..default()
            },
            ScrollPosition::default(),
        ))
        .id();
    for kind in [
        DialogKind::SceneDeletion,
        DialogKind::Folder,
        DialogKind::TerrainConflict,
    ] {
        app.world_mut()
            .resource_mut::<SceneDeletionTask>()
            .confirmation = None;
        app.world_mut().resource_mut::<SceneBrowserState>().dialog = None;
        app.world_mut()
            .resource_mut::<TerrainExternalChangeState>()
            .conflict = None;
        match kind {
            DialogKind::SceneDeletion => {
                app.world_mut()
                    .resource_mut::<SceneDeletionTask>()
                    .confirmation = Some(deletion("SCENE-wheel"))
            }
            DialogKind::Folder => {
                app.world_mut().resource_mut::<SceneBrowserState>().dialog =
                    Some(folder(SceneFolderDialogMode::Create))
            }
            DialogKind::TerrainConflict => app
                .world_mut()
                .resource_mut::<TerrainExternalChangeState>()
                .register_save_conflict(SceneId::parse("SCENE-wheel").unwrap(), [5; 32]),
        }
        let foreground = fake_scroll(&mut app, kind);
        app.update();
        wheel(&mut app, -1.0, MouseScrollUnit::Line);
        assert_eq!(
            app.world().get::<ScrollPosition>(foreground).unwrap().y,
            24.0
        );
        assert_eq!(
            app.world().get::<ScrollPosition>(background).unwrap().y,
            0.0
        );
        app.world_mut().despawn(foreground);
    }
    app.world_mut()
        .resource_mut::<TerrainExternalChangeState>()
        .conflict = None;
    wheel(&mut app, -1.0, MouseScrollUnit::Line);
    assert_eq!(
        app.world().get::<ScrollPosition>(background).unwrap().y,
        27.0
    );
}
