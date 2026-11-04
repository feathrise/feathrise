//! 动画时间轴与地图同屏时，真实输入、公共鼠标门及地图快捷键分派的所有权回归。

use super::*;
use crate::{
    authoring::{EditorAuthoringSession, EditorCommand, HeightChange, WaterNetworkEditState},
    resources::{MapDeletionTask, MapEditorState, MapEditorViewportState},
    types::EditorViewportFraming,
    ui::{
        actions::handle_editor_shortcuts,
        building_presets::BuildingPresetEditorState,
        numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard,
        surface_preset_editor::SurfacePresetEditorState,
        water_preset_editor::WaterPresetEditorState,
    },
};
use feathrise_editor_ui::{ActiveNumericInput, ActiveNumericInputScrub};
use feathrise_editor_core::EditorSettings;
use crate::authoring::placement_batch::{PlacementBatchState, handle_placement_batch};
use crate::viewport::multi_select::{PlacementSelectionSet, SelectedMember};
use bevy::ecs::{
    schedule::{graph::DagGroups, ScheduleGraph, SystemKey, SystemSetKey},
    system::IntoSystem,
};

fn fixture(read_only: bool, history: bool) -> (App, Entity) {
    let (mut app, _, _) = super::super::editor_tests::app_context();
    let draft = crate::authoring::test_heightfield_draft(vec![0; 121].into());
    let map_id = draft.map_id.clone();
    let command = EditorCommand::TerrainChanged(vec![HeightChange {
        index: 0,
        before: 0.0,
        after: 1.0,
    }]);
    app.insert_resource(State::new(feathrise_scene_core::state::ScenePhase::Active))
        .insert_resource(feathrise_map_content::ActiveDynamicMap { map_id: Some(map_id.clone()) })
        .insert_resource(EditorAuthoringSession {
            draft: Some(draft),
            undo: vec![command],
            ..default()
        })
        .insert_resource(MapEditorViewportState {
            _image: Handle::default(),
            camera: Entity::PLACEHOLDER,
            center: Vec3::ZERO,
            scene_center: Vec3::ZERO,
            scene_half_extents: Vec3::ONE,
            extent: 50.0,
            yaw: 0.0,
            pitch: 0.5,
            framed_map_id: Some(map_id.clone()),
            render_size: UVec2::new(800, 600),
        })
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ActiveNumericInput>()
        .init_resource::<ActiveNumericInputScrub>()
        .init_resource::<MapDeletionTask>()
        .init_resource::<WaterNetworkEditState>()
        .init_resource::<SurfacePresetEditorState>()
        .init_resource::<WaterPresetEditorState>()
        .init_resource::<BuildingPresetEditorState>()
        .init_resource::<EditorSettings>()
        // 公共门与地图分派沿正式顺序执行，动画输入的先后约束由生产安装函数提供。
        .add_systems(Update, (
            update_map_editor_numeric_scrub_mouse_guard,
            handle_editor_shortcuts,
        ).chain());
    super::super::install_input(&mut app);
    app.world_mut().resource_mut::<MapEditorState>().selected_map_id = Some(map_id);
    let binding = {
        let mut state = app.world_mut().resource_mut::<AnimationEditorState>();
        let session = state.current_mut().unwrap();
        std::sync::Arc::make_mut(&mut session.document).read_only = read_only;
        session.selected = KeySelection::single(0, 0.0);
        if history {
            session.history.push(Edit::Interpolation {
                clip: 0,
                channel: 0,
                before: "STEP".into(),
                after: "LINEAR".into(),
            });
            session.cursor = 1;
        }
        state.revision
    };
    let track = app.world_mut().spawn((
        AnimationTrack { row: None, binding },
        ComputedNode {
            size: Vec2::new(400.0, 24.0),
            inverse_scale_factor: 1.0,
            ..default()
        },
        RelativeCursorPosition {
            cursor_over: true,
            normalized: Some(Vec2::ZERO),
        },
        Interaction::Hovered,
    )).id();
    (app, track)
}

fn press(app: &mut App, keys: &[KeyCode]) {
    let mut keyboard = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    keyboard.reset_all();
    for &key in keys {
        keyboard.press(key);
    }
    drop(keyboard);
    app.update();
}

fn leave_timeline(app: &mut App, track: Entity) {
    app.world_mut().get_mut::<RelativeCursorPosition>(track).unwrap().cursor_over = false;
    *app.world_mut().get_mut::<Interaction>(track).unwrap() = Interaction::None;
}

