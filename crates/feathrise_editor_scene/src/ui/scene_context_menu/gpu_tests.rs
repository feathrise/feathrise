//! 正式中文字体与主题的真实GPU画面回归；离屏渲染不启动窗口或保存用户设置。

use feathrise_editor_ui::InteractionButton;
use super::{components::*, resources::SceneContextMenuState, types::*};
use crate::{
    authoring::{EditorAuthoringSession, test_heightfield_draft},
    components::SceneEditorRoot,
    resources::SceneEditorState,
};
use bevy::{
    prelude::*,
    render::{
        gpu_readback::{Readback, ReadbackComplete},
        render_resource::{TextureFormat, TextureUsages},
    },
    window::{ExitCondition, PrimaryWindow},
    winit::WinitPlugin,
};

fn frames(app: &mut App, font: &Handle<Font>, count: usize) {
    for _ in 0..count {
        let world = app.world_mut();
        for mut text in world.query::<&mut TextFont>().iter_mut(world) {
            text.font = font.clone().into();
        }
        app.update();
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}

fn snapshot(app: &mut App, target: Handle<Image>, name: &str) {
    let (sender, receiver) = std::sync::mpsc::channel();
    app.world_mut().spawn(Readback::texture(target)).observe(
        move |event: On<ReadbackComplete>, mut commands: Commands| {
            let _ = sender.send(event.data.clone());
            commands.entity(event.entity).try_despawn();
        },
    );
    let start = std::time::Instant::now();
    let mut bytes = loop {
        app.update();
        if let Ok(bytes) = receiver.try_recv() {
            break bytes;
        }
        assert!(start.elapsed().as_secs() < 10, "场景菜单GPU回读超时");
        std::thread::sleep(std::time::Duration::from_millis(16));
    };
    assert_eq!(bytes.len(), 1280 * 720 * 4); // BGRA行满足GPU的256字节对齐。
    for pixel in bytes.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let directory =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.local/validation");
    std::fs::create_dir_all(&directory).unwrap();
    image::RgbaImage::from_raw(1280, 720, bytes)
        .unwrap()
        .save(directory.join(name))
        .unwrap();
}

#[test]
#[ignore = "需要真实GPU；检查菜单与中文重命名浮层，不启动原生窗口"]
fn scene_context_menu_gpu_snapshot() {
    let mut draft = test_heightfield_draft(vec![0; 121].into());
    let mut record = crate::viewport::placement_move::interaction_tests::record();
    std::sync::Arc::make_mut(&mut record.editor).name = "村口铜灯".into();
    let object = SceneObjectTarget {
        scene_id: draft.scene_id.clone(),
        revision: 0,
        key: SceneObjectKey::Placement {
            index: 0,
            id: record.id.clone(),
            mesh: None,
        },
        name: record.editor.name.clone(),
        hidden: false,
        locked: false,
        copyable: true,
    };
    draft.placements.placements.push(record);
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .set(AssetPlugin {
                file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets").into(),
                watch_for_changes_override: Some(false),
                ..default()
            })
            .disable::<WinitPlugin>()
            .disable::<bevy::log::LogPlugin>(),
    )
    .add_plugins(feathrise_editor_ui::EditorThemePlugin)
    .insert_resource(UiScale(1.7))
    .insert_resource(ClearColor(Color::srgb(0.14, 0.12, 0.10)))
    .insert_resource(SceneEditorState {
        open: true,
        selected_scene_id: Some(draft.scene_id.clone()),
        ..default()
    })
    .insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        ..default()
    })
    .init_resource::<SceneContextMenuState>()
    .init_resource::<bevy::input_focus::InputFocus>()
    .add_systems(Update, (super::scene::setup, super::scene::sync).chain());
    crate::ui::theme::install(&mut app);
    if !app.is_plugin_added::<bevy::input_focus::InputFocusPlugin>() {
        app.add_plugins(bevy::input_focus::InputFocusPlugin);
    }
    if !app.is_plugin_added::<bevy::input_focus::InputDispatchPlugin>() {
        app.add_plugins(bevy::input_focus::InputDispatchPlugin);
    }
    if !app.is_plugin_added::<bevy::ui_widgets::TextInputPlugin>() {
        app.add_plugins(bevy::ui_widgets::TextInputPlugin);
    }
    app.finish();
    app.cleanup();
    app.world_mut().spawn((
        Window {
            focused: true,
            ..default()
        },
        PrimaryWindow,
    )); // 只提供正式窗口焦点数据，渲染目标仍是Image。
    let target = app.world_mut().resource_mut::<Assets<Image>>().add({
        let mut image = Image::new_target_texture(1280, 720, TextureFormat::Bgra8UnormSrgb, None);
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        image
    });
    let camera = app
        .world_mut()
        .spawn((Camera2d, bevy::camera::RenderTarget::from(target.clone())))
        .id();
    let font = app.world_mut().resource_mut::<Assets<Font>>().add(Font::from_bytes(
        feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec(),
    ));
    let row = app
        .world_mut()
        .spawn((
            InteractionButton,
            SceneObjectRow(Some(object.clone())),
            Node {
                width: percent(100),
                height: px(28),
                padding: UiRect::all(px(4)),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(crate::ui::theme::SELECT_BG),
            BorderColor::all(crate::ui::theme::COPPER_BORDER),
        ))
        .with_child((
            Text::new("村口铜灯 · 资产"),
            TextFont {
                font: font.clone().into(),
                font_size: 11.0.into(),
                ..default()
            },
            TextColor(crate::ui::theme::TEXT_PRIMARY),
        ))
        .id();
    let sidebar = app
        .world_mut()
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(12),
                top: px(20),
                width: px(230),
                height: px(200),
                flex_direction: FlexDirection::Column,
                row_gap: px(8),
                padding: UiRect::all(px(8)),
                ..default()
            },
            BackgroundColor(crate::ui::theme::PANEL_BG),
        ))
        .with_child((
            Text::new("场景树"),
            TextFont {
                font: font.clone().into(),
                font_size: 12.0.into(),
                ..default()
            },
            TextColor(crate::ui::theme::TEXT_ACCENT),
        ))
        .add_child(row)
        .id();
    app.world_mut()
        .spawn((
            SceneEditorRoot,
            bevy::ui::UiTargetCamera(camera),
            Node {
                width: percent(100),
                height: percent(100),
                ..default()
            },
        ))
        .add_child(sidebar);
    frames(&mut app, &font, 30);
    {
        let mut menu = app.world_mut().resource_mut::<SceneContextMenuState>();
        menu.target = Some(MenuTarget { row, object });
        menu.anchor = Vec2::new(1280.0 / 1.7 - 2.0, 720.0 / 1.7 - 2.0);
    }
    frames(&mut app, &font, 5);
    let world = app.world_mut();
    let (node, transform) = world
        .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<Popup>>()
        .single(world)
        .unwrap();
    let panel = Rect::from_center_size(transform.translation, node.size());
    assert!(panel.min.cmpge(Vec2::ZERO).all() && panel.max.cmple(Vec2::new(1280.0, 720.0)).all());
    let mut count = 0;
    for (node, transform, children) in world
        .query_filtered::<(&ComputedNode, &UiGlobalTransform, &Children), With<MenuButton>>()
        .iter(world)
    {
        let rect = Rect::from_center_size(transform.translation, node.size());
        assert!(
            rect.min.cmpge(panel.min - Vec2::ONE).all()
                && rect.max.cmple(panel.max + Vec2::ONE).all()
        );
        assert!(children.iter().any(|child| {
            world
                .get::<bevy::text::TextLayoutInfo>(child)
                .is_some_and(|text| !text.glyphs.is_empty())
        }));
        count += 1;
    }
    assert_eq!(count, 6);
    snapshot(&mut app, target.clone(), "scene-context-menu.png");
    {
        let world = app.world_mut();
        let (entity, mut input) = world
            .query_filtered::<(Entity, &mut bevy::text::EditableText), With<RenameInput>>()
            .single_mut(world)
            .unwrap();
        super::systems::set_name(&mut input, "村口铜灯");
        world
            .resource_mut::<bevy::input_focus::InputFocus>()
            .set(entity, bevy::input_focus::FocusCause::Navigated);
    }
    app.world_mut()
        .resource_mut::<SceneContextMenuState>()
        .renaming = true;
    frames(&mut app, &font, 5);
    let world = app.world_mut();
    let text = world
        .query_filtered::<&bevy::text::TextLayoutInfo, With<RenameInput>>()
        .single(world)
        .unwrap();
    assert!(!text.glyphs.is_empty());
    snapshot(&mut app, target.clone(), "scene-context-menu-rename.png");

    // 同一正式 GPU／中文字体链验证四种作者内容；夹具只构造内存草稿。
    crate::ui::scene_layers::tests::load_author_layers_fixture(&mut app);
    app.world_mut()
        .resource_mut::<SceneContextMenuState>()
        .target = None;
    app.world_mut().resource_mut::<UiScale>().0 = 1.1;
    let world = app.world_mut();
    let root = world
        .query_filtered::<Entity, With<SceneEditorRoot>>()
        .single(world)
        .unwrap();
    let children = world.get::<Children>(root).unwrap().to_vec();
    for entity in children {
        world.get_mut::<Node>(entity).unwrap().display = Display::None;
    }
    let tree = world
        .spawn_scene(crate::ui::scene_layers::panel())
        .unwrap()
        .id();
    world
        .entity_mut(tree)
        .insert(BackgroundColor(crate::ui::theme::PANEL_BG));
    world.entity_mut(root).add_child(tree);
    crate::ui::scene_layers::tests::install_snapshot_sync(&mut app);
    frames(&mut app, &font, 8);
    crate::ui::scene_layers::tests::assert_snapshot_glyphs(&mut app);
    snapshot(&mut app, target, "scene-tree-author-fixture.png");
}
