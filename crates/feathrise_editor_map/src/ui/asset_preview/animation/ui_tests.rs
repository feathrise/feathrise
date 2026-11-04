//! 动作表固定预算、跨页完整可达及数字输入身份的真实ECS回归。

use super::super::{
    components::*,
    data::AnimationDocument,
    presentation,
    resources::{AnimationEditorState, Session},
};
use bevy::{asset::AssetPlugin, ecs::system::RunSystemOnce, prelude::*};
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, EditorButtonClick, NumericInputDisabled,
    NumericInputSetValue, NumericInputValue, NumericValue,
};
use std::path::PathBuf;

struct Source(PathBuf);

impl Source {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "rpg-animation-ui-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("动作.glb");
        // 九条通道、各35键，同时跨越8通道和32关键帧两个固定池边界。
        let mut binary = Vec::new();
        for index in 0..35 {
            binary.extend_from_slice(&(index as f32 / 30.0).to_le_bytes());
        }
        for _ in 0..35 {
            for value in [0.0_f32, 0.0, 0.0, 1.0] {
                binary.extend_from_slice(&value.to_le_bytes());
            }
        }
        let nodes: Vec<_> = (0..9)
            .map(|index| serde_json::json!({"name":format!("骨骼{index}")}))
            .collect();
        let channels: Vec<_> = (0..9)
            .map(|index| serde_json::json!({"sampler":0,"target":{"node":index,"path":"rotation"}}))
            .collect();
        let mut json = serde_json::to_vec(&serde_json::json!({
            "asset":{"version":"2.0"}, "nodes":nodes,
            "buffers":[{"byteLength":binary.len()}],
            "bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":140},{"buffer":0,"byteOffset":140,"byteLength":560}],
            "accessors":[{"bufferView":0,"componentType":5126,"count":35,"type":"SCALAR"},
                {"bufferView":1,"componentType":5126,"count":35,"type":"VEC4"}],
            "animations":[{"name":"双手剑 测试动作","samplers":[{"input":0,"output":1,"interpolation":"LINEAR"}],"channels":channels}]
        })).unwrap();
        while json.len() % 4 != 0 {
            json.push(b' ');
        }
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"glTF");
        bytes.extend_from_slice(&2_u32.to_le_bytes());
        bytes.extend_from_slice(&((12 + 8 + json.len() + 8 + binary.len()) as u32).to_le_bytes());
        bytes.extend_from_slice(&(json.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&0x4e4f534a_u32.to_le_bytes());
        bytes.extend_from_slice(&json);
        bytes.extend_from_slice(&(binary.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&0x004e4942_u32.to_le_bytes());
        bytes.extend_from_slice(&binary);
        std::fs::write(&path, bytes).unwrap();
        Self(path)
    }

    fn state(&self) -> AnimationEditorState {
        let mut state = AnimationEditorState::default();
        state.active = Some(self.0.clone());
        let mut session = Session::new(AnimationDocument::load(&self.0).unwrap());
        session.key = Some(0);
        state.sessions.insert(self.0.clone(), session);
        state.visible = true;
        state
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0.parent().unwrap());
    }
}

fn fixture() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        bevy::scene::ScenePlugin,
    ))
    .init_asset::<Font>()
    .init_resource::<AnimationEditorState>()
    .init_resource::<ActiveNumericInput>()
    .init_resource::<ActiveNumericInputScrub>()
    .add_message::<NumericInputSetValue>()
    .add_systems(
        Update,
        (super::setup_numeric_inputs, presentation::sync).chain(),
    );
    app.world_mut()
        .run_system_once::<_, (), _>(super::panel.spawn())
        .unwrap();
    app.world_mut()
        .run_system_once::<_, (), _>(super::pose_toolbar.spawn())
        .unwrap();
    app
}

fn count<M: Component>(app: &mut App) -> usize {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<M>>()
        .iter(world)
        .count()
}

fn values(app: &mut App) -> Vec<NumericInputSetValue> {
    app.world_mut()
        .resource_mut::<Messages<NumericInputSetValue>>()
        .drain()
        .collect()
}

fn apply_values(app: &mut App) {
    for event in values(app) {
        app.world_mut()
            .get_mut::<NumericInputValue>(event.entity)
            .unwrap()
            .0 = event.value;
    }
}

