//! 真实GPU和鼠标命中验证音频侧栏/中央迁移；临时静音PCM避免验证时干扰作者，输出设备缺失必须明确失败。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use super::super::{components::*, resources::*, types::*, workspace};
use super::{components::*, resources::AudioPreviewState, source::PreviewAudio, types::*};
use crate::{
    components::{SceneEditorAssetRow, SceneEditorPresetTabScroll, SceneEditorRoot},
    resources::*,
    types::EditorAssetEntry,
    ui::{sidebars::*, vegetation_preset_editor::*},
};
use bevy::{
    asset::AssetPlugin,
    audio::AudioSinkPlayback,
    ecs::system::RunSystemOnce,
    prelude::*,
    render::{
        gpu_readback::{Readback, ReadbackComplete},
        render_resource::{TextureFormat, TextureUsages},
    },
    ui::{FocusPolicy, RelativeCursorPosition},
    window::{ExitCondition, PrimaryWindow, WindowPlugin},
    winit::WinitPlugin,
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 768;
const FIRST: &str = "音频/第一段 剑声.WAV";
const SECOND: &str = "音频/第二段 音乐.wav";

#[test]
#[ignore = "需要真实GPU与可选系统音频设备；用--ignored --test-threads=1运行"]
fn gpu_asset_audio_real_load_controls_multitabs_and_single_panel() {
    let mut test = Fixture::new();
    wait_ready(&mut test.app, FIRST);
    let panel = audio_panel(&mut test.app);
    let original_panel = preview_panel(&mut test.app);
    let input_ids = editable_entities(&mut test.app);
    assert_eq!(
        test.app
            .world()
            .get::<ChildOf>(original_panel)
            .unwrap()
            .parent(),
        test.sidebar
    );
    assert_eq!(
        test.app.world().get::<Node>(panel).unwrap().display,
        Display::Flex
    );
    snapshot(
        &mut test.app,
        test.target.clone(),
        "asset-browser-audio-sidebar.png",
    );

    // 正式固定目录行的双击建立媒体标签；同一路径重复打开应激活原标签。
    double_click(&mut test, 0);
    frames(&mut test.app, 4);
    let first = test
        .app
        .world()
        .resource::<AssetPreviewWorkspace>()
        .expanded
        .as_ref()
        .unwrap()
        .id;
    assert_eq!(
        test.app
            .world()
            .get::<ChildOf>(original_panel)
            .unwrap()
            .parent(),
        test.center
    );
    assert_eq!(audio_panel(&mut test.app), panel);
    let position_track = track(&mut test.app, SliderField::Position);
    slider_drag(&mut test, position_track, &[0.5]);
    assert!((test.app.world().resource::<AudioPreviewState>().position - 3.0).abs() < 0.02);
    slider_drag(&mut test, position_track, &[0.5, 1.5]);
    assert!((test.app.world().resource::<AudioPreviewState>().position - 6.0).abs() < 0.02);
    slider_drag(&mut test, position_track, &[0.5]);
    let volume_track = track(&mut test.app, SliderField::Volume);
    slider_drag(&mut test, volume_track, &[0.3]);
    assert!((test.app.world().resource::<AudioPreviewState>().volume - 0.3).abs() < 0.01);

    let toggle = button(&mut test.app, AudioAction::Toggle);
    click_audio(&mut test, toggle);
    let has_output = wait_output_or_device_failure(&mut test.app);
    if has_output {
        let before = test.app.world().resource::<AudioPreviewState>().position;
        wait_real_playback_progress(&mut test.app, before);
        click_audio(&mut test, toggle);
        assert!(!test.app.world().resource::<AudioPreviewState>().playing);
        let output = test
            .app
            .world()
            .resource::<AudioPreviewState>()
            .output
            .unwrap();
        assert!(
            test.app
                .world()
                .get::<AudioSink>(output)
                .unwrap()
                .is_paused()
        );
        let stop = button(&mut test.app, AudioAction::Stop);
        click_audio(&mut test, stop);
        assert_eq!(
            test.app.world().resource::<AudioPreviewState>().position,
            0.0
        );
        slider_drag(&mut test, position_track, &[0.5]);
    } else {
        let state = test.app.world().resource::<AssetPreviewState>();
        assert!(state.status.contains("输出") && state.status.contains("设备"));
        assert!(
            test.app
                .world()
                .resource::<AudioPreviewState>()
                .output
                .is_none()
        );
        // 明确设备失败后仍可重新载入原资产；不把设备失败冒充音频文件损坏。
        let reload = button(&mut test.app, AudioAction::Reload);
        click_audio(&mut test, reload);
        wait_ready(&mut test.app, FIRST);
        let position_track = track(&mut test.app, SliderField::Position);
        slider_drag(&mut test, position_track, &[0.5]);
    }
    frames(&mut test.app, 8);
    snapshot(
        &mut test.app,
        test.target.clone(),
        "asset-browser-audio-center.png",
    );
    assert_one_preview(&mut test.app, original_panel, panel, &input_ids);

    double_click(&mut test, 1);
    wait_ready(&mut test.app, SECOND);
    let second = test
        .app
        .world()
        .resource::<AssetPreviewWorkspace>()
        .expanded
        .as_ref()
        .unwrap()
        .id;
    assert_ne!(first, second);
    assert_eq!(
        test.app
            .world()
            .resource::<AssetPreviewWorkspace>()
            .tabs()
            .count(),
        2
    );
    assert_eq!(
        test.app.world().resource::<AudioPreviewState>().position,
        0.0
    );
    let position_track = track(&mut test.app, SliderField::Position);
    slider_drag(&mut test, position_track, &[0.25]);
    double_click(&mut test, 0);
    wait_ready(&mut test.app, FIRST);
    assert_eq!(
        test.app
            .world()
            .resource::<AssetPreviewWorkspace>()
            .tabs()
            .count(),
        2
    );
    assert_eq!(
        test.app
            .world()
            .resource::<AssetPreviewWorkspace>()
            .expanded
            .as_ref()
            .unwrap()
            .id,
        first
    );
    assert!((test.app.world().resource::<AudioPreviewState>().position - 3.0).abs() < 0.05);
    assert!(
        (test
            .app
            .world()
            .resource::<AssetPreviewWorkspace>()
            .parked
            .iter()
            .find(|tab| tab.id == second)
            .unwrap()
            .audio_position
            - 1.5)
            .abs()
            < 0.05
    );
    assert!(!test.app.world().resource::<AudioPreviewState>().playing);
    assert_one_preview(&mut test.app, original_panel, panel, &input_ids);

    // 放大和返回只迁移原面板；在宿主切换后必须保留位置并暂停，不能意外恢复发声。
    let restore = workspace_button(&mut test.app, AssetPreviewWorkspaceAction::ToggleExpand);
    if has_output {
        let toggle = button(&mut test.app, AudioAction::Toggle);
        click_audio(&mut test, toggle);
        assert!(wait_output_or_device_failure(&mut test.app));
    }
    click_workspace(&mut test, restore);
    frames(&mut test.app, 4);
    assert_eq!(
        test.app
            .world()
            .get::<ChildOf>(original_panel)
            .unwrap()
            .parent(),
        test.sidebar
    );
    assert!(!test.app.world().resource::<AudioPreviewState>().playing);
    if let Some(output) = test.app.world().resource::<AudioPreviewState>().output {
        assert!(
            test.app
                .world()
                .get::<AudioSink>(output)
                .unwrap()
                .is_paused()
        );
    }
    double_click(&mut test, 0);
    wait_ready(&mut test.app, FIRST);
    if has_output {
        let toggle = button(&mut test.app, AudioAction::Toggle);
        click_audio(&mut test, toggle);
        assert!(wait_output_or_device_failure(&mut test.app));
    }
    let tab_position = {
        let active = test
            .app
            .world()
            .resource::<AssetPreviewWorkspace>()
            .expanded
            .as_ref()
            .unwrap()
            .id;
        let world = test.app.world_mut();
        world
            .query::<(&AssetPreviewWorkspaceAction, &UiGlobalTransform)>()
            .iter(world)
            .find(|(action, _)| **action == AssetPreviewWorkspaceAction::ActivateTab(active))
            .unwrap()
            .1
            .affine()
            .translation
    };
    super::super::tab_menu::gpu_open_menu(&mut test.app, test.window, test.camera, tab_position);
    frames(&mut test.app, 2);
    super::super::tab_menu::gpu_click_close_all(&mut test.app, test.window, test.camera);
    frames(&mut test.app, 4);
    assert!(
        !test
            .app
            .world()
            .resource::<AssetPreviewWorkspace>()
            .has_tabs()
    );
    assert!(!test.app.world().resource::<AudioPreviewState>().playing);

    // F12关闭走生产生命周期，音频资产、输出与迟到任务都不能继续发声。
    if has_output {
        let toggle = button(&mut test.app, AudioAction::Toggle);
        click_audio(&mut test, toggle);
        assert!(wait_output_or_device_failure(&mut test.app));
    }
    test.app.world_mut().resource_mut::<SceneEditorState>().open = false;
    frames(&mut test.app, 4);
    assert!(
        test.app
            .world()
            .resource::<AudioPreviewState>()
            .key
            .is_none()
    );
    assert!(
        test.app
            .world()
            .resource::<AudioPreviewState>()
            .output
            .is_none()
    );
    assert!(!test.app.world().resource::<AudioPreviewState>().playing);
    assert_one_preview(&mut test.app, original_panel, panel, &input_ids);
}

struct Fixture {
    app: App,
    root: PathBuf,
    window: Entity,
    camera: Entity,
    target: Handle<Image>,
    sidebar: Entity,
    center: Entity,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "rpg-audio-gpu-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("音频")).unwrap();
        write_wav(&root.join(FIRST));
        write_wav(&root.join(SECOND));
        let target_font = root.join(crate::ui::scene::FONT_PATH);
        std::fs::create_dir_all(target_font.parent().unwrap()).unwrap();
        std::fs::write(target_font, feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES).unwrap();
        let mut sidebar_state = SceneEditorSidebarState::default();
        sidebar_state.right = EditorRightSidebarTab::Assets;
        let mut app = App::new();
        app.add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .set(AssetPlugin {
                    file_path: root.to_string_lossy().into(),
                    watch_for_changes_override: Some(false),
                    ..default()
                })
                .disable::<WinitPlugin>()
                .disable::<bevy::log::LogPlugin>(),
        )
        .init_resource::<feathrise_editor_core::EditorSettings>()
        .init_resource::<SceneBrowserIndexState>()
        .init_resource::<EditorTextInputGuard>()
        .init_resource::<SceneEditorNumericScrubMouseGuard>()
        .init_resource::<feathrise_editor_ui::ActiveNumericInputScrub>()
        .init_resource::<SceneBrowserDragState>()
        .init_resource::<SceneBrowserState>()
        .init_resource::<SceneDeletionTask>()
        .init_resource::<crate::authoring::TerrainExternalChangeState>()
        .init_resource::<VegetationPresetEditorState>()
        .init_resource::<SceneEditorLayoutState>()
        .init_resource::<EditorAssetCatalog>()
        .insert_resource(sidebar_state)
        .insert_resource(SceneEditorState {
            open: true,
            selected_asset: Some(FIRST.into()),
            ..default()
        })
        .configure_sets(
            Update,
            (
                crate::SceneEditorSystemSet::Input,
                crate::SceneEditorSystemSet::Tasks,
                crate::SceneEditorSystemSet::Presentation,
            )
                .chain(),
        );
        // 完整预览面板还含动作数值控件，沿正式入口装载共享UI插件，避免夹具缺少输入资源。
        app.add_plugins((feathrise_editor_ui::EditorUiPlugin, feathrise_editor_documents::DocumentsEditorPlugin));
        super::super::install(&mut app);
        app.world_mut()
            .resource_mut::<super::super::DocumentState>()
            .set_asset_root_for_tests(root.clone());
        app.add_systems(
            Update,
            crate::ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard
                .in_set(crate::SceneEditorSystemSet::Input),
        );
        app.add_systems(
            Update,
            crate::ui::layout::handle_editor_layout_input
                .before(workspace::actions)
                .in_set(crate::SceneEditorSystemSet::Input),
        ); // 正式布局输入每帧归还迁移点击，夹具不能把owns_mouse_this_frame永久保留。
        app.world_mut().resource_mut::<EditorAssetCatalog>().entries = [FIRST, SECOND]
            .into_iter()
            .map(|path| EditorAssetEntry {
                asset_path: path.into(),
                display_name: path.rsplit('/').next().unwrap().into(),
                is_directory: false,
                kind: feathrise_scene_content::SceneAssetKind::Unsupported,
            })
            .collect();
        app.finish();
        app.cleanup();
        let window = app
            .world_mut()
            .spawn((
                Window {
                    focused: true,
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .resolution
            .set_physical_resolution(WIDTH, HEIGHT);
        let target = app.world_mut().resource_mut::<Assets<Image>>().add({
            let mut image =
                Image::new_target_texture(WIDTH, HEIGHT, TextureFormat::Bgra8UnormSrgb, None);
            image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
            image
        });
        let camera = app
            .world_mut()
            .spawn((
                Camera2d,
                Camera::default(),
                bevy::camera::RenderTarget::from(target.clone()),
            ))
            .id();
        let ui_root = app
            .world_mut()
            .spawn((
                SceneEditorRoot,
                bevy::ui::UiTargetCamera(camera),
                Node {
                    position_type: PositionType::Absolute,
                    width: px(WIDTH as f32),
                    height: px(HEIGHT as f32),
                    ..default()
                },
            ))
            .id();
        app.world_mut()
            .run_system_once::<_, (), _>(workspace::center_host.spawn())
            .unwrap();
        let center = {
            let world = app.world_mut();
            world
                .query_filtered::<Entity, With<AssetPreviewCenterHost>>()
                .single(world)
                .unwrap()
        };
        app.world_mut().entity_mut(center).insert((
            ChildOf(ui_root),
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                left: px(12),
                top: px(56),
                width: px(936),
                height: px(700),
                min_height: px(0),
                min_width: px(0),
                overflow: Overflow::clip(),
                ..default()
            },
        ));
        let sidebar = app
            .world_mut()
            .spawn((
                ChildOf(ui_root),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(960),
                    top: px(56),
                    width: px(308),
                    height: px(700),
                    padding: UiRect::all(px(4)),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(4),
                    ..default()
                },
            ))
            .id();
        let font = app
            .world()
            .resource::<AssetServer>()
            .load(crate::ui::scene::FONT_PATH);
        for (slot, path) in [FIRST, SECOND].into_iter().enumerate() {
            app.world_mut()
                .spawn((
                    InteractionButton,
                    SceneEditorAssetRow {
                        slot,
                        entry_index: Some(slot),
                    },
                    ChildOf(sidebar),
                    Node {
                        width: percent(100),
                        height: px(26),
                        flex_shrink: 0.0,
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(px(6)),
                        border: UiRect::all(px(1)),
                        ..default()
                    },
                    BackgroundColor(crate::ui::theme::SLOT_BG),
                    BorderColor::all(crate::ui::theme::COPPER_BORDER_WEAK),
                ))
                .with_child((
                    Text::new(format!("[音频] {}", path.rsplit('/').next().unwrap())),
                    TextFont {
                        font: font.clone().into(),
                        font_size: 11.0.into(),
                        ..default()
                    },
                    TextColor(crate::ui::theme::TEXT_PRIMARY),
                    FocusPolicy::Pass,
                    TextLayout::no_wrap(),
                ));
        }
        app.world_mut()
            .run_system_once::<_, (), _>((|| super::super::scene::asset_preview_panel(&feathrise_editor_documents::scenes())).spawn())
            .unwrap();
        let panel = preview_panel(&mut app);
        app.world_mut().entity_mut(panel).insert(ChildOf(sidebar));
        app.world_mut()
            .run_system_once::<_, (), _>((|| workspace::sidebar_launch(&feathrise_editor_documents::scenes())).spawn())
            .unwrap();
        let launch = {
            let world = app.world_mut();
            world
                .query_filtered::<Entity, With<AssetPreviewSidebarLaunch>>()
                .single(world)
                .unwrap()
        };
        app.world_mut().entity_mut(launch).insert(ChildOf(sidebar));
        app.world_mut().spawn((
            SceneEditorPresetTabScroll,
            ChildOf(ui_root),
            Node {
                position_type: PositionType::Absolute,
                left: px(12),
                top: px(12),
                width: px(1256),
                height: px(36),
                ..default()
            },
        ));
        frames(&mut app, 4);
        Self {
            app,
            root,
            window,
            camera,
            target,
            sidebar,
            center,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // 临时目录清理只作用于已核验的本测试独立根；watch关闭，先等待持有的文件任务退出。
        assert_eq!(
            self.root.parent().unwrap().canonicalize().unwrap(),
            std::env::temp_dir().canonicalize().unwrap()
        );
        for _ in 0..100 {
            if self
                .app
                .world()
                .resource::<AudioPreviewState>()
                .task
                .is_none()
            {
                break;
            }
            self.app.update();
            std::thread::sleep(Duration::from_millis(2));
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn write_wav(path: &std::path::Path) {
    let rate = 48_000u32;
    let bytes = rate * 6 * 2;
    let mut wav = Vec::with_capacity(bytes as usize + 44);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(bytes + 36).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&bytes.to_le_bytes());
    wav.resize(bytes as usize + 44, 0);
    std::fs::write(path, wav).unwrap();
}

fn frames(app: &mut App, count: usize) {
    for _ in 0..count {
        app.update();
        std::thread::sleep(Duration::from_millis(16));
    }
}

fn wait_ready(app: &mut App, path: &str) {
    let start = Instant::now();
    loop {
        frames(app, 1);
        let audio = app.world().resource::<AudioPreviewState>();
        let preview = app.world().resource::<AssetPreviewState>();
        if audio.key.as_ref().is_some_and(|key| key.path == path)
            && audio.data.is_some()
            && matches!(
                preview.content,
                AssetPreviewContent::Audio {
                    phase: AssetPreviewPhase::Ready
                }
            )
            && !app.world().resource::<AssetPreviewWorkspace>().restore_view
        {
            frames(app, 3);
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "真实WAV加载未完成：{}",
            preview.status
        );
        assert!(
            !matches!(
                preview.content,
                AssetPreviewContent::Audio {
                    phase: AssetPreviewPhase::Failed
                }
            ),
            "{}",
            preview.status
        );
    }
}

fn wait_output_or_device_failure(app: &mut App) -> bool {
    let start = Instant::now();
    loop {
        frames(app, 1);
        let audio = app.world().resource::<AudioPreviewState>();
        if let Some(sink) = audio
            .output
            .and_then(|output| app.world().get::<AudioSink>(output))
        {
            assert!(
                audio.playing,
                "真实播放点击被暂停：{}",
                playback_diagnostic(app)
            );
            if !sink.is_paused() {
                return true;
            }
        }
        let preview = app.world().resource::<AssetPreviewState>();
        if matches!(
            preview.content,
            AssetPreviewContent::Audio {
                phase: AssetPreviewPhase::Failed
            }
        ) {
            assert!(!audio.playing && audio.output.is_none());
            return false;
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "音频设备不可用必须明确失败，不能无限等待：{}",
            playback_diagnostic(app)
        );
    }
}

fn playback_diagnostic(app: &App) -> String {
    let state = app.world().resource::<AudioPreviewState>();
    let preview = app.world().resource::<AssetPreviewState>();
    let sink = state
        .output
        .and_then(|output| app.world().get::<AudioSink>(output))
        .map(|sink| (sink.is_paused(), sink.empty()));
    let cursor = state
        .cursor
        .as_ref()
        .zip(state.data.as_ref())
        .map(|(cursor, data)| (cursor.position(data), cursor.finished()));
    format!(
        "position={:.4}, playing={}, owner={:?}, output={:?}, sink(paused,empty)={sink:?}, cursor(position,finished)={cursor:?}, phase={:?}, status={}",
        state.position, state.playing, state.owner, state.output, preview.content, preview.status
    )
}

fn wait_real_playback_progress(app: &mut App, before: f64) {
    // 系统输出和10Hz UI反馈异步推进；用真实经过时间及游标条件验收，帧数不能代表声音已消费。
    let start = Instant::now();
    let mut logged = false;
    loop {
        frames(app, 1);
        let state = app.world().resource::<AudioPreviewState>();
        if state.position > before + 0.05 {
            eprintln!(
                "真实音频推进：{before:.4} -> {:.4}，用时{:?}",
                state.position,
                start.elapsed()
            );
            return;
        }
        assert!(
            state.playing,
            "播放期间失去宿主：before={before:.4}，{}",
            playback_diagnostic(app)
        );
        if !logged && start.elapsed() > Duration::from_millis(700) {
            eprintln!(
                "等待真实音频推进：before={before:.4}，{:?}，{}",
                start.elapsed(),
                playback_diagnostic(app)
            );
            logged = true;
        }
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "系统音频sink未消费静音PCM：before={before:.4}，{}",
            playback_diagnostic(app)
        );
    }
}

fn audio_panel(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<AudioPanel>>()
        .single(world)
        .unwrap()
}
fn preview_panel(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<AssetPreviewPanel>>()
        .single(world)
        .unwrap()
}
fn editable_entities(app: &mut App) -> Vec<Entity> {
    let world = app.world_mut();
    let mut ids: Vec<_> = world
        .query_filtered::<Entity, With<bevy::text::EditableText>>()
        .iter(world)
        .collect();
    ids.sort();
    ids
}
fn button(app: &mut App, action: AudioAction) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &AudioButton)>()
        .iter(world)
        .find(|(_, button)| button.action == action)
        .unwrap()
        .0
}
fn track(app: &mut App, field: SliderField) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &AudioTrack)>()
        .iter(world)
        .find(|(_, track)| track.field == field)
        .unwrap()
        .0
}
fn workspace_button(app: &mut App, action: AssetPreviewWorkspaceAction) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &AssetPreviewWorkspaceAction)>()
        .iter(world)
        .find(|(_, value)| **value == action)
        .unwrap()
        .0
}

