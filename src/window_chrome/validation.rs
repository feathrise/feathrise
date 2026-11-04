//! 显式validation下的真实主窗口验收；仅接受本轮独占目录及隔离偏好，不打开或修改用户工程。
use super::{components::{ChromeButton, Icon}, resources::ChromeParts, types::*};
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
    window::{PrimaryWindow, RawHandleWrapper},
};
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    path::PathBuf,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{HWND, RECT},
    UI::WindowsAndMessaging::*,
};

#[derive(Clone, Copy)]
struct LayoutEvidence {
    selected: [bool; 3],
    center: PixelRect,
}

#[derive(Resource)]
struct Plan {
    directory: PathBuf,
    started: Instant,
    stage: u8,
    issued: bool,
    pressing: bool,
    wait: Instant,
    baseline: Option<RECT>,
    baseline_ui_scale: Option<f32>,
    narrow_width: i32,
    initial_layout: Option<LayoutEvidence>,
    focused_layout: Option<LayoutEvidence>,
    proofs: Vec<Value>,
    hwnd: usize,
}

/// 仅既有独占顶栏验收成功后使用；普通编辑器不会安装或生成此收尾输入。
#[derive(Resource)]
struct OwnedExitConfirmation {
    started: Instant,
}

pub(super) fn install(app: &mut App) {
    let Some(directory) = std::env::var_os("FEATHRISE_CHROME_VALIDATION_DIR").map(PathBuf::from)
    else {
        return;
    };
    if crate::workspace_window::EmbedTarget::from_env()
        .ok()
        .flatten()
        .is_some()
    {
        return;
    }
    assert!(
        !std::env::args_os().any(|arg| arg == "--project" || arg == "--native-editor"),
        "顶栏验收只能从本轮无工程工作台启动"
    );
    let directory = directory
        .canonicalize()
        .expect("顶栏验收目录须由调用方独占创建");
    assert!(directory.is_absolute() && directory.is_dir());
    let preferences = std::env::var_os("FEATHRISE_PREFERENCES_FILE")
        .map(PathBuf::from)
        .expect("顶栏验收必须隔离个人偏好");
    assert!(
        preferences
            .parent()
            .and_then(|p| p.canonicalize().ok())
            .as_ref()
            == Some(&directory),
        "不得使用作者个人偏好"
    );
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(directory.join(".chrome-validation-owner"))
        .expect("拒绝复用顶栏验收目录");
    assert!(!directory.join("chrome.json").exists());
    app.insert_resource(Plan {
        directory,
        started: Instant::now(),
        stage: 9,
        issued: false,
        pressing: false,
        wait: Instant::now(),
        baseline: None,
        baseline_ui_scale: None,
        narrow_width: 0,
        initial_layout: None,
        focused_layout: None,
        proofs: Vec::new(),
        hwnd: 0,
    })
    .add_systems(
        PreUpdate,
        original_button_input
            .after(bevy::ui::UiSystems::Focus)
            .before(feathrise_editor_ui::update_editor_button_clicks),
    )
    .add_systems(Last, (drive, confirm_owned_exit).chain());
}
fn advance(plan: &mut Plan, proof: Value) {
    plan.proofs.push(proof);
    plan.stage += 1;
    plan.issued = false;
    plan.pressing = false;
    plan.wait = Instant::now();
}
fn screenshot(world: &mut World, directory: &std::path::Path, name: &str) {
    world
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(directory.join(format!("{name}.png"))));
}

fn layout_button(world: &World, index: usize) -> Option<Entity> {
    let control = *world.resource::<ChromeParts>().0.as_ref()?.layout_controls.get(index)?;
    super::scene::descendants(world, control, 2).into_iter().find(|entity| {
        world.get::<Button>(*entity).is_some()
            && world.get::<InheritedVisibility>(*entity).is_some_and(|visible| visible.get())
    })
}

