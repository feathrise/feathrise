//! 真实常驻网格页、公共数值输入及场景选择回归；字体布局不代替原生窗口验收。
use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use super::{components::*, resources::*, *};
use crate::{
    authoring::EditorAuthoringSession,
    resources::SceneEditorState,
    ui::sidebars::{EditorLeftSidebarTab, EditorRightSidebarTab, SceneEditorSidebarState},
    viewport::modes::{ViewportMode, ViewportModeState},
};
use bevy::{
    camera::{ComputedCameraValues, RenderTargetInfo, Viewport},
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
        mouse::MouseButtonInput,
    },
    prelude::*,
    window::PrimaryWindow,
};
use feathrise_editor_ui::{
    ActiveNumericInput, NumericInputCommitOutcome,
    NumericInputCommitSource, NumericInputCommitted, NumericInputDisabled, NumericInputValue, NumericValue,
};
use feathrise_scene_content::*;

#[path = "continuous_lod_tests.rs"]
mod continuous_lod_tests;
#[path = "height_tests.rs"]
mod height_tests;
#[path = "creation_placement_tests.rs"]
mod creation_placement_tests;
#[path = "creation_gpu_tests.rs"]
mod creation_gpu_tests;
#[path = "../scene_sidebar/gpu_snapshot_tests.rs"]
mod scene_directory_snapshot;

#[derive(Component, Clone, Copy, Default)]
struct PageHost;
#[derive(Component, Clone, Copy, Default)]
struct ListHost;
fn shell() -> impl Scene {
    bsn! { feathrise_editor_ui::EditorThemeRoot Node { width: px(600), height: px(700), column_gap: px(8) } Children [
        PageHost Node { width: px(280), height: px(700), min_width: px(0), flex_direction: FlexDirection::Column,
            overflow: Overflow::scroll_y(), flex_shrink: 0.0 } Children [@scene::panel()]--
        ListHost Node { width: px(280), height: px(700), min_width: px(0), flex_direction: FlexDirection::Column,
            flex_shrink: 0.0 } Children [@crate::ui::gltf_browser::scene_display_panel()-- @scene::scene_list_panel()-- @scene::properties_panel()]
    ] }
}

pub(crate) fn page() -> (App, Entity) {
    page_with_render(false)
}

/// 创建表单如今位于右侧工具页，夹具必须显式切到工具后才能编辑真实数字框。
pub(crate) fn creation_page() -> (App, Entity) {
    let (mut app, window) = page();
    app.world_mut().resource_mut::<SceneEditorSidebarState>().right = EditorRightSidebarTab::Tools;
    {
        let mut state = app.world_mut().resource_mut::<TerrainObjectsEditorState>();
        state.creation_source = CreationSource::Clipmap;
        state.placement_mode = CreationPlacementMode::Coordinates;
    } // 既有规格输入夹具显式选择适用来源与坐标用途；正式新会话默认立方体点放。
    app.update();
    (app, window)
}

fn page_with_render(render: bool) -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin {
        file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets").into(),
        watch_for_changes_override: Some(false), ..default()
    }, bevy::scene::ScenePlugin));
    if render {
        // 仅装配正式二维渲染链，离屏UI不需要DLSS项目身份、音频或三维游戏插件。
        app.add_plugins((
            WindowPlugin { primary_window: None, exit_condition: bevy::window::ExitCondition::DontExit, ..default() },
            bevy::render::RenderPlugin::default(),
            bevy::image::ImagePlugin::default(),
            bevy::mesh::MeshPlugin,
            bevy::camera::CameraPlugin,
            bevy::core_pipeline::CorePipelinePlugin,
            bevy::sprite::SpritePlugin,
            bevy::sprite_render::SpriteRenderPlugin,
        ));
    } else {
        app.add_plugins(bevy::camera::visibility::VisibilityPlugin);
    }
    app.add_plugins((
        bevy::input::InputPlugin,
        bevy::input_focus::InputFocusPlugin,
        bevy::input_focus::InputDispatchPlugin,
        bevy::picking::input::PointerInputPlugin,
        bevy::picking::PickingPlugin,
        bevy::picking::InteractionPlugin,
        bevy::transform::TransformPlugin,
        bevy::text::TextPlugin,
        bevy::ui::UiPlugin,
        bevy::ui_widgets::TextInputPlugin,
    ));
    if render { app.add_plugins(bevy::ui_render::UiRenderPlugin); }
    app.add_plugins(feathrise_editor_ui::EditorUiPlugin);
    if !render {
        // GPU夹具保留正式插件安装的默认图像和句柄分配器；再次init_asset会重置它们。
        app.init_asset::<Image>()
            .init_asset::<Mesh>()
            .init_asset::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>()
            .init_asset::<bevy::image::TextureAtlasLayout>();
    }
    app
        .add_message::<bevy::window::WindowEvent>()
        .add_message::<bevy::window::Ime>() // 无WindowPlugin的真实官方输入夹具仍须注册窗口IME消息。
        .init_resource::<crate::ui::text_input::EditorTextInputGuard>()
        .init_resource::<crate::resources::SceneEditorNumericScrubMouseGuard>()
        .init_resource::<crate::viewport::LayerCandidateState>()
        .init_resource::<feathrise_scene::TerrainObjectTransformPreview>()
        .init_resource::<ViewportModeState>();
    let mut draft = crate::authoring::test_heightfield_draft(vec![0; 121].into());
    let mut first = draft.manifest.terrain_objects_for_edit()[0].clone();
    first.id = TerrainId::parse("mesh-a").unwrap();
    first.display_name = "湖边石台与很长的中文网格名称".into();
    let mut second = first.clone();
    second.id = TerrainId::parse("mesh-b").unwrap();
    second.display_name = "第二个网格".into();
    second.transform.translation[0] = 29.0;
    draft.manifest.terrain = SceneTerrainDescriptor::empty();
    draft.manifest.terrains = Some(vec![first, second].into());
    let map = draft.scene_id.clone();
    app.insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        ..default()
    })
    .insert_resource(SceneEditorState {
        open: true,
        selected_scene_id: Some(map.clone()),
        ..default()
    })
        .init_resource::<SceneEditorSidebarState>();
    app.world_mut().resource_mut::<SceneEditorSidebarState>().left = EditorLeftSidebarTab::Meshes;
    app.world_mut().resource_mut::<SceneEditorSidebarState>().navigator = crate::ui::sidebars::EditorNavigatorTab::SceneTree;
    app.configure_sets(
        Update,
        (
            feathrise_editor_ui::EditorUiSystemSet::NumericInput,
            crate::SceneEditorSystemSet::Input,
            crate::SceneEditorSystemSet::Tasks,
            crate::SceneEditorSystemSet::Presentation,
        )
            .chain(),
    );
    install(&mut app);
    app.init_resource::<crate::ui::gltf_browser::EditorGltfBrowserState>()
        .init_resource::<crate::resources::TerrainShadingSession>()
        .init_resource::<crate::ui::placement_transform::PlacementTransformInputState>()
        .add_systems(Update, (crate::ui::scene_inspector::sync_selected_tool_properties, crate::ui::scene_inspector::sync_scene_inspector)
            .chain().in_set(crate::SceneEditorSystemSet::Presentation));
    app.add_systems(Startup, shell.spawn());
    let window = app
        .world_mut()
        .spawn((
            Window {
                resolution: (1280, 900).into(),
                focused: true,
                ..default()
            },
            PrimaryWindow,
        ))
        .id();
    let camera = app.world_mut().spawn((
        Camera2d,
        Camera {
            viewport: Some(Viewport {
                physical_size: UVec2::new(1280, 900),
                ..default()
            }),
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo {
                    physical_size: UVec2::new(1280, 900),
                    scale_factor: 1.0,
                }),
                ..default()
            },
            ..default()
        },
    )).id();
    if render {
        use bevy::render::render_resource::{TextureFormat, TextureUsages};
        let mut image = Image::new_target_texture(1280, 900, TextureFormat::Rgba8UnormSrgb, None);
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        let image = app.world_mut().resource_mut::<Assets<Image>>().add(image);
        app.world_mut().entity_mut(camera).insert(bevy::camera::RenderTarget::from(image));
        app.finish();
        app.cleanup();
    }
    let font = app
        .world_mut()
        .resource_mut::<Assets<Font>>()
        .add(Font::from_bytes(
            feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec(),
        ));
    app.update();
    if render {
        let world = app.world_mut();
        let host = world.query_filtered::<Entity, With<ListHost>>().single(world).unwrap();
        let root = world.get::<ChildOf>(host).unwrap().parent();
        world.entity_mut(root).insert(bevy::ui::UiTargetCamera(camera));
    }
    {
        let mut state = app.world_mut().resource_mut::<TerrainObjectsEditorState>();
        state.scene_id = Some(map);
        state.selected = Some(TerrainId::parse("mesh-a").unwrap());
        state.selected_index = Some(0);
    }
    app.world_mut().resource_mut::<crate::viewport::LayerCandidateState>()
        .confirm_terrain(&TerrainId::parse("mesh-a").unwrap(), 0);
    {
        let world = app.world_mut();
        for mut text in world.query::<&mut TextFont>().iter_mut(world) {
            text.font = font.clone().into();
        }
    }
    for _ in 0..5 {
        app.update();
    }
    (app, window)
}

