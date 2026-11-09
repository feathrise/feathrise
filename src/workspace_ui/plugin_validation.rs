//! 无工程真实主窗口专项：经官方鼠标/键盘来源打开原插件管理器，异步保存GPU与证据后退出。
use feathrise_editor_ui::UiInteraction;
use super::{
    components::{Action, Input},
    resources::{PluginHostValidation, Presentation},
    *,
};
use crate::native_plugins::management::{resources::PluginManagerState, types::PluginScope};
use async_channel::TryRecvError;
use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
        mouse::MouseButtonInput,
    },
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    tasks::IoTaskPool,
    text::EditableText,
    ui::InteractionDisabled,
    window::{PrimaryWindow, RawHandleWrapper},
};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    time::{Duration, Instant},
};

#[path = "plugin_validation_install.rs"]
mod install_probe;

pub(super) fn install(app: &mut App) {
    let Some(directory) =
        std::env::var_os("FEATHRISE_PLUGIN_HOST_VALIDATION_DIR").map(PathBuf::from)
    else {
        return;
    };
    assert!(
        !std::env::args_os()
            .any(|argument| argument == "--project" || argument == "--native-editor")
    );
    assert!(
        std::env::var_os("FEATHRISE_ABOUT_VALIDATION_DIR").is_none(),
        "专项不能叠加关于驱动"
    );
    assert!(
        directory.is_absolute() && directory.is_dir(),
        "先独占创建本轮输出目录"
    );
    let directory = directory.canonicalize().expect("本轮输出目录不可访问");
    let preferences =
        PathBuf::from(std::env::var_os("FEATHRISE_PREFERENCES_FILE").expect("必须隔离用户偏好"));
    assert!(preferences.is_absolute());
    assert_eq!(
        preferences.parent().unwrap().canonicalize().unwrap(),
        directory
    );
    if let Ok(info) = fs::symlink_metadata(&preferences) {
        assert!(
            info.is_file() && !info.file_type().is_symlink(),
            "隔离偏好不能是链接"
        );
        assert_eq!(
            preferences.canonicalize().unwrap().parent().unwrap(),
            directory
        );
    }
    let global = PathBuf::from(
        std::env::var_os("FEATHRISE_VALIDATION_GLOBAL_PLUGINS").expect("必须隔离全局插件根"),
    );
    let canonical_global = global.canonicalize().expect("必须先创建owner全局根");
    assert!(
        global.is_absolute()
            && canonical_global.starts_with(&directory)
            && canonical_global != directory
    );
    assert_eq!(
        fs::read(global.join(".feathrise-plugin-fixture-owner")).unwrap(),
        b"feathrise-single-binary-validation-v1"
    );
    assert!(
        app.world().resource::<WorkspaceModel>().project.is_none(),
        "不能接管已有工程"
    );
    let mut owner = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join(".plugin-host-validation-owner"))
        .expect("本轮目录不能重用");
    owner
        .write_all(b"feathrise-plugin-host-validation-v1")
        .unwrap();
    install_probe::setup(app, &directory);
    app.insert_resource(PluginHostValidation {
        directory,
        global,
        started: Instant::now(),
        step: 0,
        frames: 0,
        mouse_phase: 0,
        target: None,
        click_seen: false,
        key_phase: 0,
        issued: false,
        start_fields: Vec::new(),
        form_fields: Vec::new(),
        proofs: Vec::new(),
        captures: Vec::new(),
        capture: None,
        report: None,
        failure: None,
        open_commands: 0,
        commands: default(),
    })
    // 在现代拾取读取窗口消息前提供输入，由生产拾取及按钮系统生成三态与点击。
    .add_systems(First, input.before(bevy::picking::PickingSystems::Input))
    .add_systems(Last, drive);
}

fn ensure(condition: bool, message: &str) -> Result<(), String> {
    condition.then_some(()).ok_or_else(|| message.to_owned())
}

fn next(plan: &mut PluginHostValidation) {
    eprintln!("无工程插件管理验收阶段 {} 完成", plan.step);
    plan.proofs.push(json!({"step":plan.step,"completed":true}));
    plan.step += 1;
    plan.frames = 0;
    plan.mouse_phase = 0;
    plan.target = None;
    plan.click_seen = false;
    plan.issued = false;
}

fn action(step: u8) -> Option<Action> {
    match step {
        1 | 6 | 9 => Some(Action::ToggleMenu),
        2 | 10 => Some(Action::Plugins),
        7 => Some(Action::Show(Page::New)),
        15 => Some(Action::Show(Page::Start)),
        _ => None,
    }
}

