//! 场景资料只编辑名称备注的实际BSN树、官方文本焦点和反馈滚动回归。
use super::components::*;
use super::*;
use crate::components::{SceneEditorActionButton, NewSceneNotesText};
use crate::types::{EditorAction, SceneEditTarget};
use feathrise_scene_content::SceneId;

pub(super) fn entity<Marker: Component>(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<Marker>>()
        .single(world)
        .unwrap()
}
pub(super) fn target(id: &str) -> SceneEditTarget {
    SceneEditTarget {
        id: SceneId::parse(id).unwrap(),
        original_name: "完整中文场景".into(),
        original_notes: "第一行\n第二行".into(),
    }
}

#[test]
fn every_scene_dialog_contains_only_text_metadata_and_two_actions() {
    let mut app = super::tests::app();
    // 仅用中性目录覆盖已发布/草稿两种场景，编辑器测试不读取原游戏场景清单。
    let scenes: feathrise_scene_content::SceneCatalogAsset = serde_json::from_str(
        r#"{"schema_version":2,"scenes":[
            {"id":"MAP-workbench-published","display_name":"示例场景甲","folder":"scenes/MAP-workbench-published","published":true},
            {"id":"MAP-workbench-draft","display_name":"示例场景乙","folder":"scenes/MAP-workbench-draft","published":false}
        ]}"#,
    )
    .unwrap();
    for entry in scenes.scenes {
        {
            let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
            editor.new_scene_edit_target = Some(target(&entry.id.0));
            editor.new_scene_name = entry.display_name;
        }
        app.update();
        let world = app.world_mut();
        let actions = world
            .query::<&SceneEditorActionButton>()
            .iter(world)
            .map(|action| action.0)
            .collect::<Vec<_>>();
        assert_eq!(actions.len(), 2);
        assert!(actions.contains(&EditorAction::ConfirmNewScene));
        assert!(actions.contains(&EditorAction::CancelNewScene));
        assert_eq!(
            world
                .query::<&bevy::text::EditableText>()
                .iter(world)
                .count(),
            2
        );
        assert_eq!(
            world
                .query::<&feathrise_editor_ui::NumericInputValue>()
                .iter(world)
                .count(),
            0
        );
        for text in world.query::<&Text>().iter(world) {
            assert!(!text.0.contains("高级设置"));
            assert!(!text.0.contains("旧主网格"));
            assert!(!text.0.contains("只读"));
            assert_ne!(text.0, "显示 LOD");
            assert_ne!(text.0, "碰撞 LOD");
        }
    }
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .new_scene_edit_target = None;
    app.update();
    let world = app.world_mut();
    assert_eq!(
        world
            .query::<&bevy::text::EditableText>()
            .iter(world)
            .count(),
        2
    );
    assert_eq!(
        world.query::<&SceneEditorActionButton>().iter(world).count(),
        2
    );
}

#[test]
fn existing_multiline_notes_keep_official_text_and_focus() {
    let mut app = super::tests::app();
    {
        let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
        editor.new_scene_edit_target = Some(target("MAP-notes"));
        editor.new_scene_name = "河畔".into();
        editor.new_scene_notes = "第一行\n第二行".into();
        editor.new_scene_input_field = crate::types::NewSceneInputField::Notes;
    }
    app.update();
    let notes = entity::<NewSceneNotesText>(&mut app);
    assert_eq!(
        app.world()
            .get::<bevy::text::EditableText>(notes)
            .unwrap()
            .value(),
        "第一行\n第二行"
    );
    assert_eq!(
        app.world()
            .resource::<bevy::input_focus::InputFocus>()
            .get(),
        Some(notes)
    );
    let title = entity::<NewSceneDialogTitle>(&mut app);
    assert_eq!(app.world().get::<Text>(title).unwrap().0, "编辑场景资料");
}

#[test]
fn feedback_keeps_the_complete_message_without_dirtying_unchanged_text() {
    let mut app = super::tests::app();
    let diagnostic = format!("保存失败：{}\n尚未写入清单。", "完整中文路径/".repeat(60));
    app.world_mut().resource_mut::<SceneEditorState>().status = diagnostic.clone();
    app.update();
    let status = entity::<NewSceneStatusText>(&mut app);
    let tick = app
        .world()
        .entity(status)
        .get_ref::<Text>()
        .unwrap()
        .last_changed();
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .new_scene_name
        .push('新');
    app.update();
    assert_eq!(app.world().get::<Text>(status).unwrap().0, diagnostic);
    assert_eq!(
        app.world()
            .entity(status)
            .get_ref::<Text>()
            .unwrap()
            .last_changed(),
        tick
    );
}

#[test]
fn feedback_scroll_is_independent_bounded_and_does_not_replay_hidden_wheels() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_message::<MouseWheel>()
        .insert_resource(SceneEditorState {
            open: true,
            new_scene_dialog_open: true,
            ..default()
        })
        .add_systems(
            Update,
            (systems::scroll_feedback, scroll_new_scene_dialog_body),
        );
    let cursor = RelativeCursorPosition {
        cursor_over: true,
        normalized: Some(Vec2::splat(0.5)),
    };
    let feedback = app
        .world_mut()
        .spawn((
            DialogFeedbackScroll,
            cursor.clone(),
            ComputedNode {
                size: Vec2::new(400.0, 60.0),
                content_size: Vec2::new(400.0, 120.0),
                inverse_scale_factor: 1.0,
                ..default()
            },
            ScrollPosition::default(),
        ))
        .id();
    let body = app
        .world_mut()
        .spawn((
            NewSceneDialogBodyScroll,
            cursor,
            ComputedNode {
                size: Vec2::new(400.0, 100.0),
                content_size: Vec2::new(400.0, 500.0),
                inverse_scale_factor: 1.0,
                ..default()
            },
            ScrollPosition::default(),
        ))
        .id();
    let wheel = |app: &mut App, amount| {
        app.world_mut().write_message(MouseWheel {
            unit: MouseScrollUnit::Line,
            x: 0.0,
            y: amount,
            window: feedback,
            phase: bevy::input::touch::TouchPhase::Moved,
        });
    };
    app.update();
    wheel(&mut app, -10.0);
    app.update();
    assert_eq!(app.world().get::<ScrollPosition>(feedback).unwrap().y, 60.0);
    assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 0.0);
    app.world_mut().resource_mut::<SceneEditorState>().status = "另一项错误".into();
    app.update();
    assert_eq!(app.world().get::<ScrollPosition>(feedback).unwrap().y, 0.0);
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .new_scene_dialog_open = false;
    wheel(&mut app, -1.0);
    app.update();
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .new_scene_dialog_open = true;
    app.update();
    app.update();
    assert_eq!(app.world().get::<ScrollPosition>(feedback).unwrap().y, 0.0);
}