fn field(app: &mut App, field: Field) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &Field)>()
        .iter(world)
        .find(|(_, current)| **current == field)
        .unwrap()
        .0
}
/// 需要“当前所选层”时显式指定用途；field本身始终按固定字段身份查找，不借最后聚焦组路由。
fn selected_lod_field(app: &mut App, display: bool, column: usize) -> Entity {
    let list = *app.world().resource::<TerrainObjectsEditorState>().lod(display);
    field(app, Field::Lod(display, 8 + list.selected.checked_sub(list.start).expect("所选层已滚入固定行池") * 2 + column))
}
fn selected_lod_value(app: &mut App, display: bool, column: usize) -> f32 {
    let root = selected_lod_field(app, display, column);
    app.world().get::<NumericInputValue>(root).unwrap().0.as_f32().unwrap()
}
fn action(app: &mut App, action: Action) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &ActionButton)>()
        .iter(world)
        .find(|(_, current)| current.0 == action)
        .unwrap()
        .0
}
fn point_at(app: &mut App, window: Entity, entity: Entity) {
    // 两组真实表格放在同一外部属性滚动区；测试先滚入目标，再通过正式指针命中，不伪造Interaction。
    let mut ancestor = entity;
    while let Some(parent) = app.world().get::<ChildOf>(ancestor).map(ChildOf::parent) {
        ancestor = parent;
        if app.world().get::<crate::ui::scene_inspector::InspectorBody>(ancestor).is_none() { continue; }
        continuous_lod_tests::reveal(app, ancestor, entity);
        break;
    }
    let mut point = app
        .world()
        .get::<UiGlobalTransform>(entity)
        .unwrap()
        .affine()
        .translation;
    if app.world().get::<LodRow>(entity).is_some() {
        point.x -= app.world().get::<ComputedNode>(entity).unwrap().size().x * 0.5 - 18.0;
    } // 行号用于显式选层；数值列现已是独立公共输入，不能用行中心误点它们。
    assert!(
        app.world()
            .get::<ComputedNode>(entity)
            .unwrap()
            .size()
            .min_element()
            > 0.0
    );
    move_cursor(app, window, point);
}
fn move_cursor(app: &mut App, window: Entity, point: Vec2) {
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
fn mouse(app: &mut App, window: Entity, button: MouseButton, state: ButtonState) {
    let message = MouseButtonInput { button, state, window };
    app.world_mut().write_message(message);
    app.world_mut()
        .write_message(bevy::window::WindowEvent::MouseButtonInput(message));
    app.update();
}
fn click(app: &mut App, window: Entity, entity: Entity, button: MouseButton) {
    point_at(app, window, entity);
    mouse(app, window, button, ButtonState::Pressed);
    mouse(app, window, button, ButtonState::Released);
    app.update();
}
fn key(app: &mut App, window: Entity, code: KeyCode, logical: Key, text: Option<&str>) {
    app.world_mut().write_message(KeyboardInput {
        key_code: code,
        logical_key: logical.clone(),
        state: ButtonState::Pressed,
        text: text.map(Into::into),
        repeat: false,
        window,
    });
    app.update();
    app.world_mut().write_message(KeyboardInput {
        key_code: code,
        logical_key: logical,
        state: ButtonState::Released,
        text: None,
        repeat: false,
        window,
    });
    app.update();
}
fn late_commit(app: &mut App, entity: Entity, value: f32) {
    app.world_mut().write_message(NumericInputCommitted {
        entity,
        value: NumericValue::F32(value),
        outcome: NumericInputCommitOutcome::Accepted,
        source: NumericInputCommitSource::Text,
        changed: true,
        edited: true,
    });
    app.update();
}
fn complete(app: &mut App) {
    let started = std::time::Instant::now();
    while app
        .world()
        .resource::<TerrainObjectsEditorState>()
        .preparing()
    {
        app.update();
        assert!(started.elapsed().as_secs() < 10);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    app.update();
}

/// 仅构造内存作者记录，LOD按钮仍通过正式后台候选校验；不创建或改写测试资产。
fn native_page(kind: SceneTerrainKind, layers: usize) -> (App, Entity) {
    let (mut app, window) = page();
    configure_native(&mut app, kind, layers);
    let world = app.world_mut();
    let properties = world.query::<(Entity, &crate::ui::scene_inspector::InspectorSection)>().iter(world)
        .find(|(_, section)| **section == crate::ui::scene_inspector::InspectorSection::Terrain).unwrap().0;
    let host = world.get::<ChildOf>(properties).unwrap().parent();
    let body = world.spawn((crate::ui::scene_inspector::InspectorBody, ScrollPosition::default(),
        Node { width: percent(100), min_width: px(0), min_height: px(0), flex_grow: 1.0,
            flex_direction: FlexDirection::Column, overflow: Overflow::scroll_y(), ..default() }, ChildOf(host))).id();
    world.entity_mut(properties).insert(ChildOf(body));
    for _ in 0..5 { app.update(); }
    (app, window)
}
fn configure_native(app: &mut App, kind: SceneTerrainKind, layers: usize) {
    assert!((1..=6).contains(&layers));
    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
    let draft = session.draft.as_mut().unwrap();
    let map = draft.scene_id.clone();
    let object = &mut std::sync::Arc::make_mut(draft.manifest.terrains.as_mut().unwrap())[0];
    let folder = object.private_folder(&map);
    object.world_min = [-512.0; 2];
    object.world_max = [512.0; 2];
    object.terrain.kind = kind;
    object.terrain.glb_path.clear();
    object.terrain.base_height = -8.0;
    object.terrain.height_scale = 16.0;
    object.terrain.resolution = [4097; 2];
    let distance_lods = (0..layers).map(|index| SceneCollisionLod {
        max_distance_meters: 16.0 * (1_u32 << index) as f32,
        spacing_multiplier: (1_u32 << index).min(16),
    }).collect::<Vec<_>>();
    if kind == SceneTerrainKind::Voxel {
        object.terrain.height_path = None;
        object.terrain.display_lods = None;
        object.terrain.collision_lods.clear();
        object.voxel = Some(SceneVoxelTerrainDescriptor {
            cell_size_meters: 0.25,
            min: [-512.0, -8.0, -512.0], max: [512.0, 8.0, 512.0],
            source_path: format!("{folder}/source.scene_voxel_source.json"),
            display_lods: distance_lods.clone(), collision_lods: distance_lods,
        });
    } else {
        assert_eq!(kind, SceneTerrainKind::SparseClipmap);
        object.voxel = None;
        object.terrain.height_path = Some(format!("{folder}/height_tiles.scene_height_manifest.json"));
        object.terrain.collision_lods = distance_lods;
        object.terrain.display_lods = Some((0..layers).map(|index| SceneTerrainDisplayLod {
            outer_half_extent_meters: (index + 1 < layers).then_some(16.0 * (1_u32 << index) as f32),
            // 最远层维持可验证的上传预算；只剩一层也不能靠过大的候选失败掩盖禁用逻辑。
            spacing_multiplier: if index + 1 == layers { 32 } else { 1_u32 << index },
        }).collect());
        let spec = SparseHeightSpec::new(object.world_min, object.world_max, 0.25, -8.0, 16.0).unwrap();
        compile_display_lod_plan(&spec, object.terrain.display_lods.as_ref().unwrap()).unwrap();
        feathrise_scene::validate_layered_collision_settings(&spec, &object.terrain.collision_lods).unwrap();
    }
    validate_terrain_objects(&draft.manifest).unwrap();
}

#[test]
fn stacked_properties_keep_transform_representation_and_both_lod_sections_visible() {
    let (mut app, window) = native_page(SceneTerrainKind::SparseClipmap, 3);
    let before = app.world().resource::<EditorAuthoringSession>().next_serial;
    let world = app.world_mut();
    let editors = world.query::<(Entity, &NativeLodEditor, &ChildOf)>().iter(world)
        .map(|(entity, editor, parent)| (entity, editor.0, parent.parent())).collect::<Vec<_>>();
    assert_eq!(editors.len(), 2);
    for (entity, display, parent) in &editors {
        assert!(world.get::<PanelSection>(*parent).unwrap().0 ==
            if *display { Section::DisplayLods } else { Section::CollisionLods });
        assert!(world.get::<ComputedNode>(*entity).unwrap().size().y > 0.0, "默认两组都要有完整详情");
        let first = world.query::<(&LodRowText, &Text)>().iter(world)
            .find(|(row, _)| row.0 == 0 && row.2 == *display && matches!(row.1, LodColumn::Layer)).unwrap().1;
        assert_eq!(first.0, "0", "显示和碰撞都从LOD0开始，真实层数仍为3");
        let summary = world.query::<(&NativeLodSummary, &Text)>().iter(world)
            .find(|(summary, _)| summary.0 == *display).unwrap().1;
        assert!(summary.0.contains("3 层"));
    }
    for wanted in [Section::Basic, Section::Type, Section::DisplayLods, Section::CollisionLods] {
        assert!(world.query::<(&PanelSection, &Node)>().iter(world).any(|(section, node)| section.0 == wanted && node.display == Display::Flex));
    }
    let transform = action(&mut app, Action::BasicProperties);
    click(&mut app, window, transform, MouseButton::Left);
    assert!(!app.world().resource::<TerrainObjectsEditorState>().section_open(MeshPropertySection::Basic));
    assert!(app.world().resource::<TerrainObjectsEditorState>().section_open(MeshPropertySection::Type));
    assert!(app.world().resource::<TerrainObjectsEditorState>().section_open(MeshPropertySection::DisplayLods));
    let collision = action(&mut app, Action::CollisionLods);
    click(&mut app, window, collision, MouseButton::Left);
    app.update();
    assert!(!app.world().resource::<TerrainObjectsEditorState>().section_open(MeshPropertySection::CollisionLods));
    assert!(app.world().resource::<TerrainObjectsEditorState>().section_open(MeshPropertySection::DisplayLods));
    for (entity, display, parent) in &editors {
        assert_eq!(app.world().get::<ChildOf>(*entity).unwrap().parent(), *parent);
        assert_eq!(app.world().get::<Node>(*entity).unwrap().display, if *display { Display::Flex } else { Display::None });
    }
    click(&mut app, window, collision, MouseButton::Left);
    let world = app.world_mut();
    for (entity, _, parent) in &editors {
        assert_eq!(world.get::<ChildOf>(*entity).unwrap().parent(), *parent, "展开只能显示原地表格，不能移挂另组");
        assert_eq!(world.get::<Node>(*entity).unwrap().display, Display::Flex);
    }
    assert_eq!(world.query::<&LodRow>().iter(world).count(), super::lod_list::ROW_POOL_SIZE * 2);
    assert_eq!(world.query::<&Field>().iter(world).filter(|field| matches!(field, Field::Lod(_, _))).count(), super::lod_list::ROW_POOL_SIZE * 4);
    assert_eq!(world.resource::<EditorAuthoringSession>().next_serial, before);
    assert!(world.resource::<EditorAuthoringSession>().undo.is_empty());
}

#[test]
fn independent_lod_groups_cancel_only_the_folded_input_and_reject_cross_group_late_values() {
    let (mut app, window) = native_page(SceneTerrainKind::SparseClipmap, 3);
    let before = current_object(&app);
    let display = field(&mut app, Field::Lod(true, 8));
    let collision = field(&mut app, Field::Lod(false, 8));
    click(&mut app, window, display, MouseButton::Middle);
    key(&mut app, window, KeyCode::Digit1, Key::Character("18".into()), Some("18"));
    let fold_display = action(&mut app, Action::DisplayLods);
    click(&mut app, window, fold_display, MouseButton::Left);
    assert!(app.world().resource::<TerrainObjectsEditorState>().section_open(MeshPropertySection::CollisionLods));
    click(&mut app, window, collision, MouseButton::Middle);
    assert_eq!(app.world().resource::<ActiveNumericInput>().entity, Some(collision));
    key(&mut app, window, KeyCode::Digit2, Key::Character("24".into()), Some("24"));
    for value in [18.0, 999.0] { late_commit(&mut app, display, value); }
    assert_eq!(app.world().resource::<EditorAuthoringSession>().next_serial, 0);
    assert_eq!(app.world().resource::<ActiveNumericInput>().entity, Some(collision));
    key(&mut app, window, KeyCode::Enter, Key::Enter, None);
    complete(&mut app);
    let after = current_object(&app);
    assert_eq!(after.terrain.display_lods, before.terrain.display_lods);
    assert_eq!(after.terrain.collision_lods[0].max_distance_meters, 24.0);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
}

#[test]
fn native_lod_structure_buttons_reject_a_stale_presented_revision_or_selected_layer() {
    for display in [true, false] { for remove in [true, false] { for revision_changed in [true, false] {
        let (mut app, window) = native_page(SceneTerrainKind::SparseClipmap, 3);
        let before = current_object(&app);
        let button = action(&mut app, if remove { Action::RemoveLod(display) } else { Action::AddLod(display) });
        point_at(&mut app, window, button);
        let target = app.world().get::<LodActionBinding>(button).unwrap().0.clone().unwrap();
        assert_eq!(target.display, display);
        assert_eq!(target.index, 0);
        if revision_changed { app.world_mut().resource_mut::<EditorAuthoringSession>().revision += 1; }
        else { app.world_mut().resource_mut::<TerrainObjectsEditorState>().lod_mut(display).selected = 1; }
        mouse(&mut app, window, MouseButton::Left, ButtonState::Pressed);
        mouse(&mut app, window, MouseButton::Left, ButtonState::Released);
        assert_eq!(app.world().resource::<EditorAuthoringSession>().next_serial, 0, "过期点击必须在启动后台事务前拒绝");
        assert_eq!(current_object(&app), before);
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    } } }
}
fn current_object(app: &App) -> SceneTerrainObject {
    selected(app.world().resource::<EditorAuthoringSession>(),
        app.world().resource::<TerrainObjectsEditorState>()).unwrap().clone()
}
fn assert_other_lod_usage_preserved(before: &SceneTerrainObject, after: &SceneTerrainObject, display: bool) {
    if let (Some(before), Some(after)) = (&before.voxel, &after.voxel) {
        if display { assert_eq!(before.collision_lods, after.collision_lods); }
        else { assert_eq!(before.display_lods, after.display_lods); }
    } else if display {
        assert_eq!(before.terrain.collision_lods, after.terrain.collision_lods);
    } else {
        assert_eq!(before.terrain.display_lods, after.terrain.display_lods);
    }
}
fn numeric_value(app: &mut App, field_id: Field) -> f32 {
    let entity = field(app, field_id);
    app.world().get::<NumericInputValue>(entity).unwrap().0.as_f32().unwrap()
}

#[test]
fn mesh_page_and_scene_share_one_row_pool_and_keep_creation_fields_distinct() {
    let (mut app, _) = page();
    let world = app.world_mut();
    let left = world
        .query_filtered::<Entity, With<MeshPageRoot>>()
        .single(world)
        .unwrap();
    let right = world
        .query_filtered::<Entity, With<MeshSceneListRoot>>()
        .single(world)
        .unwrap();
    let rows = world
        .query::<(Entity, &Row)>()
        .iter(world)
        .map(|(entity, _)| entity)
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), crate::ui::list_layout::SCENE_OBJECT_LIST_ROWS);
    for row in rows {
        let mut current = row;
        while let Some(parent) = world.get::<ChildOf>(current) {
            current = parent.parent();
            assert_ne!(current, left);
            if current == right {
                break;
            }
        }
        assert_eq!(current, right);
    }
    let fields = world
        .query::<&Field>()
        .iter(world)
        .copied()
        .collect::<Vec<_>>();
    assert_eq!(
        fields
            .iter()
            .filter(|field| matches!(field, Field::Creation(_)))
            .count(),
        8
    );
    assert_eq!(
        fields
            .iter()
            .filter(|field| matches!(field, Field::Selected(_)))
            .count(),
        7
    );
    assert_eq!(fields.iter().filter(|field| matches!(field, Field::Rotation(_))).count(), 3);
    // 两组LOD数值根独立于对象字段；各自的固定池不能混成一套共享输入。
    for display in [true, false] {
        assert_eq!(fields.iter().filter(|field| matches!(field, Field::Lod(usage, _) if *usage == display)).count(),
            super::lod_list::ROW_POOL_SIZE * 2);
    }
    assert_eq!(
        fields
            .iter()
            .filter(|field| matches!(field, Field::Policy(_)))
            .count(),
        4
    );
    assert!(!world.resource::<TerrainObjectsEditorState>().creation_open);
}

#[test]
fn mesh_creation_entry_preserves_other_workspaces_and_toggles_normally() {
    use crate::ui::vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState};

    let (mut app, window) = creation_page();
    app.init_resource::<VegetationPresetEditorState>();
    let toggle = action(&mut app, Action::ToggleCreation);
    let remove = action(&mut app, Action::Remove);
    let map = app.world().resource::<TerrainObjectsEditorState>().scene_id.clone();
    let revision = app.world().resource::<EditorAuthoringSession>().revision;
    for tab in [PresetWorkspaceTab::Document, PresetWorkspaceTab::AssetPreview, PresetWorkspaceTab::Vegetation(7)] {
        for already_open in [false, true] {
            app.world_mut().resource_mut::<VegetationPresetEditorState>().active_tab = tab;
            let ui_revision = app.world().resource::<VegetationPresetEditorState>().ui_revision;
            app.world_mut().resource_mut::<TerrainObjectsEditorState>().creation_open = already_open;
            app.update();
            assert!(app.world().get::<bevy::ui::InteractionDisabled>(toggle).is_none(),
                "主场景仍在时，其他中央标签不能禁用添加入口");
            assert!(app.world().get::<bevy::ui::InteractionDisabled>(remove).is_some(),
                "中央显示其他工作区时，隐藏的对象操作仍应禁止");
            click(&mut app, window, toggle, MouseButton::Left);
            let workspace = app.world().resource::<VegetationPresetEditorState>();
            assert_eq!(workspace.active_tab, tab);
            assert_eq!(workspace.ui_revision, ui_revision, "添加区不应重建中央工作区");
            let state = app.world().resource::<TerrainObjectsEditorState>();
            assert_eq!(state.creation_open, !already_open, "添加区在原工作区正常展开和折叠");
            assert_eq!(state.scene_id, map);
            let session = app.world().resource::<EditorAuthoringSession>();
            assert_eq!(session.revision, revision);
            assert!(session.undo.is_empty());
            assert!(!state.preparing(), "展开入口不应启动创建或文件任务");
        }
    }
}

#[test]
fn mesh_creation_entry_keeps_scene_and_modal_guards_in_other_workspaces() {
    use crate::ui::vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState};

    for blocked in 0..5 {
        let (mut app, window) = creation_page();
        app.init_resource::<VegetationPresetEditorState>();
        app.world_mut().resource_mut::<VegetationPresetEditorState>().active_tab = PresetWorkspaceTab::Document;
        match blocked {
            0 => app.world_mut().resource_mut::<EditorAuthoringSession>().draft = None,
            1 => app.world_mut().resource_mut::<EditorAuthoringSession>().saving = true,
            2 => app.world_mut().resource_mut::<SceneEditorState>().scene_transition_in_progress = true,
            3 => app.world_mut().resource_mut::<SceneEditorState>().new_scene_dialog_open = true,
            _ => {
                app.init_resource::<crate::resources::SceneEditorSettingsState>();
                app.world_mut().resource_mut::<crate::resources::SceneEditorSettingsState>().open = true;
            }
        }
        app.update();
        let toggle = action(&mut app, Action::ToggleCreation);
        assert!(app.world().get::<bevy::ui::InteractionDisabled>(toggle).is_some());
        click(&mut app, window, toggle, MouseButton::Left);
        assert_eq!(app.world().resource::<VegetationPresetEditorState>().active_tab, PresetWorkspaceTab::Document);
        assert!(!app.world().resource::<TerrainObjectsEditorState>().creation_open);
    }
}

