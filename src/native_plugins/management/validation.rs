//! 插件管理器显式 GPU／子进程验收：只操作本轮 owner 夹具，沿用生产按钮、后台事务与真实 DLL 发现。
use feathrise_editor_ui::UiInteraction;
use super::{components::*, packages, resources::PluginManagerState, types::*};
use crate::project_log::gpu_tests::{assert_dialog_chrome, frame, screenshot};
use bevy::{
    camera::RenderTarget,
    input::{ButtonState, mouse::MouseButtonInput},
    prelude::*,
    render::render_resource::{Extent3d, TextureFormat, TextureUsages},
    window::{CursorLeft, CursorMoved, PrimaryWindow, WindowEvent, WindowRef},
};
use feathrise_editor_scene::workbench::WorkbenchHeader;
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[path = "validation_feedback.rs"]
mod feedback;
#[path = "../notifications_validation.rs"]
mod notification_checks;

#[derive(Resource, Clone)]
struct CursorFixture {
    window: Entity,
    camera: Entity,
    image: Handle<Image>,
    position: Option<Vec2>,
}

/// 反馈与通知子场景同样复现窗口后端双通道，避免只更新ButtonInput却绕过现代拾取。
fn window_input(world: &mut World, input: impl bevy::ecs::message::Message + Clone + Into<WindowEvent>) {
    let event: WindowEvent = input.clone().into();
    world.write_message(input);
    world.write_message(event);
}

fn provide_cursor(world: &mut World) {
    let cursor = world.resource::<CursorFixture>().clone();
    let scale = {
        let mut window = world.get_mut::<Window>(cursor.window).unwrap();
        window.set_physical_cursor_position(cursor.position.map(|p| p.as_dvec2()));
        window.scale_factor()
    };
    // First中的现代拾取只读聚合窗口事件；窗口外坐标同时清除CursorLeft留下的旧悬停。
    let moved = CursorMoved { window: cursor.window,
        position: cursor.position.map_or(Vec2::splat(-1.), |position| position / scale), delta: None };
    world.write_message(moved.clone());
    world.write_message(WindowEvent::CursorMoved(moved));
    if cursor.position.is_none() {
        let left = CursorLeft { window: cursor.window };
        world.write_message(left.clone());
        world.write_message(WindowEvent::CursorLeft(left));
    }
    world
        .entity_mut(cursor.camera)
        .insert(RenderTarget::Window(WindowRef::Entity(cursor.window)));
}

fn restore_target(world: &mut World) {
    let cursor = world.resource::<CursorFixture>().clone();
    world
        .entity_mut(cursor.camera)
        .insert(RenderTarget::from(cursor.image));
}

/// 精简夹具使用与正式编辑器相同的 SDK 内嵌中文字体，不读取用户字体或偏好。
fn use_editor_font(mut fonts: Query<&mut TextFont>) {
    for mut font in &mut fonts {
        if font.font != Handle::<Font>::default().into() {
            font.font = Handle::<Font>::default().into();
        }
    }
}

fn entity<M: Component>(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<M>>()
        .single(world)
        .unwrap()
}

fn bounds(app: &App, target: Entity) -> Rect {
    Rect::from_center_size(
        app.world()
            .get::<UiGlobalTransform>(target)
            .unwrap()
            .translation,
        app.world().get::<ComputedNode>(target).unwrap().size(),
    )
}

fn settle(app: &mut App) {
    for _ in 0..12 {
        frame(app);
    }
}

fn wait_for_idle(app: &mut App) {
    let started = Instant::now();
    while app.world().resource::<PluginManagerState>().busy {
        frame(app);
        assert!(
            started.elapsed() < Duration::from_secs(45),
            "插件管理后台任务没有结束"
        );
    }
    settle(app);
}

/// 提供官方 Window 鼠标来源，原 Focus 与共享按下／释放协议负责产生按钮点击。
/// 不直接写 Interaction、成功状态、计划列表或文件回执。
fn click(app: &mut App, matches: impl Fn(&ManagerAction) -> bool) {
    settle(app);
    let target = {
        let world = app.world_mut();
        world
            .query::<(
                Entity,
                &ManagerAction,
                &Node,
                &ComputedNode,
                &InheritedVisibility,
            )>()
            .iter(world)
            .find_map(|(entity, action, node, computed, visible)| {
                (matches(action)
                    && node.display != Display::None
                    && visible.get()
                    && !computed.is_empty())
                .then_some(entity)
            })
            .expect("必须存在当前实际可见的生产插件管理按钮")
    };
    assert!(
        app.world()
            .get::<bevy::ui::InteractionDisabled>(target)
            .is_none(),
        "生产动作不能处于禁用状态"
    );
    let center = bounds(app, target).center();
    app.world_mut().resource_mut::<CursorFixture>().position = Some(center);
    settle(app);
    assert_eq!(
        *app.world().get::<UiInteraction>(target).unwrap(),
        UiInteraction::Hovered
    );
    let window = app.world().resource::<CursorFixture>().window;
    let pressed = MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Pressed,
        window,
    };
    // 同真实窗口后端一样向InputPlugin与现代拾取分别投递对应的官方消息。
    app.world_mut().write_message(pressed);
    app.world_mut().write_message(WindowEvent::MouseButtonInput(pressed));
    frame(app);
    let released = MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Released,
        window,
    };
    app.world_mut().write_message(released);
    app.world_mut().write_message(WindowEvent::MouseButtonInput(released));
    frame(app);
    app.world_mut().resource_mut::<CursorFixture>().position = None;
    wait_for_idle(app);
}

fn entry(app: &App, id: &str) -> PackageEntry {
    app.world()
        .resource::<PluginManagerState>()
        .rows
        .iter()
        .find(|entry| entry.id == id)
        .unwrap()
        .clone()
}