#[test]
fn timeline_undo_consumes_the_edge_before_map_dispatch_and_releases_on_leave() {
    let (mut app, track) = fixture(false, true);
    press(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyZ]);
    assert!(matches!(app.world().resource::<AnimationEditorState>().pending, Some(PendingAction::Undo)));
    assert!(!app.world().resource::<MapEditorState>().undo_requested);
    assert!(!app.world().resource::<crate::resources::MapEditorNumericScrubMouseGuard>().blocked(), "单独快捷键不应持有鼠标或阻塞下一次地图输入");
    assert!(!app.world().resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::KeyZ));
    assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::ControlLeft));

    // 此夹具只运行输入分派；移除已交给后台的请求，再验证同一真实地图历史重新可用。
    app.world_mut().resource_mut::<AnimationEditorState>().pending = None;
    leave_timeline(&mut app, track);
    press(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyZ]);
    assert!(app.world().resource::<MapEditorState>().undo_requested);
    assert!(app.world().resource::<AnimationEditorState>().pending.is_none());
}

#[test]
fn timeline_home_fits_animation_without_framing_the_map_until_pointer_leaves() {
    let (mut app, track) = fixture(false, false);
    {
        let mut state = app.world_mut().resource_mut::<AnimationEditorState>();
        let session = state.current_mut().unwrap();
        session.window_start = 0.5;
        session.window_span = 0.25;
    }
    press(&mut app, &[KeyCode::Home]);
    let state = app.world().resource::<AnimationEditorState>();
    let session = state.current().unwrap();
    assert_eq!(session.window_start, 0.0);
    assert_eq!(session.window_span, session.metadata.durations[session.clip].max(1.0 / 30.0));
    assert!(app.world().resource::<MapEditorState>().viewport_framing_requested.is_none());
    assert!(!app.world().resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::Home));

    leave_timeline(&mut app, track);
    press(&mut app, &[KeyCode::Home]);
    assert_eq!(app.world().resource::<MapEditorState>().viewport_framing_requested, Some(EditorViewportFraming::Map));
}

#[test]
fn unavailable_animation_undo_stays_in_timeline_instead_of_falling_back_to_map() {
    for (read_only, history) in [(false, false), (true, true)] {
        let (mut app, track) = fixture(read_only, history);
        press(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyZ]);
        assert!(app.world().resource::<AnimationEditorState>().pending.is_none());
        assert!(!app.world().resource::<MapEditorState>().undo_requested, "只读={read_only}、有动画历史={history}");
        leave_timeline(&mut app, track);
        press(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyZ]);
        assert!(app.world().resource::<MapEditorState>().undo_requested);
    }
}

#[test]
fn busy_timeline_consumes_shortcuts_without_replacing_the_pending_operation() {
    for key in [KeyCode::KeyZ, KeyCode::Home] {
        let (mut app, track) = fixture(false, true);
        app.world_mut().resource_mut::<AnimationEditorState>().pending = Some(PendingAction::Reload);
        let keys = if key == KeyCode::KeyZ { vec![KeyCode::ControlLeft, key] } else { vec![key] };
        press(&mut app, &keys);
        assert!(matches!(app.world().resource::<AnimationEditorState>().pending, Some(PendingAction::Reload)));
        let editor = app.world().resource::<MapEditorState>();
        assert!(!editor.undo_requested);
        assert!(editor.viewport_framing_requested.is_none());
        assert!(!app.world().resource::<ButtonInput<KeyCode>>().just_pressed(key));

        app.world_mut().resource_mut::<AnimationEditorState>().pending = None;
        leave_timeline(&mut app, track);
        press(&mut app, &keys);
        let editor = app.world().resource::<MapEditorState>();
        if key == KeyCode::KeyZ {
            assert!(editor.undo_requested);
        } else {
            assert_eq!(editor.viewport_framing_requested, Some(EditorViewportFraming::Map));
        }
    }
}