#[test]
fn real_numeric_creation_edit_does_not_change_the_selected_mesh() {
    let (mut app, window) = creation_page();
    app.init_resource::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>();
    app.world_mut().resource_mut::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>().active_tab =
        crate::ui::vegetation_preset_editor::PresetWorkspaceTab::Document;
    let toggle = action(&mut app, Action::ToggleCreation);
    click(&mut app, window, toggle, MouseButton::Left);
    let root = field(&mut app, Field::Creation(0));
    click(&mut app, window, root, MouseButton::Middle);
    assert_eq!(
        app.world().resource::<ActiveNumericInput>().entity,
        Some(root)
    );
    key(
        &mut app,
        window,
        KeyCode::Digit1,
        Key::Character("17".into()),
        Some("17"),
    );
    key(&mut app, window, KeyCode::Enter, Key::Enter, None);
    assert_eq!(
        app.world().resource::<TerrainObjectsEditorState>().creation[0],
        17.0
    );
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(
        session
            .draft
            .as_ref()
            .unwrap()
            .manifest
            .terrains
            .as_ref()
            .unwrap()[0]
            .transform
            .translation[0],
        0.0
    );
    assert!(session.undo.is_empty());
    assert_eq!(app.world().resource::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>().active_tab,
        crate::ui::vegetation_preset_editor::PresetWorkspaceTab::Document);
}

#[test]
#[ignore = "独立进程验证真实网格创建，只写隔离作者目录，勿与其他作者测试并发"]
fn mesh_creation_in_document_adds_all_backends_to_the_current_scene() {
    use crate::ui::vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState};

    // 创建会真实生成源与作者画布；全局覆盖仅用于此独立测试进程，绝不写用户场景。
    struct AssetRootReset;
    impl Drop for AssetRootReset {
        fn drop(&mut self) {
            // 失败时保留隔离根，避免仍在途的worker恢复到正式assets后写入。
            if !std::thread::panicking() { feathrise_editor_core::set_runtime_test_asset_root(None); }
        }
    }
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../target/mesh-document-creation-{stamp}"));
    std::fs::create_dir_all(root.join("fixtures")).unwrap();
    std::fs::write(root.join("fixtures/import.glb"), feathrise_terrain::build_flat_terrain_glb(
        feathrise_terrain::TerrainSampleSpec { resolution: [3, 3], world_min: [-1.0; 2], world_max: [1.0; 2],
            base_height: 0.0, height_scale: 16.0 }, 0.0).unwrap()).unwrap();
    feathrise_editor_core::set_runtime_test_asset_root(Some(root.clone()));
    let _reset = AssetRootReset;
    for (create, kind) in [(Action::Clipmap, SceneTerrainKind::SparseClipmap),
        (Action::Voxel, SceneTerrainKind::Voxel)] {
        let (mut app, window) = creation_page();
        app.init_resource::<VegetationPresetEditorState>();
        app.world_mut().resource_mut::<VegetationPresetEditorState>().active_tab = PresetWorkspaceTab::Document;
        app.world_mut().resource_mut::<SceneEditorState>().selected_asset = Some("fixtures/import.glb".into());
        app.world_mut().resource_mut::<TerrainObjectsEditorState>().creation[0] = 17.0;
        let session = app.world().resource::<EditorAuthoringSession>();
        let map = session.draft.as_ref().unwrap().scene_id.clone();
        let original = session.draft.as_ref().unwrap().manifest.terrains.clone().unwrap();
        let ui_revision = app.world().resource::<VegetationPresetEditorState>().ui_revision;
        app.update();
        let toggle = action(&mut app, Action::ToggleCreation);
        click(&mut app, window, toggle, MouseButton::Left);
        let create = action(&mut app, create);
        assert!(app.world().get::<bevy::ui::InteractionDisabled>(create).is_none());
        click(&mut app, window, create, MouseButton::Left);
        let coordinates = action(&mut app, Action::PlacementCoordinates);
        click(&mut app, window, coordinates, MouseButton::Left);
        if kind == SceneTerrainKind::SparseClipmap {
            // 隐藏的旧高度域不得泄漏到Clipmap规格或后台格式，真实生成验证归一快照。
            app.world_mut().resource_mut::<TerrainObjectsEditorState>().creation[4] = f32::NAN;
            app.update();
        }
        let start = action(&mut app, Action::StartCreation);
        assert!(app.world().get::<bevy::ui::InteractionDisabled>(start).is_none());
        click(&mut app, window, start, MouseButton::Left);
        complete(&mut app);
        let session = app.world().resource::<EditorAuthoringSession>();
        let draft = session.draft.as_ref().unwrap();
        let objects = draft.manifest.terrains.as_ref().unwrap();
        assert_eq!(draft.scene_id, map);
        assert_eq!(objects.len(), original.len() + 1, "{:?}创建失败：{}", kind, app.world().resource::<SceneEditorState>().status);
        assert_eq!(&objects[..original.len()], original.as_ref());
        let added = objects.last().unwrap();
        assert_eq!(added.terrain.kind, kind);
        assert_eq!(added.transform.translation[0], 17.0);
        let source = added.voxel.as_ref().map(|voxel| voxel.source_path.as_str())
            .or(added.terrain.height_path.as_deref()).unwrap_or(&added.terrain.glb_path);
        assert!(root.join(source).is_file(), "必须真实生成或导入作者源：{source}");
        assert_eq!(session.undo.len(), 1, "一次添加沿用一条场景历史");
        let workspace = app.world().resource::<VegetationPresetEditorState>();
        assert_eq!(workspace.active_tab, PresetWorkspaceTab::Document);
        assert_eq!(workspace.ui_revision, ui_revision, "创建完成也不改动中央文档工作区");
    }
}

#[test]
fn real_selected_scale_changes_only_pose_and_records_one_complete_undo() {
    let (mut app, window) = page();
    let before = selected(
        app.world().resource::<EditorAuthoringSession>(),
        app.world().resource::<TerrainObjectsEditorState>(),
    )
    .unwrap()
    .clone();
    let root = field(&mut app, Field::Selected(4));
    click(&mut app, window, root, MouseButton::Middle);
    key(
        &mut app,
        window,
        KeyCode::Digit3,
        Key::Character("3.5".into()),
        Some("3.5"),
    );
    key(&mut app, window, KeyCode::Enter, Key::Enter, None);
    complete(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    let after = selected(session, app.world().resource::<TerrainObjectsEditorState>()).unwrap();
    assert_eq!(after.transform.scale, [1.0, 3.5, 1.0]);
    assert_eq!(after.terrain, before.terrain);
    assert_eq!(after.world_min, before.world_min);
    assert_eq!(after.world_max, before.world_max);
    assert_eq!(after.voxel, before.voxel);
    assert_eq!(session.undo.len(), 1);
    let crate::authoring::EditorCommand::TerrainObjectsChanged { before: saved, .. } =
        &session.undo[0]
    else {
        panic!("缩放应为完整集合历史")
    };
    assert_eq!(
        saved.objects.as_ref().unwrap()[0].transform,
        before.transform
    );
}

#[test]
fn real_clipmap_xyz_rotation_preserves_source_and_records_complete_undo_redo() {
    let (mut app, window) = native_page(SceneTerrainKind::SparseClipmap, 3);
    let initial = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().manifest.clone();
    let original = initial.terrains.as_ref().unwrap()[0].clone();
    let mut expected = original.transform;
    for (axis, text, degrees) in [(0, "17.5", 17.5_f32), (1, "65", 65.0), (2, "-12.25", -12.25)] {
        let root = field(&mut app, Field::Rotation(axis));
        assert!(app.world().get::<NumericInputDisabled>(root).is_none());
        click(&mut app, window, root, MouseButton::Middle);
        key(&mut app, window, KeyCode::Digit1, Key::Character(text.into()), Some(text));
        key(&mut app, window, KeyCode::Enter, Key::Enter, None);
        complete(&mut app);
        match axis {
            0 => expected.pitch_degrees = degrees.rem_euclid(360.0),
            1 => expected.yaw_degrees = degrees.rem_euclid(360.0),
            2 => expected.roll_degrees = degrees.rem_euclid(360.0),
            _ => unreachable!(),
        }
        let session = app.world().resource::<EditorAuthoringSession>();
        let objects = session.draft.as_ref().unwrap().manifest.terrains.as_ref().unwrap();
        let mut source = objects[0].clone();
        assert_eq!(source.transform, expected);
        source.transform = original.transform;
        assert_eq!(source, original, "旋转只改变实例姿态，不重写源网格、格距或LOD");
        assert_eq!(objects[1], initial.terrains.as_ref().unwrap()[1]);
        assert_eq!(session.undo.len(), axis + 1, "每次真实数值提交只新增一条完整历史");
    }
    let session = app.world().resource::<EditorAuthoringSession>();
    let final_manifest = &session.draft.as_ref().unwrap().manifest;
    let reopened: SceneManifestAsset = serde_json::from_slice(&serde_json::to_vec(final_manifest).unwrap()).unwrap();
    assert_eq!(reopened.terrains, final_manifest.terrains);
    // 沿正式集合历史反向撤销、顺向重做，验证三轴和所有未编辑字段一起恢复。
    let mut restored = final_manifest.clone();
    for command in session.undo.iter().rev() {
        let crate::authoring::EditorCommand::TerrainObjectsChanged { before, .. } = command else {
            panic!("三轴旋转应保存完整网格集合历史");
        };
        before.apply(&mut restored);
    }
    assert_eq!(restored.terrains, initial.terrains);
    for command in &session.undo {
        let crate::authoring::EditorCommand::TerrainObjectsChanged { after, .. } = command else { unreachable!(); };
        after.apply(&mut restored);
    }
    assert_eq!(restored.terrains, final_manifest.terrains);
}

#[test]
fn late_xyz_rotation_commit_cannot_follow_a_new_selection_or_author_revision() {
    for revision_changed in [true, false] {
        let (mut app, window) = native_page(SceneTerrainKind::SparseClipmap, 3);
        let before = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().manifest.terrains.clone();
        let root = field(&mut app, Field::Rotation(0));
        click(&mut app, window, root, MouseButton::Middle);
        if revision_changed {
            app.world_mut().resource_mut::<EditorAuthoringSession>().revision += 1;
        } else {
            let id = TerrainId::parse("mesh-b").unwrap();
            {
                let mut state = app.world_mut().resource_mut::<TerrainObjectsEditorState>();
                state.selected = Some(id.clone());
                state.selected_index = Some(1);
            }
            app.world_mut().resource_mut::<crate::viewport::LayerCandidateState>().confirm_terrain(&id, 1);
        }
        app.update();
        for value in [20.0, 40.0] { late_commit(&mut app, root, value); }
        complete(&mut app);
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.draft.as_ref().unwrap().manifest.terrains, before);
        assert!(session.undo.is_empty());
    }
}

#[test]
fn late_selected_numeric_commit_cannot_follow_a_new_scene_selection() {
    let (mut app, window) = page();
    let root = field(&mut app, Field::Selected(0));
    app.world_mut().resource_mut::<ActiveNumericInput>().entity = Some(root);
    app.update();
    {
        let mut state = app.world_mut().resource_mut::<TerrainObjectsEditorState>();
        state.selected = Some(TerrainId::parse("mesh-b").unwrap());
        state.selected_index = Some(1);
    }
    app.world_mut().resource_mut::<crate::viewport::LayerCandidateState>()
        .confirm_terrain(&TerrainId::parse("mesh-b").unwrap(), 1);
    app.update();
    app.world_mut().resource_mut::<ActiveNumericInput>().entity = None;
    app.world_mut().write_message(NumericInputCommitted {
        entity: root,
        value: NumericValue::F32(123.0),
        outcome: NumericInputCommitOutcome::Canceled,
        source: NumericInputCommitSource::Text,
        changed: true,
        edited: true,
    });
    app.update();
    for value in [123.0, 124.0, 125.0] {
        late_commit(&mut app, root, value);
    }
    complete(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(
        session
            .draft
            .as_ref()
            .unwrap()
            .manifest
            .terrains
            .as_ref()
            .unwrap()[1]
            .transform
            .translation[0],
        29.0
    );
    assert!(session.undo.is_empty());
    // 墓碑不使控件永久失效；真实中键重新聚焦后，只有新的一笔能编辑B。
    click(&mut app, window, root, MouseButton::Middle);
    late_commit(&mut app, root, 777.0); // 新焦点仍活动时，旧终结消息不能代替新编辑的Enter。
    assert_eq!(app.world().resource::<EditorAuthoringSession>().next_serial, 0);
    key(
        &mut app,
        window,
        KeyCode::Digit4,
        Key::Character("46".into()),
        Some("46"),
    );
    key(&mut app, window, KeyCode::Enter, Key::Enter, None);
    complete(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    let objects = session
        .draft
        .as_ref()
        .unwrap()
        .manifest
        .terrains
        .as_ref()
        .unwrap();
    assert_eq!(objects[0].transform.translation[0], 0.0);
    assert_eq!(objects[1].transform.translation[0], 46.0);
    assert_eq!(session.undo.len(), 1);
}

#[test]
fn native_lod_commit_cannot_follow_a_folded_group_changed_layer_or_author_revision() {
    for change in [0, 1, 2, 3] {
        let (mut app, _) = native_page(SceneTerrainKind::SparseClipmap, 6);
        let root = selected_lod_field(&mut app, true, 0);
        app.world_mut().resource_mut::<ActiveNumericInput>().entity = Some(root);
        app.update();
        match change {
            0 => {
                app.world_mut().resource_mut::<TerrainObjectsEditorState>()
                    .set_section_open(MeshPropertySection::DisplayLods, false)
            }
            1 => {
                app.world_mut()
                    .resource_mut::<TerrainObjectsEditorState>()
                    .lod_mut(true).selected = 4
            }
            2 => {
                app.world_mut()
                    .resource_mut::<EditorAuthoringSession>()
                    .revision += 1
            }
            _ => app.world_mut().resource_mut::<TerrainObjectsEditorState>().lod_mut(true).selected = 1,
        }
        app.update();
        app.world_mut().resource_mut::<ActiveNumericInput>().entity = None;
        late_commit(&mut app, root, 12.0);
        assert_eq!(app.world().resource::<EditorAuthoringSession>().next_serial, 0,
            "迟到值必须在任何后台准备/完整校验前拒绝，不能靠后续源校验失败掩盖串写");
        assert!(!app.world().resource::<TerrainObjectsEditorState>().preparing());
        assert!(
            app.world()
                .resource::<EditorAuthoringSession>()
                .undo
                .is_empty()
        );
    }
}

#[test]
fn clipmap_real_properties_edit_display_and_collision_independently() {
    let (mut app, window) = native_page(SceneTerrainKind::SparseClipmap, 2);
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let object = &mut std::sync::Arc::make_mut(session.draft.as_mut().unwrap().manifest.terrains.as_mut().unwrap())[0];
        object.terrain.collision_lods = default_collision_lods_for_resolution(object.terrain.resolution);
    }
    app.update();
    let before = current_object(&app);
    let distance = selected_lod_field(&mut app, true, 0);
    let multiplier = selected_lod_field(&mut app, true, 1);
    assert_eq!(selected_lod_value(&mut app, true, 0), 16.0);
    assert_eq!(selected_lod_value(&mut app, true, 1), 1.0);
    assert!(app.world().get::<NumericInputDisabled>(distance).is_none());
    assert!(app.world().get::<NumericInputDisabled>(multiplier).is_none());
    for button in [Action::AddLod(true), Action::RemoveLod(true)] {
        let entity = action(&mut app, button);
        assert!(app.world().get::<bevy::ui::InteractionDisabled>(entity).is_none());
    }
    for _ in 0..4 { app.update(); }
    assert_eq!(current_object(&app), before);
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());

    // 两张表同时展开；直接编辑各自常驻输入，不先切换用途或移走另一张表。
    let collision_distance = field(&mut app, Field::Lod(false, 8));
    assert_ne!(collision_distance, distance);
    click(&mut app, window, collision_distance, MouseButton::Middle);
    key(&mut app, window, KeyCode::Digit2, Key::Character("24".into()), Some("24"));
    key(&mut app, window, KeyCode::Enter, Key::Enter, None);
    complete(&mut app);
    let collision_saved = current_object(&app);
    assert_eq!(collision_saved.terrain.collision_lods[0].max_distance_meters, 24.0);
    assert_eq!(collision_saved.terrain.display_lods, before.terrain.display_lods);
    click(&mut app, window, distance, MouseButton::Middle);
    key(&mut app, window, KeyCode::Digit2, Key::Character("20".into()), Some("20"));
    key(&mut app, window, KeyCode::Enter, Key::Enter, None);
    complete(&mut app);
    let after = current_object(&app);
    assert_eq!(after.terrain.display_lods.as_ref().unwrap()[0].outer_half_extent_meters, Some(20.0));
    assert_eq!(after.terrain.collision_lods, collision_saved.terrain.collision_lods);
    assert_eq!(field(&mut app, Field::Lod(false, 8)), collision_distance);
    assert_eq!(field(&mut app, Field::Lod(true, 8)), distance);
    assert!(app.world().resource::<TerrainObjectsEditorState>().section_open(MeshPropertySection::DisplayLods));
    assert!(app.world().resource::<TerrainObjectsEditorState>().section_open(MeshPropertySection::CollisionLods));
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 2);
}

