//! 无设备的真实后台音频会话回归：验证完整采用、单槽换代、可见性、控件身份和作者数据隔离。

use feathrise_editor_ui::UiInteraction;
use super::{components::*, resources::AudioPreviewState, source::*, types::*, *};
use crate::{
    resources::*,
    ui::{
        asset_preview::{
            AssetPreviewWorkspace, DocumentState,
            types::{
                AssetPreviewContent, AssetPreviewPhase, AssetPreviewState, ExpandedAssetPreview,
            },
        },
        sidebars::{EditorRightSidebarTab, MapEditorSidebarState},
        vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState},
    },
};
use bevy::{
    audio::Decodable,
    ecs::system::RunSystemOnce,
    input::mouse::MouseWheel,
    tasks::{IoTaskPool, block_on},
    ui::RelativeCursorPosition,
    window::PrimaryWindow,
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "rpg-audio-state-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, path: &str, bytes: &[u8]) {
        let target = self.0.join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, bytes).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // 仅回收本测试新建且带唯一身份的临时目录，不触碰项目缓存或资产。
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn wav(frames: usize, rate: u32) -> Vec<u8> {
    let data_bytes = (frames * 2) as u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_bytes.to_le_bytes());
    for index in 0..frames {
        bytes.extend_from_slice(&((index % 64) as i16 * 256).to_le_bytes());
    }
    bytes
}

fn app(fixture: &Fixture, path: &str) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<Assets<PreviewAudio>>()
        .init_resource::<AudioPreviewState>()
        .init_resource::<MapEditorState>()
        .init_resource::<MapEditorSidebarState>()
        .init_resource::<MapEditorLayoutState>()
        .init_resource::<MapEditorSettingsState>()
        .init_resource::<MapEditorNumericScrubMouseGuard>()
        .init_resource::<VegetationPresetEditorState>()
        .init_resource::<AssetPreviewWorkspace>()
        .init_resource::<AssetPreviewState>()
        .init_resource::<DocumentState>()
        .init_resource::<ButtonInput<MouseButton>>()
        .add_message::<MouseWheel>()
        .add_systems(Update, (control::input, drive::drive).chain());
    app.world_mut()
        .resource_mut::<DocumentState>()
        .set_asset_root_for_tests(fixture.0.clone());
    app.world_mut().resource_mut::<MapEditorState>().open = true;
    app.world_mut()
        .resource_mut::<MapEditorSidebarState>()
        .right = EditorRightSidebarTab::Assets;
    select(&mut app, path);
    app
}

fn select(app: &mut App, path: &str) {
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .selected_asset = Some(path.into());
    let mut preview = app.world_mut().resource_mut::<AssetPreviewState>();
    preview.selected_path = Some(path.into());
    preview.content = AssetPreviewContent::Audio {
        phase: AssetPreviewPhase::Loading,
    };
}