fn pointer(test: &mut Fixture, position: Vec2) {
    test.app
        .world_mut()
        .get_mut::<Window>(test.window)
        .unwrap()
        .set_physical_cursor_position(Some(position.as_dvec2()));
    let target = test
        .app
        .world()
        .get::<bevy::camera::RenderTarget>(test.camera)
        .unwrap()
        .clone();
    *test
        .app
        .world_mut()
        .get_mut::<bevy::camera::RenderTarget>(test.camera)
        .unwrap() =
        bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(test.window));
    test.app
        .world_mut()
        .run_system_cached(bevy::ui::ui_focus_system)
        .unwrap();
    test.app.world_mut().run_system_once(feathrise_editor_ui::update_editor_button_clicks).unwrap();
    *test
        .app
        .world_mut()
        .get_mut::<bevy::camera::RenderTarget>(test.camera)
        .unwrap() = target;
}
fn center(test: &Fixture, entity: Entity) -> Vec2 {
    test.app
        .world()
        .get::<UiGlobalTransform>(entity)
        .unwrap()
        .affine()
        .translation
}

fn finish_mouse(test: &mut Fixture) {
    let playing = test.app.world().resource::<AudioPreviewState>().playing;
    let pressed_audio = {
        let world = test.app.world_mut();
        world
            .query::<(&AudioButton, &UiInteraction)>()
            .iter(world)
            .find(|(_, interaction)| **interaction == UiInteraction::Pressed)
            .map(|(button, _)| button.action)
    };
    if pressed_audio.is_some() {
        eprintln!(
            "音频点击释放前 action={pressed_audio:?}，{}",
            playback_diagnostic(&test.app)
        );
    }
    // press/release必须分成两个真实输入边沿；同时保留两种just标记时ui_focus会先清Pressed再重新按下。
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    let position = test
        .app
        .world()
        .get::<Window>(test.window)
        .unwrap()
        .physical_cursor_position()
        .unwrap_or(Vec2::ZERO);
    pointer(test, position); // 真实释放命中先清Pressed，正式Update和缓存输入都只看到松开边沿。
    {
        let world = test.app.world_mut();
        assert!(
            world
                .query_filtered::<&UiInteraction, With<AudioButton>>()
                .iter(world)
                .all(|interaction| *interaction != UiInteraction::Pressed),
            "释放边沿后音频按钮仍Pressed"
        );
    }
    if pressed_audio.is_some() {
        eprintln!(
            "音频真实释放focus后 action={pressed_audio:?}，{}",
            playback_diagnostic(&test.app)
        );
    }
    test.app
        .world_mut()
        .run_system_cached(super::control::input)
        .unwrap();
    test.app.world_mut().run_system_once(workspace::actions).unwrap();
    test.app.world_mut().run_system_once(crate::asset_catalog::handle_asset_row_clicks).unwrap();
    test.app.world_mut().run_system_once(super::super::opening::open).unwrap();
    if pressed_audio.is_none() {
        assert_eq!(test.app.world().resource::<AudioPreviewState>().playing, playing,
            "非按钮手势释放不得触发音频播放：{}", playback_diagnostic(&test.app));
    }
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    frames(&mut test.app, 3);
    if pressed_audio.is_some() {
        eprintln!(
            "音频释放完整帧后 action={pressed_audio:?}，{}",
            playback_diagnostic(&test.app)
        );
    }
}

