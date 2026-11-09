//! 复用正式网格页和原生UI指针链，检查真实命中、中文输入及确认后的同对象历史。

use feathrise_editor_ui::UiInteraction;
use super::{SceneContextMenuState, SceneMenuAction, SceneObjectKey, components::*};
use crate::{authoring::EditorAuthoringSession, resources::MapEditorState};
use bevy::{
    input::{ButtonState, mouse::MouseButtonInput},
    prelude::*,
};

fn point_at(app: &mut App, window: Entity, entity: Entity) {
    let point = app
        .world()
        .get::<UiGlobalTransform>(entity)
        .unwrap()
        .translation;
    assert!(
        app.world()
            .get::<ComputedNode>(entity)
            .unwrap()
            .size()
            .min_element()
            > 0.0
    );
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(Some(point));
    app.world_mut()
        .write_message(bevy::window::WindowEvent::CursorMoved(
            bevy::window::CursorMoved {
                window,
                position: point,
                delta: None,
            },
        ));
    app.update();
}

fn point_at_stable_row(app: &mut App, window: Entity, entity: Entity) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut previous = None;
    let mut stable_frames = 0;
    loop {
        app.update();
        let world = app.world();
        let node = world.get::<ComputedNode>(entity).unwrap();
        let center = world.get::<UiGlobalTransform>(entity).unwrap().translation;
        let layout = (center, node.size());
        let glyphs_ready = world.get::<Children>(entity).is_some_and(|children| children.iter().any(|child|
            world.get::<bevy::text::TextLayoutInfo>(child).is_some_and(|text| !text.glyphs.is_empty())));
        if glyphs_ready && node.size().min_element() > 0.0 && previous == Some(layout) {
            stable_frames += 1;
        } else { stable_frames = 0; }
        if stable_frames >= 2 { break; }
        previous = Some(layout);
        assert!(std::time::Instant::now() < deadline, "场景行中文字和实际布局必须有限稳定：{layout:?}");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    // 稳定后只取一次当前坐标，随后仍只发送一次真实按下；不靠反复右键掩盖命中失败。
    point_at(app, window, entity);
}

fn mouse(app: &mut App, window: Entity, state: ButtonState, button: MouseButton) {
    let event = MouseButtonInput {
        window,
        state,
        button,
    };
    app.world_mut().write_message(event);
    app.world_mut()
        .write_message(bevy::window::WindowEvent::MouseButtonInput(event));
    app.update();
}

