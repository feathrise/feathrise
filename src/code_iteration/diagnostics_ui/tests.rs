//! 诊断行使用真实源位置和释放脉冲；验证滚动换绑拒收、窗口外键盘归属和布局。

use super::{components::DiagnosticRow, resources::DiagnosticsOverlayState, systems};
use super::super::{resources::CodeIterationState, types::CodeIterationPhase};
use crate::compiler_diagnostics::{CompilerDiagnostic, DiagnosticLevel, SourceLocation};
use bevy::{input::mouse::MouseWheel, input_focus::InputFocus, prelude::*};
use feathrise_editor_scene::standalone::OpenProjectSourceLocation;
use std::sync::Arc;

fn state() -> CodeIterationState {
    CodeIterationState {
        project_id: "diagnostic-ui-fixture".into(),
        project_root: std::env::temp_dir().join("feathrise-diagnostic-ui-fixture"),
        source_context: 0,
        diagnostics: Arc::new(vec![CompilerDiagnostic { level: DiagnosticLevel::Error,
            message: "找不到变量 中文".into(), code: Some("E0425".into()),
            location: Some(SourceLocation { path: "src/main.rs".into(), line: 12, column: 8 }) }]),
        diagnostic_revision: 7, diagnostics_open: true, phase: CodeIterationPhase::Failed,
        status: "编译失败，原运行实例保持".into(), generation: 1, pending: None, applying: None, applied: None,
    }
}

fn row() -> DiagnosticRow { DiagnosticRow { slot: 0, index: Some(0), revision: 7, window_revision: 3 } }
fn view() -> DiagnosticsOverlayState {
    DiagnosticsOverlayState { revision: Some(7), first: 0,
        scroll: Vec2::ZERO, window_revision: 3 }
}

#[test]
fn diagnostic_rows_reject_old_result_revision_or_any_unadopted_scroll() {
    let mut state = state();
    let view = view();
    assert!(systems::current(&row(), &state, &view, Vec2::ZERO));
    assert!(!systems::current(&row(), &state, &view, Vec2::new(0., 44.)));
    state.diagnostic_revision += 1;
    assert!(!systems::current(&row(), &state, &view, Vec2::ZERO));
}

#[test]
fn clicking_real_diagnostic_location_closes_panel_and_dispatches_once() {
    let mut app = App::new();
    app.insert_resource(state()).insert_resource(view()).init_resource::<InputFocus>()
        .init_resource::<ButtonInput<KeyCode>>().add_message::<MouseWheel>()
        .add_message::<OpenProjectSourceLocation>().add_systems(Update, systems::control);
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
    app.update();
    assert!(app.world().resource::<CodeIterationState>().diagnostics_open, "浮窗外的 Esc 仍归工作台");
    assert!(app.world().resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::Escape));

    app.world_mut().spawn((row(), feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
    assert!(!app.world().resource::<CodeIterationState>().diagnostics_open);
    let messages = app.world().resource::<Messages<OpenProjectSourceLocation>>();
    let mut cursor = bevy::ecs::message::MessageCursor::default();
    let requests: Vec<_> = cursor.read(messages).collect();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, std::path::PathBuf::from("src/main.rs"));
    assert_eq!((requests[0].line, requests[0].column, requests[0].context), (12, 8, 0));
    app.update();
    assert_eq!(cursor.read(app.world().resource::<Messages<OpenProjectSourceLocation>>()).count(), 0);
}

#[test]
fn diagnostic_summary_bounds_visible_glyphs_but_keeps_source_position_separate() {
    let mut diagnostic = state().diagnostics[0].clone();
    diagnostic.message = "中文".repeat(1024);
    let summary = systems::row_text(&diagnostic, true, feathrise_editor_ui::localization::EditorLocale::Chinese);
    assert!(summary.len() <= 1027);
    assert!(summary.ends_with('…'));
    let position = systems::row_text(&diagnostic, false, feathrise_editor_ui::localization::EditorLocale::Chinese);
    assert!(position.contains("E0425"));
    assert!(position.contains("main.rs:12:8"));
    assert_eq!(diagnostic.message.len(), 6144);
}