/// 只读取原按钮配色与实际中央区域，不借用或改写布局资源充当成功结果。
fn layout_evidence(world: &mut World) -> Result<LayoutEvidence, String> {
    let mut selected = [false; 3];
    for (index, active) in selected.iter_mut().enumerate() {
        let button = layout_button(world, index).ok_or("缺少实际可见的布局按钮")?;
        let palette = world.get::<feathrise_editor_ui::ButtonColors>(button).ok_or("布局按钮缺少配色")?;
        *active = palette.normal == feathrise_editor_ui::colors::SELECT_BG;
    }
    let center = world.query_filtered::<(&ComputedNode, &UiGlobalTransform, &InheritedVisibility),
        With<feathrise_editor_map::workbench::WorkbenchCentralPageHost>>()
        .iter(world).find(|(node, _, visible)| visible.get() && node.size().min_element() > 0.)
        .map(|(node, transform, _)| PixelRect::from_node(node, transform))
        .ok_or("缺少实际可见的中央工作区")?;
    Ok(LayoutEvidence { selected, center })
}

fn layout_action(stage: u8) -> Option<usize> {
    match stage {
        12 | 14 | 15 | 16 | 18 | 19 => Some(2), // 从任意单侧进入专注后，再点专注都展开两栏。
        13 => Some(1),
        17 => Some(0),
        _ => None,
    }
}

fn same_horizontal(a: PixelRect, b: PixelRect) -> bool {
    (a.left - b.left).abs() <= 2 && (a.right - b.right).abs() <= 2
}

fn verify_layout_action(plan: &Plan, actual: LayoutEvidence) -> Result<&'static str, String> {
    let initial = plan.initial_layout.ok_or("缺少进入专注前的布局基准")?;
    let (expected, name) = match plan.stage {
        12 => ([false, false, true], "focus-only"),
        13 => ([false, true, false], "focus-open-right"),
        14 => ([false, false, true], "focus-from-right"),
        15 => ([true, true, false], "focus-open-both-from-right"),
        16 => ([false, false, true], "focus-before-left"),
        17 => ([true, false, false], "focus-open-left"),
        18 => ([false, false, true], "focus-from-left"),
        19 => ([true, true, false], "focus-open-both-from-left"),
        _ => return Err("不是布局交互验收阶段".into()),
    };
    if actual.selected != expected {
        return Err(format!("专注切换选中态错误：{name}，实际{:?}，期望{expected:?}", actual.selected));
    }
    let center = actual.center;
    let focused = plan.focused_layout.map(|layout| layout.center);
    let geometry_matches = match plan.stage {
        12 => center.left < initial.center.left - 4 && center.right > initial.center.right + 4,
        13 => focused.is_some_and(|full| (center.left - full.left).abs() <= 2 && center.right < full.right - 4),
        14 | 16 | 18 => focused.is_some_and(|full| same_horizontal(center, full)),
        15 | 19 => same_horizontal(center, initial.center),
        17 => focused.is_some_and(|full| center.left > full.left + 4 && (center.right - full.right).abs() <= 2),
        _ => false,
    };
    if !geometry_matches {
        return Err(format!("专注切换的实际侧栏或中央区域不符：{name} {center:?}"));
    }
    Ok(name)
}