fn assert_production_batch_order(app: &App) {
    fn key(graph: &ScheduleGraph, ty: std::any::TypeId) -> SystemKey {
        let matches: Vec<_> = graph.systems.iter()
            .filter(|(_, system, _)| system.system_type() == ty)
            .map(|(key, _, _)| key).collect();
        assert_eq!(matches.len(), 1);
        matches[0]
    }
    let graph = app.get_schedule(Update).unwrap().graph();
    let from = key(graph, input.system_type_id());
    let to = key(graph, handle_placement_batch.system_type_id());
    // before(fn)实际连接到系统类型集合；展开生产配置的集合，防止夹具固定链掩盖漏注册。
    let mut hierarchy = graph.hierarchy().clone();
    let (order, hierarchy_graph) = hierarchy.toposort_and_graph().expect("输入层级不得成环");
    let groups: DagGroups<SystemSetKey, SystemKey> = DagGroups::new(hierarchy_graph, order);
    let mut dependencies = groups.flatten(graph.dependency().clone(), |_, _, _, _| {});
    assert!(dependencies.analyze().expect("真实输入依赖不得成环").connected().contains(&(from, to)),
        "生产注册必须保证时间轴先于地图批量复制/粘贴/删除，而不是依赖调度器碰巧选择的执行顺序");
}

fn batch_fixture() -> (App, Entity) {
    let (mut app, track) = fixture(false, false);
    app.add_plugins(bevy::asset::AssetPlugin::default())
        .init_resource::<PlacementBatchState>()
        .init_resource::<PlacementSelectionSet>()
        .init_resource::<crate::viewport::placement_move::PlacementMoveState>()
        .init_resource::<crate::viewport::modes::ViewportModeState>()
        .init_resource::<crate::viewport::LayerCandidateState>()
        .init_resource::<crate::ui::gltf_browser::EditorGltfBrowserState>()
        .init_resource::<crate::ui::placement_transform::PlacementTransformInputState>()
        .init_resource::<crate::ui::courtyard::CourtyardUiState>()
        .init_resource::<crate::ui::text_input::EditorTextInputGuard>()
        .init_resource::<crate::resources::MapEditorSettingsState>()
        .init_resource::<crate::resources::MapBrowserState>()
        .init_resource::<crate::resources::MapBrowserDragState>()
        .init_resource::<crate::resources::MapEditorLayoutState>()
        .init_resource::<crate::authoring::TerrainExternalChangeState>()
        .init_resource::<crate::authoring::courtyards::CourtyardAuthoringState>()
        .add_systems(PreUpdate, |mut editor: ResMut<MapEditorState>| {
            editor.suppress_world_pick_this_frame = false;
        })
        // 这里只给批量域原有的公共门顺序；时间轴与批量域之间仅靠production install_input的约束。
        .add_systems(Update, handle_placement_batch.before(update_map_editor_numeric_scrub_mouse_guard));
    assert_production_batch_order(&app);
    let mut record = crate::viewport::placement_move::interaction_tests::record();
    record.asset_path.clear();
    record.asset_target = feathrise_map_content::MapPlacementAssetTarget::Cuboid {
        size_mm: [2000; 3], color_srgb: [160; 3], roughness_milli: 700, collision: true,
    };
    record.water_surfaces.clear();
    let member = SelectedMember { id: record.id.clone(), index: 0, position: Vec3::from_array(record.translation) };
    app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap()
        .placements.placements.push(record);
    app.world_mut().resource_scope(|world, mut selection: Mut<PlacementSelectionSet>| {
        selection.publish(vec![member], world.resource::<EditorAuthoringSession>());
    });
    leave_timeline(&mut app, track);
    press(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyC]);
    assert!(app.world().resource::<PlacementBatchState>().busy(), "先证明真实地图复制可启动");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while app.world().resource::<PlacementBatchState>().busy() {
        assert!(std::time::Instant::now() < deadline, "地图剪贴板应在有界等待内完成");
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset_all();
        app.update();
        std::thread::yield_now();
    }
    assert!(app.world().resource::<MapEditorState>().status.contains("已复制 1 个对象"));
    app.world_mut().get_mut::<RelativeCursorPosition>(track).unwrap().cursor_over = true;
    *app.world_mut().get_mut::<Interaction>(track).unwrap() = Interaction::Hovered;
    (app, track)
}