fn owner(root: &Path) -> Value {
    serde_json::from_slice(&fs::read(root.parent().unwrap().join("owner.json")).unwrap()).unwrap()
}

fn assert_owned(root: &Path) {
    let evidence = owner(root);
    assert_eq!(
        PathBuf::from(evidence["project"].as_str().unwrap()),
        root.canonicalize().unwrap()
    );
    assert_eq!(evidence["kind"], "plugin-manager-validation");
    assert_eq!(root.file_name().unwrap(), "fixture");
}

fn write_new(path: &Path, value: &[u8]) {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap()
        .write_all(value)
        .unwrap();
}

fn asset_bytes(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn visit(root: &Path, directory: &Path, files: &mut Vec<(PathBuf, Vec<u8>)>) {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let kind = entry.file_type().unwrap();
            assert!(!kind.is_symlink(), "本轮自建资产夹具不接受符号链接");
            if kind.is_dir() {
                visit(root, &entry.path(), files);
            } else if kind.is_file() {
                files.push((
                    entry.path().strip_prefix(root).unwrap().to_owned(),
                    fs::read(entry.path()).unwrap(),
                ));
            }
        }
    }
    let mut files = Vec::new();
    visit(root, root, &mut files);
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}

fn apply_on_reopen(app: &mut App, root: &Path) {
    assert_owned(root);
    let roots = fixture_roots(root);
    let pending_roots = roots.clone();
    let warnings = std::thread::spawn(move || packages::apply_pending_with_roots(&pending_roots))
        .join()
        .unwrap();
    assert!(warnings.is_empty(), "重开采用失败：{warnings:?}");
    // 重建管理资源模拟新作者实例。真实 DLL 生命周期由独立 helper 进程验证，不能在本进程伪卸载。
    let scope = app.world().resource::<PluginManagerState>().scope;
    let mut manager = PluginManagerState::with_roots(roots.project, roots.global);
    manager.scope = scope;
    let _ = manager.refresh();
    app.insert_resource(manager);
    super::open_from_workspace(app.world_mut());
    wait_for_idle(app);
}

fn fixture_roots(root: &Path) -> PluginRoots {
    assert_owned(root);
    PluginRoots { project: Some(root.to_owned()), global: root.parent().unwrap().join("global-plugins") }
}

fn native_probe(root: &Path, output: &Path, name: &str, version: Option<&str>) -> Value {
    assert_owned(root);
    let log = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join(format!("{name}.log")))
        .unwrap();
    let report = output.join(format!("{name}.json"));
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "native_plugins::management::validation::native_load_helper",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("FEATHRISE_PLUGIN_MANAGER_HELPER_ROOT", root)
        .env("FEATHRISE_PLUGIN_MANAGER_HELPER_REPORT", &report)
        .env(
            "FEATHRISE_PLUGIN_MANAGER_HELPER_LOADED",
            if version.is_some() { "1" } else { "0" },
        )
        .env("FEATHRISE_PLUGIN_MANAGER_HELPER_VERSION", version.unwrap_or_default())
        .stdout(Stdio::from(log.try_clone().unwrap()))
        .stderr(Stdio::from(log))
        .spawn()
        .unwrap();
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if started.elapsed() > Duration::from_secs(40) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("真实插件发现子进程超时：{name}");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(
        status.success(),
        "真实插件发现失败，查看 {}",
        output.join(format!("{name}.log")).display()
    );
    serde_json::from_slice(&fs::read(report).unwrap()).unwrap()
}

