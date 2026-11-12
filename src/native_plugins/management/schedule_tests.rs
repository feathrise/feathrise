//! 实际生产插件的组合调度回归：独立作者与同窗口工作台均不得在启动时形成排序环。
use bevy::prelude::*;
use std::{
    fs,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const OWNER: &[u8] = b"plugin-production-schedule-fixture-v1";
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

struct Fixture {
    directory: PathBuf,
    workers_drained: bool,
}

impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "feathrise-plugin-schedule-{}-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&directory).unwrap();
        let directory = directory.canonicalize().unwrap();
        fs::write(directory.join("owner"), OWNER).unwrap();
        fs::create_dir(directory.join("assets")).unwrap();
        fs::create_dir(directory.join("src")).unwrap();
        fs::write(directory.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(
            directory.join("Cargo.toml"),
            "[package]\nname='schedule_fixture'\nversion='0.1.0'\nedition='2024'\n",
        )
        .unwrap();
        fs::write(
            directory.join("editor-project.json"),
            serde_json::to_vec(&serde_json::json!({
                "format": 2,
                "id": "8713b203-9cfb-4ef6-8750-d62a325a9101",
                "name": "生产组合调度回归",
                "cargo_manifest": "Cargo.toml",
                "package": "schedule_fixture",
                "asset_roots": ["assets"],
                "editor_data": ".editor"
            }))
            .unwrap(),
        )
        .unwrap();
        Self { directory, workers_drained: false }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // 生产代码监听即使遇到调度失败也必须先结束；超时则保留夹具，不能删除仍被访问的目录。
        let temporary = std::env::temp_dir().canonicalize().ok();
        if self.workers_drained
            && self.directory.is_absolute()
            && self.directory.parent() == temporary.as_deref()
            && self.directory.canonicalize().ok().as_ref() == Some(&self.directory)
            && self.directory.file_name().is_some_and(|name| {
                name.to_string_lossy().starts_with("feathrise-plugin-schedule-")
            })
            && fs::read(self.directory.join("owner")).ok().as_deref() == Some(OWNER)
        {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }
}

fn drain_code_workers(app: &mut App) -> Result<(), String> {
    let started = Instant::now();
    loop {
        if crate::code_iteration::ready_to_exit(app.world_mut())? {
            return Ok(());
        }
        if started.elapsed() >= Duration::from_secs(10) {
            return Err("组合调度夹具的代码监听线程未在十秒内收口，目录保留".into());
        }
        // 此处仅为测试收尾等待；不把阻塞轮询装入生产帧系统。
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn check_production_composition(workspace: bool) {
    let mut fixture = Fixture::new();
    let project = crate::project::open(&fixture.directory).unwrap();
    let preferences = crate::user_preferences::Preferences::load_at(
        fixture.directory.join("preferences.json"),
        fixture.directory.join("home"),
    )
    .unwrap();
    // 只传已有开发缓存的位置；测试不发送构建请求，也不创建另一份 target。
    let target = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target");
    let mut app = App::new();
    let mut completion = None;
    let initialized = catch_unwind(AssertUnwindSafe(|| {
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            // 工程作者必须安装完整场景插件，官方文字处理器才来自与生产相同的唯一入口。
            .add_plugins((feathrise_editor_scene::SceneEditorPlugin, feathrise_editor_glb::GlbEditorPlugin, feathrise_editor_animation::AnimationEditorPlugin, feathrise_editor_project_browser::ProjectBrowserPlugin))
            .add_plugins(feathrise_editor_scene::standalone::StandaloneEditorPlugin)
            .add_plugins(crate::project_log::ProjectLogPlugin::new(&project))
            .add_plugins(crate::native_project_close::NativeProjectClosePlugin)
            .add_plugins(crate::native_project_menu::NativeProjectMenuPlugin::new(project.clone()))
            .add_plugins(crate::native_play::NativePlayPlugin::new(project.clone(), target.clone()));
        completion = Some(crate::viewport_mode::install(&mut app, &preferences, None));
        app.add_plugins(crate::embedded_viewport::EmbeddedViewportPlugin::new(project.clone(), target.clone()));
        app.add_plugins(crate::code_iteration::CodeIterationPlugin::new(project.clone(), target.clone()));
        app.add_plugins(crate::project_export::ProjectExportPlugin::new(project.clone(), target));
        if workspace {
            app.add_plugins(crate::workspace_ui::WorkspaceUiPlugin)
                .add_plugins(crate::window_chrome::WindowChromePlugin);
        }
        super::install(&mut app, project.root.clone());
        // 真实 install 保留全部排序关系；只初始化图，不执行窗口、字库、剪贴板或工程业务系统。
        app.world_mut().schedule_scope(Update, |world, schedule| {
            schedule.initialize(world).map(|_| ()).map_err(|error| format!("{error:?}"))
        })
    }));

    // Result 断言和 panic 传播都放在收尾之后，避免失败路径只 cancel 却未 join 后台线程。
    let drained = drain_code_workers(&mut app);
    fixture.workers_drained = drained.is_ok();
    drop(app);
    let completed = completion.map_or(Ok(()), |completion| completion.finish());
    match initialized {
        Ok(result) => {
            assert!(drained.is_ok(), "{}", drained.unwrap_err());
            completed.expect("未修改模式的夹具偏好应可正常收尾");
            assert!(result.is_ok(), "生产插件组合调度初始化失败：{}", result.unwrap_err());
        }
        Err(payload) => {
            if let Err(error) = drained {
                eprintln!("生产装配 panic 后的后台收尾失败：{error}");
            }
            resume_unwind(payload);
        }
    }
}

#[test]
fn standalone_production_plugin_composition_initializes_update_schedule() {
    check_production_composition(false);
}

#[test]
fn workspace_production_plugin_composition_initializes_update_schedule() {
    check_production_composition(true);
}