fn click_audio(test: &mut Fixture, entity: Entity) {
    eprintln!(
        "音频点击前 Interaction={:?}，{}",
        test.app.world().get::<UiInteraction>(entity).unwrap(),
        playback_diagnostic(&test.app)
    );
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    let position = center(test, entity);
    pointer(test, position);
    assert_eq!(
        *test.app.world().get::<UiInteraction>(entity).unwrap(),
        UiInteraction::Pressed,
        "鼠标必须真正命中音频按钮"
    );
    test.app
        .world_mut()
        .run_system_cached(super::control::input)
        .unwrap();
    eprintln!(
        "音频按下input后 Interaction={:?}，{}",
        test.app.world().get::<UiInteraction>(entity).unwrap(),
        playback_diagnostic(&test.app)
    );
    test.app
        .world_mut()
        .run_system_once(super::drive::drive)
        .unwrap();
    eprintln!(
        "音频按下drive后 Interaction={:?}，{}",
        test.app.world().get::<UiInteraction>(entity).unwrap(),
        playback_diagnostic(&test.app)
    );
    finish_mouse(test);
}

fn click_workspace(test: &mut Fixture, entity: Entity) {
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    let position = center(test, entity);
    pointer(test, position);
    assert_eq!(
        *test.app.world().get::<UiInteraction>(entity).unwrap(),
        UiInteraction::Pressed
    );
    test.app
        .world_mut()
        .run_system_once(workspace::actions)
        .unwrap();
    finish_mouse(test);
}