#[test]
fn inline_native_lod_first_click_and_drag_edit_the_hit_cell_for_both_usages() {
    for kind in [SceneTerrainKind::SparseClipmap, SceneTerrainKind::Voxel] {
        for display in [true, false] {
            let (mut app, window) = native_page(kind, 6);
            app.world_mut().resource_mut::<TerrainObjectsEditorState>().display_lods = display;
            app.update();
            let before = current_object(&app);
            assert_eq!(app.world().resource::<TerrainObjectsEditorState>().lod(display).selected, 0);
            let multiplier = field(&mut app, Field::Lod(display, 9 + 2 * 2));
            let row = app.world().get::<LodFieldRow>(multiplier).unwrap().0;
            assert_eq!(app.world().get::<LodRowBinding>(row).unwrap().0.as_ref().unwrap().index, 2);
            // 未先选层，第一次中键就进入命中的倍率框；选层只跟随此次真实接管。
            click(&mut app, window, multiplier, MouseButton::Middle);
            assert_eq!(app.world().resource::<ActiveNumericInput>().entity, Some(multiplier));
            assert_eq!(app.world().resource::<TerrainObjectsEditorState>().lod(display).selected, 2);
            key(&mut app, window, KeyCode::Digit8, Key::Character("8".into()), Some("8"));
            key(&mut app, window, KeyCode::Enter, Key::Enter, None);
            complete(&mut app);
            assert_eq!(jobs::field_value(&current_object(&app), 9 + 2 * 2, display), Some(8.0));
            let distance = field(&mut app, Field::Lod(display, 8 + 3 * 2));
            point_at(&mut app, window, distance);
            let start = app.world().get::<Window>(window).unwrap().cursor_position().unwrap();
            mouse(&mut app, window, MouseButton::Left, ButtonState::Pressed);
            assert_eq!(app.world().resource::<feathrise_editor_ui::ActiveNumericInputScrub>().entity, Some(distance));
            assert_eq!(app.world().resource::<TerrainObjectsEditorState>().lod(display).selected, 3);
            move_cursor(&mut app, window, start + Vec2::new(6.0, 0.0));
            assert_eq!(app.world().get::<NumericInputValue>(distance).unwrap().0.as_f32(), Some(188.0));
            assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1, "拖动途中只更新公共数值，松开才生成作者事务");
            mouse(&mut app, window, MouseButton::Left, ButtonState::Released);
            complete(&mut app);
            let after = current_object(&app);
            assert_eq!(jobs::field_value(&after, 8 + 3 * 2, display), Some(188.0));
            assert_eq!(jobs::field_value(&after, 9 + 2 * 2, display), Some(8.0));
            for index in 0..6 {
                for column in 0..2 {
                    if (index, column) == (2, 1) || (index, column) == (3, 0) { continue; }
                    let field = 8 + index * 2 + column;
                    assert_eq!(jobs::field_value(&after, field, display), jobs::field_value(&before, field, display));
                }
            }
            assert_other_lod_usage_preserved(&before, &after, display);
            assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 2);
        }
    }
}

#[test]
fn native_lod_real_continuous_rows_keep_inline_inputs_stable_and_edit_their_own_layer() {
    for kind in [SceneTerrainKind::SparseClipmap, SceneTerrainKind::Voxel] {
        let (mut app, window) = native_page(kind, 6);
        let rows = {
            let world = app.world_mut();
            let mut rows = world.query::<(Entity, &LodRow)>().iter(world)
                .filter(|(_, row)| row.1).map(|(entity, row)| (row.0, entity)).collect::<Vec<_>>();
            rows.sort_unstable_by_key(|row| row.0);
            assert_eq!(rows.len(), lod_list::ROW_POOL_SIZE, "连续列表复用固定行池，不以行池截断作者层数");
            rows
        };
        let first_distance = selected_lod_field(&mut app, true, 0);
        let first_multiplier = selected_lod_field(&mut app, true, 1);
        let numeric_roots = {
            let world = app.world_mut();
            world.query_filtered::<Entity, With<LodFieldRow>>().iter(world).collect::<Vec<_>>()
        };
        assert_eq!(numeric_roots.len(), lod_list::ROW_POOL_SIZE * 4);
        let middle = action(&mut app, Action::SelectLod(2, true));
        click(&mut app, window, middle, MouseButton::Left);
        assert_eq!(app.world().resource::<TerrainObjectsEditorState>().lod(true).selected, 2);
        assert_eq!(selected_lod_value(&mut app, true, 0), 64.0);
        assert_eq!(selected_lod_value(&mut app, true, 1), 4.0);
        let next = action(&mut app, Action::SelectLod(4, true));
        click(&mut app, window, next, MouseButton::Left);
        assert_eq!(app.world().resource::<TerrainObjectsEditorState>().lod(true).selected, 4);
        assert_eq!(selected_lod_value(&mut app, true, 0), 256.0);
        assert_ne!(selected_lod_field(&mut app, true, 0), first_distance);
        assert_ne!(selected_lod_field(&mut app, true, 1), first_multiplier);
        assert_eq!(app.world().get::<NumericInputValue>(first_distance).unwrap().0.as_f32(), Some(16.0));
        for (local, entity) in rows {
            assert_eq!(app.world().get::<LodRow>(entity).unwrap().0, local);
            assert_eq!(app.world().get::<Node>(entity).unwrap().display != Display::None, local < 6);
        }
        let last = action(&mut app, Action::SelectLod(5, true));
        click(&mut app, window, last, MouseButton::Left);
        assert_eq!(app.world().resource::<TerrainObjectsEditorState>().lod(true).selected, 5);
        let distance = selected_lod_field(&mut app, true, 0);
        let multiplier = selected_lod_field(&mut app, true, 1);
        if kind == SceneTerrainKind::SparseClipmap {
            assert!(app.world().get::<NumericInputDisabled>(distance).is_some());
            let world = app.world_mut();
            let (text, node) = world.query::<(&LodValueFallback, &Text, &Node)>().iter(world)
                .find(|(fallback, ..)| fallback.0 == Field::Lod(true, 8 + 5 * 2))
                .map(|(_, text, node)| (text, node)).unwrap();
            assert_eq!(text.0, "网格边界");
            assert_eq!(node.display, Display::Flex);
        } else {
            assert!(app.world().get::<NumericInputDisabled>(distance).is_none());
            assert_eq!(selected_lod_value(&mut app, true, 0), 512.0);
        }
        assert!(app.world().get::<NumericInputDisabled>(multiplier).is_none());
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
        let world = app.world_mut();
        assert_eq!(world.query_filtered::<Entity, With<LodFieldRow>>().iter(world).collect::<Vec<_>>(), numeric_roots);
        // 每行数值根不重生；真实中键/键盘提交只更新该单元格冻结的真实下标。
        let before = current_object(&app);
        click(&mut app, window, multiplier, MouseButton::Middle);
        key(&mut app, window, KeyCode::Digit6, Key::Character("64".into()), Some("64"));
        key(&mut app, window, KeyCode::Enter, Key::Enter, None);
        complete(&mut app);
        let after = current_object(&app);
        if let (Some(before), Some(after)) = (&before.voxel, &after.voxel) {
            assert_eq!(&after.display_lods[..5], &before.display_lods[..5]);
            assert_eq!(after.display_lods[5].spacing_multiplier, 64);
        } else {
            let before = before.terrain.display_lods.as_ref().unwrap();
            let after = after.terrain.display_lods.as_ref().unwrap();
            assert_eq!(&after[..5], &before[..5]);
            assert_eq!(after[5].spacing_multiplier, 64);
            assert_eq!(after[5].outer_half_extent_meters, None);
        }
        assert_other_lod_usage_preserved(&before, &after, true);
        assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
    }
}

#[test]
fn native_lod_real_held_add_waits_for_release_and_a_second_click_adds_one_selected_layer() {
    for kind in [SceneTerrainKind::SparseClipmap, SceneTerrainKind::Voxel] {
        for display in [true, false] {
            let (mut app, window) = native_page(kind, 6);
            app.world_mut().resource_mut::<TerrainObjectsEditorState>().display_lods = display;
            app.update();
            let before = current_object(&app);
            let add = action(&mut app, Action::AddLod(display));
            point_at(&mut app, window, add);
            mouse(&mut app, window, MouseButton::Left, ButtonState::Pressed);
            assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
            for _ in 0..16 { app.update(); }
            assert_eq!(current_object(&app), before, "按住只提供反馈，不提前添加层");
            assert_eq!(app.world().resource::<EditorAuthoringSession>().next_serial, 0);
            mouse(&mut app, window, MouseButton::Left, ButtonState::Released);
            complete(&mut app);
            for _ in 0..16 { app.update(); }
            let after = current_object(&app);
            assert_eq!(presentation::lod_count(Some(&after), display), 7);
            assert_eq!(app.world().resource::<TerrainObjectsEditorState>().lod(display).selected, 6);
            assert_eq!(app.world().resource::<EditorAuthoringSession>().next_serial, 1);
            assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
            assert!(!app.world().resource::<TerrainObjectsEditorState>().preparing());
            assert_other_lod_usage_preserved(&before, &after, display);
            click(&mut app, window, add, MouseButton::Left);
            complete(&mut app);
            let after_second = current_object(&app);
            assert_eq!(presentation::lod_count(Some(&after_second), display), 8);
            assert_eq!(app.world().resource::<TerrainObjectsEditorState>().lod(display).selected, 7);
            assert_eq!(app.world().resource::<EditorAuthoringSession>().next_serial, 2);
            assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 2);
            assert_other_lod_usage_preserved(&before, &after_second, display);
        }
    }
}

#[test]
fn native_lod_real_held_remove_waits_for_release_and_deletes_selected_middle_once() {
    for kind in [SceneTerrainKind::SparseClipmap, SceneTerrainKind::Voxel] {
        for display in [true, false] {
            let (mut app, window) = native_page(kind, 6);
            app.world_mut().resource_mut::<TerrainObjectsEditorState>().display_lods = display;
            app.update();
            let middle = action(&mut app, Action::SelectLod(2, display));
            click(&mut app, window, middle, MouseButton::Left);
            let before = current_object(&app);
            let remove = action(&mut app, Action::RemoveLod(display));
            point_at(&mut app, window, remove);
            mouse(&mut app, window, MouseButton::Left, ButtonState::Pressed);
            for _ in 0..16 { app.update(); }
            assert_eq!(current_object(&app), before, "删除按钮按住期间不能修改作者列表");
            assert_eq!(app.world().resource::<EditorAuthoringSession>().next_serial, 0);
            mouse(&mut app, window, MouseButton::Left, ButtonState::Released);
            complete(&mut app);
            for _ in 0..16 { app.update(); }
            let after = current_object(&app);
            assert_eq!(presentation::lod_count(Some(&after), display), 5);
            assert_eq!(app.world().resource::<TerrainObjectsEditorState>().lod(display).selected, 2);
            assert_eq!(selected_lod_value(&mut app, display, 0), 128.0,
                "删除中间层后应选择原下一层，不能始终pop末层");
            assert_eq!(app.world().resource::<EditorAuthoringSession>().next_serial, 1);
            assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
            assert!(!app.world().resource::<TerrainObjectsEditorState>().preparing());
            assert_other_lod_usage_preserved(&before, &after, display);
            click(&mut app, window, remove, MouseButton::Left);
            complete(&mut app);
            let after_second = current_object(&app);
            assert_eq!(presentation::lod_count(Some(&after_second), display), 4);
            assert_eq!(selected_lod_value(&mut app, display, 0), 256.0);
            assert_eq!(app.world().resource::<EditorAuthoringSession>().next_serial, 2);
            assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 2);
            assert_other_lod_usage_preserved(&before, &after_second, display);
        }
    }
}