#[test]
fn animation_ui_fixed_pool_setup_and_all_pages_reuse_entities() {
    let source = Source::new();
    let mut app = fixture();
    assert_eq!(count::<AnimationChannelButton>(&mut app), CHANNEL_SLOTS);
    assert_eq!(
        count::<AnimationKeyButton>(&mut app),
        CHANNEL_SLOTS * KEY_SLOTS
    );
    let panel = app
        .world_mut()
        .query_filtered::<Entity, With<AnimationPanel>>()
        .single(app.world())
        .unwrap();
    assert_eq!(
        app.world().get::<Node>(panel).unwrap().display,
        Display::None
    );
    app.update();
    assert_eq!(count::<AnimationNumericInput>(&mut app), 4);
    let fields: Vec<_> = app
        .world_mut()
        .query_filtered::<Entity, With<AnimationNumericInput>>()
        .iter(app.world())
        .collect();
    assert!(
        fields
            .iter()
            .all(|entity| app.world().get::<NumericInputDisabled>(*entity).is_some())
    );
    app.insert_resource(source.state());
    app.update();
    apply_values(&mut app);
    assert_eq!(
        app.world().get::<Node>(panel).unwrap().display,
        Display::Flex
    );
    assert!(
        fields
            .iter()
            .all(|entity| app.world().get::<NumericInputDisabled>(*entity).is_none())
    );
    let entity_count = app.world().entities().len();
    let mut reached = std::collections::HashSet::new();
    for channel_page in 0..2 {
        for key_page in 0..2 {
            {
                let mut state = app.world_mut().resource_mut::<AnimationEditorState>();
                let session = state.current_mut().unwrap();
                session.page = channel_page;
                session.bone_filter = false;
                session.window_start = key_page as f32 * 0.6;
                session.window_span = 0.6;
                state.changed();
            }
            app.update();
            let world = app.world_mut();
            for (marker, node) in world.query::<(&AnimationKeyButton, &Node)>().iter(world) {
                if node.display == Display::Flex {
                    reached.insert((marker.channel.unwrap(), marker.index.unwrap()));
                }
            }
            assert_eq!(app.world().entities().len(), entity_count);
            assert_eq!(count::<AnimationNumericInput>(&mut app), 4);
        }
    }
    assert_eq!(
        reached.len(),
        9 * 35,
        "平移连续时间视区必须可访问全部通道和关键帧，固定池不截断作者内容"
    );
    {
        let mut state = app.world_mut().resource_mut::<AnimationEditorState>();
        let session = state.current_mut().unwrap();
        session.channel = Some(8);
        session.key = Some(34);
        state.changed();
    }
    app.update();
    apply_values(&mut app);
    for (_, marker, value) in app
        .world_mut()
        .query::<(Entity, &AnimationNumericInput, &NumericInputValue)>()
        .iter(app.world())
    {
        match marker.field {
            AnimationNumericField::Frame => {
                assert!((value.0.as_f32().unwrap() - 34.0).abs() < 0.001)
            }
            AnimationNumericField::Value(3) => assert_eq!(value.0, NumericValue::F32(1.0)),
            AnimationNumericField::Value(_) => assert_eq!(value.0, NumericValue::F32(0.0)),
        }
    }
}

#[test]
fn animation_ui_read_only_disables_authoring_but_keeps_playback_and_key_navigation() {
    let source = Source::new();
    let mut app = fixture();
    let mut state = source.state();
    std::sync::Arc::make_mut(&mut state.current_mut().unwrap().document).read_only = true;
    app.insert_resource(state);
    app.update();
    let world = app.world_mut();
    assert!(
        world
            .query_filtered::<Entity, With<AnimationNumericInput>>()
            .iter(world)
            .all(|entity| world.get::<NumericInputDisabled>(entity).is_some())
    );
    for (marker, disabled) in world
        .query::<(&AnimationButton, Has<bevy::ui::InteractionDisabled>)>()
        .iter(world)
    {
        match marker.action {
            AnimationAction::InsertKey
            | AnimationAction::DeleteKey
            | AnimationAction::CycleInterpolation
            | AnimationAction::Save => assert!(disabled),
            AnimationAction::TogglePlayback | AnimationAction::NextKey => assert!(!disabled),
            _ => {}
        }
    }
}