/// 沿用工程日志的无系统窗口装配：真实 TextPlugin + UiPlugin 计算字体和 flex 布局。
/// 不创建 GPU 框架，不手写 ComputedNode 尺寸来替代这次故障涉及的实际空间分配。
fn layout_app() -> App {
    use bevy::{camera::{ComputedCameraValues, RenderTargetInfo},
        input_focus::{InputDispatchPlugin, InputFocusPlugin},
        ui_widgets::ScrollbarPlugin, window::PrimaryWindow};
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin { file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/assets").into(),
            watch_for_changes_override: Some(false), ..default() },
        bevy::scene::ScenePlugin,
        bevy::input::InputPlugin,
        InputFocusPlugin,
        InputDispatchPlugin,
        bevy::picking::PickingPlugin,
        bevy::picking::InteractionPlugin,
        bevy::transform::TransformPlugin,
        bevy::text::TextPlugin,
        feathrise_runtime::DefaultFontPlugin,
        bevy::ui::UiPlugin,
        ScrollbarPlugin,
        feathrise_editor_ui::EditorButtonInputPlugin,
    ))
        // UiPlugin 负责布局，层级 InheritedVisibility 仍须沿用生产可见性传播；
        // 同项目其他无GPU布局夹具也安装这对插件，不能手写 VISIBLE 替代真实传播。
        .add_plugins((bevy::mesh::MeshPlugin, bevy::camera::visibility::VisibilityPlugin))
        .init_asset::<Image>().init_asset::<bevy::image::TextureAtlasLayout>()
        .add_message::<bevy::window::Ime>().add_message::<OpenProjectSourceLocation>()
        .insert_resource(state())
        .add_systems(Update, use_default_test_font.after(super::scene::mount).before(systems::present));
    super::install(&mut app);
    app.world_mut().spawn((Window::default(), PrimaryWindow));
    let camera = app.world_mut().spawn((Camera2d, Camera { computed: ComputedCameraValues {
        target_info: Some(RenderTargetInfo { physical_size: UVec2::new(1280, 720), scale_factor: 1. }), ..default()
    }, ..default() })).id();
    app.world_mut().spawn((feathrise_editor_scene::workbench::WorkbenchHeader, bevy::ui::UiTargetCamera(camera),
        Node { width: percent(100), height: percent(100), ..default() }));
    app
}

fn use_default_test_font(mut fonts: Query<&mut TextFont>) {
    for mut font in &mut fonts {
        let value = Handle::<Font>::default().into();
        if font.font != value { font.font = value; }
    }
}

fn bounds(app: &App, entity: Entity) -> Rect {
    Rect::from_center_size(app.world().get::<UiGlobalTransform>(entity).unwrap().translation,
        app.world().get::<ComputedNode>(entity).unwrap().size())
}

#[test]
fn multiline_ansi_failure_keeps_one_status_line_and_three_actual_visible_diagnostics() {
    use super::components::{DiagnosticRow, DiagnosticsPane, DiagnosticsStatus};
    let mut app = layout_app();
    let raw = format!("工程构建失败（退出码101）\n{}\x1b[1;31merror\x1b[0m: invalid value\n",
        "\x1b[1;96mBuilding\x1b[0m [========>] 527/529\r\n".repeat(80));
    {
        let mut state = app.world_mut().resource_mut::<CodeIterationState>();
        state.status = raw.clone();
        let diagnostic = state.diagnostics[0].clone();
        state.diagnostics = Arc::new((0..3).map(|index| CompilerDiagnostic {
            message: format!("真实诊断{index}：不能找到变量"), ..diagnostic.clone()
        }).collect());
    }
    for _ in 0..16 { app.update(); }
    let status = {
        let world = app.world_mut();
        world.query_filtered::<Entity, With<DiagnosticsStatus>>().single(world).unwrap()
    };
    let pane = {
        let world = app.world_mut();
        world.query_filtered::<Entity, With<DiagnosticsPane>>().single(world).unwrap()
    };
    let text = app.world().get::<Text>(status).unwrap().as_str();
    assert_eq!(text, "代码构建或应用失败 · 完整输出见日志");
    assert!(!text.chars().any(char::is_control));
    assert!(!app.world().get::<bevy::text::TextLayoutInfo>(status).unwrap().glyphs.is_empty(), "短状态必须实际排版字形");
    assert!(bounds(&app, status).height() <= 20.1, "真实flex布局中的状态只能占一行");
    let pane_bounds = bounds(&app, pane);
    assert!(pane_bounds.height() >= 3. * super::resources::ROW_HEIGHT, "错误上下文不能把三条诊断的空间挤没：{pane_bounds:?}");
    let rows: Vec<_> = {
        let world = app.world_mut();
        world.query::<(Entity, &DiagnosticRow, &InheritedVisibility)>().iter(world)
            .filter(|(_, row, _)| row.index.is_some_and(|index| index < 3))
            .map(|(entity, row, visible)| (entity, row.index.unwrap(), visible.get())).collect()
    };
    assert_eq!(rows.len(), 3);
    for (entity, index, visible) in rows {
        let row = bounds(&app, entity);
        assert!(visible && row.height() >= super::resources::ROW_HEIGHT - 0.1
            && row.min.y >= pane_bounds.min.y - 0.1 && row.max.y <= pane_bounds.max.y + 0.1,
            "诊断{index}必须有实际可见行框：visible={visible} {row:?}/{pane_bounds:?}");
    }
    assert_eq!(app.world().resource::<CodeIterationState>().status, raw, "呈现摘要不能覆盖完整失败原因");
    let root = {
        let world = app.world_mut();
        world.query_filtered::<Entity, With<feathrise_editor_scene::workbench::WorkbenchHeader>>().single(world).unwrap()
    };
    // 同一个真实层级隐藏/恢复后子行必须跟随，证明上面的可见断言来自传播系统，而非夹具预置值。
    for (visibility, expected) in [(Visibility::Hidden, false), (Visibility::Inherited, true)] {
        app.world_mut().entity_mut(root).insert(visibility);
        for _ in 0..2 { app.update(); }
        let world = app.world_mut();
        assert!(world.query::<(&DiagnosticRow, &InheritedVisibility)>().iter(world)
            .filter(|(row, _)| row.index.is_some_and(|index| index < 3))
            .all(|(_, inherited)| inherited.get() == expected), "诊断行应沿原根节点传播显隐");
    }
}