#[test]
fn source_and_stitch_headers_wait_for_inside_release_and_cancel_outside_release() {
    for (toggle, section) in [(Action::SourceProperties, MeshPropertySection::Source),
        (Action::StitchProperties, MeshPropertySection::Stitch)] {
        let (mut app, window) = native_page(SceneTerrainKind::SparseClipmap, 3);
        app.world_mut().resource_mut::<TerrainObjectsEditorState>().set_section_open(section, true);
        for _ in 0..3 { app.update(); }
        let header = action(&mut app, toggle);
        point_at(&mut app, window, header);
        mouse(&mut app, window, MouseButton::Left, ButtonState::Pressed);
        for _ in 0..5 { app.update(); }
        assert!(app.world().resource::<TerrainObjectsEditorState>().section_open(section),
            "拼接和来源标题按住期间应保持展开");
        mouse(&mut app, window, MouseButton::Left, ButtonState::Released);
        for _ in 0..5 { app.update(); }
        assert!(!app.world().resource::<TerrainObjectsEditorState>().section_open(section),
            "原标题内释放只折叠一次");

        point_at(&mut app, window, header);
        mouse(&mut app, window, MouseButton::Left, ButtonState::Pressed);
        move_cursor(&mut app, window, Vec2::new(1000.0, 850.0)); // 窗口内空白，离开按钮但保持原生指针有效。
        mouse(&mut app, window, MouseButton::Left, ButtonState::Released);
        assert!(!app.world().resource::<TerrainObjectsEditorState>().section_open(section),
            "移出原标题后松开应取消这次折叠操作");

        point_at(&mut app, window, header);
        mouse(&mut app, window, MouseButton::Left, ButtonState::Pressed);
        move_cursor(&mut app, window, Vec2::new(1000.0, 850.0));
        point_at(&mut app, window, header);
        mouse(&mut app, window, MouseButton::Left, ButtonState::Released);
        assert!(app.world().resource::<TerrainObjectsEditorState>().section_open(section),
            "仍持有原按钮的手势移回后可以合法确认");
        assert_eq!(app.world().resource::<EditorAuthoringSession>().next_serial, 0);
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    }
}

#[test]
fn cancelled_lod_navigation_releases_the_original_numeric_focus_commit() {
    let (mut app, window) = native_page(SceneTerrainKind::SparseClipmap, 3);
    let root = field(&mut app, Field::Lod(true, 8));
    click(&mut app, window, root, MouseButton::Middle);
    key(&mut app, window, KeyCode::Digit1, Key::Character("18".into()), Some("18"));
    let fold = action(&mut app, Action::DisplayLods);
    point_at(&mut app, window, fold);
    mouse(&mut app, window, MouseButton::Left, ButtonState::Pressed);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().next_serial, 0,
        "标题尚未合法松开，不能先提交旧数值并抢占导航事务");
    move_cursor(&mut app, window, Vec2::new(1000.0, 850.0));
    mouse(&mut app, window, MouseButton::Left, ButtonState::Released);
    complete(&mut app);
    assert!(app.world().resource::<TerrainObjectsEditorState>().section_open(MeshPropertySection::DisplayLods));
    assert_eq!(current_object(&app).terrain.display_lods.as_ref().unwrap()[0].outer_half_extent_meters, Some(18.0),
        "折叠手势取消后仍按原数值身份提交失焦编辑，不能吞掉合法修改");
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
}

#[test]
fn held_lod_mutation_is_cancelled_when_author_revision_or_selected_layer_changes() {
    for change_revision in [true, false] {
        let (mut app, window) = native_page(SceneTerrainKind::SparseClipmap, 6);
        let before = current_object(&app);
        let add = action(&mut app, Action::AddLod(true));
        point_at(&mut app, window, add);
        mouse(&mut app, window, MouseButton::Left, ButtonState::Pressed);
        assert_eq!(app.world().resource::<EditorAuthoringSession>().next_serial, 0);
        if change_revision {
            app.world_mut().resource_mut::<EditorAuthoringSession>().revision += 1;
        } else {
            app.world_mut().resource_mut::<TerrainObjectsEditorState>().lod_mut(true).selected = 2;
        }
        app.update(); // 回显换代时显式撤销旧按钮捕获，不能把释放改派给新作者身份。
        point_at(&mut app, window, add);
        mouse(&mut app, window, MouseButton::Left, ButtonState::Released);
        for _ in 0..5 { app.update(); }
        assert_eq!(current_object(&app), before);
        assert_eq!(app.world().resource::<EditorAuthoringSession>().next_serial, 0);
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    }
}

#[test]
fn native_lod_real_same_frame_fast_click_adds_or_removes_exactly_one_layer_and_history_entry() {
    for kind in [SceneTerrainKind::SparseClipmap, SceneTerrainKind::Voxel] {
        for change in [Action::AddLod(true), Action::RemoveLod(true)] {
            let (mut app, window) = native_page(kind, 6);
            let middle = action(&mut app, Action::SelectLod(2, true));
            click(&mut app, window, middle, MouseButton::Left);
            let before = current_object(&app);
            let button = action(&mut app, change);
            point_at(&mut app, window, button);
            // 一个真实事件批次包含按下和松开，按键最终已释放仍代表完整的一次点击。
            for state in [ButtonState::Pressed, ButtonState::Released] {
                let message = MouseButtonInput { button: MouseButton::Left, state, window };
                app.world_mut().write_message(message);
                app.world_mut().write_message(bevy::window::WindowEvent::MouseButtonInput(message));
            }
            app.update();
            let mouse = app.world().resource::<ButtonInput<MouseButton>>();
            assert!(mouse.just_pressed(MouseButton::Left) && mouse.just_released(MouseButton::Left));
            assert!(!mouse.pressed(MouseButton::Left));
            assert_eq!(app.world().resource::<EditorAuthoringSession>().next_serial, 1,
                "同帧完整点击须被消费一次，不能因最终pressed=false漏掉");
            complete(&mut app);
            for _ in 0..8 { app.update(); }
            let after = current_object(&app);
            let adding = change == Action::AddLod(true);
            assert_eq!(presentation::lod_count(Some(&after), true), if adding { 7 } else { 5 });
            assert_eq!(app.world().resource::<TerrainObjectsEditorState>().lod(true).selected, if adding { 6 } else { 2 });
            assert_eq!(app.world().resource::<EditorAuthoringSession>().next_serial, 1);
            assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
            assert!(!app.world().resource::<TerrainObjectsEditorState>().preparing());
            assert_other_lod_usage_preserved(&before, &after, true);
        }
    }
}

#[test]
fn native_lod_one_remaining_layer_and_locked_mesh_disable_mutations_before_background_start() {
    for kind in [SceneTerrainKind::SparseClipmap, SceneTerrainKind::Voxel] {
        let (mut app, window) = native_page(kind, 1);
        let remove = action(&mut app, Action::RemoveLod(true));
        assert!(app.world().get::<bevy::ui::InteractionDisabled>(remove).is_some());
        let world = app.world_mut();
        let label = world.query::<(&ActionLabel, &Text)>().iter(world)
            .find(|(label, _)| label.0 == Action::RemoveLod(true)).unwrap().1;
        assert_eq!(label.0, "至少保留一层");
        click(&mut app, window, remove, MouseButton::Left);
        assert_eq!(app.world().resource::<EditorAuthoringSession>().next_serial, 0);
        let remove = action(&mut app, Action::RemoveLod(false));
        assert!(app.world().get::<bevy::ui::InteractionDisabled>(remove).is_some());
        click(&mut app, window, remove, MouseButton::Left);
        assert_eq!(app.world().resource::<EditorAuthoringSession>().next_serial, 0);
        {
            let mut state = app.world_mut().resource_mut::<TerrainObjectsEditorState>();
            let id = state.selected.clone().unwrap();
            state.locked.insert(id);
        }
        app.update();
        let add = action(&mut app, Action::AddLod(false));
        assert!(app.world().get::<bevy::ui::InteractionDisabled>(add).is_some());
        click(&mut app, window, add, MouseButton::Left);
        assert_eq!(app.world().resource::<EditorAuthoringSession>().next_serial, 0);
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    }
}

#[test]
fn native_lod_real_focus_cannot_commit_into_a_new_layer_or_folded_group() {
    for change in [Action::SelectLod(1, true), Action::DisplayLods, Action::SelectLod(4, true)] {
        let (mut app, window) = native_page(SceneTerrainKind::SparseClipmap, 6);
        let before = current_object(&app);
        let root = selected_lod_field(&mut app, true, 0);
        click(&mut app, window, root, MouseButton::Middle);
        assert_eq!(app.world().resource::<ActiveNumericInput>().entity, Some(root));
        key(&mut app, window, KeyCode::Digit1, Key::Character("18".into()), Some("18"));
        let button = action(&mut app, change);
        let change_name = match change {
            Action::SelectLod(_, _) => "换层", Action::DisplayLods => "折叠原组",
            _ => unreachable!(),
        };
        click(&mut app, window, button, MouseButton::Left);
        let state = app.world().resource::<TerrainObjectsEditorState>();
        assert_eq!(app.world().resource::<ActiveNumericInput>().entity, None,
            "LOD{change_name}后旧数值焦点应结束，释放帧不得借移动后的新数值框重新聚焦");
        match change {
            Action::SelectLod(index, _) => assert_eq!(state.lod(true).selected, index),
            Action::DisplayLods => assert!(!state.section_open(MeshPropertySection::DisplayLods)),
            _ => unreachable!(),
        }
        // 公共点击终结、取消墓碑和多条旧Accepted都必须拒收；不能先启动worker再靠坏候选挡住。
        for value in [18.0, 19.0, 20.0] { late_commit(&mut app, root, value); }
        assert_eq!(app.world().resource::<EditorAuthoringSession>().next_serial, 0);
        assert!(!app.world().resource::<TerrainObjectsEditorState>().preparing());
        assert_eq!(current_object(&app), before);
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    }
}

#[test]
fn scene_row_opens_mesh_attributes_without_entering_edit_mode_and_respects_mode_gate() {
    let (mut app, window) = page();
    app.world_mut().resource_mut::<SceneEditorSidebarState>().left = EditorLeftSidebarTab::Scene;
    {
        let mut state = app.world_mut().resource_mut::<TerrainObjectsEditorState>();
        state.lod_mut(true).start = 8;
        state.lod_mut(true).selected = 9;
    }
    let row = {
        let world = app.world_mut();
        world
            .query::<(Entity, &Row)>()
            .iter(world)
            .find(|(_, row)| row.0 == 1)
            .unwrap()
            .0
    };
    click(&mut app, window, row, MouseButton::Left);
    assert_eq!(
        app.world().resource::<SceneEditorSidebarState>().left,
        EditorLeftSidebarTab::Scene
    );
    assert_eq!(
        app.world()
            .resource::<TerrainObjectsEditorState>()
            .selected_index,
        Some(1)
    );
    assert_eq!(app.world().resource::<TerrainObjectsEditorState>().lod(true).start, 0);
    assert_eq!(app.world().resource::<TerrainObjectsEditorState>().lod(true).selected, 0);
    assert_eq!(
        app.world().resource::<ViewportModeState>().mode,
        ViewportMode::Select
    );
    app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Edit;
    let first = {
        let world = app.world_mut();
        world
            .query::<(Entity, &Row)>()
            .iter(world)
            .find(|(_, row)| row.0 == 0)
            .unwrap()
            .0
    };
    click(&mut app, window, first, MouseButton::Left);
    assert_eq!(
        app.world()
            .resource::<TerrainObjectsEditorState>()
            .selected_index,
        Some(1)
    );
}

#[test]
fn continuous_mesh_list_scrolls_to_later_objects_and_selects_the_displayed_row() {
    let (mut app, window) = page();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let template = draft.manifest.terrains.as_ref().unwrap()[0].clone();
        draft.manifest.terrains = Some((0..40).map(|index| {
            let mut object = template.clone();
            object.id = feathrise_scene_content::TerrainId::parse(format!("scroll-{index}")).unwrap();
            object.display_name = format!("连续网格{index}");
            object
        }).collect::<Vec<_>>().into());
        session.revision += 1;
    }
    app.update();
    let viewport = {
        let world = app.world_mut();
        assert_eq!(world.query::<&Row>().iter(world).count(), crate::ui::list_layout::SCENE_OBJECT_LIST_ROWS);
        world.query_filtered::<Entity, With<ListViewport>>().single(world).unwrap()
    };
    app.world_mut().get_mut::<ScrollPosition>(viewport).unwrap().y = 27.0 * 24.0;
    app.update();
    assert_eq!(app.world().resource::<TerrainObjectsEditorState>().list_start, 23);
    let row = {
        let world = app.world_mut();
        assert!(!world.query::<&Text>().iter(world).any(|text| text.0 == "上一页" || text.0 == "下一页"));
        assert!(world.query::<(&RowText, &Text)>().iter(world).any(|(row, text)| row.0 == 2 && text.0.contains("连续网格25")));
        world.query::<(Entity, &Row)>().iter(world).find(|(_, row)| row.0 == 2).unwrap().0
    };
    click(&mut app, window, row, MouseButton::Left);
    assert_eq!(app.world().resource::<TerrainObjectsEditorState>().selected_index, Some(25));
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
}

fn row_action(app: &mut App, slot: usize, action: Action) -> Entity {
    let world = app.world_mut();
    world.query::<(Entity, &RowAction, &ActionButton)>().iter(world)
        .find(|(_, row, button)| row.slot == slot && button.0 == action).unwrap().0
}

#[test]
fn row_icons_target_their_own_mesh_and_keep_locked_mutations_out() {
    let (mut app, window) = page();
    app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Edit;
    app.world_mut().resource_mut::<TerrainObjectsEditorState>().set_section_open(MeshPropertySection::Type, true);
    app.update(); // 行内开关独立于下方属性页，点击另一行先选中真实目标。
    let visible = row_action(&mut app, 1, Action::Visible);
    click(&mut app, window, visible, MouseButton::Left);
    let second = TerrainId::parse("mesh-b").unwrap();
    let state = app.world().resource::<TerrainObjectsEditorState>();
    assert_eq!(state.selected.as_ref(), Some(&second));
    assert!(state.hidden.contains(&second));
    assert_eq!(state.hidden.len(), 1);
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    let lock = row_action(&mut app, 1, Action::Lock);
    click(&mut app, window, lock, MouseButton::Left);
    assert!(app.world().resource::<TerrainObjectsEditorState>().locked.contains(&second));
    let enabled = row_action(&mut app, 1, Action::Enabled);
    click(&mut app, window, enabled, MouseButton::Left);
    assert!(!app.world().resource::<TerrainObjectsEditorState>().preparing());
    assert!(current_object(&app).enabled);
    click(&mut app, window, lock, MouseButton::Left);
    click(&mut app, window, enabled, MouseButton::Left);
    complete(&mut app);
    assert!(!current_object(&app).enabled);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert!(session.draft.as_ref().unwrap().manifest.terrains.as_ref().unwrap()[0].enabled);
    assert_eq!(session.undo.len(), 1);
    // 离开行后仍使用与场景目录完全相同的整行选中底色和边框。
    app.world_mut().get_mut::<Window>(window).unwrap().set_cursor_position(Some(Vec2::new(900.0, 800.0)));
    app.update();
    let world = app.world_mut();
    let (_, background, border) = world.query::<(&Row, &BackgroundColor, &BorderColor)>().iter(world)
        .find(|(row, ..)| row.0 == 1).unwrap();
    let (fill, outline) = crate::ui::scene_sidebar::tree_row_colors(true, false, UiInteraction::None);
    assert_eq!(background.0, fill);
    assert_eq!(*border, outline);
}

