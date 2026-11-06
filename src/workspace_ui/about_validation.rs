//! 关于功能的真实窗口专项：用原按钮按下／松开协议取证，只允许独占目录和隔离偏好的无工程工作台。

use super::{components::*, resources::{AboutState, AboutValidation, Presentation}, *};
use bevy::{camera::RenderTarget, input::mouse::{MouseScrollUnit, MouseWheel},
    picking::{backend::HitData, events::{Drag, DragEnd, DragStart, Pointer}, pointer::{Location, PointerButton, PointerId}},
    prelude::*, render::view::screenshot::{Screenshot, save_to_disk}, ui::RelativeCursorPosition,
    ui_widgets::{Scrollbar, ScrollbarDragState, ScrollbarThumb}, window::{PrimaryWindow, WindowRef}};
use serde_json::json;
use std::{fs::OpenOptions, path::PathBuf, time::{Duration, Instant}};

pub(super) fn install(app: &mut App) {
    let Some(directory) = std::env::var_os("FEATHRISE_ABOUT_VALIDATION_DIR").map(PathBuf::from) else { return; };
    assert!(!std::env::args_os().any(|arg| arg == "--project" || arg == "--native-editor"));
    assert!(directory.is_absolute() && directory.is_dir());
    let directory = directory.canonicalize().expect("验收目录必须已独占创建");
    let preferences = PathBuf::from(std::env::var_os("FEATHRISE_PREFERENCES_FILE").expect("必须隔离个人偏好"));
    assert_eq!(preferences.parent().unwrap().canonicalize().unwrap(), directory);
    assert!(app.world().resource::<WorkspaceModel>().project.is_none(), "不能操作用户工程");
    OpenOptions::new().write(true).create_new(true).open(directory.join(".about-validation-owner"))
        .expect("不得重用验收目录");
    app.insert_resource(AboutValidation { directory, started: Instant::now(), step: 0, frames: 0,
        issued: false, pressing: None, fields: Vec::new(), proofs: Vec::new(), commands: default() })
        .add_systems(PreUpdate, input.after(bevy::ui::UiSystems::Focus)
            .before(feathrise_editor_ui::update_editor_button_clicks))
        .add_systems(Last, drive);
}

fn action(step: u8) -> Option<Action> {
    match step {
        0 | 5 | 7 | 13 => Some(Action::ToggleMenu),
        1 | 8 | 14 => Some(Action::ShowAbout),
        6 => Some(Action::Show(Page::New)),
        11 | 21 => Some(Action::CloseAbout),
        15 | 20 if about::LICENSE_PAGE_COUNT > 0 => Some(Action::OpenLicenses),
        18 => Some(Action::StepLicensePage(1)),
        19 => Some(Action::StepLicensePage(-1)),
        _ => None,
    }
}

fn input(world: &mut World) {
    let Some(mut plan) = world.remove_resource::<AboutValidation>() else { return; };
    if matches!(plan.step, 3 | 10) && !plan.issued {
        let mut keys = world.resource_mut::<ButtonInput<KeyCode>>();
        keys.press(if plan.step == 3 { KeyCode::Escape } else { KeyCode::Enter });
        keys.press(KeyCode::F12);
        plan.issued = true;
    }
    if plan.step == 16 && !plan.issued {
        let window = world.query_filtered::<Entity, With<PrimaryWindow>>().single(world).unwrap();
        for (body, mut cursor) in world.query::<(&AboutBody, &mut RelativeCursorPosition)>().iter_mut(world) {
            cursor.cursor_over = *body == AboutBody::Licenses;
        }
        world.write_message(MouseWheel { unit: MouseScrollUnit::Line, x: 0., y: -3., window,
            phase: bevy::input::touch::TouchPhase::Moved });
        plan.issued = true;
    }
    if let Some(target) = action(plan.step) {
        // 菜单自身也拥有外部按下判定；补齐原悬停来源，不能只给子按钮伪造Interaction。
        for mut cursor in world.query_filtered::<&mut bevy::ui::RelativeCursorPosition, With<MenuNode>>().iter_mut(world) {
            cursor.cursor_over = matches!(target, Action::ShowAbout | Action::Show(Page::New));
        }
        let selected = world.query::<(Entity, &Action, &InheritedVisibility, &ComputedNode, &ChildOf)>().iter(world)
            .find_map(|(entity, button, visible, node, parent)| {
                let correct_close = target != Action::CloseAbout || if plan.step == 11 {
                    world.get::<AboutFooter>(parent.parent()).is_some()
                } else { world.get::<AboutHeader>(parent.parent()).is_some() };
                (*button == target && correct_close && visible.get() && !node.is_empty()).then_some(entity)
            });
        if let Some(entity) = selected {
            world.get_mut::<bevy::ui::RelativeCursorPosition>(entity).unwrap().cursor_over = true;
            if !plan.issued {
                world.resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
                plan.pressing = Some(entity);
                plan.issued = true;
                plan.frames = 0;
                *world.get_mut::<Interaction>(entity).unwrap() = Interaction::Pressed;
            } else if plan.pressing.take().is_some() {
                world.resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
                world.get_mut::<bevy::ui::RelativeCursorPosition>(entity).unwrap().cursor_over = true;
                *world.get_mut::<Interaction>(entity).unwrap() = Interaction::Hovered;
            }
        }
    }
    world.insert_resource(plan);
}