fn slider_drag(test: &mut Fixture, entity: Entity, fractions: &[f32]) {
    let at = center(test, entity);
    let size = test.app.world().get::<ComputedNode>(entity).unwrap().size();
    assert!(size.x > 100.0 && size.y > 10.0);
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    for (index, fraction) in fractions.iter().enumerate() {
        pointer(test, at + Vec2::new((*fraction - 0.5) * size.x, 0.0));
        if index == 0 {
            assert_eq!(
                *test.app.world().get::<UiInteraction>(entity).unwrap(),
                UiInteraction::Pressed
            );
        }
        test.app
            .world_mut()
            .run_system_cached(super::control::input)
            .unwrap();
        assert!(
            test.app
                .world()
                .resource::<AudioPreviewState>()
                .owns_mouse()
        );
        if !(0.0..=1.0).contains(fraction) {
            assert!(
                !test
                    .app
                    .world()
                    .get::<RelativeCursorPosition>(entity)
                    .unwrap()
                    .cursor_over()
            );
        }
        test.app
            .world_mut()
            .run_system_once(
                crate::ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard,
            )
            .unwrap();
        assert!(
            test.app
                .world()
                .resource::<SceneEditorNumericScrubMouseGuard>()
                .blocked()
        );
    }
    finish_mouse(test);
    assert!(
        !test
            .app
            .world()
            .resource::<AudioPreviewState>()
            .owns_mouse()
    );
}