#[test]
fn stale_row_icon_press_cannot_follow_author_revision_or_scene() {
    let (mut app, window) = page();
    let visible = row_action(&mut app, 1, Action::Visible);
    point_at(&mut app, window, visible);
    app.world_mut().resource_mut::<EditorAuthoringSession>().revision += 1;
    mouse(&mut app, window, MouseButton::Left, ButtonState::Pressed);
    assert!(app.world().resource::<TerrainObjectsEditorState>().hidden.is_empty());
    mouse(&mut app, window, MouseButton::Left, ButtonState::Released);
    point_at(&mut app, window, visible);
    app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap().scene_id = SceneId::parse("MAP-new").unwrap();
    mouse(&mut app, window, MouseButton::Left, ButtonState::Pressed);
    assert!(app.world().resource::<TerrainObjectsEditorState>().hidden.is_empty());
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
}

#[test]
fn mesh_delete_lives_in_source_properties_and_requires_the_visible_inspector() {
    let (mut app, window) = page();
    app.world_mut().resource_mut::<TerrainObjectsEditorState>().set_section_open(MeshPropertySection::Source, true);
    app.update();
    let remove = {
        let world = app.world_mut();
        let removes = world.query::<(Entity, &ActionButton)>().iter(world)
            .filter(|(_, action)| action.0 == Action::Remove).map(|(entity, _)| entity).collect::<Vec<_>>();
        assert_eq!(removes.len(), 1, "整网格删除只在右侧列表提供一个入口，左侧层列表仍使用RemoveLod");
        let remove = removes[0];
        let mut current = remove;
        let mut scene_list = false;
        let mut right_host = false;
        while let Some(parent) = world.get::<ChildOf>(current) {
            current = parent.parent();
            assert!(world.get::<MeshPageRoot>(current).is_none());
            assert!(world.get::<PageHost>(current).is_none());
            scene_list |= world.get::<MeshSceneListRoot>(current).is_some();
            right_host |= world.get::<ListHost>(current).is_some();
        }
        assert!(!scene_list && right_host, "删除入口属于所选对象的高级属性，场景树保留正式右键删除");
        remove
    };
    {
        let mut state = app.world_mut().resource_mut::<TerrainObjectsEditorState>();
        state.selected = None;
        state.selected_index = None;
    }
    app.update();
    assert!(app.world().get::<bevy::ui::InteractionDisabled>(remove).is_some());
    // 无选择时整组对象属性退出布局，不能让真实点击夹具命中隐藏按钮。
    assert_eq!(app.world().resource::<EditorAuthoringSession>().next_serial, 0);
    let second = {
        let world = app.world_mut();
        world.query::<(Entity, &Row)>().iter(world).find(|(_, row)| row.0 == 1).unwrap().0
    };
    click(&mut app, window, second, MouseButton::Left);
    assert_eq!(app.world().resource::<TerrainObjectsEditorState>().selected_index, Some(1));
    let before = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap()
        .manifest.terrains.as_ref().unwrap().clone();
    let selected_id = before[1].id.clone();
    app.world_mut().resource_mut::<TerrainObjectsEditorState>().locked.insert(selected_id.clone());
    app.update();
    assert!(app.world().get::<bevy::ui::InteractionDisabled>(remove).is_some());
    click(&mut app, window, remove, MouseButton::Left);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().next_serial, 0);
    {
        let mut state = app.world_mut().resource_mut::<TerrainObjectsEditorState>();
        state.locked.remove(&selected_id);
        state.set_section_open(MeshPropertySection::DisplayLods, true);
    }
    {
        let mut sidebars = app.world_mut().resource_mut::<SceneEditorSidebarState>();
        sidebars.left = EditorLeftSidebarTab::Scene;
        sidebars.right = EditorRightSidebarTab::Assets;
        sidebars.navigator = crate::ui::sidebars::EditorNavigatorTab::Scenes;
    }
    app.update();
    assert!(app.world().get::<bevy::ui::InteractionDisabled>(remove).is_some());
    click(&mut app, window, remove, MouseButton::Left);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().next_serial, 0);
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    // 返回属性页后可操作已选对象，与左侧停留目录或场景树无关。
    app.world_mut().resource_mut::<SceneEditorSidebarState>().right = EditorRightSidebarTab::Scene;
    app.update();
    assert!(app.world().get::<bevy::ui::InteractionDisabled>(remove).is_none());
    click(&mut app, window, remove, MouseButton::Left);
    complete(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    let manifest = &session.draft.as_ref().unwrap().manifest;
    assert_eq!(manifest.terrains.as_ref().unwrap().as_ref(), &before[..1]);
    assert_eq!(session.next_serial, 1);
    assert_eq!(session.undo.len(), 1);
    assert!(app.world().resource::<TerrainObjectsEditorState>().selected.is_none());
    let crate::authoring::EditorCommand::TerrainObjectsChanged { before: history_before, after: history_after } = &session.undo[0] else {
        panic!("右侧删除须沿用完整网格集合历史");
    };
    let mut restored = manifest.clone();
    history_before.apply(&mut restored);
    assert_eq!(restored.terrains.as_ref().unwrap().as_ref(), before.as_ref());
    history_after.apply(&mut restored);
    assert_eq!(restored.terrains.as_ref().unwrap().as_ref(), &before[..1]);
}

#[test]
fn per_mesh_policy_only_exposes_custom_fields_and_cycles_one_undoable_policy() {
    let (mut app, window) = page();
    let display = action(&mut app, Action::DisplayPolicy);
    let root = field(&mut app, Field::Policy(0));
    assert!(app.world().get::<NumericInputDisabled>(root).is_some());
    click(&mut app, window, display, MouseButton::Left);
    complete(&mut app);
    assert_eq!(
        selected(
            app.world().resource::<EditorAuthoringSession>(),
            app.world().resource::<TerrainObjectsEditorState>()
        )
        .unwrap()
        .mesh_lod
        .unwrap()
        .display,
        MeshLodMode::Disabled
    );
    assert!(app.world().get::<NumericInputDisabled>(root).is_some());
    click(&mut app, window, display, MouseButton::Left);
    complete(&mut app);
    assert!(matches!(
        selected(
            app.world().resource::<EditorAuthoringSession>(),
            app.world().resource::<TerrainObjectsEditorState>()
        )
        .unwrap()
        .mesh_lod
        .unwrap()
        .display,
        MeshLodMode::Override(_)
    ));
    assert!(app.world().get::<NumericInputDisabled>(root).is_none());
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().undo.len(),
        2
    );
}

#[test]
fn chinese_mesh_pages_stay_inside_narrow_and_wide_hosts_at_scaled_dpi() {
    // 保留原54组宽度/缩放/用途验收，并对两种原生LOD表示重复同一组真实中文字体布局。
    let mut combinations = 0;
    for kind in [SceneTerrainKind::Gltf, SceneTerrainKind::SparseClipmap, SceneTerrainKind::Voxel] {
    let (mut app, _) = page();
    if kind != SceneTerrainKind::Gltf {
        configure_native(&mut app, kind, 6);
        app.update();
    }
    let host = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<PageHost>>()
            .single(world)
            .unwrap()
    };
    for dpi in [1.0, 2.0] {
        {
            let world = app.world_mut();
            for mut camera in world.query::<&mut Camera>().iter_mut(world) {
                camera.computed.target_info.as_mut().unwrap().scale_factor = dpi;
            }
            for mut window in world.query::<&mut Window>().iter_mut(world) {
                window.resolution.set_scale_factor_override(Some(dpi));
            }
        }
        for width in [200.0, 280.0, 420.0] {
            for scale in [0.85, 1.0, 1.25] {
                app.world_mut().resource_mut::<UiScale>().0 = scale;
                app.world_mut().get_mut::<Node>(host).unwrap().width = px(width);
                for page in [
                    MeshPropertySection::Basic,
                    MeshPropertySection::Type,
                    MeshPropertySection::DisplayLods,
                ] {
                    app.world_mut().resource_mut::<TerrainObjectsEditorState>().folded_sections = [true; 6];
                    app.world_mut()
                        .resource_mut::<TerrainObjectsEditorState>()
                        .set_section_open(page, true);
                    for _ in 0..4 {
                        app.update();
                    }
                    combinations += 1;
                    let world = app.world_mut();
                    if page == MeshPropertySection::Basic { assert_axis_column_alignment(world); }
                    let size = world.get::<ComputedNode>(host).unwrap().size();
                    let center = world
                        .get::<UiGlobalTransform>(host)
                        .unwrap()
                        .affine()
                        .translation;
                    for (entity, node, transform, children) in world.query_filtered::<(Entity, &ComputedNode, &UiGlobalTransform, Option<&Children>), With<InteractionButton>>().iter(world) {
                    let mut ancestor = entity;
                    while let Some(parent) = world.get::<ChildOf>(ancestor) { ancestor = parent.parent(); if ancestor == host { break; } }
                    if ancestor != host || node.size().min_element() <= 0.0 { continue; }
                    let x = transform.affine().translation.x;
                    assert!(x - node.size().x * 0.5 >= center.x - size.x * 0.5 - 1.0,
                        "中文网格按钮越过左边界：kind={kind:?} width={width} scale={scale} dpi={dpi} size={:?}", node.size());
                    assert!(x + node.size().x * 0.5 <= center.x + size.x * 0.5 + 1.0,
                        "中文网格按钮越过右边界：kind={kind:?} width={width} scale={scale} dpi={dpi} size={:?}", node.size());
                    if let Some(children) = children {
                        for child in children.iter() {
                            if let Some(text) = world.get::<bevy::text::TextLayoutInfo>(child) {
                                assert!(text.size.x <= node.size().x + 1.1,
                                    "LOD按钮真实中文排版越出按钮：kind={kind:?} width={width} scale={scale} dpi={dpi} text={:?} button={:?}", text.size, node.size());
                            }
                        }
                    }
                }
                }
            }
        }
    }
    }
    assert_eq!(combinations, 162);
}

/// 以实际边框盒比较三行，三轴旋转与位置、缩放使用相同列宽。
fn assert_axis_column_alignment(world: &mut World) {
    let mut columns = Vec::new();
    for expected in [Field::Selected(0), Field::Rotation(0), Field::Selected(3)] {
        let root = world.query::<(Entity, &Field)>().iter(world)
            .find_map(|(entity, field)| (*field == expected).then_some(entity)).unwrap();
        let slot = world.get::<ChildOf>(root).unwrap().parent();
        let row = world.get::<ChildOf>(slot).unwrap().parent();
        let bounds = world.get::<Children>(row).unwrap().iter().skip(1).map(|entity| {
            let size = world.get::<ComputedNode>(entity).unwrap().size();
            let at = world.get::<UiGlobalTransform>(entity).unwrap().translation;
            (at.x - size.x / 2.0, size)
        }).collect::<Vec<_>>();
        assert_eq!(bounds.len(), 3);
        columns.push(bounds);
    }
    for row in &columns[1..] {
        for (reference, actual) in columns[0].iter().zip(row) {
            assert!((reference.0 - actual.0).abs() <= 1.1 && (reference.1 - actual.1).abs().max_element() <= 1.1,
                "变换三行的轴列边缘与框尺寸应一致：{columns:?}");
        }
    }
}