/// 复现真实窗口后端的双通道投递，保证InputPlugin和现代拾取读取同一份输入。
fn window_input(world: &mut World, input: impl bevy::ecs::message::Message + Clone + Into<bevy::window::WindowEvent>) {
    let event: bevy::window::WindowEvent = input.clone().into();
    world.write_message(input);
    world.write_message(event);
}

fn input(world: &mut World) {
    let Some(mut plan) = world.remove_resource::<PluginHostValidation>() else {
        return;
    };
    if plan.failure.is_none()
        && let Err(error) = issue_input(world, &mut plan)
    {
        plan.failure = Some(error);
    }
    world.insert_resource(plan);
}

fn issue_input(world: &mut World, plan: &mut PluginHostValidation) -> Result<(), String> {
    let Some(window) = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .iter(world)
        .next()
    else {
        return Ok(());
    };
    if matches!(plan.step, 4 | 13) && plan.key_phase == 0 {
        for (key_code, logical_key) in [
            (KeyCode::Escape, Key::Escape),
            (KeyCode::Enter, Key::Enter),
            (KeyCode::F12, Key::F12),
        ] {
            world.write_message(KeyboardInput {
                key_code,
                logical_key,
                state: ButtonState::Pressed,
                text: None,
                repeat: false,
                window,
            });
        }
        plan.key_phase = 1;
    } else if matches!(plan.step, 5 | 14) && plan.key_phase == 1 {
        for (key_code, logical_key) in [
            (KeyCode::Escape, Key::Escape),
            (KeyCode::Enter, Key::Enter),
            (KeyCode::F12, Key::F12),
        ] {
            world.write_message(KeyboardInput {
                key_code,
                logical_key,
                state: ButtonState::Released,
                text: None,
                repeat: false,
                window,
            });
        }
        plan.key_phase = 2;
    }
    let Some(wanted) = action(plan.step) else {
        return Ok(());
    };
    // 释放已交给原按钮系统；菜单可能随业务回执隐藏，之后只由drive等待真实状态。
    // 不再定位已隐藏的旧目标，也不补发点击或写入管理器成功状态。
    if plan.mouse_phase >= 3 {
        return Ok(());
    }
    if plan.target.is_none() {
        plan.target = world
            .query::<(Entity, &Action, &ComputedNode, &InheritedVisibility)>()
            .iter(world)
            .find_map(|(entity, action, node, visible)| {
                (*action == wanted
                    && visible.get()
                    && !node.is_empty()
                    && world.get::<InteractionDisabled>(entity).is_none())
                .then_some(entity)
            });
    }
    let Some(target) = plan.target else {
        return Ok(());
    };
    let rect = bounds(world, target)?;
    world
        .get_mut::<Window>(window)
        .unwrap()
        .set_physical_cursor_position(Some(rect.center().as_dvec2()));
    let position = rect.center() / world.get::<Window>(window).unwrap().scale_factor();
    window_input(world, bevy::window::CursorMoved { window, position, delta: None });
    match plan.mouse_phase {
        0 => plan.mouse_phase = 1,
        1 if world.get::<UiInteraction>(target) == Some(&UiInteraction::Hovered) => {
            window_input(world, MouseButtonInput {
                button: MouseButton::Left,
                state: ButtonState::Pressed,
                window,
            });
            plan.mouse_phase = 2;
        }
        2 => {
            ensure(
                world.get::<UiInteraction>(target) == Some(&UiInteraction::Pressed),
                "官方鼠标按下没有进入生产拾取按下态",
            )?;
            window_input(world, MouseButtonInput {
                button: MouseButton::Left,
                state: ButtonState::Released,
                window,
            });
            plan.mouse_phase = 3;
        }
        _ => {}
    }
    Ok(())
}

fn bounds(world: &World, entity: Entity) -> Result<Rect, String> {
    let node = world
        .get::<ComputedNode>(entity)
        .ok_or("实际控件缺少ComputedNode")?;
    let at = world
        .get::<UiGlobalTransform>(entity)
        .ok_or("实际控件缺少UiGlobalTransform")?;
    ensure(!node.is_empty(), "实际控件布局为空")?;
    Ok(Rect::from_center_size(at.translation, node.size()))
}

fn below(world: &World, mut entity: Entity, root: Entity) -> bool {
    for _ in 0..16 {
        if entity == root {
            return true;
        }
        let Some(parent) = world.get::<ChildOf>(entity) else {
            return false;
        };
        entity = parent.parent();
    }
    false
}

