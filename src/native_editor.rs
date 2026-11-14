//! 独立编辑实例的原生产App装配：原作者工具和3D场景直接运行在本地World。
//!
//! 一个进程终生绑定一个工程。这里仅组合原插件，不复制原界面、作者算法或场景运行时。

#[path = "native_editor_assets.rs"]
mod assets;
#[path = "native_editor_config.rs"]
mod config;
#[path = "native_editor_preferences.rs"]
pub(crate) mod preferences;
#[cfg(feature = "validation")]
#[path = "native_editor_smoke.rs"]
mod smoke;
#[cfg(all(feature = "validation", windows))]
#[path = "build_environment_validation.rs"]
mod build_environment_validation;

use avian3d::{PhysicsPlugins, prelude::ColliderCachePlugin};
use bevy::{app::AppExit, prelude::*};
use feathrise_runtime::game_plugins::{EditorStartup, SceneRuntimePlugin, StartupWindowPolicy};
use bevy_framepace::FramepacePlugin;
use bevy_hanabi::HanabiPlugin;
use feathrise_editor_core::{EditorProjectPaths, bind_editor_project_paths};
use std::path::PathBuf;

/// 启动工程独占的完整原编辑器；正常入口由持续存在的主窗口呈现，项目数据不写入SDK。
pub(crate) fn run(project: crate::project::Project, target: PathBuf) -> Result<(), String> {
    #[cfg(all(feature = "validation", windows))]
    let scene_format_fixture = crate::scene_format_validation::preflight(&project)?;
    #[cfg(feature = "validation")]
    let project_browser_fixture = crate::project_browser_validation::preflight(&project)?;
    #[cfg(feature = "validation")]
    let file_manager_fixture = crate::file_manager_validation::preflight(&project)?;
    // 工程仍按实例冻结路径；正常入口在同一主窗口内呈现，不再打开第二个作者顶层窗口。
    let embedded = crate::workspace_window::EmbedTarget::from_env()?.is_some();
    // 相同工程仅允许一个作者写实例；锁句柄一直存活到真实App退出，残留文件不构成永久锁。
    let _writer_lease = crate::native_project_lease::NativeProjectLease::acquire(&project)?;
    // 启动器传入的target可能是相对路径，必须先固定，不能随下面的工程cwd改变含义。
    let target = if target.is_absolute() {
        target
    } else {
        std::env::current_dir()
            .map_err(|error| format!("无法确定构建缓存位置：{error}"))?
            .join(target)
    };
    let asset_root = project
        .context
        .asset_roots
        .first()
        .ok_or("工程未登记默认资产目录")?
        .clone();
    bind_editor_project_paths(EditorProjectPaths::new(&project.root, &asset_root)?)?;
    // 工程资产解析仍固定cwd；编辑器偏好另从用户配置读取，不能隐式采用游戏settings.json。
    std::env::set_current_dir(&project.root)
        .map_err(|error| format!("无法进入工程目录：{error}"))?;

    feathrise_runtime::prepare_vulkan_environment();

    let mut app = App::new();
    #[cfg(feature = "validation")]
    feathrise_editor_scene::standalone::install_rigging_validation(&mut app, &project.root, &asset_root)?;
    let (preferences, preferences_error) = crate::workspace::preferences()?;
    if let Some(error) = preferences_error {
        eprintln!("{error}"); // 损坏偏好只读托底，原文件受身份保护，后续保存不能静默覆盖。
    }
    let (author_preferences, author_preferences_error) = self::preferences::load(&preferences);
    if let Some(error) = author_preferences_error {
        eprintln!("{error}"); // 独立作者配置坏原文保持只读；不以默认设置覆盖，也不牵连工程最近记录。
    }
    let editor_settings = author_preferences.editor.clone().into_runtime();
    let display = feathrise_settings::StartupDisplayPreferences {
        fullscreen: editor_settings.render.fullscreen,
        vsync: editor_settings.render.graphics.vsync,
        frame_generation_mode: editor_settings.render.graphics.frame_generation_mode,
    };
    let policy = StartupWindowPolicy {
        force_windowed: embedded,
        ..default()
    };
    app.insert_resource(EditorStartup)
        .insert_resource(feathrise_scene_content::RuntimeAssetRoot::new(
            &asset_root,
        )?)
        .insert_resource(display)
        .insert_resource(policy)
        .insert_resource(editor_settings)
        // 自动编译在插件装配时即开始记录，第一条日志也要读取真实系统语言。
        .insert_resource(feathrise_editor_ui::localization::EditorSystemLocale(self::preferences::system_locale()))
        .add_plugins(feathrise_editor_core::EditorSettingsPlugin);

    #[cfg(feature = "rtx_rendering")]
    app.insert_resource(bevy::anti_alias::dlss::DlssProjectId(
        bevy::asset::uuid::uuid!("14b34155-a529-4df8-8d57-b4a76b98807d"),
    ));
    #[cfg(all(feature = "dlss_frame_generation", target_os = "windows"))]
    if display.frame_generation_mode == feathrise_settings::FrameGenerationMode::Dlss2x {
        app.add_plugins(feathrise_frame_generation::StreamlineBootstrapPlugin);
    }
    #[cfg(all(feature = "fsr3_frame_generation", target_os = "windows"))]
    if display.frame_generation_mode == feathrise_settings::FrameGenerationMode::Fsr3_2x {
        app.add_plugins(feathrise_fsr3_frame_generation::Fsr3FrameGenerationBootstrapPlugin);
    }

    // 作者实例始终读取可编辑源资产；不因release构建而寻找SDK发布包或仓库作者数据。
    let asset_plugin = assets::install(&mut app, &project.context.asset_roots)?;
    app.add_plugins(config::default_plugins(
        &project.name,
        policy,
        display,
        asset_plugin,
    ))
    // 沿用原入口的GLB换代保护，防止新几何仍用同句柄旧Trimesh缓存。
    .add_plugins(
        PhysicsPlugins::default()
            .build()
            .disable::<ColliderCachePlugin>(),
    )
    .add_plugins(HanabiPlugin)
    .add_plugins(FramepacePlugin)
    .add_plugins(SceneRuntimePlugin)
    // 场景领域插件只提供中性外观描述，由SDK负责本地作者视口的模型与附件生命周期。
    .add_plugins(feathrise_runtime::project::author_visuals::AuthorVisualsPlugin)
    .add_plugins(feathrise_fps_display::FpsDisplayPlugin) // 作者保留原FPS/F3诊断，公共场景及空工程不隐式安装。
    .add_plugins(feathrise_runtime::app::scene_loading::ProjectSceneAssetsPlugin)
    .add_plugins(crate::branding::EditorBrandingPlugin)
    // 安装原完整插件，而非共享工作台只读投影；文档、撤销和作者任务均使用原资源。
    .add_plugins((
        feathrise_editor_scene::SceneEditorPlugin,
        feathrise_editor_animation::AnimationEditorPlugin,
        feathrise_editor_project_browser::ProjectBrowserPlugin,
        feathrise_editor_surface::SurfaceEditorPlugin,
        feathrise_editor_documents::DocumentsEditorPlugin,
        feathrise_editor_weights::WeightPanelPlugin,
        feathrise_editor_glb::GlbEditorPlugin,
    ))
    .add_plugins(feathrise_editor_scene::standalone::StandaloneEditorPlugin)
    .add_plugins(crate::project_log::ProjectLogPlugin::new(&project))
    .add_plugins(crate::native_project_close::NativeProjectClosePlugin)
    .add_plugins(crate::native_project_menu::NativeProjectMenuPlugin::new(
        project.clone(),
    ))
    .add_plugins(crate::native_play::NativePlayPlugin::new(
        project.clone(),
        target.clone(),
    ));
    // 两种方式共享原作者文档；工程视口由真实工程App拥有，不把作者World当作游戏。
    let viewport_preferences = crate::viewport_mode::install(&mut app, &preferences, Some(&project));
    app.add_plugins(crate::embedded_viewport::EmbeddedViewportPlugin::new(project.clone(), target.clone()));
    app.add_plugins(crate::code_iteration::CodeIterationPlugin::new(project.clone(), target.clone()));
    app.add_plugins(crate::project_export::ProjectExportPlugin::new(project.clone(), target.clone()));
    // 作者宿主统一提供自身字体；公共传送门与FPS/F3不再选择原游戏的中文字体。
    let author_ui_font = feathrise_editor_core::editor_font(app.world().resource::<AssetServer>(), feathrise_editor_core::FONT_PATH);
    app.insert_resource(feathrise_portal::PortalLabelFont(author_ui_font.clone()));
    app.insert_resource(feathrise_fps_display::FpsDisplayFont(author_ui_font));
    // 场景库只聚合关闭事务；每个专业作者库仍拥有唯一原保存实现，避免反向依赖。
    app.insert_resource(feathrise_editor_scene::standalone::ProjectCloseSaveAdapters {
        surface: feathrise_editor_surface::request_project_save,
    });

    feathrise_runtime::install_vulkan_surface_guard(&mut app);
    crate::workspace::install_author(&mut app, &project)?;
    #[cfg(windows)]
    {
        crate::environment_diagnostics::rust_install::install(&mut app);
        // Windows 编辑器用同一份 Winit 时钟管理日常和编译上限，关闭 Framepace 的刷新率上限。
        app.insert_resource(bevy_framepace::FramepaceSettings::default().with_limiter(bevy_framepace::Limiter::Off))
            .add_systems(First, crate::editor_frame_pacing::pace_author);
    }
    #[cfg(all(feature = "validation", windows))]
    build_environment_validation::install(&mut app, &project)?;
    crate::native_plugins::install(&mut app, &project, &target);

    // 与原入口一致，先注册原Avian绘制能力，再默认关闭碰撞线；F1可按设置重新开启。
    app.add_plugins(avian3d::debug_render::PhysicsDebugPlugin);
    app.world_mut()
        .resource_mut::<bevy::gizmos::config::GizmoConfigStore>()
        .config_mut::<avian3d::debug_render::PhysicsGizmos>()
        .0
        .enabled = false;
    #[cfg(feature = "validation")]
    crate::native_play_recovery_validation::install(&mut app, &project)?;
    #[cfg(all(feature = "validation", windows))]
    crate::embedded_viewport::validation::install(&mut app, &project.root)?;
    #[cfg(all(feature = "validation", windows))]
    let build_cancel_validation = crate::embedded_viewport::validation::build_cancel::install(&mut app, &project.root)?;
    #[cfg(all(feature = "validation", windows))]
    let export_validation = crate::project_export::validation::install(&mut app, &project, &target)?;
    #[cfg(feature = "validation")]
    crate::locale_validation::install(&mut app, Some(&project.root))?;
    #[cfg(all(feature = "validation", windows))]
    if let Some(fixture) = scene_format_fixture {
        fixture.install(&mut app);
    }
    #[cfg(feature = "validation")]
    if let Some(directory) = project_browser_fixture {
        app.insert_resource(bevy::winit::WinitSettings::continuous());
        app.world_mut().resource_mut::<feathrise_editor_core::EditorSettings>()
            .ui_locale = feathrise_editor_core::EditorLocalePreference::Chinese;
        feathrise_editor_scene::standalone::install_project_browser_validation(
            &mut app, directory, asset_root.clone(),
        )?;
    }
    #[cfg(feature = "validation")]
    if let Some(directory) = file_manager_fixture {
        app.insert_resource(bevy::winit::WinitSettings::continuous());
        app.world_mut().resource_mut::<feathrise_editor_core::EditorSettings>()
            .ui_locale = feathrise_editor_core::EditorLocalePreference::Chinese;
        feathrise_editor_scene::standalone::install_file_manager_validation(
            &mut app, directory, project.root.clone(),
        )?;
    }
    #[cfg(feature = "validation")]
    smoke::install(&mut app, project.root, asset_root)?;
    let preferences_completion = self::preferences::install(&mut app, author_preferences);

    #[cfg(feature = "validation")]
    let close_validation = crate::native_project_close::validation_completion(app.world());
    let exit = app.run();
    viewport_preferences.finish()?;
    let preferences_result = preferences_completion.finish();
    #[cfg(all(feature = "validation", windows))]
    if let Some(validation) = export_validation {
        validation.finish(&exit)?;
    }
    #[cfg(all(feature = "validation", windows))]
    if let Some(validation) = build_cancel_validation {
        validation.finish(&exit)?;
    }
    #[cfg(feature = "validation")]
    if let Some(validation) = close_validation {
        validation.finish(&exit)?;
    }
    preferences_result?;
    match exit {
        AppExit::Success => Ok(()),
        exit => Err(format!("工程编辑实例退出失败：{exit:?}")),
    }
}

/// 原关闭事务授权后异步排空作者偏好；其他未装作者设置的验收App保持原有关闭行为。
pub(crate) fn preferences_ready_to_exit(world: &mut World, retry: bool) -> Result<bool, String> {
    let code = crate::code_iteration::ready_to_exit(world)?;
    let export = crate::project_export::ready_to_exit(world)?;
    let previews = crate::native_plugins::ready_to_exit(world)?;
    let author = preferences::ready_to_exit(world, retry)?;
    let viewport = crate::viewport_mode::ready_to_exit(world, retry)?;
    Ok(code && export && previews && author && viewport)
}