fn next(plan: &mut AboutValidation) {
    eprintln!("关于验收阶段 {} 完成", plan.step);
    plan.proofs.push(json!({"step":plan.step,"completed":true}));
    plan.step += 1;
    plan.frames = 0;
    plan.issued = false;
}

fn capture(world: &mut World, plan: &mut AboutValidation, name: &str) {
    let (panel, node, at) = world.query_filtered::<(Entity, &ComputedNode, &UiGlobalTransform), With<AboutPanel>>()
        .single(world).expect("关于必须只有一个实际面板");
    assert!(!node.is_empty());
    let rect = Rect::from_center_size(at.translation, node.size());
    let panel_id = panel;
    let mut words = Vec::new();
    for (text, layout, node, at, visible, parent) in world.query::<(&Text, &bevy::text::TextLayoutInfo,
        &ComputedNode, &UiGlobalTransform, &InheritedVisibility, &ChildOf)>().iter(world) {
        if !visible.get() || !matches!(text.0.as_str(), "关于 Feathrise" | "Feathrise（羽立）" | "关闭 (Esc)" | "许可与署名")
            && text.0 != concat!("版本 ", env!("CARGO_PKG_VERSION")) && text.0 != about::REPOSITORY_URL { continue; }
        let bounds = Rect::from_center_size(at.translation, node.size());
        assert!(!layout.glyphs.is_empty() && rect.contains(bounds.center()), "关于文字必须真正排版在面板内");
        if text.0 == "关于 Feathrise" {
            assert!(!node.is_empty() && world.get::<AboutHeader>(parent.parent()).is_some(), "标题文字必须直接占据非零标题栏正文");
        }
        if text.0 == about::REPOSITORY_URL {
            let button = parent.parent();
            assert!(rect.contains(bounds.min) && rect.contains(bounds.max), "GitHub 地址必须完整排版在关于面板内");
            assert!(world.get::<about::AboutRepository>(button).is_some()
                && world.get::<Button>(button).is_some() && world.get::<feathrise_editor_ui::EditorButtonClick>(button).is_some(),
                "GitHub 完整地址必须由实际可点击按钮承载");
        }
        words.push(json!({"text":text.0,"bounds":[bounds.min.x,bounds.min.y,bounds.max.x,bounds.max.y]}));
    }
    assert_eq!(words.len(), 6, "名称、版本、标题、GitHub 地址、关闭按钮和已内嵌许可入口均须可见");
    assert_eq!(world.get::<Node>(panel_id).unwrap().border, feathrise_editor_ui::dialog_panel_node().border);
    plan.proofs.push(json!({"capture":name,"words":words,"panel":[rect.min.x,rect.min.y,rect.max.x,rect.max.y]}));
    world.spawn(Screenshot::primary_window()).observe(save_to_disk(plan.directory.join(name)));
}

fn license_capture(world: &mut World, plan: &mut AboutValidation, page: usize, name: &str) {
    let (text, layout, node, parent) = world.query_filtered::<(&Text, &bevy::text::TextLayoutInfo, &ComputedNode, &ChildOf),
        With<AboutLicenseText>>().single(world).unwrap();
    assert_eq!(text.as_str(), about::license_page(page));
    assert!(!layout.glyphs.is_empty() && !node.is_empty(), "内嵌原文必须在编辑器中排版");
    assert!(world.get::<AboutBody>(parent.parent()).is_some_and(|body| *body == AboutBody::Licenses));
    assert_eq!(world.get::<ScrollPosition>(parent.parent()).unwrap().y, 0., "翻页应返回正文顶部");
    plan.proofs.push(json!({"capture":name,"embedded_page":page + 1,"utf8_bytes":text.len()}));
    world.spawn(Screenshot::primary_window()).observe(save_to_disk(plan.directory.join(name)));
}