fn fields(world: &mut World) -> Vec<(String, String)> {
    let mut values = world
        .query::<(&Input, &EditableText)>()
        .iter(world)
        .map(|(field, text)| (format!("{field:?}"), text.value().to_string()))
        .collect::<Vec<_>>();
    values.sort();
    values
}

fn panel_proof(world: &mut World) -> Result<Value, String> {
    let title = world
        .query::<(Entity, &Text, &ComputedNode, &InheritedVisibility)>()
        .iter(world)
        .find_map(|(entity, text, node, visible)| {
            (text.0 == "插件管理" && visible.get() && !node.is_empty()).then_some(entity)
        })
        .ok_or("真实插件管理标题尚未布局")?;
    let header = world
        .get::<ChildOf>(title)
        .ok_or("插件标题没有标题栏")?
        .parent();
    let panel = world
        .get::<ChildOf>(header)
        .ok_or("插件标题栏没有面板")?
        .parent();
    let rect = bounds(world, panel)?;
    let recipe = feathrise_editor_ui::dialog_panel_node();
    let node = world.get::<Node>(panel).ok_or("插件面板没有Node")?;
    ensure(
        node.border == recipe.border && node.border_radius == recipe.border_radius,
        "插件面板未采用统一弱边界与圆角",
    )?;
    ensure(
        world
            .get::<BackgroundColor>(panel)
            .is_some_and(|bg| bg.0 == feathrise_editor_ui::colors::DIALOG_BG),
        "插件面板背景没有统一",
    )?;
    let mut words = Vec::new();
    let mut disabled_project = false;
    for (entity, text, layout, computed, visible) in world
        .query::<(
            Entity,
            &Text,
            &bevy::text::TextLayoutInfo,
            &ComputedNode,
            &InheritedVisibility,
        )>()
        .iter(world)
    {
        if !visible.get()
            || computed.is_empty()
            || !below(world, entity, panel)
            || !["插件管理", "全局", "当前工程", "安装插件文件…", "刷新"].contains(&text.0.as_str())
        {
            continue;
        }
        let text_rect = bounds(world, entity)?;
        ensure(
            !layout.glyphs.is_empty() && rect.contains(text_rect.center()),
            "管理器文字没有实际glyph或超出面板",
        )?;
        if text.0 == "当前工程" {
            let button = world
                .get::<ChildOf>(entity)
                .ok_or("工程chip没有实际按钮")?
                .parent();
            disabled_project = world.get::<InteractionDisabled>(button).is_some();
        }
        words.push(json!({"text":text.0,"glyphs":layout.glyphs.len(),"bounds":[text_rect.min.x,text_rect.min.y,text_rect.max.x,text_rect.max.y]}));
    }
    ensure(
        words.len() == 5 && disabled_project,
        "无工程时当前工程chip必须真实禁用，全部标题与按钮应有glyph",
    )?;
    let directory = install_probe::directory_proof(world, panel)?;
    let installed = install_probe::visible_proof(world, panel)?;
    Ok(
        json!({"panel":[rect.min.x,rect.min.y,rect.max.x,rect.max.y],"words":words,"project_scope_disabled":disabled_project,"shared_dialog_recipe":true,
            "directory":directory,"installed":installed}),
    )
}