#[test]
#[ignore = "仅供自有插件管理GPU夹具启动独立进程，验证DLL退出后才采用文件事务"]
fn native_load_helper() {
    use crate::native_plugins::{resources::NativeTools, systems as native_systems};
    use feathrise_editor_core::{AssetPreviewFocus, WorkspaceDomainState};
    use feathrise_editor_scene::game_authoring::scene_api::{
        resources::SceneEditorState,
        ui::{
            sidebars::SceneEditorSidebarState, vegetation_preset_editor::VegetationPresetEditorState,
        },
    };
    let root = PathBuf::from(
        std::env::var_os("FEATHRISE_PLUGIN_MANAGER_HELPER_ROOT").expect("只由显式插件管理夹具启动"),
    );
    let report = PathBuf::from(std::env::var_os("FEATHRISE_PLUGIN_MANAGER_HELPER_REPORT").unwrap());
    assert_owned(&root);
    assert!(
        report
            .parent()
            .is_some_and(|parent| parent == root.parent().unwrap())
    );
    let expected = std::env::var("FEATHRISE_PLUGIN_MANAGER_HELPER_LOADED").unwrap() == "1";
    let version = std::env::var("FEATHRISE_PLUGIN_MANAGER_HELPER_VERSION").unwrap();
    let project = crate::project::open(&root).unwrap();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(NativeTools::start_with_roots(&project, fixture_roots(&root)))
        .insert_resource(PluginManagerState::with_roots(Some(root.clone()), fixture_roots(&root).global))
        .init_resource::<SceneEditorState>()
        .init_resource::<WorkspaceDomainState>()
        .init_resource::<VegetationPresetEditorState>()
        .init_resource::<bevy::clipboard::Clipboard>()
        .init_resource::<SceneEditorSidebarState>()
        .init_resource::<AssetPreviewFocus>()
        .add_systems(Update, (native_systems::poll, super::systems::control, super::systems::poll).chain());
    // 与产品一致：打开面板后由control等待启动事务与发现完成再刷新，不能提前扫描全局计划。
    super::open_from_workspace(app.world_mut());
    let started = Instant::now();
    // 事件由正式NativeTools轮询系统采用，再由正式管理器轮询投影；夹具不消费或伪造加载事件。
    loop {
        app.update();
        let native = app.world().resource::<NativeTools>();
        let manager = app.world().resource::<PluginManagerState>();
        if !native.discovery_pending && native.loading == 0 && !manager.busy {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "真实DLL发现等待超时"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    app.update();
    let native = app.world().resource::<NativeTools>();
    let manager = app.world().resource::<PluginManagerState>();
    let loaded = native
        .tools
        .values()
        .map(|tool| tool.domain.to_owned())
        .collect::<Vec<_>>();
    let errors = &native.errors;
    assert_eq!(
        loaded.iter().any(|id| id == "example.protocol_probe"),
        expected,
        "实际加载结果与重开计划不符：{loaded:?}/{errors:?}"
    );
    assert!(!errors.is_empty(), "坏包必须明确报错，不能静默消失");
    assert!(
        manager
            .load_states
            .get("broken")
            .is_some_and(|state| state.contains("加载失败")),
        "坏包发现失败必须通过生产投影进入管理器列表：{:?}",
        manager.load_states
    );
    if expected {
        assert_eq!(
            manager.load_states.get("example.protocol_probe").map(String::as_str),
            Some("已加载")
        );
        assert!(
            native
                .tools
                .values()
                .any(|tool| tool.domain == "example.protocol_probe"
                    && tool.snapshot.plugin_id == "example.protocol_probe"
                    && tool.snapshot.status.contains(&version)
                    && tool.fault.is_none()),
            "已加载文字必须来自真实C ABI Open快照和有效后台会话"
        );
    } else if manager.rows.iter().any(|row| row.id == "example.protocol_probe") {
        assert_eq!(
            manager.load_states.get("example.protocol_probe").map(String::as_str),
            Some("已停用")
        );
    } else {
        assert!(
            !manager.load_states.contains_key("example.protocol_probe"),
            "移除后新作者实例不应留下旧工具状态"
        );
    }
    write_new(&report, &serde_json::to_vec_pretty(&json!({"passed":true,"project":root,
        "actual_loaded":loaded,"actual_errors":errors,"expected_runtime_version":version,"discovery_completed":!native.discovery_pending,
        "management_load_states":manager.load_states,"management_initialization_error":manager.initialization_error,
        "process_id":std::process::id(),"source":"NativeTools::start, actual C ABI Open, production native poll and production manager load-state projection in isolated process"})).unwrap());
}

/// 应用提供自己的插件集合与执行依赖，宿主只验证通用旧包迁移、真实加载和作者数据保护。
#[cfg(feathrise_external_validation)]
pub(crate) fn legacy_coexistence_at(output: PathBuf, source: PathBuf, ids: &[&str], cargo_manifest: &str) {
    use crate::native_plugins::{resources::NativeTools, systems as native_systems};
    use feathrise_editor_core::{AssetPreviewFocus, WorkspaceDomainState};
    use feathrise_editor_scene::game_authoring::scene_api::{
        resources::SceneEditorState,
        ui::{sidebars::SceneEditorSidebarState, vegetation_preset_editor::VegetationPresetEditorState},
    };
    assert!(output.is_absolute() && source.is_absolute());
    assert!(!ids.is_empty(), "必须提供实际要验证的插件，不能以空集合报告通过");
    // 独占新目录并保留现场；真实工程和用户全局插件库全程只读或不接触。
    fs::create_dir(&output).unwrap();
    let output = output.canonicalize().unwrap();
    write_new(&output.join("owner.json"), &serde_json::to_vec(&json!({
        "kind":"plugin-migration-diagnostics", "pid":std::process::id(), "root":output,
    })).unwrap());
    let root = output.join("project");
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join("assets")).unwrap();
    fs::create_dir(root.join("plugins")).unwrap();
    fs::write(root.join("assets/author.txt"), b"author-data-must-survive").unwrap();
    fs::write(root.join("Cargo.toml"), cargo_manifest).unwrap();
    fs::write(root.join("editor-project.json"), serde_json::to_vec(&json!({
        "format":1,"id":"migration-fixture","name":"迁移诊断夹具",
        "cargo_manifest":"Cargo.toml","package":"migration_fixture",
    })).unwrap()).unwrap();
    let mut originals = Vec::new();
    for &id in ids {
        let filename = packages::binary_filename(id);
        let binary = source.join(&filename);
        let digest = packages::binary_digest(&binary).unwrap();
        fs::copy(&binary, root.join("plugins").join(filename)).unwrap();
        let legacy = root.join("plugins").join(id);
        fs::create_dir(&legacy).unwrap();
        // 正式发布目录只留单文件；旧格式清单由本轮夹具自建，不依赖用户已清理的旧包。
        let manifest = serde_json::to_vec(&json!({
            "format":1,"id":id,"tool":"旧格式迁移夹具",
        })).unwrap();
        fs::write(legacy.join("plugin.json"), &manifest).unwrap();
        fs::write(legacy.join("old.dll"), b"old-binary-must-survive").unwrap();
        originals.push((id, manifest, digest));
    }
    let project = crate::project::open(&root).unwrap();
    let roots = PluginRoots { project: Some(project.root.clone()), global: output.join("owned-global") };
    let catalog = packages::catalog_at(&roots, PluginScope::Project).unwrap();
    assert!(catalog.warnings.is_empty());
    assert_eq!(catalog.notices.len(), ids.len());
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(NativeTools::start_with_roots(&project, roots.clone()))
        .insert_resource(PluginManagerState::with_roots(roots.project, roots.global))
        .init_resource::<SceneEditorState>()
        .init_resource::<WorkspaceDomainState>()
        .init_resource::<VegetationPresetEditorState>()
        .init_resource::<bevy::clipboard::Clipboard>()
        .init_resource::<SceneEditorSidebarState>()
        .init_resource::<AssetPreviewFocus>()
        .add_systems(Update, (native_systems::poll, super::systems::control, super::systems::poll).chain());
    crate::native_plugins::adapter::install(&mut app);
    super::open_from_workspace(app.world_mut());
    let started = Instant::now();
    loop {
        app.update();
        let native = app.world().resource::<NativeTools>();
        let manager = app.world().resource::<PluginManagerState>();
        if !native.discovery_pending && native.loading == 0 && !manager.busy {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(40), "四个真实DLL加载超时");
        std::thread::sleep(Duration::from_millis(10));
    }
    app.update();
    let native = app.world().resource::<NativeTools>();
    let manager = app.world().resource::<PluginManagerState>();
    assert!(native.errors.is_empty(), "正常迁移不得误报加载失败：{:?}", native.errors);
    assert!(native.management_errors.is_empty());
    assert_eq!(native.tools.len(), ids.len());
    for (id, manifest, digest) in originals {
        assert_eq!(native.load_states.get(id).map(String::as_str), Some("已加载"));
        assert_eq!(manager.load_states.get(id).map(String::as_str), Some("已加载"));
        assert!(native.tools.values().any(|tool| tool.domain == id
            && tool.snapshot.plugin_id == id && tool.fault.is_none()));
        assert_eq!(fs::read(root.join("plugins").join(id).join("plugin.json")).unwrap(), manifest);
        assert_eq!(fs::read(root.join("plugins").join(id).join("old.dll")).unwrap(), b"old-binary-must-survive");
        assert_eq!(packages::binary_digest(&source.join(packages::binary_filename(id))).unwrap(), digest);
    }
    assert!(manager.status.contains("迁移提示："));
    assert_eq!(fs::read(root.join("assets/author.txt")).unwrap(), b"author-data-must-survive");
    let play_blocker = feathrise_editor_scene::standalone::saved_play_blocker(app.world());
    assert!(play_blocker.is_none(), "正常迁移不得阻断保存后运行：{play_blocker:?}");
    write_new(&output.join("evidence.json"), &serde_json::to_vec_pretty(&json!({
        "passed":true,"actual_loaded":native.load_states,"actual_errors":native.errors,
        "management_load_states":manager.load_states,"management_status":manager.status,
        "migration_notices":catalog.notices,"saved_play_blocker":play_blocker,
        "old_files_preserved":true,"author_bytes_preserved":true,"source_binaries_unchanged":true,
        "source":"actual plugin C ABI Open, production native poll and manager projection",
        "gpu_or_physical_input_verified":false,
    })).unwrap());
}

fn capture(
    app: &mut App,
    target: &Handle<Image>,
    output: &Path,
    name: &str,
    width: usize,
) -> Value {
    settle(app);
    let pixels = screenshot(app, target, &output.join(format!("{name}.png")));
    let panel = entity::<ManagerPanel>(app);
    let header = entity::<ManagerHeader>(app);
    let footer = entity::<ManagerFooter>(app);
    let chrome = assert_dialog_chrome(app, panel, header, footer, width, &pixels);
    let panel_rect = bounds(app, panel);
    assert!(panel_rect.min.x >= 0. && panel_rect.max.x <= width as f32 + 1.);
    let world = app.world_mut();
    let texts = world
        .query::<(&Text, &ComputedNode, &InheritedVisibility)>()
        .iter(world)
        .filter(|(_, computed, visible)| visible.get() && !computed.is_empty())
        .map(|(text, ..)| text.0.clone())
        .collect::<Vec<_>>();
    assert!(
        texts.iter().any(|text| text.contains("插件")),
        "必须真实排版插件中文文案"
    );
    json!({"image":format!("{name}.png"),"chrome":chrome,"visible_text":texts})
}

#[test]
#[ignore = "显式validation GPU与真实DLL子进程验收；设置新的绝对FEATHRISE_PLUGIN_MANAGER_GPU_REPORT目录"]
fn production_plugin_manager_gpu_and_reopen_transactions() {
    let output = PathBuf::from(
        std::env::var_os("FEATHRISE_PLUGIN_MANAGER_GPU_REPORT").expect("设置新的独占报告目录"),
    );
    assert!(output.is_absolute());
    fs::create_dir(&output).expect("报告目录必须尚未存在，保留旧证据");
    let output = output.canonicalize().unwrap();
    let root = output.join("fixture");
    fs::create_dir(&root).unwrap();
    let root = root.canonicalize().unwrap();
    write_new(
        &output.join("owner.json"),
        &serde_json::to_vec_pretty(&json!({"kind":"plugin-manager-validation", "project":root,
        "output":output,"process_id":std::process::id()}))
        .unwrap(),
    );
    fs::create_dir(root.join("src")).unwrap();
    write_new(
        &root.join("Cargo.toml"),
        b"[package]\nname='manager_fixture'\nversion='0.1.0'\nedition='2024'\n[workspace]\n",
    );
    write_new(&root.join("src/main.rs"), b"fn main() {}\n");
    write_new(&root.join("editor-project.json"), &serde_json::to_vec_pretty(&json!({"format":1,"id":format!("plugin-manager-{}",std::process::id()),
        "name":"插件管理器验收","cargo_manifest":"Cargo.toml","package":"manager_fixture","asset_roots":["assets"]})).unwrap());
    feathrise_project::seed::create_empty(&root.join("assets")).unwrap();
    let author_data = "作者数据保护：停用、升级和移除插件均不能修改这份内容。\n".as_bytes();
    write_new(&root.join("assets/author-data.txt"), author_data);
    let author_baseline = asset_bytes(&root.join("assets"));
    fs::create_dir_all(root.join("plugins")).unwrap();
    let broken = root.join("plugins/broken.dll");
    write_new(&broken, b"invalid native binary");
    let global = fixture_roots(&root).global;
    fs::create_dir(&global).unwrap();
    write_new(&global.join(".feathrise-plugin-fixture-owner"), b"feathrise-single-binary-validation-v1");
    // 两份库由本轮真实rustc生成，快照代码含不同版本；不通过改外置JSON伪造升级。
    let releases = super::validation_binary::build_versions(&output).unwrap();

    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin {
            file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/assets").into(),
            watch_for_changes_override: Some(false),
            ..default()
        },
        bevy::scene::ScenePlugin,
    ));
    app.add_plugins((
        WindowPlugin {
            primary_window: None,
            exit_condition: bevy::window::ExitCondition::DontExit,
            ..default()
        },
        bevy::render::RenderPlugin::default(),
        bevy::image::ImagePlugin::default(),
        bevy::mesh::MeshPlugin,
        bevy::camera::CameraPlugin,
        bevy::core_pipeline::CorePipelinePlugin,
        bevy::sprite::SpritePlugin,
        bevy::sprite_render::SpriteRenderPlugin,
    ));
    app.add_plugins((
        bevy::input::InputPlugin,
        bevy::input_focus::InputFocusPlugin,
        bevy::input_focus::InputDispatchPlugin,
        bevy::picking::input::PointerInputPlugin,
        bevy::picking::PickingPlugin,
        bevy::picking::InteractionPlugin,
        bevy::transform::TransformPlugin,
        bevy::text::TextPlugin,
        feathrise_runtime::DefaultFontPlugin,
        bevy::ui::UiPlugin,
        bevy::ui_widgets::ScrollbarPlugin,
        bevy::ui_render::UiRenderPlugin,
        feathrise_editor_ui::EditorUiPlugin,
    ));
    super::install(&mut app, root.clone());
    notification_checks::install(&mut app);
    app.insert_resource(PluginManagerState::with_roots(Some(root.clone()), global.clone()));
    app.add_systems(
        Update,
        use_editor_font
            .after(super::ui::mount)
            .after(super::ui::present),
    );
    app.add_systems(
        First,
        provide_cursor
            .before(bevy::picking::PickingSystems::Input),
    );
    app.add_systems(
        PreUpdate,
        restore_target
            .after(feathrise_editor_ui::UiInteractionSystems::Sync)
            .after(feathrise_editor_ui::update_editor_button_clicks),
    );
    let mut image = Image::new_target_texture(1280, 720, TextureFormat::Rgba8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let target = app.world_mut().resource_mut::<Assets<Image>>().add(image);
    let camera = app
        .world_mut()
        .spawn((Camera2d, RenderTarget::from(target.clone())))
        .id();
    let window = app
        .world_mut()
        .spawn((
            Window {
                resolution: (1280, 720).into(),
                ..default()
            },
            PrimaryWindow,
        ))
        .id();
    app.insert_resource(CursorFixture {
        window,
        camera,
        image: target.clone(),
        position: None,
    });
    app.world_mut().spawn((
        WorkbenchHeader,
        feathrise_editor_ui::EditorThemeRoot,
        bevy::ui::UiTargetCamera(camera),
        Node {
            width: percent(100),
            height: percent(100),
            ..default()
        },
        BackgroundColor(feathrise_editor_ui::colors::PANEL_BG),
    ));
    app.finish();
    app.cleanup();
    super::open_from_workspace(app.world_mut());
    let _ = app
        .world_mut()
        .resource_mut::<PluginManagerState>()
        .refresh();
    wait_for_idle(&mut app);
    let mut images = vec![capture(&mut app, &target, &output, "initial-error", 1280)];
    let mut probes = Vec::new();
    let mut feedback_proofs = Vec::new();
    let notification_proof = notification_checks::verify(&mut app, &target, &root, &global, &output);

    // 目录选择框不在离屏验收范围；本地源仍经过正式request、后台校验和文件事务。
    app.world_mut()
        .resource_mut::<PluginManagerState>()
        .request(ManagementCommand::Install {
            source: releases[0].clone(),
            replace: false,
        })
        .unwrap();
    wait_for_idle(&mut app);
    assert!(matches!(
        entry(&app, "example.protocol_probe").pending.unwrap().kind,
        PendingKind::Install
    ));
    assert!(
        !root.join("plugins").join(packages::binary_filename("example.protocol_probe")).exists(),
        "登记安装不能原地改动运行中的发布目录"
    );
    images.push(capture(&mut app, &target, &output, "pending-install", 1280));
    apply_on_reopen(&mut app, &root);
    probes.push(native_probe(&root, &output, "installed-native", Some("1.0.0")));
    assert_eq!(
        entry(&app, "example.protocol_probe").version.as_deref(),
        Some("1.0.0")
    );
    assert_eq!(
        entry(&app, "example.protocol_probe").runtime_missing,
        vec!["example_runtime"]
    );

    click(
        &mut app,
        |action| matches!(action, ManagerAction::Select(id) if id=="example.protocol_probe"),
    );
    feedback_proofs.extend(feedback::operation_buttons(&mut app, &target, &output, "installed", false));
    feedback_proofs.push(feedback::directory(&mut app, PluginScope::Project, &root.join("plugins")));
    click(
        &mut app,
        |action| matches!(action, ManagerAction::SetEnabled{id,enabled:false} if id=="example.protocol_probe"),
    );
    assert!(
        entry(&app, "example.protocol_probe").enabled,
        "停用只应登记到下次重开"
    );
    images.push(capture(&mut app, &target, &output, "pending-disable", 1280));
    feedback_proofs.extend(feedback::operation_buttons(&mut app, &target, &output, "pending", true));
    click(&mut app, |action| matches!(action, ManagerAction::CancelPending(id) if id == "example.protocol_probe"));
    assert!(entry(&app, "example.protocol_probe").pending.is_none());
    assert!(entry(&app, "example.protocol_probe").enabled, "撤销停用不得改变当前启用状态");
    images.push(capture(&mut app, &target, &output, "cancelled-disable", 1280));
    click(&mut app, |action| matches!(action, ManagerAction::SetEnabled { id, enabled: false } if id == "example.protocol_probe"));
    apply_on_reopen(&mut app, &root);
    assert!(!entry(&app, "example.protocol_probe").enabled);
    probes.push(native_probe(&root, &output, "disabled-native", None));
    click(
        &mut app,
        |action| matches!(action, ManagerAction::Select(id) if id=="example.protocol_probe"),
    );
    click(
        &mut app,
        |action| matches!(action, ManagerAction::SetEnabled{id,enabled:true} if id=="example.protocol_probe"),
    );
    apply_on_reopen(&mut app, &root);
    probes.push(native_probe(&root, &output, "reenabled-native", Some("1.0.0")));

    app.world_mut()
        .resource_mut::<PluginManagerState>()
        .request(ManagementCommand::Install {
            source: releases[1].clone(),
            replace: true,
        })
        .unwrap();
    wait_for_idle(&mut app);
    assert_eq!(
        entry(&app, "example.protocol_probe").version.as_deref(),
        Some("1.0.0")
    );
    images.push(capture(&mut app, &target, &output, "pending-update", 1280));
    apply_on_reopen(&mut app, &root);
    assert_eq!(
        entry(&app, "example.protocol_probe").version.as_deref(),
        Some("1.1.0")
    );
    probes.push(native_probe(&root, &output, "updated-native", Some("1.1.0")));
    let installed_path = root.join("plugins").join(packages::binary_filename("example.protocol_probe"));
    let installed_manifest = fs::read(&installed_path).unwrap();
    app.world_mut()
        .resource_mut::<PluginManagerState>()
        .request(ManagementCommand::Install {
            source: broken.clone(),
            replace: true,
        })
        .unwrap();
    wait_for_idle(&mut app);
    let rejected = packages::inspect_package(&broken)
        .err()
        .expect("自建坏清单必须真实解析失败");
    assert_eq!(
        app.world().resource::<PluginManagerState>().status,
        rejected,
        "后台拒绝原因必须如实投递到管理器"
    );
    assert!(entry(&app, "example.protocol_probe").pending.is_none());
    assert_eq!(
        fs::read(&installed_path).unwrap(),
        installed_manifest
    );
    images.push(capture(&mut app, &target, &output, "update-rejected", 1280));

    click(
        &mut app,
        |action| matches!(action, ManagerAction::Select(id) if id=="example.protocol_probe"),
    );
    click(
        &mut app,
        |action| matches!(action, ManagerAction::RequestRemove(id) if id=="example.protocol_probe"),
    );
    images.push(capture(
        &mut app,
        &target,
        &output,
        "remove-confirmation",
        1280,
    ));
    click(&mut app, |action| {
        matches!(action, ManagerAction::CancelRemove)
    });
    assert!(entry(&app, "example.protocol_probe").pending.is_none());
    click(
        &mut app,
        |action| matches!(action, ManagerAction::RequestRemove(id) if id=="example.protocol_probe"),
    );
    click(&mut app, |action| {
        matches!(action, ManagerAction::ConfirmRemove)
    });
    assert!(matches!(
        entry(&app, "example.protocol_probe").pending.unwrap().kind,
        PendingKind::Remove
    ));
    images.push(capture(&mut app, &target, &output, "pending-remove", 1280));

    app.world_mut()
        .resource_mut::<Assets<Image>>()
        .get_mut(&target)
        .unwrap()
        .resize(Extent3d {
            width: 620,
            height: 720,
            depth_or_array_layers: 1,
        });
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .resolution
        .set(620., 720.);
    images.push(capture(
        &mut app,
        &target,
        &output,
        "pending-remove-narrow",
        620,
    ));
    apply_on_reopen(&mut app, &root);
    assert!(
        !app.world()
            .resource::<PluginManagerState>()
            .rows
            .iter()
            .any(|row| row.id == "example.protocol_probe")
    );
    assert_eq!(
        fs::read(root.join("assets/author-data.txt")).unwrap(),
        author_data
    );
    assert_eq!(
        asset_bytes(&root.join("assets")),
        author_baseline,
        "所有夹具作者资产的文件集合和原字节必须保留"
    );
    probes.push(native_probe(&root, &output, "removed-native", None));
    images.push(capture(&mut app, &target, &output, "removed-narrow", 620));

    // 旧目录包只有显式确认后才能归档；不删除目录中的原始内容或作者资产。
    let legacy = root.join("plugins/legacy-removal-fixture");
    fs::create_dir(&legacy).unwrap();
    // 覆盖Windows旧作者工具写出的UTF-8 BOM：发现、诊断和归档应采用同一原始清单身份。
    let mut legacy_manifest = vec![0xef, 0xbb, 0xbf];
    legacy_manifest.extend_from_slice(r#"{"format":1,"id":"example.legacy","tool":"旧包归档夹具"}"#.as_bytes());
    write_new(&legacy.join("plugin.json"), &legacy_manifest);
    write_new(&legacy.join("old.dll"), b"legacy-bytes-must-survive");
    let legacy_baseline = asset_bytes(&legacy);

    app.world_mut().resource_mut::<Assets<Image>>().get_mut(&target).unwrap().resize(Extent3d {
        width: 1280, height: 720, depth_or_array_layers: 1,
    });
    app.world_mut().get_mut::<Window>(window).unwrap().resolution.set(1280., 720.);
    let legacy_failure = notification_checks::begin_legacy_failure(&mut app, &root, &global, "example.legacy");
    app.world_mut().resource_mut::<PluginManagerState>().refresh().unwrap();
    wait_for_idle(&mut app);
    click(&mut app, |action| matches!(action, ManagerAction::Select(id) if id == "example.legacy"));
    assert!(entry(&app, "example.legacy").legacy);
    feedback_proofs.push(feedback::legacy_controls(&mut app));
    click(&mut app, |action| matches!(action, ManagerAction::RequestRemove(id) if id == "example.legacy"));
    images.push(capture(&mut app, &target, &output, "legacy-remove-confirmation", 1280));
    click(&mut app, |action| matches!(action, ManagerAction::CancelRemove));
    assert_eq!(asset_bytes(&legacy), legacy_baseline);
    click(&mut app, |action| matches!(action, ManagerAction::RequestRemove(id) if id == "example.legacy"));
    click(&mut app, |action| matches!(action, ManagerAction::ConfirmRemove));
    assert!(!legacy.exists(), "确认后旧目录必须从插件发现根移出");
    let archive = fs::read_dir(root.join(".editor/plugin-management/backups")).unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("legacy/legacy-removal-fixture"))
        .find(|path| path.is_dir()).expect("旧目录必须仍存在于事务归档中");
    assert_eq!(asset_bytes(&archive), legacy_baseline, "归档不得丢失或改写旧包字节");
    let archived_manifest = fs::read(archive.join("plugin.json")).unwrap();
    let legacy_utf8_bom_preserved = archived_manifest.starts_with(&[0xef, 0xbb, 0xbf])
        && archived_manifest == legacy_manifest;
    assert!(legacy_utf8_bom_preserved, "归档后的清单必须保留UTF-8 BOM和完整原字节");
    assert!(!app.world().resource::<PluginManagerState>().rows.iter().any(|row| row.id == "example.legacy"));
    let receipts: Vec<Value> = fs::read_dir(root.join(".editor/plugin-management/receipts")).unwrap()
        .map(|item| serde_json::from_slice(&fs::read(item.unwrap().path()).unwrap()).unwrap())
        .filter(|receipt: &Value| receipt["kind"] == "legacy_archive" && receipt["id"] == "example.legacy")
        .collect();
    assert_eq!(receipts.len(), 1, "诊断消解必须对应唯一的真实归档完成回执");
    assert_eq!(receipts[0]["scope"], "project");
    assert_eq!(PathBuf::from(receipts[0]["archived"].as_str().unwrap()).canonicalize().unwrap(), archive.canonicalize().unwrap());
    assert_eq!(crate::project::display_path(Path::new(receipts[0]["original"].as_str().unwrap())), crate::project::display_path(&legacy));
    assert!(receipts[0]["sha256"].as_str().is_some_and(|digest| digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())));
    let mut legacy_diagnostic_proof = notification_checks::finish_legacy_failure(&mut app, &target, &output, legacy_failure);
    legacy_diagnostic_proof["archive_receipt"] = receipts[0].clone();
    images.push(capture(&mut app, &target, &output, "legacy-archived", 1280));

    // 同一生产面板在没有工程时管理全局；当前工程范围必须真实禁用。
    app.insert_resource(PluginManagerState::with_roots(None, global.clone()));
    super::open_from_workspace(app.world_mut());
    app.world_mut().resource_mut::<PluginManagerState>().refresh().unwrap();
    wait_for_idle(&mut app);
    let project_scope = app.world_mut().query::<(Entity, &ManagerAction)>().iter(app.world())
        .find_map(|(entity, action)| matches!(action, ManagerAction::SetScope(PluginScope::Project)).then_some(entity)).unwrap();
    assert!(app.world().get::<bevy::ui::InteractionDisabled>(project_scope).is_some());
    feedback_proofs.push(feedback::directory(&mut app, PluginScope::Global, &global));
    images.push(capture(&mut app, &target, &output, "global-without-project", 1280));
    app.world_mut().resource_mut::<PluginManagerState>().request(ManagementCommand::Install {
        source: releases[0].clone(), replace: false,
    }).unwrap();
    wait_for_idle(&mut app);
    assert_eq!(entry(&app, "example.protocol_probe").scope, PluginScope::Global);
    feedback::assert_global_committed(&app, &global, "example.protocol_probe");
    images.push(capture(&mut app, &target, &output, "global-install-effective", 1280));
    click(&mut app, |action| matches!(action, ManagerAction::Select(id) if id == "example.protocol_probe"));
    click(&mut app, |action| matches!(action, ManagerAction::SetEnabled { id, enabled: false } if id == "example.protocol_probe"));
    assert!(!entry(&app, "example.protocol_probe").enabled);
    feedback::assert_global_committed(&app, &global, "example.protocol_probe");
    images.push(capture(&mut app, &target, &output, "global-disable-effective", 1280));
    click(&mut app, |action| matches!(action, ManagerAction::SetEnabled { id, enabled: true } if id == "example.protocol_probe"));
    assert!(entry(&app, "example.protocol_probe").enabled);
    feedback::assert_global_committed(&app, &global, "example.protocol_probe");
    let first_global = entry(&app, "example.protocol_probe").path;
    let first_global_bytes = fs::read(&first_global).unwrap();
    app.world_mut().resource_mut::<PluginManagerState>().request(ManagementCommand::Install {
        source: releases[1].clone(), replace: true,
    }).unwrap();
    wait_for_idle(&mut app);
    feedback::assert_global_committed(&app, &global, "example.protocol_probe");
    assert_eq!(entry(&app, "example.protocol_probe").version.as_deref(), Some("1.1.0"));
    let second_global = entry(&app, "example.protocol_probe").path;
    assert_ne!(first_global, second_global);
    assert_eq!(fs::read(&first_global).unwrap(), first_global_bytes, "更新不能覆盖旧会话仍可能映射的全局对象");
    images.push(capture(&mut app, &target, &output, "global-update-effective", 1280));
    probes.push(native_probe(&root, &output, "global-updated-native", Some("1.1.0")));
    click(&mut app, |action| matches!(action, ManagerAction::RequestRemove(id) if id == "example.protocol_probe"));
    click(&mut app, |action| matches!(action, ManagerAction::CancelRemove));
    assert_eq!(entry(&app, "example.protocol_probe").path, second_global);
    click(&mut app, |action| matches!(action, ManagerAction::RequestRemove(id) if id == "example.protocol_probe"));
    click(&mut app, |action| matches!(action, ManagerAction::ConfirmRemove));
    assert!(!app.world().resource::<PluginManagerState>().rows.iter().any(|row| row.id == "example.protocol_probe"));
    assert!(first_global.is_file() && second_global.is_file(), "移除全局索引不能删除其他进程可能仍使用的对象");
    images.push(capture(&mut app, &target, &output, "global-removed-effective", 1280));
    // 模拟旧版首页留下的真实计划：由生产stage创建，再重建管理器触发恢复，不直接写成功索引。
    packages::stage_at(&PluginRoots { project: None, global: global.clone() }, PluginScope::Global,
        ManagementCommand::Install { source: releases[0].clone(), replace: false }).unwrap();
    app.insert_resource(PluginManagerState::with_roots(None, global.clone()));
    super::open_from_workspace(app.world_mut());
    app.world_mut().resource_mut::<PluginManagerState>().refresh().unwrap();
    wait_for_idle(&mut app);
    feedback::assert_global_committed(&app, &global, "example.protocol_probe");
    assert_eq!(entry(&app, "example.protocol_probe").version.as_deref(), Some("1.0.0"));
    images.push(capture(&mut app, &target, &output, "global-pending-recovered-at-home", 1280));
    // 安装已实际完成；独立作者进程只负责证明已发布对象可以继承并真正加载。
    probes.push(native_probe(&root, &output, "global-inherited-native", Some("1.0.0")));
    app.world_mut().resource_mut::<PluginManagerState>().refresh().unwrap();
    wait_for_idle(&mut app);
    let immutable_global = entry(&app, "example.protocol_probe").path;
    assert!(immutable_global.starts_with(&global));
    assert!(!root.join("plugins").join(packages::binary_filename("example.protocol_probe")).exists());

    app.insert_resource(PluginManagerState::with_roots(Some(root.clone()), global.clone()));
    super::open_from_workspace(app.world_mut());
    app.world_mut().resource_mut::<PluginManagerState>().refresh().unwrap();
    wait_for_idle(&mut app);
    assert!(entry(&app, "example.protocol_probe").inherited);
    images.push(capture(&mut app, &target, &output, "project-inherits-global", 1280));
    app.world_mut().resource_mut::<PluginManagerState>().request(ManagementCommand::Install {
        source: releases[1].clone(), replace: false,
    }).unwrap();
    wait_for_idle(&mut app);
    apply_on_reopen(&mut app, &root);
    probes.push(native_probe(&root, &output, "project-overrides-global-native", Some("1.1.0")));
    assert_eq!(entry(&app, "example.protocol_probe").scope, PluginScope::Project);
    assert!(!entry(&app, "example.protocol_probe").inherited);
    click(&mut app, |action| matches!(action, ManagerAction::SetScope(PluginScope::Global)));
    assert!(entry(&app, "example.protocol_probe").overridden_by_project);
    images.push(capture(&mut app, &target, &output, "global-overridden-by-project", 1280));
    click(&mut app, |action| matches!(action, ManagerAction::SetScope(PluginScope::Project)));
    click(&mut app, |action| matches!(action, ManagerAction::Select(id) if id == "example.protocol_probe"));
    click(&mut app, |action| matches!(action, ManagerAction::SetEnabled { id, enabled: false } if id == "example.protocol_probe"));
    apply_on_reopen(&mut app, &root);
    probes.push(native_probe(&root, &output, "disabled-project-no-global-fallback-native", None));
    click(&mut app, |action| matches!(action, ManagerAction::Select(id) if id == "example.protocol_probe"));
    click(&mut app, |action| matches!(action, ManagerAction::RequestRemove(id) if id == "example.protocol_probe"));
    images.push(capture(&mut app, &target, &output, "remove-project-uses-global-confirmation", 1280));
    click(&mut app, |action| matches!(action, ManagerAction::ConfirmRemove));
    apply_on_reopen(&mut app, &root);
    probes.push(native_probe(&root, &output, "removed-project-uses-global-native", Some("1.0.0")));
    assert!(entry(&app, "example.protocol_probe").inherited);
    assert_eq!(entry(&app, "example.protocol_probe").path, immutable_global);
    assert_eq!(asset_bytes(&root.join("assets")), author_baseline);
    click(&mut app, |action| matches!(action, ManagerAction::Close));
    assert!(!app.world().resource::<PluginManagerState>().open);

    write_new(&output.join("evidence.json"), &serde_json::to_vec_pretty(&json!({"passed":true,"project":root,"images":images,
        "native_process_probes":probes,"global_fixture":global,"actual_two_binary_versions":true,"button_feedback":feedback_proofs,
        "legacy_archive":archive,"legacy_bytes_preserved":true,"legacy_utf8_bom_preserved":legacy_utf8_bom_preserved,
        "home_global_install_toggle_update_remove_and_recovery_committed":true,"os_directory_window_verified":false,
        "failure_notification":notification_proof,
        "legacy_failed_discovery_cleanup":legacy_diagnostic_proof,
        "author_data_unchanged":true,"author_files_checked":author_baseline.len(),"os_input_verified":false,
        "source":"production manager UI, official Bevy mouse input, background transactions, RenderApp readback, isolated native DLL processes",
        "not_verified":["native directory chooser","physical keyboard and mouse","OS focus feel","Linux binary loading","real author save and close lifecycle"]})).unwrap());
}