fn drag_license_scrollbar(world: &mut World, plan: &mut AboutValidation, down: bool) -> bool {
    let (bar, track, transform, children, visible) = world.query_filtered::<
        (&Scrollbar, &ComputedNode, &UiGlobalTransform, &Children, &InheritedVisibility), With<AboutLicenseScrollbar>>()
        .single(world).unwrap();
    let body = bar.target;
    assert!(world.get::<AboutBody>(body).is_some_and(|body| *body == AboutBody::Licenses));
    let viewport = world.get::<ComputedNode>(body).unwrap();
    let viewport_height = viewport.size().y * viewport.inverse_scale_factor();
    let content_height = viewport.content_size().y * viewport.inverse_scale_factor();
    let maximum = content_height - viewport_height;
    if maximum <= 0. {
        assert!(!visible.get(), "可完整显示的许可页应隐藏滚动条");
        return false;
    }
    let thumb = children.into_iter().copied().find(|&child| world.get::<ScrollbarThumb>(child).is_some()).unwrap();
    let thumb_node = world.get::<ComputedNode>(thumb).unwrap();
    let thumb_center = world.get::<UiGlobalTransform>(thumb).unwrap().translation;
    let before = world.get::<ScrollPosition>(body).unwrap().y;
    assert!(visible.get() && !track.is_empty() && !thumb_node.is_empty());
    let track_height = track.size().y;
    if down {
        let track_top = transform.translation.y - track_height / 2.;
        assert!(before > 0. && thumb_center.y - thumb_node.size().y / 2. > track_top,
            "滚轮滚动后滑块必须跟随正文下移");
    } else {
        let bottom = transform.translation.y + track_height / 2.;
        assert!((bottom - thumb_center.y - thumb_node.size().y / 2.).abs() < 2., "滑块应位于轨道底端");
        let layout = world.query_filtered::<&bevy::text::TextLayoutInfo, With<AboutLicenseText>>().single(world).unwrap();
        assert!(content_height - layout.size.y < viewport_height / 2.,
            "页底必须仍能读到正文：内容 {content_height}，排版 {}，视口 {viewport_height}", layout.size.y);
        world.spawn(Screenshot::primary_window()).observe(save_to_disk(plan.directory.join("licenses-scrollbar-bottom.png")));
    }
    let (window_id, window) = world.query_filtered::<(Entity, &Window), With<PrimaryWindow>>().single(world).unwrap();
    let scale = window.scale_factor();
    let location = Location { target: RenderTarget::Window(WindowRef::Entity(window_id)).normalize(Some(window_id)).unwrap(),
        position: thumb_center / scale };
    let button = PointerButton::Primary;
    let distance = Vec2::Y * if down { track_height / scale } else { -track_height / scale };
    let mut moved = location.clone();
    moved.position += distance;
    // 触发 Bevy 原生拖动观察器，正文位置只由实际滚动条更新。
    world.trigger(Pointer::new(PointerId::Mouse, location, DragStart {
        button, hit: HitData::new(Entity::PLACEHOLDER, 0., None, None),
    }, thumb));
    world.trigger(Pointer::new(PointerId::Mouse, moved.clone(), Drag { button, distance, delta: distance }, thumb));
    let after = world.get::<ScrollPosition>(body).unwrap().y;
    let expected = if down { maximum } else { 0. };
    assert!((after - expected).abs() < 1., "原生滑块拖动应到达正文边界");
    world.trigger(Pointer::new(PointerId::Mouse, moved, DragEnd { button, distance }, thumb));
    assert!(!world.get::<ScrollbarDragState>(thumb).unwrap().dragging);
    plan.proofs.push(json!({"license_scrollbar": if down { "drag_down" } else { "drag_up" }, "from": before, "to": after}));
    true
}