fn capture(world: &mut World, plan: &mut PluginHostValidation, name: &str) -> Result<(), String> {
    ensure(plan.capture.is_none(), "一次只能有一份GPU截图在途")?;
    let mut proof = panel_proof(world)?;
    let (logical, physical) = {
        let window = world.query_filtered::<&Window, With<PrimaryWindow>>().single(world).map_err(|error| error.to_string())?;
        ([window.width(), window.height()], [window.physical_width(), window.physical_height()])
    };
    let requested_width = if name.contains("narrow") { 720. } else { 1280. };
    ensure((logical[0] - requested_width).abs() < 2., "真实主窗口尚未达到本阶段1280/720逻辑宽度")?;
    proof["window"] = json!({"logical":logical,"physical":physical,"requested_width":requested_width});
    let path = plan.directory.join(name);
    let filename = name.to_owned();
    let (sender, receiver) = async_channel::bounded(1);
    plan.capture = Some(receiver);
    world.spawn(Screenshot::primary_window()).observe(move |event: On<ScreenshotCaptured>| {
        let image = event.image.clone();
        let sender = sender.clone();
        let path = path.clone();
        let filename = filename.clone();
        let proof = proof.clone();
        IoTaskPool::get().spawn(async move {
            let result = (|| {
                let image = image.try_into_dynamic().map_err(|error| error.to_string())?;
                let pixels = image.to_rgba8();
                ensure([pixels.width(), pixels.height()] == physical, "GPU截图必须来自本次实际窗口尺寸，不能沿用resize前旧帧")?;
                let colors = pixels.pixels().map(|pixel| pixel.0).collect::<HashSet<_>>().len();
                ensure(colors > 64, "真实窗口GPU截图缺少实际文字/面板像素")?;
                let mut file = OpenOptions::new().write(true).create_new(true).open(&path).map_err(|error| error.to_string())?;
                image.write_to(&mut file, bevy::image::ImageFormat::Png.as_image_crate_format().unwrap()).map_err(|error| error.to_string())?;
                file.sync_all().map_err(|error| error.to_string())?;
                Ok(json!({"file":filename,"width":pixels.width(),"height":pixels.height(),"colors":colors,
                    "source":"primary-window ScreenshotCaptured from RenderApp","panel":proof}))
            })();
            let _ = sender.try_send(result);
        }).detach();
    });
    Ok(())
}

fn drive(world: &mut World) {
    let Some(mut plan) = world.remove_resource::<PluginHostValidation>() else {
        return;
    };
    if plan.failure.is_none()
        && let Err(error) = advance(world, &mut plan)
    {
        plan.failure = Some(error);
    }
    if plan.failure.is_some() && plan.report.is_none() {
        publish(&mut plan);
    }
    if let Some(report) = &plan.report {
        match report.try_recv() {
            Ok(Ok(())) => {
                world.write_message(if plan.failure.is_none() {
                    AppExit::Success
                } else {
                    AppExit::error()
                });
                return;
            }
            Ok(Err(error)) => {
                eprintln!("插件主窗证据写入失败：{error}");
                world.write_message(AppExit::error());
                return;
            }
            Err(TryRecvError::Closed) => {
                eprintln!("插件主窗证据写者异常结束");
                world.write_message(AppExit::error());
                return;
            }
            Err(TryRecvError::Empty) => {}
        }
    }
    world.insert_resource(plan);
}