fn double_click(test: &mut Fixture, index: usize) {
    let entity = {
        let world = test.app.world_mut();
        world
            .query::<(Entity, &SceneEditorAssetRow)>()
            .iter(world)
            .find(|(_, row)| row.entry_index == Some(index))
            .unwrap()
            .0
    };
    for _ in 0..2 {
        test.app
            .world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        let position = center(test, entity);
        pointer(test, position);
        assert_eq!(
            *test.app.world().get::<UiInteraction>(entity).unwrap(),
            UiInteraction::Pressed
        );
        test.app
            .world_mut()
            .run_system_once(crate::asset_catalog::handle_asset_row_clicks)
            .unwrap();
        test.app
            .world_mut()
            .run_system_once(super::super::opening::open)
            .unwrap();
        finish_mouse(test);
    }
}

fn assert_one_preview(app: &mut App, preview: Entity, audio: Entity, inputs: &[Entity]) {
    assert_eq!(preview_panel(app), preview);
    assert_eq!(audio_panel(app), audio);
    assert_eq!(editable_entities(app), inputs);
    let world = app.world_mut();
    assert_eq!(
        world
            .query_filtered::<Entity, With<AssetPreviewCamera>>()
            .iter(world)
            .count(),
        1
    );
    assert!(
        world
            .query_filtered::<&Camera, With<AssetPreviewCamera>>()
            .iter(world)
            .all(|camera| !camera.is_active)
    );
    assert!(
        world
            .query_filtered::<Entity, With<AudioPlayer<PreviewAudio>>>()
            .iter(world)
            .count()
            <= 1
    );
}

fn snapshot(app: &mut App, target: Handle<Image>, filename: &str) {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let readback = app
        .world_mut()
        .spawn(Readback::texture(target))
        .observe(move |event: On<ReadbackComplete>| {
            let _ = sender.try_send(event.data.clone());
        })
        .id();
    let start = Instant::now();
    let mut bytes = loop {
        frames(app, 1);
        if let Ok(bytes) = receiver.try_recv() {
            assert_eq!(bytes.len(), (WIDTH * HEIGHT * 4) as usize);
            let glyph_pixels = bytes
                .chunks_exact(4)
                .filter(|pixel| pixel[0] > 130 && pixel[1] > 130 && pixel[2] > 130)
                .count();
            if glyph_pixels > 300 {
                break bytes;
            }
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "音频界面GPU截图未就绪"
        );
    };
    app.world_mut().entity_mut(readback).despawn();
    for pixel in bytes.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let output = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.local/validation");
    std::fs::create_dir_all(&output).unwrap();
    image::RgbaImage::from_raw(WIDTH, HEIGHT, bytes)
        .unwrap()
        .save(output.join(filename))
        .unwrap();
}