/// 核对真实布局中的分组与相邻关系，覆盖窄窗重排；不以实体顺序冒充实际可见位置。
fn layout_proof(world: &World) -> Result<Value, String> {
    let parts = world.resource::<ChromeParts>().0.as_ref().ok_or("顶栏尚未挂载")?;
    let rect = |entity| -> Result<PixelRect, String> {
        let node = world.get::<ComputedNode>(entity).ok_or("控件尚未布局")?;
        if node.size().min_element() <= 0. {
            return Err(format!("顶栏控件不可见：{entity:?}"));
        }
        let transform = world.get::<UiGlobalTransform>(entity).ok_or("控件没有实际位置")?;
        Ok(PixelRect::from_node(node, transform))
    };
    let header = rect(parts.header)?;
    let settings = rect(parts.settings)?;
    let more = rect(parts.more_toggle)?;
    let drag = rect(parts.spacer)?;
    let run = rect(parts.run)?;
    // 开始页未装工程运行配置；工程作者顶栏有此入口时才核对其实际位置。
    let profile = parts.profile.map(rect).transpose()?;
    let captions = rect(parts.captions)?;
    if settings.right > more.left + 1 || more.right > drag.left + 1
        || drag.right > run.left + 1 || run.right > captions.left + 1
        || profile.is_some_and(|profile| run.right > profile.left + 1 || profile.right > captions.left + 1)
        || captions.right > header.right + 1
    {
        return Err(format!("顶栏分组或边界不符：{settings:?} {more:?} {drag:?} {run:?} {profile:?} {captions:?}"));
    }
    let encode = |r: PixelRect| [r.left, r.top, r.right, r.bottom];
    // 核对原槽位、真实图标与绘制归属；不能只看到三个新实体就当作原布局入口已展开。
    use feathrise_editor_map::workbench::WorkbenchHeaderItem as Item;
    let expected = [(Item::Navigation, Glyph::Navigation), (Item::Inspector, Glyph::Inspector), (Item::Focus, Glyph::Focus)];
    if parts.layout_controls.len() != expected.len() {
        return Err("顶栏必须保留三个原布局入口".into());
    }
    let mut layout_controls = Vec::new();
    let mut previous_right = profile.map_or(run.right, |profile| profile.right);
    for (&control, (item, glyph)) in parts.layout_controls.iter().zip(expected) {
        let control_rect = rect(control)?;
        if world.get::<Item>(control) != Some(&item)
            || world.get::<ChildOf>(control).map(ChildOf::parent) != Some(parts.header)
            || !world.get::<InheritedVisibility>(control).is_some_and(|visible| visible.get())
            || previous_right > control_rect.left + 1 || control_rect.right > captions.left + 1
            || control_rect.top < header.top - 1 || control_rect.bottom > header.bottom + 1
        {
            return Err(format!("原布局入口未按顺序显示在运行配置与窗口键之间：{item:?} {control_rect:?}"));
        }
        let descendants = super::scene::descendants(world, control, 3);
        let button = descendants.iter().copied().find(|entity| world.get::<Button>(*entity).is_some())
            .ok_or_else(|| format!("原布局入口缺少按钮：{item:?}"))?;
        let button_rect = rect(button)?;
        if button_rect.left < control_rect.left - 1 || button_rect.right > control_rect.right + 1
            || button_rect.top < header.top - 1 || button_rect.bottom > header.bottom + 1
        {
            return Err(format!("布局按钮的实际命中区越过原入口：{item:?} {button_rect:?}"));
        }
        if !world.get::<InheritedVisibility>(button).is_some_and(|visible| visible.get()) {
            return Err(format!("原布局按钮未真实显示：{item:?}"));
        }
        // 通用UI会补齐按钮配色；原布局系统须持续同步它的实际选中底色，而非移除组件。
        let palette = world.get::<feathrise_editor_ui::ButtonColors>(button)
            .ok_or_else(|| format!("原布局按钮缺少共享配色：{item:?}"))?;
        let background = world.get::<BackgroundColor>(button)
            .ok_or_else(|| format!("原布局按钮缺少实际底色：{item:?}"))?;
        let interaction = world.get::<Interaction>(button)
            .ok_or_else(|| format!("原布局按钮缺少原交互状态：{item:?}"))?;
        if *interaction == Interaction::None && palette.normal != background.0 {
            return Err(format!("原布局选中底色与共享普通态配色不一致：{item:?}"));
        }
        let icons: Vec<_> = descendants.iter().filter_map(|entity| world.get::<Icon>(*entity).map(|icon| icon.0)).collect();
        let labels: Vec<_> = descendants.iter().copied().filter(|entity| world.get::<Text>(*entity).is_some()).collect();
        if icons != [glyph] || labels.is_empty()
            || labels.iter().any(|entity| world.get::<Node>(*entity).is_none_or(|node| node.display != Display::None))
        {
            return Err(format!("原布局入口的图标或隐藏文字不符：{item:?}"));
        }
        layout_controls.push(json!({"item":format!("{item:?}"),"glyph":format!("{glyph:?}"),
            "rect":encode(control_rect),"button":format!("{button:?}"),"original_labels_hidden":true}));
        previous_right = control_rect.right;
    }
    Ok(json!({"settings":encode(settings),"more":encode(more),"drag":encode(drag),
        "run":encode(run),"profile":profile.map(encode),"captions":encode(captions),"header":encode(header),
        "layout_controls":layout_controls,
        "actual_ui_scale":world.get_resource::<UiScale>().map(|scale| scale.0)}))
}
fn original_button_input(world: &mut World) {
    let Some(mut plan) = world.remove_resource::<Plan>() else {
        return;
    };
    if plan.stage == 10 && !plan.issued {
        // 输入边沿必须注入在InputSystems之后，不能在Last写入后被下一帧的清理吃掉。
        world
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        plan.issued = true;
    }
    let focused = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .iter(world)
        .all(|window| window.focused)
        && world.resource::<super::resources::ChromeState>().focused;
    if !focused {
        if plan.pressing {
            world
                .resource_mut::<ButtonInput<MouseButton>>()
                .release(MouseButton::Left);
            plan.pressing = false;
            plan.issued = false;
        }
        world.insert_resource(plan);
        return;
    }
    let target = match plan.stage {
        1 | 2 => Some(ChromeButton::Window(WindowAction::Maximize)),
        3 => Some(ChromeButton::Window(WindowAction::Minimize)),
        9 => Some(ChromeButton::Toggle(Menu::More)),
        _ => None,
    };
    let selected = if let Some(index) = layout_action(plan.stage) {
        layout_button(world, index)
    } else { target.and_then(|target| {
        world
            .query::<(Entity, &ChromeButton, &InheritedVisibility)>()
            .iter(world)
            .find_map(|(entity, button, visible)| {
                let same = match (target, *button) {
                    (ChromeButton::Window(a), ChromeButton::Window(b)) => a == b,
                    (ChromeButton::Toggle(a), ChromeButton::Toggle(b)) => a == b,
                    _ => false,
                };
                (same && visible.get()).then_some(entity)
            })
    }) };
    if let Some(entity) = selected
        && (!plan.issued || plan.pressing)
    {
        if !plan.issued {
            world
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Left);
            plan.issued = true;
            plan.pressing = true;
            plan.wait = Instant::now();
        }
        let release = plan.wait.elapsed() > Duration::from_millis(120);
        if let Some(mut cursor) = world.get_mut::<bevy::ui::RelativeCursorPosition>(entity) {
            cursor.cursor_over = true;
        }
        if let Some(mut interaction) = world.get_mut::<Interaction>(entity) {
            *interaction = if release {
                Interaction::Hovered
            } else {
                Interaction::Pressed
            };
        }
        if release {
            world
                .resource_mut::<ButtonInput<MouseButton>>()
                .release(MouseButton::Left);
            plan.pressing = false;
        }
        // 只驱动原按钮输入，不写EditorButtonClick，更不直接写入窗口操作的成功结果。
    }
    world.insert_resource(plan);
}
fn finish(world: &mut World, plan: &Plan, passed: bool, error: Option<&str>) {
    let report = json!({"scope":"real_window_original_button_input_and_native_results","passed":passed,"pid":std::process::id(),"hwnd":plan.hwnd,
        "steps":plan.proofs,"error":error,"physical_input_verified":false,"elapsed_seconds":plan.started.elapsed().as_secs_f64()});
    fs::write(
        plan.directory.join("chrome.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    if passed {
        world.insert_resource(OwnedExitConfirmation { started: Instant::now() });
        unsafe {
            PostMessageW(plan.hwnd as HWND, WM_CLOSE, 0, 0);
        }
    } else {
        world.write_message(AppExit::error());
    }
}

fn confirm_owned_exit(world: &mut World) {
    let Some(pending) = world.get_resource::<OwnedExitConfirmation>() else { return; };
    // 不替可能被人工打开的工程作保存/放弃选择；本入口只拥有原无工程工作台及隔离偏好。
    if world.get_resource::<crate::workspace_ui::WorkspaceModel>().is_none_or(|model| model.project.is_some()) {
        world.remove_resource::<OwnedExitConfirmation>();
        return;
    }
    if pending.started.elapsed() > Duration::from_secs(30) {
        world.remove_resource::<OwnedExitConfirmation>();
        error!("顶栏验收已完成，但本轮无工程退出确认未能收口");
        world.write_message(AppExit::error());
        return;
    }
    if world.get_resource::<crate::native_project_close::NativeProjectCloseState>().is_some_and(|close| close.active()) {
        // 复用原关闭业务输入，仍由宿主等任务收口并发布AppExit，不直接写成功或退出授权。
        use bevy::ecs::system::RunSystemOnce;
        if let Err(error) = world.run_system_once(crate::native_project_close::confirm_workspace_validation_close) {
            error!("无法确认本轮顶栏验收退出：{error}");
        }
    }
}
fn drive(world: &mut World) {
    let Some(mut plan) = world.remove_resource::<Plan>() else {
        return;
    };
    if plan.started.elapsed() > Duration::from_secs(100) {
        finish(
            world,
            &plan,
            false,
            Some(&format!("阶段{}超时", plan.stage)),
        );
        return;
    }
    let geometry = world
        .get_resource::<ChromeGeometry>()
        .copied()
        .unwrap_or_default();
    let ready = world
        .get_resource::<ChromeParts>()
        .is_some_and(|p| p.0.is_some());
    let own = world
        .query_filtered::<&RawHandleWrapper, With<PrimaryWindow>>()
        .iter(world)
        .next()
        .and_then(crate::workspace_window::primary_hwnd);
    let Some(own) = own.filter(|_| ready) else {
        world.insert_resource(plan);
        return;
    };
    plan.hwnd = own;
    let window = own as HWND;
    let mut rect = RECT::default();
    unsafe {
        GetWindowRect(window, &mut rect);
    }
    let zoomed = unsafe { IsZoomed(window) } != 0;
    match plan.stage {
        9 if world.resource::<super::resources::ChromeState>().menu == Some(Menu::More) => {
            screenshot(world, &plan.directory, "more");
            advance(&mut plan, json!({"stage":"more_opened_by_original_input"}));
        }
        10 => {
            if plan.issued {
                world
                    .resource_mut::<ButtonInput<KeyCode>>()
                    .release(KeyCode::Escape);
                if world
                    .resource::<super::resources::ChromeState>()
                    .menu
                    .is_none()
                {
                    plan.proofs
                        .push(json!({"stage":"escape_closed_more_before_start_page_input"}));
                    plan.stage = 0;
                    plan.issued = false;
                    plan.pressing = false;
                    plan.wait = Instant::now();
                }
            }
        }
        0 if plan.started.elapsed() > Duration::from_secs(2) => {
            let layout = match layout_proof(world) {
                Ok(proof) => proof,
                Err(error) => { finish(world, &plan, false, Some(&error)); return; }
            };
            let initial = match layout_evidence(world) {
                Ok(actual) if actual.selected == [true, true, false] => actual,
                Ok(actual) => { finish(world, &plan, false, Some(&format!("隔离宽窗初始布局不是两栏展开：{:?}", actual.selected))); return; }
                Err(error) => { finish(world, &plan, false, Some(&error)); return; }
            };
            plan.initial_layout = Some(initial);
            plan.baseline = Some(rect);
            screenshot(world, &plan.directory, "wide");
            advance(
                &mut plan,
                json!({"stage":"wide_ready","layout":layout,"geometry":format!("{geometry:?}"),"outer":[rect.left,rect.top,rect.right,rect.bottom]}),
            );
            plan.stage = 12;
        }
        12..=19 if plan.issued && !plan.pressing && plan.wait.elapsed() > Duration::from_millis(350) => {
            let actual = match layout_evidence(world) {
                Ok(actual) => actual,
                Err(error) => { finish(world, &plan, false, Some(&error)); return; }
            };
            let name = match verify_layout_action(&plan, actual) {
                Ok(name) => name,
                Err(error) => { finish(world, &plan, false, Some(&error)); return; }
            };
            if plan.stage == 12 { plan.focused_layout = Some(actual); }
            screenshot(world, &plan.directory, name);
            let center = actual.center;
            let final_layout_step = plan.stage == 19;
            advance(&mut plan, json!({"stage":name,"selected":actual.selected,
                "center":[center.left,center.top,center.right,center.bottom],"original_button_input":true}));
            // 固定八步结束后继续原最大化、最小化和宽窄窗流程，不跳过旧验收。
            if final_layout_step { plan.stage = 1; }
        }
        1 => {
            if zoomed {
                screenshot(world, &plan.directory, "maximized");
                advance(
                    &mut plan,
                    json!({"stage":"maximized_by_original_ui","zoomed":true}),
                );
            }
        }
        2 if plan.wait.elapsed() > Duration::from_millis(300) => {
            if !zoomed {
                advance(
                    &mut plan,
                    json!({"stage":"restored_by_original_ui","zoomed":false}),
                );
            }
        }
        3 if plan.wait.elapsed() > Duration::from_millis(300) => {
            if unsafe { IsIconic(window) } != 0 {
                advance(
                    &mut plan,
                    json!({"stage":"minimized_by_original_ui","iconic":true}),
                );
                let own = window as usize;
                bevy::tasks::IoTaskPool::get()
                    .spawn(async move {
                        std::thread::sleep(Duration::from_millis(600));
                        let mut pid = 0;
                        unsafe {
                            if GetWindowThreadProcessId(own as HWND, &mut pid) != 0
                                && pid == std::process::id()
                            {
                                PostMessageW(own as HWND, WM_SYSCOMMAND, SC_RESTORE as usize, 0);
                            }
                        }
                    })
                    .detach();
            }
        }
        4 if unsafe { IsIconic(window) } == 0 => {
            // 原生SetWindowPos能绕过拖动的最小尺寸限制，窄窗场景也应使用产品实际允许的宽度。
            plan.narrow_width = world.query_filtered::<&Window, With<PrimaryWindow>>()
                .iter(world).next().map(|window| (window.resize_constraints.min_width * window.scale_factor()).ceil() as i32)
                .unwrap_or(760).max(760);
            unsafe {
                SetWindowPos(
                    window,
                    std::ptr::null_mut(),
                    0,
                    0,
                    plan.narrow_width,
                    620,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            advance(&mut plan, json!({"stage":"restored_after_minimize"}));
        }
        5 if plan.wait.elapsed() > Duration::from_millis(500) && rect.right - rect.left == plan.narrow_width => {
            let layout = match layout_proof(world) {
                Ok(proof) => proof,
                Err(error) => { finish(world, &plan, false, Some(&error)); return; }
            };
            screenshot(world, &plan.directory, "narrow");
            plan.proofs.push(json!({"stage":"narrow_resized","width":rect.right-rect.left,"layout":layout}));
            {
                let mut scale = world.resource_mut::<UiScale>();
                plan.baseline_ui_scale = Some(scale.0);
                scale.0 = 1.5;
            }
            // SetWindowPos可绕过系统最小尺寸；高缩放验收使用产品实际允许的最小窗口，避免测试虚构尺寸。
            let minimum_width = world.query_filtered::<&Window, With<PrimaryWindow>>()
                .iter(world).next().map(|window| (window.resize_constraints.min_width * window.scale_factor()).ceil() as i32)
                .unwrap_or(760).max(760);
            unsafe {
                SetWindowPos(window, std::ptr::null_mut(), 0, 0, minimum_width, 620,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
            }
            plan.stage = 11;
            plan.wait = Instant::now();
        }
        11 if plan.wait.elapsed() > Duration::from_millis(500) => {
            if (world.resource::<UiScale>().0 - 1.5).abs() > 0.001 {
                finish(world, &plan, false, Some("实际界面缩放未采用150%"));
                return;
            }
            let layout = match layout_proof(world) {
                Ok(proof) => proof,
                Err(error) => { finish(world, &plan, false, Some(&error)); return; }
            };
            screenshot(world, &plan.directory, "narrow-150");
            plan.proofs.push(json!({"stage":"narrow_150_percent","width":rect.right-rect.left,"layout":layout}));
            if let Some(scale) = plan.baseline_ui_scale {
                world.resource_mut::<UiScale>().0 = scale;
            }
            plan.stage = 6;
            plan.wait = Instant::now();
        }
        6 if plan.wait.elapsed() > Duration::from_millis(400) => {
            let base = plan.baseline.unwrap();
            unsafe {
                SetWindowPos(
                    window,
                    std::ptr::null_mut(),
                    base.left,
                    base.top,
                    base.right - base.left,
                    base.bottom - base.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            advance(&mut plan, json!({"stage":"size_restored"}));
        }
        7 if plan.wait.elapsed() > Duration::from_millis(400) => {
            finish(world, &plan, true, None);
            return;
        }
        _ => {}
    }
    world.insert_resource(plan);
}