#[test]
fn animation_ui_rebinding_cancels_old_click_and_only_replaces_numeric_buffer_on_identity_change() {
    let source = Source::new();
    let mut app = fixture();
    app.insert_resource(source.state());
    app.update();
    apply_values(&mut app);
    let input = app
        .world_mut()
        .query::<(Entity, &AnimationNumericInput)>()
        .iter(app.world())
        .find(|(_, marker)| marker.field == AnimationNumericField::Value(0))
        .unwrap()
        .0;
    app.world_mut().resource_mut::<ActiveNumericInput>().entity = Some(input);
    app.world_mut()
        .get_mut::<NumericInputValue>(input)
        .unwrap()
        .0 = NumericValue::F32(999.0);
    app.update();
    assert!(
        !values(&mut app).iter().any(|event| event.entity == input),
        "同身份刷新不能覆盖用户字符缓冲"
    );
    let button = app
        .world_mut()
        .query_filtered::<Entity, With<AnimationKeyButton>>()
        .iter(app.world())
        .next()
        .unwrap();
    app.world_mut()
        .entity_mut(button)
        .insert(EditorButtonClick::triggered());
    app.world_mut()
        .resource_mut::<ActiveNumericInputScrub>()
        .entity = Some(input);
    app.world_mut()
        .resource_mut::<AnimationEditorState>()
        .changed();
    app.update();
    assert_eq!(
        app.world().resource::<ActiveNumericInputScrub>().entity,
        None,
        "旧数字拖动必须在换代时终止，松键不能改写新目标"
    );
    assert!(
        !app.world()
            .get::<EditorButtonClick>(button)
            .unwrap()
            .is_clicked()
    );
    assert!(
        values(&mut app).iter().any(|event| event.entity == input
            && event.cancel_edit
            && event.value == NumericValue::F32(0.0)),
        "冻结身份换代必须清除旧输入，不能把旧数字改派到新选择"
    );
}