fn drive(world: &mut World) {
    let Some(mut plan) = world.remove_resource::<AboutValidation>() else { return; };
    assert!(world.resource::<WorkspaceModel>().project.is_none(), "专项不能接管作者已有工程");
    let unexpected = plan.commands.read(world.resource::<Messages<WorkspaceCommand>>())
        .any(|command| !matches!(command, WorkspaceCommand::QuitApp));
    assert!(!unexpected, "关于浮层不能泄漏新建、打开或其他后台命令");
    if plan.started.elapsed() > Duration::from_secs(90) {
        std::fs::write(plan.directory.join("about.json"), json!({"passed":false,"step":plan.step}).to_string()).unwrap();
        world.write_message(AppExit::error());
        return;
    }
    plan.frames += 1;
    if plan.frames < 8 { world.insert_resource(plan); return; }
    let open = world.resource::<AboutState>().open;
    let captured = world.resource::<AboutState>().captures_input();
    let license_page = world.resource::<AboutState>().license_page;
    let page = world.resource::<Presentation>().page;
    let menu = world.resource::<Presentation>().menu;
    match plan.step {
        0 | 5 | 7 | 13 if plan.issued && menu => {
            if plan.step == 0 {
                world.spawn(Screenshot::primary_window()).observe(save_to_disk(plan.directory.join("about-menu.png")));
            }
            next(&mut plan);
        }
        1 | 8 | 14 if plan.issued && open => next(&mut plan),
        2 => { assert!(open && page == Page::Start); capture(world, &mut plan, "about-wide.png"); next(&mut plan); }
        3 if plan.issued && !open => {
            world.resource_mut::<ButtonInput<KeyCode>>().release_all();
            next(&mut plan);
        }
        4 if !captured && page == Page::Start => next(&mut plan),
        6 if plan.issued && page == Page::New => {
            plan.fields = world.resource::<WorkspaceProbe>().fields.iter().map(|field| (field.field.clone(),field.value.clone())).collect();
            next(&mut plan);
        }
        9 => {
            world.query_filtered::<&mut Window, With<PrimaryWindow>>().single_mut(world).unwrap().resolution.set(760., 560.);
            world.resource_mut::<UiScale>().0 = 1.5;
            next(&mut plan);
        }
        10 if plan.issued => {
            assert!(open && page == Page::New, "Enter与F12不得提交或切走背后的新建表单");
            world.resource_mut::<ButtonInput<KeyCode>>().release_all();
            capture(world, &mut plan, "about-narrow-150.png"); next(&mut plan);
        }
        11 | 21 if plan.issued && !open => next(&mut plan),
        12 if !captured => {
            let fields: Vec<_> = world.resource::<WorkspaceProbe>().fields.iter().map(|field| (field.field.clone(),field.value.clone())).collect();
            assert_eq!(fields, plan.fields, "关闭关于后保留原表单"); next(&mut plan);
        }
        15 if about::LICENSE_PAGE_COUNT == 0 => { plan.step = 21; plan.frames = 0; }
        15 if plan.issued && license_page == Some(0) => {
            license_capture(world, &mut plan, 0, "licenses-in-about.png");
            next(&mut plan);
        }
        16 if plan.issued => {
            let dragged = drag_license_scrollbar(world, &mut plan, true);
            next(&mut plan);
            if !dragged { plan.step = if about::LICENSE_PAGE_COUNT == 1 { 20 } else { 18 }; }
        }
        17 => {
            assert!(drag_license_scrollbar(world, &mut plan, false));
            next(&mut plan);
            if about::LICENSE_PAGE_COUNT == 1 { plan.step = 20; }
        }
        18 if plan.issued && license_page == Some(1) => {
            license_capture(world, &mut plan, 1, "licenses-next-page.png");
            next(&mut plan);
        }
        19 if plan.issued && license_page == Some(0) => next(&mut plan),
        20 if plan.issued && open && license_page.is_none() => {
            assert_eq!(world.query_filtered::<&Text, With<AboutTitle>>().single(world).unwrap().as_str(), "关于 Feathrise");
            next(&mut plan);
        }
        22 if !captured => {
            assert_eq!(page, Page::New);
            assert!(plan.directory.join("about-wide.png").is_file() && plan.directory.join("about-narrow-150.png").is_file());
            let embedded_licenses = about::LICENSE_PAGE_COUNT > 0;
            if embedded_licenses {
                assert!(plan.directory.join("licenses-in-about.png").is_file());
            }
            if about::LICENSE_PAGE_COUNT > 1 {
                assert!(plan.directory.join("licenses-next-page.png").is_file());
            }
            let report = json!({"passed":true,"pid":std::process::id(),"version":env!("CARGO_PKG_VERSION"),
                "scope":"real_window_original_button_input","physical_input_verified":false,
                "embedded_licenses_in_about":embedded_licenses,"proofs":plan.proofs});
            std::fs::write(plan.directory.join("about.json"), serde_json::to_vec_pretty(&report).unwrap()).unwrap();
            world.write_message(WorkspaceCommand::QuitApp);
            next(&mut plan);
        }
        23 if world.resource::<crate::native_project_close::NativeProjectCloseState>().active() => {
            use bevy::ecs::system::RunSystemOnce;
            world.run_system_once(crate::native_project_close::confirm_workspace_validation_close).unwrap();
        }
        _ => {}
    }
    world.insert_resource(plan);
}