#[test]
#[ignore = "显式离屏渲染场景网格侧栏，保留用户正在编辑的原生窗口"]
fn mesh_sidebar_gpu_snapshot() {
    use bevy::ecs::system::RunSystemOnce;
    use bevy::render::{gpu_readback::{Readback, ReadbackComplete}, render_resource::{TextureFormat, TextureUsages}};
    use std::time::{Duration, Instant};
    let (mut app, window) = page_with_render(true);
    crate::ui::theme::install(&mut app);
    app.add_systems(Update, crate::ui::gltf_browser::sync_scene_base_gltf_row);
    let map = app.world().resource::<SceneEditorState>().selected_scene_id.clone();
    {
        let mut shading = app.world_mut().resource_mut::<crate::resources::TerrainShadingSession>();
        shading.scene_id = map;
        shading.preview_expanded = true;
    }
    let hidden = TerrainId::parse("mesh-b").unwrap();
    {
        let mut state = app.world_mut().resource_mut::<TerrainObjectsEditorState>();
        state.hidden.insert(hidden.clone()); state.locked.insert(hidden);
    }
    let (camera, target) = {
        let world = app.world_mut();
        let (camera, target) = world.query::<(Entity, &bevy::camera::RenderTarget)>().single(world).unwrap();
        let bevy::camera::RenderTarget::Image(target) = target else { panic!("必须使用独立离屏图像") };
        (camera, target.handle.clone())
    };
    let mut image = Image::new_target_texture(768, 900, TextureFormat::Rgba8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    app.world_mut().resource_mut::<Assets<Image>>().insert(target.id(), image).unwrap();
    app.world_mut().get_mut::<Camera>(camera).unwrap().viewport = None;
    {
        let world = app.world_mut();
        for mut node in world.query_filtered::<&mut Node, With<PageHost>>().iter_mut(world) { node.display = Display::None; }
        for entity in world.query_filtered::<Entity, With<ListHost>>().iter(world).collect::<Vec<_>>() {
            world.entity_mut(entity).insert(BackgroundColor(crate::ui::theme::PANEL_BG));
        }
    }
    app.world_mut().resource_mut::<UiScale>().0 = 1.5;
    for width in [260.0, 420.0] {
        {
            let world = app.world_mut();
            for mut node in world.query_filtered::<&mut Node, With<ListHost>>().iter_mut(world) { node.width = px(width); }
        }
        for _ in 0..80 { app.update(); std::thread::sleep(Duration::from_millis(16)); }
        // 实际布局验证名称输入和确认同高、行尾图标完整且没有把输入框挤出边界。
        let rename = action(&mut app, Action::Rename);
        let world = app.world_mut();
        let input = world.query_filtered::<Entity, With<NameInput>>().single(world).unwrap();
        let input_size = world.get::<ComputedNode>(input).unwrap().size();
        let input_center = world.get::<UiGlobalTransform>(input).unwrap().translation;
        let button_center = world.get::<UiGlobalTransform>(rename).unwrap().translation;
        assert!(input_size.x > 100.0);
        assert!((input_center.y - button_center.y).abs() < 1.0);
        assert!(input_center.x + input_size.x / 2.0 < button_center.x);
        let icon_sizes: Vec<_> = world.query::<(&RowIcon, &ImageNode, &ComputedNode)>().iter(world)
            .filter(|(icon, ..)| icon.slot < 2).map(|(icon, image, node)| {
                assert!(world.resource::<Assets<Image>>().get(&image.image).is_some(), "行尾图标必须实际载入");
                (icon.slot, node.size())
            }).collect();
        let (sender, receiver) = std::sync::mpsc::channel();
        world.spawn(Readback::texture(target.clone())).observe(move |event: On<ReadbackComplete>, mut commands: Commands| {
            let _ = sender.send(event.data.clone()); commands.entity(event.entity).try_despawn();
        });
        let started = Instant::now();
        let bytes = loop {
            app.update();
            if let Ok(bytes) = receiver.try_recv() { break bytes; }
            assert!(started.elapsed().as_secs() < 10);
            std::thread::sleep(Duration::from_millis(16));
        };
        let output = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/mesh-sidebar-validation");
        std::fs::create_dir_all(&output).unwrap();
        image::RgbaImage::from_raw(768, 900, bytes).unwrap().save(output.join(format!("mesh-sidebar-{width}.png"))).unwrap();
        assert!(icon_sizes.iter().all(|(_, size)| size.min_element() >= 14.0), "行尾图标被挤压：{icon_sizes:?}");
    }
    // 同一正式渲染夹具复核三列表格与唯一数值对；不载入场景、不改用户设置或作者文件。
    configure_native(&mut app, SceneTerrainKind::SparseClipmap, 6);
    app.world_mut().resource_mut::<TerrainObjectsEditorState>().set_section_open(MeshPropertySection::DisplayLods, true);
    for _ in 0..12 { app.update(); std::thread::sleep(Duration::from_millis(16)); }
    let (sender, receiver) = std::sync::mpsc::channel();
    app.world_mut().spawn(Readback::texture(target.clone())).observe(move |event: On<ReadbackComplete>, mut commands: Commands| {
        let _ = sender.send(event.data.clone()); commands.entity(event.entity).try_despawn();
    });
    let started = Instant::now();
    let bytes = loop {
        app.update();
        if let Ok(bytes) = receiver.try_recv() { break bytes; }
        assert!(started.elapsed().as_secs() < 10);
        std::thread::sleep(Duration::from_millis(16));
    };
    image::RgbaImage::from_raw(768, 900, bytes).unwrap()
        .save(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/mesh-sidebar-validation/mesh-lods-compact.png")).unwrap();

    // 同一正式UI离屏验证两种箭头；文档工作区中的真实点击保持中央标签不变。
    app.init_resource::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>();
    // 创建工具页独立于中央文档标签，必须显式揭示后再验证真实箭头点击。
    app.world_mut().resource_mut::<SceneEditorSidebarState>().right = EditorRightSidebarTab::Tools;
    {
        let world = app.world_mut();
        for mut node in world.query_filtered::<&mut Node, With<ListHost>>().iter_mut(world) { node.display = Display::None; }
        for mut node in world.query_filtered::<&mut Node, With<PageHost>>().iter_mut(world) { node.display = Display::Flex; node.width = px(280); }
    }
    for expanded in [false, true] {
        if expanded {
            app.world_mut().resource_mut::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>().active_tab =
                crate::ui::vegetation_preset_editor::PresetWorkspaceTab::Document;
            app.update();
            let toggle = action(&mut app, Action::ToggleCreation);
            // Bevy官方命中只接受窗口目标；只在执行命中系统时临时映射，渲染仍使用原离屏图像。
            let position = app.world().get::<UiGlobalTransform>(toggle).unwrap().translation;
            app.world_mut().get_mut::<Window>(window).unwrap()
                .set_physical_cursor_position(Some(position.as_dvec2()));
            let original_target = app.world().get::<bevy::camera::RenderTarget>(camera).unwrap().clone();
            *app.world_mut().get_mut::<bevy::camera::RenderTarget>(camera).unwrap() =
                bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(window));
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
            app.world_mut().run_system_cached(bevy::ui::ui_focus_system).unwrap();
            app.world_mut().run_system_once(feathrise_editor_ui::update_editor_button_clicks).unwrap();
            assert_eq!(*app.world().get::<UiInteraction>(toggle).unwrap(), UiInteraction::Pressed);
            assert!(!app.world().get::<feathrise_editor_ui::EditorButtonClick>(toggle).unwrap().is_clicked());
            app.world_mut().run_system_once(systems::input).unwrap();
            assert!(!app.world().resource::<TerrainObjectsEditorState>().creation_open,
                "真实按住只显示反馈，创建区必须等原按钮内释放后展开");
            // 分开真实按下和释放边沿；两次命中共用正式焦点状态，不伪造已确认点击。
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
            app.world_mut().run_system_cached(bevy::ui::ui_focus_system).unwrap();
            app.world_mut().run_system_once(feathrise_editor_ui::update_editor_button_clicks).unwrap();
            assert!(app.world().get::<feathrise_editor_ui::EditorButtonClick>(toggle).unwrap().is_clicked(),
                "在原箭头内释放必须由共享点击系统确认");
            app.world_mut().run_system_once(systems::input).unwrap();
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
            *app.world_mut().get_mut::<bevy::camera::RenderTarget>(camera).unwrap() = original_target;
            assert_eq!(app.world().resource::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>().active_tab,
                crate::ui::vegetation_preset_editor::PresetWorkspaceTab::Document);
            assert!(app.world().resource::<TerrainObjectsEditorState>().creation_open);
        }
        for _ in 0..16 { app.update(); std::thread::sleep(Duration::from_millis(16)); }
        let (sender, receiver) = std::sync::mpsc::channel();
        app.world_mut().spawn(Readback::texture(target.clone())).observe(move |event: On<ReadbackComplete>, mut commands: Commands| {
            let _ = sender.send(event.data.clone()); commands.entity(event.entity).try_despawn();
        });
        let started = Instant::now();
        let bytes = loop {
            app.update();
            if let Ok(bytes) = receiver.try_recv() { break bytes; }
            assert!(started.elapsed().as_secs() < 10);
            std::thread::sleep(Duration::from_millis(16));
        };
        let output = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/mesh-sidebar-validation");
        image::RgbaImage::from_raw(768, 900, bytes).unwrap()
            .save(output.join(if expanded { "mesh-create-expanded.png" } else { "mesh-create-collapsed.png" })).unwrap();
    }
}

#[test]
#[ignore = "显式离屏验证完整编辑器工作台，不打开或接管用户的原生窗口"]
fn editor_workbench_gpu_snapshot() {
    use bevy::ecs::system::RunSystemOnce;
    use bevy::render::{gpu_readback::{Readback, ReadbackComplete}, render_resource::{TextureFormat, TextureUsages}};
    use crate::ui::sidebars::{EditorNavigatorTab, sync_editor_sidebar_presentation};
    use std::time::{Duration, Instant};
    let (mut app, window) = page_with_render(true);
    // 用真实内存Clipmap作者配置验收参考图中的表示与LOD，不能用等待态空表单代表完成。
    configure_native(&mut app, SceneTerrainKind::SparseClipmap, 3);
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.manifest.display_name = "测试场景 a".into();
        // 覆盖截图中的连续路径以及多行中文备注；无路径的内存句柄不能掩盖字段换行。
        draft.manifest.notes = "用于验证多行备注与下一项保持间距。\nmaps/MAP-1792154968450/scene.scene_manifest.json".into();
        std::sync::Arc::make_mut(draft.manifest.terrains.as_mut().unwrap())[0].display_name = "主地形".into();
    }
    crate::viewport::modes::install_snapshot_controls(&mut app);
    // 挂载正式动态行池和图片，资产页不再以“等待目录”的空壳作为视觉覆盖。
    app.init_resource::<crate::resources::SceneBrowserIndexState>()
        .init_resource::<crate::resources::SceneBrowserState>()
        .init_resource::<crate::resources::SceneBrowserDragState>()
        .init_resource::<crate::resources::EditorAssetCatalog>()
        .add_systems(Update, (crate::ui::rows::spawn_dynamic_rows, crate::ui::rows::sync_dynamic_list_layouts,
            crate::ui::rows::sync_scene_rows, crate::ui::rows::sync_asset_rows).chain().in_set(crate::SceneEditorSystemSet::Presentation));
    let mut assets = std::fs::read_dir(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/icons/editor")).unwrap()
        .map(|entry| entry.unwrap()).collect::<Vec<_>>();
    assets.sort_by_key(|entry| entry.file_name());
    app.world_mut().resource_mut::<crate::resources::EditorAssetCatalog>().entries = assets.into_iter().take(12).map(|entry| {
        let name = entry.file_name().to_string_lossy().into_owned();
        crate::types::EditorAssetEntry { asset_path: format!("icons/editor/{name}"), display_name: name, is_directory: entry.file_type().unwrap().is_dir(), kind: SceneAssetKind::Texture }
    }).collect();
    app.world_mut().resource_mut::<crate::resources::EditorAssetCatalog>().current_folder = "icons/editor".into();
    app.world_mut().resource_mut::<crate::resources::EditorAssetCatalog>().feedback = Some(crate::asset_catalog::AssetCatalogFeedback { revision: 0, result: Ok(()) });
    crate::ui::asset_preview::install_snapshot_presentation(&mut app);
    crate::ui::context_help::install(&mut app);
    crate::ui::status_bar::install(&mut app); // 与正式工作台一致：重复反馈不参与正文布局，完整文字进入详情。
    app.add_systems(Update, crate::ui::gltf_browser::sync_scene_base_gltf_row);
    crate::ui::theme::install(&mut app);
    app.init_resource::<crate::resources::SceneEditorLayoutState>()
        .init_resource::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>()
        .add_systems(Update, crate::ui::sidebars::handle_editor_sidebar_input.in_set(crate::SceneEditorSystemSet::Input))
        .add_systems(Update, (sync_editor_sidebar_presentation, crate::ui::layout::sync_editor_layout).chain()
            .after(crate::SceneEditorSystemSet::Presentation));
    crate::ui::water_sidebar::install_snapshot_presentation(&mut app);
    crate::ui::terrain_sidebar::install_snapshot_presentation(&mut app);
    crate::ui::building_sidebar::install_snapshot_presentation(&mut app);
    crate::ui::gameplay_sidebar::install_snapshot_presentation(&mut app);
    crate::ui::environment::install_snapshot_presentation(&mut app);
    crate::ui::scene_inspector::install_selected_tool_properties_snapshot_presentation(&mut app);
    crate::ui::scene_sidebar::install_properties_snapshot_presentation(&mut app);
    // 场景属性使用同一份内存作者身份，左侧也采用真实单图目录行；不启动目录加载或文件事务。
    let scene_properties_id = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().scene_id.clone();
    // 已加载正式清单与作者草稿保持同身份，真实编辑按钮参与快照；测试句柄没有路径时不伪造清单地址。
    let saved_manifest = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().manifest.clone();
    app.init_resource::<Assets<SceneManifestAsset>>().init_resource::<SceneContentRegistry>();
    let saved_handle = app.world_mut().resource_mut::<Assets<SceneManifestAsset>>().add(saved_manifest);
    {
        let mut registry = app.world_mut().resource_mut::<SceneContentRegistry>();
        registry.revision = 1;
        registry.scenes.insert(scene_properties_id.clone(), SceneContentHandles {
            manifest: saved_handle, portals: Handle::default(), placements: Handle::default(),
            characters_path: None, surface: None, vegetation: None, water: None, boundaries: None,
        });
    }
    {
        let mut browser = app.world_mut().resource_mut::<crate::resources::SceneBrowserState>();
        browser.select_only(Some(crate::types::SceneBrowserItem::Scene(scene_properties_id.clone())));
        browser.initial_load_resolved = true;
        browser.write_blocked = None;
    }
    {
        let entries = std::sync::Arc::new(vec![crate::types::SceneBrowserEntry {
            item: crate::types::SceneBrowserItem::Scene(scene_properties_id.clone()),
            display_name: "测试场景 a".into(), published: false, depth: 1, parent_folder_id: None,
        }]);
        let signature = crate::types::SceneBrowserIndexSignature { folder_revision: 0, catalog_revision: 1 };
        let mut index = app.world_mut().resource_mut::<crate::resources::SceneBrowserIndexState>();
        index.entries = std::sync::Arc::clone(&entries);
        index.full_entries = entries;
        index.total_entries = 1;
        index.applied_signature = Some(signature.clone()); index.desired_signature = Some(signature.clone());
        index.applied_projection = Some((signature.clone(), 0)); index.desired_projection = Some((signature, 0));
    }
    app.world_mut().resource_mut::<feathrise_editor_core::EditorSettings>().editor_color_theme = feathrise_editor_core::EditorColorTheme::NeutralGray;
    let (camera, target) = {
        let world = app.world_mut();
        let old = world.query_filtered::<Entity, With<ListHost>>().single(world).unwrap();
        let old = world.get::<ChildOf>(old).unwrap().parent();
        world.entity_mut(old).despawn();
        let (camera, target) = world.query::<(Entity, &bevy::camera::RenderTarget)>().single(world).unwrap();
        let bevy::camera::RenderTarget::Image(target) = target else { panic!("必须使用独立离屏图像") };
        (camera, target.handle.clone())
    };
    crate::ui::vegetation_preset_editor::install_workspace_tab_fixture(&mut app);
    let root = app.world_mut().spawn_scene(crate::ui::scene::scene_editor_scene(
        &crate::ui::sidebars::test_document_scenes(),
        &feathrise_editor_weights::scenes(),
        &feathrise_editor_animation::scenes(),
        false,
    )).unwrap().id();
    app.world_mut().entity_mut(root).insert((feathrise_editor_ui::EditorThemeRoot, bevy::ui::UiTargetCamera(camera)));
    app.world_mut().get_mut::<Node>(root).unwrap().display = Display::Flex;
    app.world_mut().get_mut::<Camera>(camera).unwrap().viewport = None;
    // 离屏夹具没有启动场景目录IO；标题从同一份真实内存草稿取值，中央不伪造场景内容。
    {
        let world = app.world_mut();
        for mut text in world.query_filtered::<&mut Text, With<crate::components::SceneEditorCurrentSceneText>>().iter_mut(world) {
            text.0 = "当前场景：测试场景 a".into();
        }
        for mut text in world.query_filtered::<&mut Text, With<crate::components::SceneEditorStatusText>>().iter_mut(world) {
            text.0 = "内存作者草稿 · UI排版验证（未加载三维场景）".into();
        }
    }
    let font = app.world_mut().resource_mut::<Assets<Font>>().add(Font::from_bytes(
        feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec(),
    ));
    let output = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.local/validation");
    std::fs::create_dir_all(&output).unwrap();
    let tab_entity = |app: &mut App, target| {
        let world = app.world_mut();
        world.query::<(Entity, &crate::ui::sidebars::SceneEditorSidebarTabButton)>().iter(world)
            .find(|(_, button)| button.0 == target).unwrap().0
    };
    let environment_tab = tab_entity(&mut app, crate::ui::sidebars::EditorSidebarTab::Left(EditorLeftSidebarTab::Environment));
    let properties_tab = tab_entity(&mut app, crate::ui::sidebars::EditorSidebarTab::Right(EditorRightSidebarTab::Scene));
    let tools_tab = tab_entity(&mut app, crate::ui::sidebars::EditorSidebarTab::Right(EditorRightSidebarTab::Tools));
    let meshes_tab = tab_entity(&mut app, crate::ui::sidebars::EditorSidebarTab::Left(EditorLeftSidebarTab::Meshes));
    assert_eq!(app.world().get::<ChildOf>(environment_tab).unwrap().parent(), app.world().get::<ChildOf>(meshes_tab).unwrap().parent(), "环境必须位于顶部模块栏");
    assert_eq!(app.world().get::<ChildOf>(properties_tab).unwrap().parent(), app.world().get::<ChildOf>(tools_tab).unwrap().parent(), "工具需要独立右侧页签");
    // 复用正式按钮和输入系统，验证环境、对象属性及动态工具页签可往返，且模块记忆不丢失。
    for (button, expected) in [(environment_tab, EditorRightSidebarTab::Environment), (properties_tab, EditorRightSidebarTab::Scene), (tools_tab, EditorRightSidebarTab::Environment)] {
        app.world_mut().entity_mut(button).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
        // 离屏夹具没有物理鼠标；先交给正式输入系统消费边沿，避免PreUpdate拾取重置人工按下。
        app.world_mut().run_system_once(crate::ui::sidebars::handle_editor_sidebar_input).unwrap();
        app.update();
        assert_eq!(app.world().resource::<SceneEditorSidebarState>().right, expected);
        assert_eq!(app.world().resource::<SceneEditorSidebarState>().left, EditorLeftSidebarTab::Environment);
        *app.world_mut().get_mut::<UiInteraction>(button).unwrap() = UiInteraction::None;
        app.update();
    }
    // 正式文件行从左侧浏览器选中，右侧仅显示文件属性；返回工具再回属性仍记忆文件来源。
    app.world_mut().resource_mut::<SceneEditorSidebarState>().navigator = EditorNavigatorTab::Assets;
    for _ in 0..3 { app.update(); }
    let file_row = {
        let world = app.world_mut();
        world.query::<(Entity, &crate::components::SceneEditorAssetRow)>().iter(world)
            .find_map(|(entity, row)| (row.entry_index == Some(0)).then_some(entity)).unwrap()
    };
    app.world_mut().entity_mut(file_row).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.world_mut().run_system_once(crate::asset_catalog::handle_asset_row_clicks).unwrap();
    app.update();
    assert_eq!(app.world().resource::<SceneEditorSidebarState>().right, EditorRightSidebarTab::Assets);
    assert!(app.world().resource::<SceneEditorSidebarState>().assets_browser_visible());
    assert_eq!(app.world().resource::<SceneEditorSidebarState>().left, EditorLeftSidebarTab::Environment);
    *app.world_mut().get_mut::<UiInteraction>(file_row).unwrap() = UiInteraction::None;
    for (button, expected) in [(tools_tab, EditorRightSidebarTab::Environment), (properties_tab, EditorRightSidebarTab::Assets)] {
        app.world_mut().entity_mut(button).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
        app.world_mut().run_system_once(crate::ui::sidebars::handle_editor_sidebar_input).unwrap();
        app.update();
        assert_eq!(app.world().resource::<SceneEditorSidebarState>().right, expected);
        *app.world_mut().get_mut::<UiInteraction>(button).unwrap() = UiInteraction::None;
        app.update();
    }
    let before = app.world().resource::<EditorAuthoringSession>().next_serial;
    for (width, height, scale) in [(1600, 900, 1.0), (1920, 1080, 1.25), (1280, 720, 1.0), (1280, 720, 1.5)] {
        let mut image = Image::new_target_texture(width, height, TextureFormat::Rgba8UnormSrgb, None);
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        app.world_mut().resource_mut::<Assets<Image>>().insert(target.id(), image).unwrap();
        app.world_mut().get_mut::<Window>(window).unwrap().resolution.set(width as f32, height as f32);
        app.world_mut().resource_mut::<UiScale>().0 = scale;
        for (name, module, page) in [
            ("properties", EditorLeftSidebarTab::Meshes, EditorRightSidebarTab::Scene),
            ("mesh-actions", EditorLeftSidebarTab::Meshes, EditorRightSidebarTab::Scene),
            ("lod-both-expanded", EditorLeftSidebarTab::Meshes, EditorRightSidebarTab::Scene),
            ("lod-display-folded", EditorLeftSidebarTab::Meshes, EditorRightSidebarTab::Scene),
            ("lod-collision-folded", EditorLeftSidebarTab::Meshes, EditorRightSidebarTab::Scene),
            ("map-properties", EditorLeftSidebarTab::Meshes, EditorRightSidebarTab::SceneEntry),
            ("sculpt", EditorLeftSidebarTab::Terrain, EditorRightSidebarTab::Tools),
            ("water", EditorLeftSidebarTab::Water, EditorRightSidebarTab::Tools),
            ("building", EditorLeftSidebarTab::Building, EditorRightSidebarTab::Tools),
            ("gameplay", EditorLeftSidebarTab::Gameplay, EditorRightSidebarTab::Tools),
            ("vegetation", EditorLeftSidebarTab::Vegetation, EditorRightSidebarTab::Tools),
            ("assets", EditorLeftSidebarTab::Meshes, EditorRightSidebarTab::Assets),
            ("environment", EditorLeftSidebarTab::Environment, EditorRightSidebarTab::Environment),
            ("settings", EditorLeftSidebarTab::Meshes, EditorRightSidebarTab::Scene),
            ("help", EditorLeftSidebarTab::Meshes, EditorRightSidebarTab::Scene),
            ("documents", EditorLeftSidebarTab::Meshes, EditorRightSidebarTab::Scene),
            ("surface-properties", EditorLeftSidebarTab::Environment, EditorRightSidebarTab::Scene),
            ("portal-properties", EditorLeftSidebarTab::Environment, EditorRightSidebarTab::Scene),
            ("boundary-properties", EditorLeftSidebarTab::Environment, EditorRightSidebarTab::Scene),
        ] {
            {
                let mut state = app.world_mut().resource_mut::<TerrainObjectsEditorState>();
                state.set_section_open(MeshPropertySection::Stitch, name == "mesh-actions");
                state.set_section_open(MeshPropertySection::Source, name == "mesh-actions");
                for section in [MeshPropertySection::Type, MeshPropertySection::DisplayLods, MeshPropertySection::CollisionLods] {
                    state.set_section_open(section, name != "mesh-actions");
                }
                state.set_section_open(MeshPropertySection::Basic, !name.starts_with("lod-"));
                if name.starts_with("lod-") { state.set_section_open(MeshPropertySection::Type, false); }
                if name == "lod-display-folded" { state.set_section_open(MeshPropertySection::DisplayLods, false); }
                if name == "lod-collision-folded" { state.set_section_open(MeshPropertySection::CollisionLods, false); }
                state.display_lods = name != "lod-display-folded";
                state.reset_lod_selection();
            }
            {
                let world = app.world_mut();
                for mut scroll in world.query_filtered::<&mut ScrollPosition, With<crate::ui::scene_inspector::InspectorBody>>().iter_mut(world) {
                    scroll.y = 0.0;
                }
            }
            let property_kind = match name {
                "surface-properties" => Some(crate::ui::scene_inspector::SelectedToolPropertiesKind::Surface),
                "portal-properties" => Some(crate::ui::scene_inspector::SelectedToolPropertiesKind::Portal),
                "boundary-properties" => Some(crate::ui::scene_inspector::SelectedToolPropertiesKind::Boundary),
                _ => None,
            };
            crate::ui::scene_inspector::set_selected_tool_properties_fixture(&mut app, property_kind);
            crate::ui::vegetation_preset_editor::set_workspace_tab_fixture_context(app.world_mut(), name == "documents");
            {
                let mut sidebar = app.world_mut().resource_mut::<SceneEditorSidebarState>();
                sidebar.navigator = match name { "assets" => EditorNavigatorTab::Assets, "help" | "map-properties" => EditorNavigatorTab::Scenes, _ => EditorNavigatorTab::SceneTree };
                sidebar.left = module;
                sidebar.close_help();
                match page {
                    EditorRightSidebarTab::Assets => sidebar.reveal_asset_properties(),
                    EditorRightSidebarTab::Scene => sidebar.reveal_scene_properties(),
                    EditorRightSidebarTab::SceneEntry => sidebar.reveal_scene_entry_properties(),
                    _ => sidebar.right = page,
                }
            }
            app.world_mut().resource_mut::<SceneEditorState>().status = "内存作者草稿 · UI排版验证（未加载三维场景）".into();
            {
                let world = app.world_mut();
                for mut node in world.query_filtered::<&mut Node, With<crate::components::SceneEditorSettingsPanel>>().iter_mut(world) {
                    node.display = if name == "settings" { Display::Flex } else { Display::None };
                }
            }
            for step in 0..12 {
                let world = app.world_mut();
                for mut text in world.query::<&mut TextFont>().iter_mut(world) {
                    if text.font != font.clone().into() { text.font = font.clone().into(); }
                }
                app.update(); std::thread::sleep(Duration::from_millis(16));
                if name == "help" && step == 3 {
                    // 正式点击发生在可见布局之后；不能以刚切页的零尺寸请求冒充已打开帮助。
                    app.world_mut().resource_mut::<SceneEditorSidebarState>()
                        .toggle_help(crate::ui::sidebars::EditorSidebarHelp::SceneBrowser);
                }
            }
            let world = app.world_mut();
            if matches!(name, "properties" | "mesh-actions") { assert_axis_column_alignment(world); }
            if name == "properties" || name.starts_with("lod-") {
                for display in [true, false] {
                    let open = (display || name != "lod-collision-folded") && (!display || name != "lod-display-folded");
                    let (editor, node, computed, parent) = world.query::<(&NativeLodEditor, &Node, &ComputedNode, &ChildOf)>().iter(world)
                        .find(|(editor, ..)| editor.0 == display).unwrap();
                    assert!(world.get::<PanelSection>(parent.parent()).unwrap().0 ==
                        if editor.0 { Section::DisplayLods } else { Section::CollisionLods });
                    assert_eq!(node.display, if open { Display::Flex } else { Display::None });
                    assert_eq!(computed.size().y > 0.0, open, "截图必须渲染本组完整详情，不能借另一组表格或只读摘要代替");
                    let bindings = world.query::<(&Field, &NumericFieldBinding)>().iter(world)
                        .filter(|(field, _)| matches!(field, Field::Lod(usage, _) if *usage == display))
                        .map(|(field, binding)| (*field, binding.current.clone())).collect::<Vec<_>>();
                    assert_eq!(bindings.len(), lod_list::ROW_POOL_SIZE * 2);
                    assert_eq!(bindings.iter().any(|(_, target)| target.is_some()), open);
                    for (field, target) in bindings {
                        if let Some(target) = target { assert_eq!(target.purpose, field); assert_eq!(target.display_lods, display); }
                    }
                }
            }
            if name == "map-properties" {
                crate::ui::scene_sidebar::assert_properties_snapshot(world, &scene_properties_id, "测试场景 a");
            }
            if name == "help" {
                assert!(world.query_filtered::<Entity, With<crate::ui::context_help::ContextHelpMounted>>()
                    .iter(world).next().is_some(), "帮助快照必须包含正式借出的正文");
            }
            let root_size = world.get::<ComputedNode>(root).unwrap().size();
            assert!((root_size.x - width as f32).abs() < 1.1);
            assert!(world.get::<Children>(tools_tab).unwrap().iter().any(|child| world.get::<Text>(child).is_some_and(|text| text.as_str() == module.label())), "工具栏目名称必须跟随顶部模块");
            let (bar, bar_at, parent) = world.query_filtered::<(&ComputedNode, &UiGlobalTransform, &ChildOf), With<crate::ui::vegetation_preset_editor::PresetWorkspaceTabBar>>().single(world).unwrap();
            let parent_at = world.get::<UiGlobalTransform>(parent.parent()).unwrap().translation;
            let parent_size = world.get::<ComputedNode>(parent.parent()).unwrap().size();
            assert!((bar.size().y - 28.0 * scale).abs() < 1.1, "多文档标签栏固定28逻辑像素");
            assert!((bar_at.translation.y - bar.size().y / 2.0 - (parent_at.y - parent_size.y / 2.0)).abs() < 1.1, "多场景和多文件标签应常驻中央工作区顶部");
            for (button, computed, transform) in world.query::<(&crate::ui::sidebars::SceneEditorSidebarTabButton, &ComputedNode, &UiGlobalTransform)>().iter(world) {
                if computed.size() == Vec2::ZERO { continue; }
                let x = transform.translation.x;
                assert!(x - computed.size().x / 2.0 >= -1.1 && x + computed.size().x / 2.0 <= width as f32 + 1.1, "工作台页签越界：{:?}", button.0);
            }
            // 为每个模块保存完整真实BSN外壳；不启动场景IO或作者事务。
            let (sender, receiver) = std::sync::mpsc::channel();
            world.spawn(Readback::texture(target.clone())).observe(move |event: On<ReadbackComplete>, mut commands: Commands| {
                let _ = sender.send(event.data.clone()); commands.entity(event.entity).try_despawn();
            });
            let started = Instant::now();
            let bytes = loop {
                app.update();
                if let Ok(bytes) = receiver.try_recv() { break bytes; }
                assert!(started.elapsed().as_secs() < 10);
                std::thread::sleep(Duration::from_millis(16));
            };
            image::RgbaImage::from_raw(width, height, bytes).unwrap().save(output.join(format!("workbench-{name}-{width}-{scale}.png"))).unwrap();
        }
    }
    assert_eq!(app.world().resource::<EditorAuthoringSession>().next_serial, before);
}