#[test]
#[ignore = "需要真实GPU；用--ignored --test-threads=1运行"]
fn gpu_animation_editor_wide_and_narrow_formal_bsn_layout() {
    use bevy::{
        render::{
            gpu_readback::{Readback, ReadbackComplete},
            render_resource::{TextureFormat, TextureUsages},
        },
        window::{ExitCondition, WindowPlugin},
        winit::WinitPlugin,
    };
    let source = Source::new();
    for (width, height, name, formal_parent) in [
        (960_u32, 640_u32, "wide", false),
        (336, 900, "narrow", false),
        (960, 640, "combined", true),
    ] {
        let mut app = App::new();
        app.add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .set(AssetPlugin {
                    file_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("../../assets")
                        .to_string_lossy()
                        .into_owned(),
                    watch_for_changes_override: Some(false),
                    ..default()
                })
                .disable::<WinitPlugin>()
                .disable::<bevy::log::LogPlugin>(),
        )
        .add_plugins(feathrise_editor_ui::EditorUiPlugin)
        .insert_resource(feathrise_editor_ui::EditorPalette::neutral_gray())
        .insert_resource(source.state())
        .add_systems(
            Update,
            (super::setup_numeric_inputs, presentation::sync).chain(),
        );
        app.finish();
        app.cleanup();
        let target = app.world_mut().resource_mut::<Assets<Image>>().add({
            let mut image =
                Image::new_target_texture(width, height, TextureFormat::Bgra8UnormSrgb, None);
            image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
            image
        });
        let camera = app
            .world_mut()
            .spawn((
                Camera2d,
                Camera {
                    clear_color: ClearColorConfig::Custom(Color::srgb(0.02, 0.02, 0.02)),
                    ..default()
                },
                bevy::camera::RenderTarget::from(target.clone()),
            ))
            .id();
        if formal_parent {
            app.world_mut()
                .run_system_once::<_, (), _>(
                    (|| {
                        super::super::super::scene::asset_preview_panel(
                            &feathrise_editor_documents::scenes(),
                        )
                    })
                    .spawn(),
                )
                .unwrap();
        } else {
            app.world_mut()
                .run_system_once::<_, (), _>(super::panel.spawn())
                .unwrap();
        }
        let panel = app
            .world_mut()
            .query_filtered::<Entity, With<AnimationPanel>>()
            .single(app.world())
            .unwrap();
        let root = if formal_parent {
            app.world_mut()
                .query_filtered::<Entity, With<super::super::super::types::AssetPreviewPanel>>()
                .single(app.world())
                .unwrap()
        } else {
            panel
        };
        app.world_mut().entity_mut(root).insert((
            feathrise_editor_ui::EditorThemeRoot,
            bevy::ui::UiTargetCamera(camera),
        ));
        {
            let mut node = app.world_mut().get_mut::<Node>(root).unwrap();
            node.display = Display::Flex;
            node.position_type = PositionType::Absolute;
            node.left = px(12);
            node.top = px(12);
            node.width = px((width - 24) as f32);
            if formal_parent {
                // 使用正式AssetPreviewPanel父布局，保留标题、26px导航及至少170px画布。
                node.height = px((height - 24) as f32);
                node.min_height = px(0);
                node.flex_shrink = 1.0;
            }
        }
        if formal_parent {
            let world = app.world_mut();
            for mut node in world.query_filtered::<&mut Node, With<super::super::super::types::AssetPreviewControls>>().iter_mut(world) {
                node.display = Display::Flex;
            }
            for mut node in world.query_filtered::<&mut Node, With<super::super::super::types::AssetPreviewSubAssetPanel>>().iter_mut(world) {
                node.display = Display::Flex; // 模拟原目录已Ready，正式动画表现必须主动收起它。
            }
        }
        // 布局夹具直接装载 SDK 唯一字体源；不要求编辑器额外分发同一份 30 MiB 字体文件。
        let font = app.world_mut().resource_mut::<Assets<Font>>().add(Font::from_bytes(
            feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec(),
        ));
        for _ in 0..24 {
            let world = app.world_mut();
            for mut text in world.query::<&mut TextFont>().iter_mut(world) {
                text.font = font.clone().into();
            }
            app.update();
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
        let size = app.world().get::<ComputedNode>(panel).unwrap().size();
        assert!(
            size.y > 250.0 && size.y < height as f32 - 24.0,
            "{name}正式动画表高度溢出：{size:?}"
        );
        if formal_parent {
            let world = app.world_mut();
            let canvas = world
                .query_filtered::<Entity, With<super::super::super::types::AssetPreviewCanvas>>()
                .single(world)
                .unwrap();
            assert!(
                world.get::<ComputedNode>(canvas).unwrap().size().y >= 169.0,
                "动画表挤掉了实际预览画布"
            );
            let toolbar = world.query_filtered::<Entity, With<AnimationPoseToolbar>>().single(world).unwrap();
            assert_eq!(world.get::<Node>(toolbar).unwrap().display, Display::Flex);
            let center = |entity| world.get::<UiGlobalTransform>(entity).unwrap().affine().translation;
            let canvas_min = center(canvas) - world.get::<ComputedNode>(canvas).unwrap().size() * 0.5;
            let toolbar_min = center(toolbar) - world.get::<ComputedNode>(toolbar).unwrap().size() * 0.5;
            assert!((toolbar_min - canvas_min).abs_diff_eq(Vec2::splat(9.), 1.0), "移动/旋转入口必须固定在画布左上角");
            assert!(world.get::<ComputedNode>(toolbar).unwrap().size().y >= 54.0);
            let bottom = |entity| {
                world
                    .get::<UiGlobalTransform>(entity)
                    .unwrap()
                    .affine()
                    .translation
                    .y
                    + world.get::<ComputedNode>(entity).unwrap().size().y * 0.5
            };
            assert!(
                bottom(panel) <= bottom(root) + 1.0,
                "正式父布局裁掉了动画表下部"
            );
            assert!(world.query_filtered::<&Node, With<super::super::super::types::AssetPreviewSubAssetPanel>>().iter(world)
                .all(|node| node.display == Display::None));
        }
        {
            let world = app.world_mut();
            for (marker, node) in world
                .query::<(&AnimationNumericInput, &ComputedNode)>()
                .iter(world)
            {
                assert!(
                    node.size().x >= 88.0 && node.size().y >= 24.0,
                    "{name}数值字段{:?}命中区被压扁：{:?}",
                    marker.field,
                    node.size()
                );
            }
        }
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let readback = app
            .world_mut()
            .spawn(Readback::texture(target))
            .observe(move |event: On<ReadbackComplete>| {
                let _ = sender.try_send(event.data.clone());
            })
            .id();
        let started = std::time::Instant::now();
        let bytes = loop {
            app.update();
            if let Ok(bytes) = receiver.try_recv() {
                break bytes;
            }
            assert!(started.elapsed().as_secs() < 20, "{name}动画表GPU回读超时");
            std::thread::sleep(std::time::Duration::from_millis(16));
        };
        app.world_mut().entity_mut(readback).despawn();
        let row_bytes = width as usize * 4;
        let stride = row_bytes.div_ceil(256) * 256;
        assert_eq!(bytes.len(), stride * height as usize);
        let mut pixels: Vec<_> = bytes
            .chunks_exact(stride)
            .flat_map(|row| row[..row_bytes].iter().copied())
            .collect();
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        let light_pixels = pixels
            .chunks_exact(4)
            .filter(|pixel| pixel[0] > 120 && pixel[1] > 120 && pixel[2] > 120)
            .count();
        assert!(
            light_pixels > 800,
            "{name}正式BSN没有渲染出足够的中文文字/关键帧：{light_pixels}"
        );
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../target/animation-editor-{name}.png"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        image::RgbaImage::from_raw(width, height, pixels)
            .unwrap()
            .save(path)
            .unwrap();
    }
}