fn settle(app: &mut App) {
    // 只在测试中让后台读取/解码得到调度机会；生产系统每帧始终非阻塞轮询。
    for _ in 0..3000 {
        app.update();
        let state = app.world().resource::<AudioPreviewState>();
        if state.task.is_none() && state.pending.is_none() {
            return;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    panic!("音频后台槽未完成");
}

fn ready(app: &App) -> bool {
    matches!(
        app.world().resource::<AssetPreviewState>().content,
        AssetPreviewContent::Audio {
            phase: AssetPreviewPhase::Ready
        }
    )
}

fn press(app: &mut App, action: AudioAction, key: Option<AudioKey>) {
    let button = app
        .world_mut()
        .spawn((AudioButton { action, key }, feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed))
        .id();
    app.update();
    app.world_mut().entity_mut(button).despawn();
}

fn action(app: &mut App, action: AudioAction) {
    let key = app.world().resource::<AudioPreviewState>().key.clone();
    press(app, action, key);
}

fn tab(id: u64, path: &str) -> ExpandedAssetPreview {
    ExpandedAssetPreview {
        id,
        path: path.into(),
        return_tab: PresetWorkspaceTab::Map,
        view: default(),
        target: default(),
        audio_position: 0.0,
        restore_cursor: 0,
    }
}

#[test]
fn audio_session_real_wav_publishes_complete_pcm_without_output_or_writes() {
    let fixture = Fixture::new();
    let bytes = wav(32, 8);
    fixture.write("sounds/雪.wav", &bytes);
    let mut app = app(&fixture, "sounds/雪.wav");
    app.update();
    assert!(app.world().resource::<AudioPreviewState>().task.is_some());
    assert!(app.world().resource::<AudioPreviewState>().data.is_none());
    settle(&mut app);
    assert!(ready(&app));
    let state = app.world().resource::<AudioPreviewState>();
    let data = state.data.as_ref().unwrap();
    assert_eq!(
        (data.frames(), data.channels.get(), data.sample_rate.get()),
        (32, 1, 8)
    );
    assert_eq!(data.duration(), 4.0);
    assert!(!state.playing && state.output.is_none() && state.handle.is_none());
    assert!(
        app.world()
            .resource::<AssetPreviewState>()
            .status
            .contains("4.0秒")
    );
    assert_eq!(
        std::fs::read(fixture.0.join("sounds/雪.wav")).unwrap(),
        bytes
    );
}

#[test]
fn audio_session_rapid_switch_keeps_old_slot_and_rejects_late_pcm() {
    let fixture = Fixture::new();
    for (path, frames) in [("old.wav", 8), ("middle.wav", 16), ("latest.wav", 24)] {
        fixture.write(path, &wav(frames, 8));
    }
    let mut app = app(&fixture, "old.wav");
    settle(&mut app);
    let old_key = app.world().resource::<AudioPreviewState>().key.clone();
    let old_data = app
        .world()
        .resource::<AudioPreviewState>()
        .data
        .clone()
        .unwrap();
    let (release, receiver) = std::sync::mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    app.world_mut().resource_mut::<AudioPreviewState>().task = Some(AudioLoadSlot {
        cancel: cancel.clone(),
        task: IoTaskPool::get().spawn(async move {
            let _ = receiver.recv();
            AudioLoadResult {
                key: old_key,
                data: Ok(old_data),
            }
        }),
    });
    select(&mut app, "middle.wav");
    app.update();
    assert!(cancel.load(Ordering::Relaxed));
    let middle_generation = app.world().resource::<AudioPreviewState>().generation;
    select(&mut app, "latest.wav");
    app.update();
    let state = app.world().resource::<AudioPreviewState>();
    assert!(state.task.is_some() && state.data.is_none());
    assert_eq!(state.pending.as_ref().unwrap().path, "latest.wav");
    assert!(state.generation > middle_generation);
    assert!(state.task.as_ref().unwrap().cancel.load(Ordering::Relaxed));
    release.send(()).unwrap();
    settle(&mut app);
    let state = app.world().resource::<AudioPreviewState>();
    assert_eq!(state.key.as_ref().unwrap().path, "latest.wav");
    assert_eq!(state.data.as_ref().unwrap().frames(), 24);
    assert!(ready(&app) && !state.playing && state.output.is_none());
}

#[test]
fn audio_session_failure_waits_for_explicit_reload_before_retry() {
    let fixture = Fixture::new();
    fixture.write("broken.wav", b"bad audio");
    let mut app = app(&fixture, "broken.wav");
    settle(&mut app);
    assert!(matches!(
        app.world().resource::<AssetPreviewState>().content,
        AssetPreviewContent::Audio {
            phase: AssetPreviewPhase::Failed
        }
    ));
    let failed_generation = app.world().resource::<AudioPreviewState>().generation;
    fixture.write("broken.wav", &wav(16, 8));
    for _ in 0..3 {
        app.update();
    }
    assert!(!ready(&app));
    assert!(app.world().resource::<AudioPreviewState>().task.is_none());
    action(&mut app, AudioAction::Reload);
    settle(&mut app);
    assert!(ready(&app));
    let state = app.world().resource::<AudioPreviewState>();
    assert!(state.generation > failed_generation);
    assert_eq!(state.data.as_ref().unwrap().duration(), 2.0);
    assert!(!state.playing);
}

#[test]
fn audio_session_old_button_identity_cannot_play_stop_or_reload_new_file() {
    let fixture = Fixture::new();
    fixture.write("first.wav", &wav(16, 8));
    fixture.write("second.wav", &wav(24, 8));
    let mut app = app(&fixture, "first.wav");
    settle(&mut app);
    let old = app.world().resource::<AudioPreviewState>().key.clone();
    select(&mut app, "second.wav");
    settle(&mut app);
    app.world_mut()
        .resource_mut::<AudioPreviewState>()
        .seek(1.0);
    let generation = app.world().resource::<AudioPreviewState>().generation;
    for action in [AudioAction::Toggle, AudioAction::Stop, AudioAction::Reload] {
        press(&mut app, action, old.clone());
        let state = app.world().resource::<AudioPreviewState>();
        assert_eq!(state.generation, generation);
        assert_eq!(state.position, 1.0);
        assert!(!state.playing && !state.retry && state.output.is_none());
    }
}

#[test]
fn audio_session_eof_play_restarts_and_stop_returns_zero_without_copying_pcm() {
    let fixture = Fixture::new();
    fixture.write("loop.wav", &wav(16, 8));
    let mut app = app(&fixture, "loop.wav");
    settle(&mut app);
    let data = app
        .world()
        .resource::<AudioPreviewState>()
        .data
        .clone()
        .unwrap();
    app.world_mut()
        .resource_mut::<AudioPreviewState>()
        .seek(data.duration());
    action(&mut app, AudioAction::Toggle);
    let state = app.world().resource::<AudioPreviewState>();
    assert!(state.playing);
    assert_eq!(state.position, 0.0);
    let first = state.output.unwrap();
    let handle = state.handle.clone().unwrap();
    let cursor = state.cursor.clone().unwrap();
    let mut decoder = app
        .world()
        .resource::<Assets<PreviewAudio>>()
        .get(&handle)
        .unwrap()
        .decoder();
    assert_eq!(decoder.by_ref().count(), 16);
    assert!(cursor.finished());
    {
        let mut state = app.world_mut().resource_mut::<AudioPreviewState>();
        state.playing = false;
        state.position = data.duration();
    }
    action(&mut app, AudioAction::Toggle);
    let state = app.world().resource::<AudioPreviewState>();
    assert!(state.playing && state.output.is_some_and(|entity| entity != first));
    assert_eq!(state.position, 0.0);
    assert!(Arc::ptr_eq(&data, state.data.as_ref().unwrap()));
    assert!(!Arc::ptr_eq(&cursor, state.cursor.as_ref().unwrap()));
    action(&mut app, AudioAction::Stop);
    let state = app.world().resource::<AudioPreviewState>();
    assert!(!state.playing);
    assert_eq!(state.position, 0.0);
    assert_eq!(state.cursor.as_ref().unwrap().position(&data), 0.0);
}

#[test]
fn audio_session_no_device_timeout_releases_output_and_reports_failure() {
    let fixture = Fixture::new();
    fixture.write("sound.wav", &wav(16, 8));
    let mut app = app(&fixture, "sound.wav");
    settle(&mut app);
    action(&mut app, AudioAction::Toggle);
    let output = app.world().resource::<AudioPreviewState>().output.unwrap();
    assert!(
        app.world()
            .get::<AudioPlayer<PreviewAudio>>(output)
            .is_some()
    );
    // 调整独立真实时钟的起点，验证两秒阈值而不等待或依赖系统声卡。
    app.world_mut().resource_mut::<AudioPreviewState>().waiting =
        Some(Instant::now() - Duration::from_secs(3));
    app.update();
    let state = app.world().resource::<AudioPreviewState>();
    assert!(
        !state.playing
            && state.output.is_none()
            && state.handle.is_none()
            && state.cursor.is_none()
    );
    assert!(app.world().get_entity(output).is_err());
    assert!(matches!(
        app.world().resource::<AssetPreviewState>().content,
        AssetPreviewContent::Audio {
            phase: AssetPreviewPhase::Failed
        }
    ));
    assert!(
        app.world()
            .resource::<AssetPreviewState>()
            .status
            .contains("输出不可用")
    );
}

#[test]
fn audio_session_sidebar_hidden_modal_or_focus_loss_pauses_without_auto_resume() {
    let fixture = Fixture::new();
    fixture.write("sound.wav", &wav(32, 8));
    let cases: [(fn(&mut App), fn(&mut App)); 4] = [
        (
            |app| {
                app.world_mut()
                    .resource_mut::<MapEditorSidebarState>()
                    .right = EditorRightSidebarTab::Scene
            },
            |app| {
                app.world_mut()
                    .resource_mut::<MapEditorSidebarState>()
                    .right = EditorRightSidebarTab::Assets
            },
        ),
        (
            |app| {
                app.world_mut()
                    .resource_mut::<MapEditorLayoutState>()
                    .inspector_visible = false
            },
            |app| {
                app.world_mut()
                    .resource_mut::<MapEditorLayoutState>()
                    .inspector_visible = true
            },
        ),
        (
            |app| {
                app.world_mut()
                    .resource_mut::<MapEditorSettingsState>()
                    .open = true
            },
            |app| {
                app.world_mut()
                    .resource_mut::<MapEditorSettingsState>()
                    .open = false
            },
        ),
        (
            |app| {
                app.world_mut().spawn((
                    Window {
                        focused: false,
                        ..default()
                    },
                    PrimaryWindow,
                ));
            },
            |app| {
                let world = app.world_mut();
                let mut windows = world.query::<&mut Window>();
                for mut window in windows.iter_mut(world) {
                    window.focused = true;
                }
            },
        ),
    ];
    for (hide, restore) in cases {
        let mut app = app(&fixture, "sound.wav");
        settle(&mut app);
        action(&mut app, AudioAction::Toggle);
        assert!(app.world().resource::<AudioPreviewState>().playing);
        hide(&mut app);
        app.update();
        let state = app.world().resource::<AudioPreviewState>();
        assert!(!state.playing && state.owner.is_none());
        restore(&mut app);
        app.update();
        let state = app.world().resource::<AudioPreviewState>();
        assert!(!state.playing && state.owner.is_some());
    }
}

#[test]
fn audio_session_f12_close_cancels_load_releases_output_and_reopens_paused() {
    let fixture = Fixture::new();
    fixture.write("sound.wav", &wav(32, 8));
    let mut app = app(&fixture, "sound.wav");
    settle(&mut app);
    action(&mut app, AudioAction::Toggle);
    let output = app.world().resource::<AudioPreviewState>().output.unwrap();
    let old_key = app.world().resource::<AudioPreviewState>().key.clone();
    let old_data = app
        .world()
        .resource::<AudioPreviewState>()
        .data
        .clone()
        .unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let (release, receiver) = std::sync::mpsc::channel();
    app.world_mut().resource_mut::<AudioPreviewState>().task = Some(AudioLoadSlot {
        cancel: cancel.clone(),
        task: IoTaskPool::get().spawn(async move {
            let _ = receiver.recv();
            AudioLoadResult {
                key: old_key,
                data: Ok(old_data),
            }
        }),
    });
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .close_requested = true;
    app.update();
    let state = app.world().resource::<AudioPreviewState>();
    assert!(
        state.key.is_none() && state.output.is_none() && state.data.is_none() && !state.playing
    );
    assert!(state.task.is_some() && state.pending.is_none());
    assert!(cancel.load(Ordering::Relaxed));
    assert!(app.world().get_entity(output).is_err());
    release.send(()).unwrap();
    settle(&mut app);
    assert!(app.world().resource::<AudioPreviewState>().data.is_none());
    app.world_mut().resource_mut::<MapEditorState>().open = false;
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .close_requested = false;
    app.update();
    assert!(app.world().resource::<AudioPreviewState>().key.is_none());
    app.world_mut().resource_mut::<MapEditorState>().open = true;
    settle(&mut app);
    assert!(ready(&app));
    assert!(!app.world().resource::<AudioPreviewState>().playing);
    assert!(app.world().resource::<AudioPreviewState>().output.is_none());
}

#[test]
fn audio_session_media_tabs_preserve_independent_positions_and_pause_on_author_page() {
    let fixture = Fixture::new();
    fixture.write("first.wav", &wav(32, 8));
    fixture.write("second.wav", &wav(48, 8));
    let mut app = app(&fixture, "first.wav");
    app.world_mut()
        .resource_mut::<AssetPreviewWorkspace>()
        .expanded = Some(tab(1, "first.wav"));
    app.world_mut()
        .resource_mut::<VegetationPresetEditorState>()
        .active_tab = PresetWorkspaceTab::AssetPreview;
    settle(&mut app);
    app.world_mut()
        .resource_mut::<AudioPreviewState>()
        .seek(1.25);
    app.world_mut()
        .resource_mut::<AudioPreviewState>()
        .next_feedback = Instant::now();
    app.update();
    {
        let mut media = app.world_mut().resource_mut::<AssetPreviewWorkspace>();
        let old = media.expanded.take().unwrap();
        assert_eq!(old.audio_position, 1.25);
        media.parked.push(old);
        media.expanded = Some(tab(2, "second.wav"));
    }
    select(&mut app, "second.wav");
    settle(&mut app);
    assert_eq!(app.world().resource::<AudioPreviewState>().position, 0.0);
    app.world_mut()
        .resource_mut::<AudioPreviewState>()
        .seek(2.5);
    app.world_mut()
        .resource_mut::<AudioPreviewState>()
        .next_feedback = Instant::now();
    app.update();
    {
        let mut media = app.world_mut().resource_mut::<AssetPreviewWorkspace>();
        let old = media.expanded.take().unwrap();
        assert_eq!(old.audio_position, 2.5);
        media.expanded = Some(media.parked.remove(0));
        media.parked.push(old);
    }
    select(&mut app, "first.wav");
    settle(&mut app);
    assert_eq!(app.world().resource::<AudioPreviewState>().position, 1.25);
    assert_eq!(
        app.world().resource::<AssetPreviewWorkspace>().parked[0].audio_position,
        2.5
    );
    action(&mut app, AudioAction::Toggle);
    app.world_mut()
        .resource_mut::<VegetationPresetEditorState>()
        .active_tab = PresetWorkspaceTab::Map;
    app.update();
    assert!(!app.world().resource::<AudioPreviewState>().playing);
    app.world_mut()
        .resource_mut::<VegetationPresetEditorState>()
        .active_tab = PresetWorkspaceTab::AssetPreview;
    app.update();
    assert!(!app.world().resource::<AudioPreviewState>().playing);
}

#[test]
fn audio_session_same_path_under_new_asset_root_cannot_inherit_old_tab_position() {
    let first = Fixture::new();
    let second = Fixture::new();
    first.write("sound.wav", &wav(32, 8));
    second.write("sound.wav", &wav(48, 8));
    let mut app = app(&first, "sound.wav");
    app.world_mut()
        .resource_mut::<AssetPreviewWorkspace>()
        .expanded = Some(tab(1, "sound.wav"));
    app.world_mut()
        .resource_mut::<VegetationPresetEditorState>()
        .active_tab = PresetWorkspaceTab::AssetPreview;
    settle(&mut app);
    app.world_mut()
        .resource_mut::<AudioPreviewState>()
        .seek(2.0);
    app.world_mut()
        .resource_mut::<AudioPreviewState>()
        .next_feedback = Instant::now();
    app.update();
    assert_eq!(
        app.world()
            .resource::<AssetPreviewWorkspace>()
            .expanded
            .as_ref()
            .unwrap()
            .audio_position,
        2.0
    );
    app.world_mut()
        .resource_mut::<DocumentState>()
        .set_asset_root_for_tests(second.0.clone());
    let old_key = app.world().resource::<AudioPreviewState>().key.clone();
    let button = app
        .world_mut()
        .spawn((
            AudioButton {
                action: AudioAction::Toggle,
                key: old_key,
            },
            feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed,
        ))
        .id();
    // 先独立运行输入门，排除drive换代后的playing=false掩盖旧根按钮误接受。
    app.world_mut().run_system_once(control::input).unwrap();
    assert!(!app.world().resource::<AudioPreviewState>().playing);
    app.world_mut().entity_mut(button).despawn();
    settle(&mut app);
    let state = app.world().resource::<AudioPreviewState>();
    assert_eq!(state.key.as_ref().unwrap().root, second.0);
    assert_eq!(state.position, 0.0);
    assert_eq!(state.data.as_ref().unwrap().duration(), 6.0);
    assert!(!state.playing);
}

#[test]
fn audio_session_sidebar_browsing_parked_audio_restores_only_its_bookmark() {
    let fixture = Fixture::new();
    fixture.write("first.wav", &wav(32, 8));
    fixture.write("second.wav", &wav(48, 8));
    let mut app = app(&fixture, "second.wav");
    {
        let mut media = app.world_mut().resource_mut::<AssetPreviewWorkspace>();
        let mut first = tab(1, "first.wav");
        first.audio_position = 1.25;
        let mut second = tab(2, "second.wav");
        second.audio_position = 2.5;
        media.expanded = Some(first);
        media.parked.push(second);
    }
    settle(&mut app);
    assert_eq!(app.world().resource::<AudioPreviewState>().position, 2.5);
    app.world_mut()
        .resource_mut::<AudioPreviewState>()
        .seek(3.0);
    app.world_mut()
        .resource_mut::<AudioPreviewState>()
        .next_feedback = Instant::now();
    app.update();
    let media = app.world().resource::<AssetPreviewWorkspace>();
    assert_eq!(media.expanded.as_ref().unwrap().audio_position, 1.25);
    assert_eq!(media.parked[0].audio_position, 3.0);
    select(&mut app, "first.wav");
    settle(&mut app);
    assert_eq!(app.world().resource::<AudioPreviewState>().position, 1.25);
    assert_eq!(
        app.world().resource::<AssetPreviewWorkspace>().parked[0].audio_position,
        3.0
    );
}

#[test]
fn audio_session_slider_volume_is_session_only_and_input_guard_discards_press() {
    let fixture = Fixture::new();
    let bytes = wav(32, 8);
    fixture.write("sound.wav", &bytes);
    let mut app = app(&fixture, "sound.wav");
    settle(&mut app);
    let editor_before = format!("{:?}", app.world().resource::<MapEditorState>());
    let key = app.world().resource::<AudioPreviewState>().key.clone();
    let track = app
        .world_mut()
        .spawn((
            AudioTrack {
                field: SliderField::Volume,
                key: key.clone(),
            },
            ComputedNode {
                size: Vec2::new(100.0, 16.0),
                ..default()
            },
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::new(-0.3, 0.0)),
            },
            UiInteraction::Hovered,
        ))
        .id();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    assert!((app.world().resource::<AudioPreviewState>().volume - 0.2).abs() < 0.0001);
    assert!(app.world().resource::<AudioPreviewState>().owns_mouse());
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.update();
    assert!(app.world().resource::<AudioPreviewState>().drag.is_none());
    app.world_mut().entity_mut(track).despawn();
    app.world_mut()
        .resource_mut::<MapEditorNumericScrubMouseGuard>()
        .advance(true, true, false);
    press(&mut app, AudioAction::Toggle, key);
    assert!(!app.world().resource::<AudioPreviewState>().playing);
    app.world_mut()
        .resource_mut::<MapEditorNumericScrubMouseGuard>()
        .advance(false, false, false);
    app.update();
    assert!(!app.world().resource::<AudioPreviewState>().playing);
    assert_eq!(
        format!("{:?}", app.world().resource::<MapEditorState>()),
        editor_before
    );
    assert!(
        app.world()
            .resource::<VegetationPresetEditorState>()
            .tabs
            .is_empty()
    );
    assert!(
        !app.world()
            .resource::<DocumentState>()
            .has_unsaved_draft("sound.wav")
    );
    assert_eq!(std::fs::read(fixture.0.join("sound.wav")).unwrap(), bytes);
}

#[test]
fn audio_io_rejects_escape_directory_and_cancel_without_changing_source() {
    let fixture = Fixture::new();
    let bytes = wav(16, 8);
    fixture.write("inside.wav", &bytes);
    std::fs::create_dir_all(fixture.0.join("directory.wav")).unwrap();
    let mut bootstrap = App::new();
    bootstrap.add_plugins(MinimalPlugins);
    for path in [
        "../outside.wav",
        "/outside.wav",
        "C:/outside.wav",
        "inside.wav#Scene0",
        "bad\\inside.wav",
        "directory.wav",
        "gone.wav",
        "",
    ] {
        let root = fixture.0.clone();
        let result =
            block_on(IoTaskPool::get().spawn(async move {
                io::load(&root, path, Arc::new(AtomicBool::new(false))).await
            }));
        assert!(result.is_err(), "应拒绝 {path}");
    }
    let root = fixture.0.clone();
    let result = block_on(IoTaskPool::get().spawn(async move {
        io::load(&root, "inside.wav", Arc::new(AtomicBool::new(true))).await
    }));
    assert!(result.is_err());
    assert_eq!(std::fs::read(fixture.0.join("inside.wav")).unwrap(), bytes);
}