#[test]
fn scene_context_menu_real_pointer_and_chinese_input_rename_the_right_clicked_mesh_once() {
    let (mut app, window) = crate::ui::terrain_objects::page_tests::page();
    let world = app.world_mut();
    let root = world
        .query_filtered::<Entity, With<feathrise_editor_ui::EditorThemeRoot>>()
        .single(world)
        .unwrap();
    world
        .entity_mut(root)
        .insert(crate::components::MapEditorRoot);
    super::install_controls(&mut app);
    app.add_systems(
        Update,
        crate::ui::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard
            .in_set(crate::MapEditorSystemSet::Input),
    );
    app.add_systems(
        PreUpdate,
        crate::ui::text_input::collect_ime_guard.after(bevy::input::InputSystems),
    );
    for _ in 0..4 {
        app.update();
    }
    let world = app.world_mut();
    let row = world.query::<(Entity, &SceneObjectRow)>().iter(world).find_map(|(entity, row)| {
        row.0.as_ref().filter(|target| matches!(&target.key, SceneObjectKey::Terrain { id, .. } if id.as_str() == "mesh-b")).map(|_| entity)
    }).unwrap();
    let mut original_entities = world
        .query_filtered::<Entity, Or<(With<MenuButton>, With<RenameInput>, With<Popup>)>>()
        .iter(world)
        .collect::<Vec<_>>();
    original_entities.sort_unstable();
    point_at(&mut app, window, row);
    mouse(&mut app, window, ButtonState::Pressed, MouseButton::Right);
    mouse(&mut app, window, ButtonState::Released, MouseButton::Right);
    assert!(
        app.world()
            .resource::<SceneContextMenuState>()
            .target
            .is_some()
    );
    let world = app.world_mut();
    let rename = world
        .query::<(Entity, &MenuButton)>()
        .iter(world)
        .find(|(_, button)| button.0 == SceneMenuAction::Rename)
        .unwrap()
        .0;
    point_at(&mut app, window, rename);
    mouse(&mut app, window, ButtonState::Pressed, MouseButton::Left);
    mouse(&mut app, window, ButtonState::Released, MouseButton::Left);
    assert!(app.world().resource::<SceneContextMenuState>().renaming);
    let world = app.world_mut();
    let field = world
        .query_filtered::<Entity, With<RenameField>>()
        .single(world)
        .unwrap();
    let input = world
        .query_filtered::<Entity, With<RenameInput>>()
        .single(world)
        .unwrap();
    point_at(&mut app, window, field);
    mouse(&mut app, window, ButtonState::Pressed, MouseButton::Left);
    mouse(&mut app, window, ButtonState::Released, MouseButton::Left);
    assert!(
        app.world().resource::<SceneContextMenuState>().renaming,
        "点击名称输入框不能被误判为外部取消"
    );
    assert_eq!(
        app.world()
            .resource::<bevy::input_focus::InputFocus>()
            .get(),
        Some(input)
    );
    let event = bevy::window::Ime::Commit {
        window,
        value: "村口铜灯".into(),
    };
    app.world_mut().write_message(event.clone());
    app.world_mut()
        .write_message(bevy::window::WindowEvent::Ime(event));
    app.update();
    app.update();
    let name = app
        .world()
        .get::<bevy::text::EditableText>(input)
        .unwrap()
        .value()
        .to_string();
    assert!(
        name.contains("村口铜灯"),
        "应通过正式EditableText接收中文提交：{name}"
    );
    let world = app.world_mut();
    let confirm = world
        .query_filtered::<Entity, With<RenameConfirm>>()
        .single(world)
        .unwrap();
    point_at(&mut app, window, confirm);
    mouse(&mut app, window, ButtonState::Pressed, MouseButton::Left);
    mouse(&mut app, window, ButtonState::Released, MouseButton::Left);
    let start = std::time::Instant::now();
    loop {
        app.update();
        let session = app.world().resource::<EditorAuthoringSession>();
        if session.undo.len() == 1 {
            break;
        }
        assert!(
            start.elapsed().as_secs() < 5,
            "右键名称事务未完成：{}",
            app.world().resource::<MapEditorState>().status
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let session = app.world().resource::<EditorAuthoringSession>();
    let objects = session
        .draft
        .as_ref()
        .unwrap()
        .manifest
        .terrains
        .as_ref()
        .unwrap();
    assert_eq!(objects[0].display_name, "湖边石台与很长的中文网格名称");
    assert_eq!(objects[1].display_name, name);
    assert_eq!(session.undo.len(), 1);
    let world = app.world_mut();
    let mut current_entities = world
        .query_filtered::<Entity, Or<(With<MenuButton>, With<RenameInput>, With<Popup>)>>()
        .iter(world)
        .collect::<Vec<_>>();
    current_entities.sort_unstable();
    assert_eq!(
        current_entities, original_entities,
        "打开和确认不应重建菜单树"
    );
}

#[test]
fn scene_layers_real_pointer_selects_water_vegetation_and_domain_and_opens_exact_context_menu() {
    let (mut app, window) = crate::ui::terrain_objects::page_tests::page();
    crate::ui::scene_layers::tests::load_author_layers_fixture(&mut app);
    let world = app.world_mut();
    let ready_font = world.query::<(&TextFont, &bevy::text::TextLayoutInfo)>().iter(world)
        .find(|(_, layout)| !layout.glyphs.is_empty()).expect("正式网格页应已呈现中文字体").0.font.clone();
    let root = world
        .query_filtered::<Entity, With<feathrise_editor_ui::EditorThemeRoot>>()
        .single(world)
        .unwrap();
    let children = world.get::<Children>(root).unwrap().to_vec();
    for entity in children {
        world.get_mut::<Node>(entity).unwrap().display = Display::None;
    }
    world
        .entity_mut(root)
        .insert(crate::components::MapEditorRoot);
    let tree = world
        .spawn_scene(crate::ui::scene_layers::panel())
        .unwrap()
        .id();
    world.entity_mut(root).add_child(tree);
    // page()只替换当时已存在的文字；新树必须复用同一已加载字体，不能再等路径字体异步到达而挤移行。
    for mut text in world.query::<&mut TextFont>().iter_mut(world) { text.font = ready_font.clone(); }
    app.init_resource::<crate::game_authoring::map_interaction::MapInteractionState>()
        .init_resource::<crate::authoring::WaterBrushStroke>()
        .init_resource::<crate::authoring::VegetationBrushStroke>()
        .init_resource::<crate::authoring::WaterNetworkEditState>();
    super::install_controls(&mut app);
    crate::ui::scene_layers::install(&mut app);
    app.add_systems(
        Update,
        crate::ui::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard
            .in_set(crate::MapEditorSystemSet::Input),
    );
    app.update(); // 先建立真实行绑定；每次点击前由字形和行矩形的稳定门等待完整布局。
    for kind in 0..3 {
        let world = app.world_mut();
        let row = world
            .query::<(Entity, &SceneObjectRow)>()
            .iter(world)
            .find_map(|(entity, row)| {
                row.0
                    .as_ref()
                    .filter(|target| match (&target.key, kind) {
                        (SceneObjectKey::Water { index: 0, .. }, 0) => true,
                        (SceneObjectKey::Vegetation { index: 0, .. }, 1) => true,
                        (
                            SceneObjectKey::Domain {
                                kind: crate::game_authoring::map_interaction::DomainItemKind::Object,
                                index: 0,
                                ..
                            },
                            2,
                        ) => true,
                        _ => false,
                    })
                    .map(|_| entity)
            })
            .unwrap();
        point_at_stable_row(&mut app, window, row);
        mouse(&mut app, window, ButtonState::Pressed, MouseButton::Left);
        mouse(&mut app, window, ButtonState::Released, MouseButton::Left);
        assert_eq!(
            app.world().resource::<MapEditorState>().tool,
            crate::types::EditorTool::Inspect
        );
        let selected = app
            .world()
            .resource::<crate::viewport::LayerCandidateState>()
            .selected
            .as_ref()
            .unwrap();
        assert!(matches!(
            (selected, kind),
            (crate::viewport::LayerTarget::Water(..), 0)
                | (crate::viewport::LayerTarget::Vegetation(_), 1)
                | (crate::viewport::LayerTarget::Domain(_), 2)
        ));
        if kind == 2 {
            let id = "fixture-source-0".to_owned();
            assert_eq!(
                app.world()
                    .resource::<crate::game_authoring::map_interaction::MapInteractionState>()
                    .selected
                    .as_ref(),
                Some(&id)
            );
        }
        point_at_stable_row(&mut app, window, row); // 左选后的属性重排也完整稳定后再取一次右键位置。
        assert_eq!(
            app.world().get::<UiInteraction>(row),
            Some(&UiInteraction::Hovered),
            "类别 {kind} 的行应能真实悬停"
        );
        mouse(&mut app, window, ButtonState::Pressed, MouseButton::Right);
        if app
            .world()
            .resource::<SceneContextMenuState>()
            .target
            .is_none()
        {
            use bevy::ecs::system::RunSystemOnce;
            let diagnostic = app
                .world_mut()
                .run_system_once(
                    |context: super::systems::MenuContext,
                     editor: Res<MapEditorState>,
                     guard: Res<crate::resources::MapEditorNumericScrubMouseGuard>,
                     rows: Query<(Entity, &SceneObjectRow, &UiInteraction)>| {
                        (
                            context.enabled(&editor),
                            guard.blocked(),
                            rows.iter()
                                .filter(|(_, _, interaction)| **interaction != UiInteraction::None)
                                .map(|(entity, row, interaction)| {
                                    (
                                        entity,
                                        row.0
                                            .as_ref()
                                            .map(|target| {
                                                (
                                                    &target.key,
                                                    target.current(&editor, &context.session),
                                                )
                                            })
                                            .map(|(key, current)| (format!("{key:?}"), current)),
                                        format!("{interaction:?}"),
                                    )
                                })
                                .collect::<Vec<_>>(),
                        )
                    },
                )
                .unwrap();
            panic!("类别 {kind} 的右键应打开菜单：{diagnostic:?}");
        }
        mouse(&mut app, window, ButtonState::Released, MouseButton::Right);
        let target = &app
            .world()
            .resource::<SceneContextMenuState>()
            .target
            .as_ref()
            .unwrap()
            .object;
        assert_eq!(
            app.world().get::<SceneObjectRow>(row).unwrap().0.as_ref(),
            Some(target)
        );
        mouse(&mut app, window, ButtonState::Pressed, MouseButton::Right);
        mouse(&mut app, window, ButtonState::Released, MouseButton::Right);
        for _ in 0..3 {
            app.update();
        }
    }
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
}