fn advance(world: &mut World, plan: &mut PluginHostValidation) -> Result<(), String> {
    ensure(
        plan.started.elapsed() < Duration::from_secs(120),
        &format!("插件无工程主窗专项超时，阶段{}", plan.step),
    )?;
    ensure(
        world.resource::<WorkspaceModel>().project.is_none(),
        "专项不能创建或打开工程",
    )?;
    for command in plan
        .commands
        .read(world.resource::<Messages<WorkspaceCommand>>())
    {
        match command {
            WorkspaceCommand::OpenPlugins => plan.open_commands += 1,
            _ => return Err(format!("插件弹窗向背景泄漏命令：{command:?}")),
        }
    }
    if let Some(target) = plan.target
        && plan.mouse_phase == 3
        && !plan.click_seen
        && world
            .get::<feathrise_editor_ui::EditorButtonClick>(target)
            .is_some_and(|click| click.is_clicked())
    {
        plan.click_seen = true;
        plan.proofs.push(json!({"action":format!("{:?}", action(plan.step)),"official_mouse_press_release":true,"production_button_click_observed":true}));
    }
    if let Some(receiver) = &plan.capture {
        match receiver.try_recv() {
            Ok(result) => {
                plan.captures.push(result?);
                plan.capture = None;
            }
            Err(TryRecvError::Closed) => return Err("GPU截图写者没有返回回执".into()),
            Err(TryRecvError::Empty) => {}
        }
    }
    let Some(manager) = world.get_resource::<PluginManagerState>() else {
        return Ok(());
    };
    ensure(
        !manager.has_project()
            && manager.scope == PluginScope::Global
            && manager.roots.global == plan.global,
        "无工程管理器没有使用本轮隔离全局根",
    )?;
    let open = manager.open;
    let captured = manager.captures_input();
    let busy = manager.busy;
    if matches!(plan.step, 4 | 13) && plan.key_phase == 1 && !open {
        ensure(captured, "Esc关闭当帧必须继续隔离背景输入")?;
        plan.proofs.push(
            json!({"escape_release_frame":plan.step,"manager_closed":true,"captured":captured,
            "enter_f12_in_same_frame":true,"no_background_command":true}),
        );
        next(plan);
        return Ok(());
    }
    plan.frames += 1;
    if plan.frames < 8 {
        return Ok(());
    }
    let page = world.resource::<Presentation>().page;
    let menu = world.resource::<Presentation>().menu;
    match plan.step {
        0 => {
            ensure(page == Page::Start, "无工程主窗必须从开始页启动")?;
            let entity = world
                .query_filtered::<Entity, With<PrimaryWindow>>()
                .single(world)
                .map_err(|error| error.to_string())?;
            ensure(
                world.get::<RawHandleWrapper>(entity).is_some(),
                "专项必须使用实际原生主窗口",
            )?;
            world
                .get_mut::<Window>(entity)
                .unwrap()
                .resolution
                .set(1280., 800.);
            plan.start_fields = fields(world);
            next(plan);
        }
        1 | 6 | 9 if plan.click_seen && menu => next(plan),
        2 | 10 if plan.click_seen && open && !busy => {
            plan.key_phase = 0;
            next(plan);
        }
        3 | 12 if open && !busy => {
            if !install_probe::ready(world, plan)? {
                return Ok(());
            }
            if !plan.issued {
                let name = if plan.step == 3 {
                    "plugin-host-wide.png"
                } else {
                    "plugin-host-narrow.png"
                };
                capture(world, plan, name)?;
                plan.issued = true;
            } else if plan.capture.is_none() {
                next(plan);
            }
        }
        5 if !captured => {
            ensure(
                page == Page::Start && fields(world) == plan.start_fields,
                "关闭插件管理后开始页现场必须保持",
            )?;
            next(plan);
        }
        7 if plan.click_seen && page == Page::New => next(plan),
        8 => {
            if !plan.issued {
                // 复用真实目录选择回填入口准备可区分的表单现场，不改生产成功/加载状态。
                fill_path(
                    world,
                    WorkspaceField::NewParent,
                    plan.directory.to_string_lossy().into_owned(),
                );
                plan.issued = true;
                plan.frames = 0;
            } else {
                plan.form_fields = fields(world);
                ensure(
                    plan.form_fields.iter().any(|(field, value)| {
                        field == "Parent" && PathBuf::from(value) == plan.directory
                    }),
                    "新建表单尚未采用自有目录",
                )?;
                next(plan);
            }
        }
        11 => {
            world
                .query_filtered::<&mut Window, With<PrimaryWindow>>()
                .single_mut(world)
                .map_err(|error| error.to_string())?
                .resolution
                .set(720., 560.);
            next(plan);
        }
        14 if !captured => {
            ensure(
                page == Page::New && fields(world) == plan.form_fields,
                "Esc及释放帧后新建表单不得被提交或重置",
            )?;
            next(plan);
        }
        15 if plan.click_seen && page == Page::Start => {
            ensure(
                fields(world) == plan.form_fields,
                "返回开始页仍须保留新建标签表单",
            )?;
            next(plan);
        }
        16 => {
            ensure(
                plan.captures.len() == 2 && plan.capture.is_none() && plan.open_commands == 2,
                "两次原入口、实际GPU截图和异步落盘回执必须齐全",
            )?;
            ensure(
                !open && !captured && !busy,
                "退出验收前管理器必须真实关闭并归还输入",
            )?;
            if plan.report.is_none() {
                publish(plan);
            }
        }
        _ => {}
    }
    Ok(())
}

fn publish(plan: &mut PluginHostValidation) {
    let report = json!({"passed":plan.failure.is_none(),"error":plan.failure,"step":plan.step,"pid":std::process::id(),
        "scope":"real primary window without project; official Bevy input and production menu/button systems",
        "physical_input_verified":false,"linux_macos_verified":false,"has_project":false,"global_root":plan.global,
        "global_scope":true,"open_plugins_commands":plan.open_commands,"proofs":plan.proofs,"captures":plan.captures,
        "start_fields":plan.start_fields,"preserved_form_fields":plan.form_fields});
    let path = plan.directory.join("evidence.json");
    let (sender, receiver) = async_channel::bounded(1);
    plan.report = Some(receiver);
    IoTaskPool::get()
        .spawn(async move {
            let result = (|| {
                let bytes =
                    serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?;
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)
                    .map_err(|error| error.to_string())?;
                file.write_all(&bytes)
                    .and_then(|_| file.sync_all())
                    .map_err(|error| error.to_string())
            })();
            let _ = sender.try_send(result);
        })
        .detach();
}