#[test]
fn timeline_copy_paste_delete_precede_real_batch_input_and_leave_map_untouched() {
    for key in [KeyCode::KeyC, KeyCode::KeyV, KeyCode::Delete] {
        let (mut app, track) = batch_fixture();
        if key == KeyCode::KeyV {
            let mut state = app.world_mut().resource_mut::<AnimationEditorState>();
            state.clipboard = Some(super::super::editing::copy(
                &state.current().unwrap().document, 0, &KeySelection::single(0, 0.0), state.active.clone().unwrap(),
            ).unwrap());
        }
        let selection = app.world().resource::<PlacementSelectionSet>().members.clone();
        let (revision, history, records) = {
            let session = app.world().resource::<EditorAuthoringSession>();
            (session.revision, session.undo.len(), session.draft.as_ref().unwrap().placements.placements.clone())
        };
        let keys = if key == KeyCode::Delete { vec![key] } else { vec![KeyCode::ControlLeft, key] };
        press(&mut app, &keys);
        assert!(matches!(app.world().resource::<AnimationEditorState>().pending, Some(PendingAction::Command(_))));
        assert!(!app.world().resource::<PlacementBatchState>().busy(), "时间轴按键不能启动地图捕获、剪贴板改写或粘贴事务：{key:?}");
        assert!(!app.world().resource::<PlacementBatchState>().owns_mouse());
        assert!(std::sync::Arc::ptr_eq(&selection, &app.world().resource::<PlacementSelectionSet>().members));
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.revision, revision);
        assert_eq!(session.undo.len(), history);
        assert_eq!(session.draft.as_ref().unwrap().placements.placements, records);
        app.world_mut().resource_mut::<AnimationEditorState>().pending = None;
        leave_timeline(&mut app, track);
        press(&mut app, &keys);
        assert!(app.world().resource::<PlacementBatchState>().busy(), "移开时间轴后地图原有复制/粘贴/删除应恢复：{key:?}");
    }
}

#[test]
fn timeline_history_shortcuts_accept_right_hand_modifiers() {
    for redo in [false, true] {
        let (mut app, _) = fixture(false, true);
        let mut keys = vec![KeyCode::ControlRight, KeyCode::KeyZ];
        if redo {
            app.world_mut().resource_mut::<AnimationEditorState>().current_mut().unwrap().cursor = 0;
            keys.push(KeyCode::ShiftRight);
        }
        press(&mut app, &keys);
        let state = app.world().resource::<AnimationEditorState>();
        assert!(if redo { matches!(state.pending, Some(PendingAction::Redo)) } else { matches!(state.pending, Some(PendingAction::Undo)) });
        assert!(!app.world().resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::KeyZ));
        assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::ControlRight));
    }
}

#[test]
fn undeclared_modifier_chords_are_not_consumed_by_the_timeline() {
    for modifier in [KeyCode::AltLeft, KeyCode::SuperRight] {
        for key in [KeyCode::KeyC, KeyCode::KeyV, KeyCode::KeyZ, KeyCode::Delete, KeyCode::Space, KeyCode::Home] {
            let (mut app, _) = fixture(false, true);
            let mut keys = vec![modifier, key];
            if matches!(key, KeyCode::KeyC | KeyCode::KeyV | KeyCode::KeyZ) { keys.push(KeyCode::ControlLeft); }
            press(&mut app, &keys);
            assert!(app.world().resource::<AnimationEditorState>().pending.is_none());
            assert!(app.world().resource::<ButtonInput<KeyCode>>().just_pressed(key), "未声明的组合应保留边沿：{keys:?}");
        }
    }
    for modifier in [KeyCode::ControlRight, KeyCode::ShiftRight] {
        for key in [KeyCode::Delete, KeyCode::Space, KeyCode::Home] {
            let (mut app, _) = fixture(false, true);
            press(&mut app, &[modifier, key]);
            assert!(app.world().resource::<AnimationEditorState>().pending.is_none());
            assert!(app.world().resource::<ButtonInput<KeyCode>>().just_pressed(key));
        }
    }
}

#[test]
fn numeric_and_ime_focus_keep_the_timeline_shortcut_edge() {
    for numeric in [false, true] {
        let (mut app, _) = fixture(false, true);
        if numeric {
            app.world_mut().resource_mut::<ActiveNumericInput>().entity = Some(Entity::PLACEHOLDER);
        } else {
            app.init_resource::<crate::ui::text_input::EditorTextInputGuard>();
            app.world_mut().resource_mut::<crate::ui::text_input::EditorTextInputGuard>().block_shortcuts = true;
        }
        press(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyZ]);
        assert!(app.world().resource::<AnimationEditorState>().pending.is_none());
        assert!(app.world().resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::KeyZ));
    }
}
