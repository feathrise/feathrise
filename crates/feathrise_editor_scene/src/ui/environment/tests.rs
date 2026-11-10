//! 使用真实环境页、原二十四个数值根与世界控件验证作用域；不读写用户配置。

use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
use bevy::prelude::*;
use bevy::ui::{
    ComputedUiRenderTargetInfo, ComputedUiTargetCamera, ui_layout_system, ui_surface::UiSurface,
    update::propagate_ui_target_cameras,
};
use feathrise_atmosphere::{
    AuthoredSkyboxPhase, AuthoredSkyboxStatus, WeatherDebugOverride, WeatherState, WorldClock,
    WorldEnvironmentModifiers,
};
use feathrise_editor_ui::*;

use super::{components::*, sync_editor_environment_text, types::*};
use crate::authoring::*;
use crate::components::{SceneEditorActionButton, SceneEditorCamera, SceneEditorRightPanelScroll};
use crate::resources::SceneEditorState;
use crate::ui::sidebars::{EditorRightSidebarTab, SceneEditorSidebarState};
use crate::world_environment_controls::*;

#[derive(Resource)]
struct TestFont(Handle<Font>);

fn page() -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        bevy::scene::ScenePlugin,
        bevy::text::TextPlugin,
        bevy::transform::TransformPlugin,
        EditorUiPlugin,
    ));
    app.init_resource::<UiSurface>()
        .insert_resource(UiScale(1.0))
        .init_resource::<feathrise_editor_core::EditorSettings>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<WorldClock>()
        .init_resource::<WorldEnvironmentModifiers>()
        .init_resource::<WeatherDebugOverride>()
        .init_resource::<WeatherState>()
        .init_resource::<CelestialNumericInputState>()
        .init_resource::<CelestialSectionState>()
        .init_resource::<EnvironmentNumericInputState>()
        .init_resource::<GlobalEnvironmentSaveTask>()
        .add_message::<NumericInputSetValue>();
    app.init_resource::<SceneEditorSidebarState>();
    app.world_mut()
        .resource_mut::<SceneEditorSidebarState>()
        .right = EditorRightSidebarTab::Environment;
    let mut draft = test_heightfield_draft(vec![0; 121].into());
    draft.manifest.display_name = "环境测试场景".into();
    draft.manifest.environment.skybox_path =
        Some("skyboxes/长中文目录/用于环境加载测试的天空Cube.ktx2".into());
    let scene_id = draft.scene_id.clone();
    app.insert_resource(SceneEditorState {
        open: true,
        selected_scene_id: Some(scene_id),
        ..default()
    })
    .insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        ..default()
    })
    .insert_resource(GlobalEnvironmentAuthoringState {
        initialized: true,
        ..default()
    });
    app.world_mut().resource_mut::<WorldClock>().set_hour(12.0);
    app.add_systems(Startup, super::scene::environment_page.spawn());
    app.add_systems(
        Update,
        (
            setup_environment_numeric_inputs,
            setup_celestial_numeric_inputs,
            setup_world_environment_controls,
            setup_world_environment_slider_layout,
            sync_environment_numeric_inputs,
            sync_celestial_controls,
            sync_world_environment_controls,
            sync_editor_environment_text,
        )
            .chain()
            .after(EditorUiSystemSet::NumericInput),
    );
    app.add_plugins((
        HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(PostUpdate),
        HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(PostUpdate),
    ));
    app.configure_sets(
        PostUpdate,
        PropagateSet::<ComputedUiRenderTargetInfo>::default()
            .after(propagate_ui_target_cameras)
            .before(bevy::ui::widget::measure_text_system),
    );
    app.add_systems(
        PostUpdate,
        (
            propagate_ui_target_cameras,
            bevy::ui::widget::measure_text_system.after(bevy::text::detect_text_needs_rerender),
            ui_layout_system,
        )
            .chain(),
    );
    let font = app
        .world_mut()
        .resource_mut::<Assets<Font>>()
        .add(Font::from_bytes(
            feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec(),
        ));
    app.insert_resource(TestFont(font));
    app.world_mut().spawn((
        Camera2d,
        Camera {
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo {
                    physical_size: UVec2::splat(4096),
                    scale_factor: 1.0,
                }),
                ..default()
            },
            viewport: Some(Viewport {
                physical_size: UVec2::splat(4096),
                ..default()
            }),
            ..default()
        },
    ));
    app.update();
    let world = app.world_mut();
    let root = world
        .query_filtered::<Entity, With<SceneEditorRightPanelScroll>>()
        .single(world)
        .unwrap();
    let host = world
        .spawn(Node {
            width: px(340),
            height: px(400),
            min_width: px(0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .id();
    world.entity_mut(root).insert(ChildOf(host));
    world.get_mut::<Node>(root).unwrap().display = Display::Flex;
    settle(&mut app);
    (app, host)
}

fn settle(app: &mut App) {
    for _ in 0..6 {
        app.update();
        let font = app.world().resource::<TestFont>().0.clone();
        let world = app.world_mut();
        for mut text in world.query::<&mut TextFont>().iter_mut(world) {
            if text.font != font.clone().into() {
                text.font = font.clone().into();
            }
        }
    }
    app.update();
}

fn text(app: &mut App, field: EnvironmentTextField) -> String {
    let world = app.world_mut();
    world
        .query::<(&EnvironmentSidebarText, &Text)>()
        .iter(world)
        .find(|(marker, _)| marker.0 == field)
        .unwrap()
        .1
        .0
        .clone()
}

fn scene_display(app: &mut App) -> Display {
    let world = app.world_mut();
    world
        .query_filtered::<&Node, With<EnvironmentMapControls>>()
        .single(world)
        .unwrap()
        .display
}

fn author(app: &App) -> (String, String, String, usize, usize, u64, u64, bool, bool) {
    let world = app.world();
    let session = world.resource::<EditorAuthoringSession>();
    let global = world.resource::<GlobalEnvironmentAuthoringState>();
    let settings = world.resource::<feathrise_editor_core::EditorSettings>();
    let settings_snapshot = format!(
        "{}|{}|{}|{}|{}|{}",
        settings.environment.world_time_paused,
        settings.environment.grass_wind_tip_strength,
        settings
            .scene_editor_scrub_sensitivities
            .get(feathrise_editor_core::SceneEditorScrubField::SkyboxBrightness),
        settings
            .scene_editor_scrub_sensitivities
            .get(feathrise_editor_core::SceneEditorScrubField::EnvironmentDiffuseIntensity),
        settings
            .scene_editor_scrub_sensitivities
            .get(feathrise_editor_core::SceneEditorScrubField::EnvironmentSpecularIntensity),
        settings
            .scene_editor_scrub_sensitivities
            .get(feathrise_editor_core::SceneEditorScrubField::SkyboxYaw)
    );
    (
        serde_json::to_string(&session.draft.as_ref().map(|draft| &draft.manifest)).unwrap(),
        serde_json::to_string(&global.draft).unwrap(),
        settings_snapshot,
        session.undo.len(),
        session.redo.len(),
        session.revision,
        session.environment_revision,
        global.dirty,
        world.resource::<SceneEditorState>().dirty,
    )
}

#[test]
fn environment_domains_keep_one_scroll_host_and_original_controls() {
    let (mut app, _) = page();
    let before = author(&app);
    let world = app.world_mut();
    assert_eq!(
        world
            .query_filtered::<Entity, With<SceneEditorRightPanelScroll>>()
            .iter(world)
            .count(),
        1
    );
    assert_eq!(
        world
            .query_filtered::<Entity, With<NumericInputRoot>>()
            .iter(world)
            .count(),
        24
    );
    for label in [
        "当前场景 · 持久参数",
        "天空与环境光",
        "世界调试 · 当前运行",
        "全局默认 · 所有动态场景",
    ] {
        assert_eq!(
            world
                .query::<&Text>()
                .iter(world)
                .filter(|text| text.as_str() == label)
                .count(),
            1
        );
    }
    assert_eq!(world.query::<(&super::components::EnvironmentSidebarText, &crate::ui::status_bar::PanelStatusText)>()
        .iter(world).filter(|(field, _)| field.0 == super::types::EnvironmentTextField::SceneFeedback).count(), 1,
        "原场景反馈实体保留，改由底栏统一呈现");
    for action in [
        crate::types::EditorAction::SkyModeDynamic,
        crate::types::EditorAction::SkyModeSkybox,
        crate::types::EditorAction::SetSelectedSkybox,
        crate::types::EditorAction::ClearSkybox,
    ] {
        assert_eq!(
            world
                .query::<&SceneEditorActionButton>()
                .iter(world)
                .filter(|marker| marker.0 == action)
                .count(),
            1
        );
    }
    assert_eq!(scene_display(&mut app), Display::Flex);
    settle(&mut app);
    assert_eq!(author(&app), before);
}

#[test]
fn environment_empty_loading_and_stale_scenes_hide_old_parameters() {
    let (mut app, _) = page();
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_scene_id = None;
    settle(&mut app);
    assert_eq!(
        text(&mut app, EnvironmentTextField::SceneContext),
        "未进入场景"
    );
    assert_eq!(scene_display(&mut app), Display::None);
    assert!(!text(&mut app, EnvironmentTextField::SceneSource).contains("Cube.ktx2"));
    assert!(text(&mut app, EnvironmentTextField::GlobalFeedback).contains("全局来源已同步"));

    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_scene_id = Some(feathrise_scene_content::SceneId("other-map".into()));
    settle(&mut app);
    assert_eq!(
        text(&mut app, EnvironmentTextField::SceneContext),
        "等待场景草稿"
    );
    assert_eq!(scene_display(&mut app), Display::None);
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .scene_transition_in_progress = true;
    settle(&mut app);
    assert_eq!(
        text(&mut app, EnvironmentTextField::SceneContext),
        "场景加载中"
    );
    assert!(!text(&mut app, EnvironmentTextField::SceneFeedback).contains("已就绪"));
}

#[test]
fn environment_fixed_scenes_do_not_offer_dynamic_overrides() {
    let (mut app, _) = page();
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft
        .as_mut()
        .unwrap()
        .manifest
        .runtime_scene = "KaifengScene".into();
    let before = author(&app);
    settle(&mut app);
    assert_eq!(scene_display(&mut app), Display::None);
    assert!(text(&mut app, EnvironmentTextField::SceneHint).contains("固定场景"));
    assert!(text(&mut app, EnvironmentTextField::CurrentValues).is_empty());
    assert_eq!(author(&app), before);
}

#[test]
fn environment_modes_separate_sources_computed_values_and_loader_evidence() {
    let (mut app, _) = page();
    assert!(text(&mut app, EnvironmentTextField::SceneSource).contains("场景气候（只读）"));
    assert!(text(&mut app, EnvironmentTextField::SceneSource).contains("亮度仅为切换保留"));
    assert!(text(&mut app, EnvironmentTextField::CurrentValues).contains("昼夜与天气计算值"));
    assert!(!text(&mut app, EnvironmentTextField::CurrentValues).contains("天空盒 "));
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft
        .as_mut()
        .unwrap()
        .manifest
        .environment
        .sky_mode = feathrise_scene_content::SceneSkyMode::Skybox;
    settle(&mut app);
    assert!(text(&mut app, EnvironmentTextField::SceneSource).contains("主Cube"));
    assert!(text(&mut app, EnvironmentTextField::CurrentValues).contains("天空盒 "));
    assert!(!text(&mut app, EnvironmentTextField::SceneFeedback).contains("已就绪"));
}

#[test]
fn environment_ibl_phases_require_current_path_and_revision() {
    let (mut app, _) = page();
    let session = app.world().resource::<EditorAuthoringSession>();
    let path = session
        .draft
        .as_ref()
        .unwrap()
        .manifest
        .environment
        .skybox_path
        .clone();
    let revision = session.environment_revision;
    let camera = app
        .world_mut()
        .spawn((
            SceneEditorCamera,
            AuthoredSkyboxStatus {
                phase: AuthoredSkyboxPhase::Loading,
                asset_path: path,
                revision,
                detail: "等待两张环境纹理".into(),
                ..default()
            },
        ))
        .id();
    for (phase, detail) in [
        (AuthoredSkyboxPhase::Loading, "等待两张环境纹理"),
        (AuthoredSkyboxPhase::Ready, "正式资源组已完整采用"),
        (
            AuthoredSkyboxPhase::Failed,
            "缺少完整中文路径的_specular.ktx2",
        ),
    ] {
        let mut status = app
            .world_mut()
            .get_mut::<AuthoredSkyboxStatus>(camera)
            .unwrap();
        status.phase = phase;
        status.detail = detail.into();
        settle(&mut app);
        let value = text(&mut app, EnvironmentTextField::SceneFeedback);
        assert!(value.contains(phase.display_name()));
        assert!(value.contains(detail));
    }
    app.world_mut()
        .get_mut::<AuthoredSkyboxStatus>(camera)
        .unwrap()
        .revision = revision.wrapping_add(1);
    settle(&mut app);
    assert!(text(&mut app, EnvironmentTextField::SceneFeedback).contains("等待当前请求"));
    assert!(!text(&mut app, EnvironmentTextField::SceneFeedback).contains("缺少"));
    let mut status = app
        .world_mut()
        .get_mut::<AuthoredSkyboxStatus>(camera)
        .unwrap();
    status.revision = revision;
    status.asset_path = Some("skyboxes/other.ktx2".into());
    settle(&mut app);
    assert!(!text(&mut app, EnvironmentTextField::SceneFeedback).contains("失败"));
}

#[test]
fn environment_global_errors_and_dirty_saving_feedback_keep_separate_scope() {
    let (mut app, _) = page();
    let mut global = app
        .world_mut()
        .resource_mut::<GlobalEnvironmentAuthoringState>();
    global.initialized = false;
    settle(&mut app);
    assert!(text(&mut app, EnvironmentTextField::GlobalFeedback).contains("暂用安全默认值"));
    let mut global = app
        .world_mut()
        .resource_mut::<GlobalEnvironmentAuthoringState>();
    global.load_error = Some("全局环境文件损坏，保留安全默认值".into());
    global.dirty = true;
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft
        .as_mut()
        .unwrap()
        .environment_dirty = true;
    settle(&mut app);
    assert!(text(&mut app, EnvironmentTextField::GlobalFeedback).contains("文件损坏"));
    assert!(text(&mut app, EnvironmentTextField::GlobalFeedback).contains("全局默认未保存"));
    assert!(text(&mut app, EnvironmentTextField::SceneFeedback).contains("场景环境未保存"));
    assert!(!text(&mut app, EnvironmentTextField::SceneFeedback).contains("全局环境文件"));
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .saving = true;
    settle(&mut app);
    assert!(text(&mut app, EnvironmentTextField::GlobalFeedback).contains("正在保存全局"));
    assert!(text(&mut app, EnvironmentTextField::SceneFeedback).contains("正在保存场景"));
}

#[derive(Resource, Default)]
struct Writes(usize);

fn observe_writes(
    mut writes: ResMut<Writes>,
    texts: Query<
        Entity,
        (
            With<EnvironmentSidebarText>,
            Or<(Changed<Text>, Changed<Node>, Changed<TextColor>)>,
        ),
    >,
    controls: Query<Entity, (With<EnvironmentMapControls>, Changed<Node>)>,
) {
    writes.0 += texts.iter().count() + controls.iter().count();
}

#[test]
fn environment_stable_clock_and_unrelated_editor_changes_do_not_rewrite_fields() {
    let (mut app, _) = page();
    app.init_resource::<Writes>()
        .add_systems(Last, observe_writes);
    app.update();
    app.world_mut().resource_mut::<Writes>().0 = 0;
    let before = author(&app);
    app.world_mut().resource_mut::<SceneEditorState>().status = "其他工具的光标状态".into();
    app.world_mut()
        .resource_mut::<WorldClock>()
        .set_hour(12.000_01);
    app.update();
    app.update();
    assert_eq!(app.world().resource::<Writes>().0, 0);
    assert_eq!(author(&app), before);
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft
        .as_mut()
        .unwrap()
        .manifest
        .environment
        .celestial_overrides
        .day_base_illuminance_lux = Some(12345.0);
    app.update();
    assert_eq!(app.world().resource::<Writes>().0, 0); // 天体数值变化不进入来源摘要的签名。
    app.world_mut().resource_mut::<WorldClock>().set_hour(0.0);
    app.update();
    assert!(app.world().resource::<Writes>().0 > 0);
    assert!(text(&mut app, EnvironmentTextField::CurrentValues).contains("环境昼夜系数 0%"));
}

#[test]
fn environment_hidden_page_defers_readonly_feedback_until_reactivated() {
    let (mut app, _) = page();
    let previous = text(&mut app, EnvironmentTextField::SceneContext);
    app.world_mut()
        .resource_mut::<SceneEditorSidebarState>()
        .right = EditorRightSidebarTab::Assets;
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_scene_id = None;
    let before = author(&app);
    app.update();
    assert_eq!(text(&mut app, EnvironmentTextField::SceneContext), previous);
    app.world_mut()
        .resource_mut::<SceneEditorSidebarState>()
        .right = EditorRightSidebarTab::Environment;
    app.update();
    assert_eq!(
        text(&mut app, EnvironmentTextField::SceneContext),
        "未进入场景"
    );
    assert_eq!(author(&app), before);
}

#[path = "layout_tests.rs"]
mod layout_tests;
