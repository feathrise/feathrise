//! 设置弹窗独立离屏GPU回归；不借资产页标题作前置断言，不启动窗口或写正式配置。

use feathrise_editor_ui::UiInteraction;
use super::components::*;
use super::*;
use crate::components::{SceneEditorShortcutBindingText, SceneEditorShortcutSettingsBody};
use bevy::ecs::system::RunSystemOnce;
use bevy::render::{
    gpu_readback::{Readback, ReadbackComplete},
    render_resource::{TextureFormat, TextureUsages},
};
use bevy::window::{ExitCondition, PrimaryWindow};
use bevy::winit::WinitPlugin;
use feathrise_editor_core::EditorShortcutAction;
use feathrise_editor_core::EditorKeyBindings;
use sha2::{Digest, Sha256};

#[derive(Resource)]
struct GpuOutput(std::path::PathBuf);

#[derive(Resource)]
struct GpuFont {
    handle: Handle<Font>,
    source: &'static str,
    sha256: String,
}

/// 独立Map夹具没有宿主的资源绑定；复用布局回归中的真实SDK字体，不能依赖迁移前的资产路径。
fn use_sdk_font(font: Res<GpuFont>, mut texts: Query<&mut TextFont>) {
    for mut text in &mut texts {
        if text.font != font.handle.clone().into() { text.font = font.handle.clone().into(); }
    }
}

fn frames(app: &mut App, count: usize) {
    for _ in 0..count {
        app.update();
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}

fn fixed_layout(app: &mut App) -> Vec<(Entity, Rect)> {
    let world = app.world_mut();
    world
        .query_filtered::<(Entity, &ComputedNode, &UiGlobalTransform), Or<(
            With<SettingsFrame>,
            With<SettingsHeader>,
            With<SettingsTabs>,
            With<SettingsFooter>,
            With<SettingsPageButton>,
        )>>()
        .iter(world)
        .map(|(entity, node, transform)| {
            (
                entity,
                Rect::from_center_size(transform.translation, node.size()),
            )
        })
        .collect()
}

fn click(app: &mut App, window: Entity, camera: Entity, action: EditorSettingsAction) {
    // 新分组增加正文长度；真实点击前先将属于常规正文的目标滚入可见区。
    let scroll = {
        let world = app.world_mut();
        let target = world.query::<(Entity, &SceneEditorSettingsButton, &ComputedNode, &UiGlobalTransform)>()
            .iter(world).find(|(_, button, _, _)| button.0 == action)
            .map(|(entity, _, node, transform)| (entity, Rect::from_center_size(transform.translation, node.size())));
        target.and_then(|(mut entity, button)| {
            for _ in 0..16 {
                if let Some(node) = world.get::<ComputedNode>(entity)
                    && world.get::<SettingsBody>(entity).is_some() {
                    let transform = world.get::<UiGlobalTransform>(entity)?;
                    let body = Rect::from_center_size(transform.translation, node.size());
                    let offset = if button.max.y > body.max.y { button.max.y - body.max.y + 4.0 }
                        else if button.min.y < body.min.y { button.min.y - body.min.y - 4.0 } else { 0.0 };
                    return (offset != 0.0).then_some((entity, offset * node.inverse_scale_factor()));
                }
                entity = world.get::<ChildOf>(entity)?.parent();
            }
            None
        })
    };
    if let Some((body, offset)) = scroll {
        app.world_mut().get_mut::<ScrollPosition>(body).unwrap().y += offset;
        frames(app, 3);
    }
    // 检查真实鼠标切页后的坐标，不能仅凭同一框架和实体仍存在判定布局稳定。
    let fixed_before = matches!(
        action,
        EditorSettingsAction::Open | EditorSettingsAction::OpenShortcuts
    )
    .then(|| fixed_layout(app));
    let (button, position) = {
        let world = app.world_mut();
        world
            .query::<(Entity, &SceneEditorSettingsButton, &UiGlobalTransform)>()
            .iter(world)
            .find(|(_, button, _)| button.0 == action)
            .map(|(entity, _, transform)| (entity, transform.translation))
            .unwrap()
    };
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_physical_cursor_position(Some(position.as_dvec2()));
    let target = app
        .world()
        .get::<bevy::camera::RenderTarget>(camera)
        .unwrap()
        .clone();
    *app.world_mut()
        .get_mut::<bevy::camera::RenderTarget>(camera)
        .unwrap() = bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(window));
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.world_mut()
        .run_system_cached(bevy::ui::ui_focus_system)
        .unwrap();
    assert_eq!(
        app.world().get::<UiInteraction>(button),
        Some(&UiInteraction::Pressed),
        "设置选项必须能由正式鼠标命中"
    );
    app.world_mut().run_system_once(feathrise_editor_ui::update_editor_button_clicks).unwrap();
    assert!(!app.world().get::<feathrise_editor_ui::EditorButtonClick>(button).unwrap().is_clicked(), "按下只显示反馈");
    app.world_mut()
        .run_system_once(handle_settings_input)
        .unwrap();
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.world_mut()
        .run_system_cached(bevy::ui::ui_focus_system)
        .unwrap();
    app.world_mut().run_system_once(feathrise_editor_ui::update_editor_button_clicks).unwrap();
    assert!(app.world().get::<feathrise_editor_ui::EditorButtonClick>(button).unwrap().is_clicked(), "原按钮内松开才执行");
    app.world_mut().run_system_once(handle_settings_input).unwrap();
    *app.world_mut()
        .get_mut::<bevy::camera::RenderTarget>(camera)
        .unwrap() = target;
    frames(app, 4);
    if let Some(before) = fixed_before {
        let after = fixed_layout(app);
        assert_eq!(before.len(), after.len());
        for (entity, expected) in before {
            let actual = after
                .iter()
                .find_map(|(current_entity, rect)| (*current_entity == entity).then_some(rect))
                .expect("切页不能替换公共控件实体");
            assert!(
                (actual.min - expected.min).abs().max_element() < 0.1
                    && (actual.max - expected.max).abs().max_element() < 0.1,
                "真实鼠标切页移动公共控件：{action:?}/{entity:?}：{expected:?} -> {actual:?}"
            );
        }
    }
}

fn capture_keys(app: &mut App, keys: &[KeyCode]) {
    let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    input.reset_all();
    for &key in keys {
        input.press(key);
    }
    drop(input);
    crate::ui::context_help::handle_input_for_test(app.world_mut());
    app.world_mut()
        .run_system_once(handle_settings_input)
        .unwrap();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    frames(app, 4);
}

fn click_shortcut_help(app: &mut App, window: Entity, camera: Entity, popup: Entity) -> (Entity, Entity) {
    use feathrise_editor_ui::context_help::{ContextHelpBody, ContextHelpButton};
    let (button, body, before_text) = {
        let world = app.world_mut();
        let button = world.query::<(Entity, &ContextHelpButton)>().iter(world)
            .find_map(|(entity, button)| (button.0 == "变换工具").then_some(entity)).unwrap();
        let (body, text) = world.query_filtered::<(Entity, &ChildOf, &Text), With<ContextHelpBody>>()
            .iter(world).find_map(|(entity, parent, text)| (parent.parent() == button).then_some((entity, text.0.clone()))).unwrap();
        (button, body, text)
    };
    // 问号属于原快捷键滚动正文；窄窗末屏可能只容纳三行中的后两行，先将原标题滚入可见区。
    let (scroll, offset) = {
        let world = app.world_mut();
        let (scroll, node, transform) = world.query_filtered::<(Entity, &ComputedNode, &UiGlobalTransform), With<SceneEditorShortcutSettingsBody>>()
            .single(world).unwrap();
        let viewport = Rect::from_center_size(transform.translation, node.size());
        let target = Rect::from_center_size(world.get::<UiGlobalTransform>(button).unwrap().translation,
            world.get::<ComputedNode>(button).unwrap().size());
        let offset = if target.max.y > viewport.max.y { target.max.y - viewport.max.y + 4.0 }
            else if target.min.y < viewport.min.y { target.min.y - viewport.min.y - 4.0 } else { 0.0 };
        (scroll, offset * node.inverse_scale_factor())
    };
    app.world_mut().get_mut::<ScrollPosition>(scroll).unwrap().y += offset;
    frames(app, 3);
    let position = app.world().get::<UiGlobalTransform>(button).unwrap().translation;
    app.world_mut().get_mut::<Window>(window).unwrap().set_physical_cursor_position(Some(position.as_dvec2()));
    let target = app.world().get::<bevy::camera::RenderTarget>(camera).unwrap().clone();
    *app.world_mut().get_mut::<bevy::camera::RenderTarget>(camera).unwrap() =
        bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(window));
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    app.world_mut().run_system_cached(bevy::ui::ui_focus_system).unwrap();
    assert_eq!(app.world().get::<UiInteraction>(button), Some(&UiInteraction::Pressed), "帮助问号必须由正式鼠标命中");
    app.world_mut().run_system_once(feathrise_editor_ui::update_editor_button_clicks).unwrap();
    app.world_mut().run_system_once(handle_settings_input).unwrap();
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    app.world_mut().run_system_cached(bevy::ui::ui_focus_system).unwrap();
    app.world_mut().run_system_once(feathrise_editor_ui::update_editor_button_clicks).unwrap();
    crate::ui::context_help::handle_input_for_test(app.world_mut());
    app.world_mut().run_system_once(handle_settings_input).unwrap();
    *app.world_mut().get_mut::<bevy::camera::RenderTarget>(camera).unwrap() = target;
    frames(app, 4);
    assert_eq!(app.world().get::<Node>(popup).unwrap().display, Display::Flex);
    assert!(app.world().get::<crate::ui::context_help::ContextHelpMounted>(body).is_some());
    let host = app.world().get::<ChildOf>(body).unwrap().parent();
    assert_eq!(app.world().get::<ChildOf>(host).unwrap().parent(), popup, "原正文必须移到同一根帮助窗口");
    assert_eq!(app.world().get::<Text>(body).unwrap().as_str(), before_text);
    assert!(app.world().resource::<SceneEditorSettingsState>().open);
    assert!(app.world().resource::<SceneEditorSettingsState>().shortcuts);
    let (rect, bounds, inverse) = {
        let world = app.world_mut();
        let node = world.get::<ComputedNode>(popup).unwrap();
        let rect = Rect::from_center_size(world.get::<UiGlobalTransform>(popup).unwrap().translation, node.size());
        let inverse = node.inverse_scale_factor();
        let (root_node, root_transform) = world.query_filtered::<(&ComputedNode, &UiGlobalTransform), With<crate::components::SceneEditorRoot>>()
            .single(world).unwrap();
        (rect, Rect::from_center_size(root_transform.translation, root_node.size()), inverse)
    };
    assert!(rect.min.cmpge(bounds.min - Vec2::splat(1.0)).all()
        && rect.max.cmple(bounds.max + Vec2::splat(1.0)).all(), "帮助必须完整位于编辑器根内：{rect:?} / {bounds:?}");
    let nearest = position.clamp(rect.min, rect.max);
    assert!((nearest - position).length() * inverse <= 18.0,
        "问号帮助应紧邻点击位置：{position:?} / {rect:?}");
    (button, body)
}

fn escape_help_only(app: &mut App, popup: Entity, button: Entity, body: Entity) {
    capture_keys(app, &[KeyCode::Escape]);
    assert_eq!(app.world().get::<Node>(popup).unwrap().display, Display::None);
    assert!(app.world().get::<crate::ui::context_help::ContextHelpMounted>(body).is_none());
    assert_eq!(app.world().get::<ChildOf>(body).unwrap().parent(), button);
    assert_eq!(app.world().get::<Node>(body).unwrap().display, Display::None);
    assert!(app.world().resource::<SceneEditorSettingsState>().open, "Esc先只关闭帮助，不能同时关闭设置");
    assert!(app.world().resource::<SceneEditorSettingsState>().shortcuts);
}

fn scroll_shortcuts_to_end(app: &mut App) {
    let body = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<SceneEditorShortcutSettingsBody>>()
            .single(world)
            .unwrap()
    };
    app.world_mut().get_mut::<ScrollPosition>(body).unwrap().y = 100_000.0;
    frames(app, 4);
}

fn snapshot(app: &mut App, target: Handle<Image>, name: &str) {
    let image = app
        .world()
        .resource::<Assets<Image>>()
        .get(&target)
        .unwrap();
    let (width, height) = (image.width(), image.height());
    let (sender, receiver) = std::sync::mpsc::channel();
    app.world_mut().spawn(Readback::texture(target)).observe(
        move |event: On<ReadbackComplete>, mut commands: Commands| {
            let _ = sender.send(event.data.clone());
            commands.entity(event.entity).try_despawn();
        },
    );
    let started = std::time::Instant::now();
    let mut bytes = loop {
        app.update();
        if let Ok(bytes) = receiver.try_recv() {
            break bytes;
        }
        assert!(started.elapsed().as_secs() < 10, "设置GPU快照超时");
        std::thread::sleep(std::time::Duration::from_millis(16));
    };
    // 测试分辨率宽度是64的倍数，BGRA四字节像素无需额外回读行填充。
    assert_eq!(bytes.len(), width as usize * height as usize * 4);
    for pixel in bytes.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    // 先保留本次实际GPU回读；后续任一可见性/字形断言失败也不能丢失诊断画面。
    // 成功证明JSON仍在全部断言之后创建，不能把失败快照误当作验收通过。
    let output = app.world().resource::<GpuOutput>().0.clone();
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(output.join(name)).unwrap();
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_raw(width, height, bytes.clone()).unwrap())
        .write_to(&mut file, image::ImageFormat::Png).unwrap();
    drop(file);
    let glyph_counts = {
        let world = app.world_mut();
        world.query::<(&Text, &bevy::text::TextLayoutInfo, &ComputedNode)>().iter(world)
            .filter(|(text, _, node)| !text.0.is_empty() && node.size().min_element() > 0.)
            .map(|(text, layout, _)| (text.0.clone(), layout.glyphs.len())).collect::<Vec<_>>()
    };
    println!("设置GPU快照 {name}: font={}, shaped_glyphs={}", app.world().resource::<GpuFont>().sha256,
        glyph_counts.iter().map(|(_, count)| count).sum::<usize>());
    assert!(
        bytes
            .chunks_exact(4)
            .filter(|pixel| pixel[0] > 100 && pixel[1] > 100 && pixel[2] > 100)
            .count()
            > 100,
        "设置文字未绘制"
    );
    let help_body = {
        let world = app.world_mut();
        world.query_filtered::<(Entity, &ComputedNode, &UiGlobalTransform), With<crate::ui::context_help::ContextHelpMounted>>()
            .single(world).ok().map(|(entity, node, transform)| {
                let rect = Rect::from_center_size(transform.translation, node.size());
                let host = world.get::<ChildOf>(entity).unwrap().parent();
                let viewport = Rect::from_center_size(world.get::<UiGlobalTransform>(host).unwrap().translation,
                    world.get::<ComputedNode>(host).unwrap().size());
                Rect { min: rect.min.max(viewport.min), max: rect.max.min(viewport.max) }
            })
    };
    let help_cover = {
        let world = app.world_mut();
        world.query_filtered::<(&Node, &ComputedNode, &UiGlobalTransform), With<crate::ui::context_help::ContextHelpPopup>>()
            .iter(world).find(|(node, computed, _)| node.display != Display::None && !computed.is_empty())
            .map(|(_, node, transform)| Rect::from_center_size(transform.translation, node.size()))
    };
    // 浮层遮住的底层键位不再采样；其他快照继续保留原键位和原生滚条的实际像素断言。
    if app.world().resource::<SceneEditorSettingsState>().shortcuts && help_body.is_none() {
        let (body, keys) = {
            let world = app.world_mut();
            let (node, transform) = world.query_filtered::<(&ComputedNode, &UiGlobalTransform), With<SceneEditorShortcutSettingsBody>>().single(world).unwrap();
            let body = Rect::from_center_size(transform.translation, node.size());
            let mut query = world.query_filtered::<
                (Entity, &Text, &ComputedNode, &UiGlobalTransform),
                With<SceneEditorShortcutBindingText>,
            >();
            let keys = query
                .iter(world)
                .map(|(entity, text, node, transform)| {
                    let parent = world.get::<ChildOf>(entity).unwrap().parent();
                    let button = world.get::<ComputedNode>(parent).unwrap();
                    let parent_transform = world.get::<UiGlobalTransform>(parent).unwrap();
                    (
                        text.as_str().to_owned(),
                        Rect::from_center_size(transform.translation, node.size()),
                        Vec2::new(
                            parent_transform.translation.x,
                            parent_transform.translation.y - button.size().y * 0.5 + 2.0,
                        ),
                    )
                })
                .collect::<Vec<_>>();
            (body, keys)
        };
        let mut visible_keys = 0;
        for (label, rect, background_sample) in keys {
            if rect.size().min_element() <= 0.0
                || !body.contains(rect.min)
                || !body.contains(rect.max)
            {
                continue;
            }
            let mut glyph_pixels = 0;
            // 从真实按钮内边距取背景，检测更亮的字形；短横线抗锯齿不能套固定亮度和像素数。
            let background_offset =
                (background_sample.y as usize * width as usize + background_sample.x as usize) * 4;
            let background_brightness = bytes[background_offset..background_offset + 3]
                .iter()
                .map(|channel| u32::from(*channel))
                .sum::<u32>();
            for row in
                (rect.min.y.max(0.0) as usize)..(rect.max.y.min(height as f32).ceil() as usize)
            {
                for column in
                    (rect.min.x.max(0.0) as usize)..(rect.max.x.min(width as f32).ceil() as usize)
                {
                    let offset = (row * width as usize + column) * 4;
                    let pixel = &bytes[offset..offset + 3];
                    glyph_pixels += usize::from(
                        pixel.iter().map(|channel| u32::from(*channel)).sum::<u32>()
                            > background_brightness + 20,
                    );
                }
            }
            assert!(glyph_pixels > 0, "真实键位文字没有绘制：{label} / {name}");
            visible_keys += 1;
        }
        if height >= 600 {
            assert!(visible_keys > 0, "快捷键页未显示任何真实键位：{name}");
        }
        let at_top = {
            let world = app.world_mut();
            world
                .query_filtered::<&ScrollPosition, With<SceneEditorShortcutSettingsBody>>()
                .single(world)
                .unwrap()
                .y
                < 0.1
        };
        if at_top
            && app
                .world()
                .resource::<SceneEditorSettingsState>()
                .shortcut_notice
                .is_empty()
        {
            assert!(
                visible_keys >= 2,
                "窄窗首屏必须绘制至少两项真实键位：{name}"
            );
        }
        let (track, thumb) = {
            let world = app.world_mut();
            let (node, transform) = world.query_filtered::<(&ComputedNode, &UiGlobalTransform), With<ShortcutScrollbarTrack>>().single(world).unwrap();
            let track = Rect::from_center_size(transform.translation, node.size());
            let (node, transform) = world.query_filtered::<(&ComputedNode, &UiGlobalTransform), With<ShortcutScrollbarThumb>>().single(world).unwrap();
            (
                track,
                Rect::from_center_size(transform.translation, node.size()),
            )
        };
        assert!(thumb.size().min_element() > 0.0);
        let thumb_offset =
            (thumb.center().y as usize * width as usize + thumb.center().x as usize) * 4;
        let rail_sample = if thumb.center().y < track.center().y {
            track.max.y - 2.0
        } else {
            track.min.y + 2.0
        };
        let rail_offset = (rail_sample as usize * width as usize + track.center().x as usize) * 4;
        assert!(
            bytes[thumb_offset..thumb_offset + 3]
                .iter()
                .zip(&bytes[rail_offset..rail_offset + 3])
                .map(|(thumb, rail)| (*thumb as i32 - *rail as i32).abs())
                .sum::<i32>()
                > 5,
            "原生滑块必须真实绘制并与轨道可区分：{name}"
        );
    }
    if let Some(rect) = help_body {
        assert!(rect.size().min_element() > 0.0, "帮助原正文必须进入真实可见视区");
        let mut glyph_pixels = 0;
        for row in (rect.min.y.max(0.0) as usize)..(rect.max.y.min(height as f32).ceil() as usize) {
            for column in (rect.min.x.max(0.0) as usize)..(rect.max.x.min(width as f32).ceil() as usize) {
                let offset = (row * width as usize + column) * 4;
                glyph_pixels += usize::from(bytes[offset..offset + 3].iter().all(|channel| *channel > 100));
            }
        }
        assert!(glyph_pixels > 20, "移挂后的帮助正文必须实际绘制：{name}");
    }
    let (frame, header, footer) = {
        let world = app.world_mut();
        let frame = world.query_filtered::<Entity, With<SettingsFrame>>().single(world).unwrap();
        let header = world.query_filtered::<Entity, With<SettingsHeader>>().single(world).unwrap();
        let footer = world.query_filtered::<Entity, With<SettingsFooter>>().single(world).unwrap();
        (frame, header, footer)
    };
    let rect = |entity: Entity| {
        let node = app.world().get::<ComputedNode>(entity).unwrap();
        Rect::from_center_size(app.world().get::<UiGlobalTransform>(entity).unwrap().translation, node.size())
    };
    let frame_rect = rect(frame);
    let header_rect = rect(header);
    let footer_rect = rect(footer);
    let common = feathrise_editor_ui::dialog_panel_node();
    let frame_node = app.world().get::<Node>(frame).unwrap();
    assert_eq!(frame_node.border, common.border, "设置必须使用公共细边界");
    assert_eq!(frame_node.border_radius, common.border_radius);
    assert_eq!(app.world().get::<BorderColor>(frame).unwrap(), &BorderColor::all(feathrise_editor_ui::colors::DIALOG_BORDER));
    // 亮边框不算中文字形。标题、两个页签与完成操作必须具有真实字体布局，防止空壳截图误过。
    for label in ["设置", "常规", "快捷键", "完成"] {
        assert!(glyph_counts.iter().any(|(text, count)| text == label && *count > 0),
            "设置公共文字必须拥有真实字形：{label} / {name}");
    }
    // 按真正渲染的共享标题栏/底栏取样；主题切换不能只改变逻辑Color而漏掉实际绘制。
    let mut chrome_samples = Vec::new();
    for (role, area, logical) in [
        ("标题栏", header_rect, feathrise_editor_ui::colors::DIALOG_HEADER_BG),
        ("底栏", footer_rect, feathrise_editor_ui::colors::DIALOG_HEADER_BG),
    ] {
        let sample = [area.min.x + 4., area.max.x - 4.].into_iter()
            .map(|x| Vec2::new(x, area.center().y))
            .find(|point| help_cover.is_none_or(|cover| !cover.contains(*point)));
        let Some(sample) = sample else {
            // 帮助浮层真实覆盖整条底栏时，像素属于帮助窗口；保留遮挡身份，不能冒充底层主题颜色。
            chrome_samples.push(serde_json::json!({"role":role,"occluded_by_help":true}));
            continue;
        };
        let offset = ((sample.y as usize) * width as usize + sample.x as usize) * 4;
        let expected = app.world().resource::<feathrise_editor_ui::EditorPalette>().resolve(logical).to_srgba();
        for (actual, expected) in bytes[offset..offset + 3].iter().zip([expected.red, expected.green, expected.blue]) {
            assert!((*actual as f32 - expected * 255.0).abs() <= 3.0, "{role}必须使用公共背景和当前主题：{name}");
        }
        chrome_samples.push(serde_json::json!({"role":role,"occluded_by_help":false,"point":sample.to_array(),
            "rgba":&bytes[offset..offset+4]}));
    }
    let font = app.world().resource::<GpuFont>();
    let proof = serde_json::json!({"source":"production SettingsFrame + GPU pixels", "image":name,
        "frame":[frame_rect.min.x,frame_rect.min.y,frame_rect.max.x,frame_rect.max.y],
        "header":[header_rect.min.x,header_rect.min.y,header_rect.max.x,header_rect.max.y],
        "footer":[footer_rect.min.x,footer_rect.min.y,footer_rect.max.x,footer_rect.max.y],
        "border":format!("{:?}",frame_node.border),"radius":format!("{:?}",frame_node.border_radius),
        "font_source":font.source,"font_sha256":font.sha256.as_str(),"glyph_counts":glyph_counts,
        "chrome_samples":chrome_samples});
    serde_json::to_writer_pretty(std::fs::OpenOptions::new().write(true).create_new(true)
        .open(output.join(format!("{name}.json"))).unwrap(), &proof).unwrap();
}

#[test]
#[ignore = "需要真实GPU；验证设置两页、三套配色、正式点击/录入及窄窗大倍率，不打开原生窗口"]
fn settings_gpu_three_themes_clicks_and_narrow_high_scale() {
    let output = std::path::PathBuf::from(std::env::var_os("FEATHRISE_SETTINGS_UI_GPU_REPORT")
        .expect("显式指定本轮独占FEATHRISE_SETTINGS_UI_GPU_REPORT"));
    assert!(output.is_absolute());
    std::fs::create_dir(&output).expect("输出目录必须为本轮新建，禁止覆盖旧截图");
    serde_json::to_writer_pretty(std::fs::OpenOptions::new().write(true).create_new(true)
        .open(output.join("owner.json")).unwrap(),
        &serde_json::json!({"fixture":"editor-settings-ui-v2","pid":std::process::id()})).unwrap();
    let mut app = App::new();
    app.insert_resource(GpuOutput(output));
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .set(AssetPlugin {
                file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets").into(),
                watch_for_changes_override: Some(false),
                ..default()
            })
            .disable::<WinitPlugin>()
            .disable::<bevy::log::LogPlugin>(),
    )
    .add_plugins(feathrise_editor_ui::EditorThemePlugin)
    .add_plugins(feathrise_editor_ui::EditorButtonInputPlugin)
    .insert_resource(SceneEditorState {
        open: true,
        ..default()
    })
    .insert_resource(SceneEditorSettingsState {
        open: true,
        ..default()
    })
    .init_resource::<SceneEditorNumericScrubMouseGuard>()
    .init_resource::<crate::resources::SceneEditorLayoutState>()
    .init_resource::<crate::ui::sidebars::SceneEditorSidebarState>()
    .add_systems(Update, use_sdk_font)
    .add_systems(
        Update,
        (
            scroll_settings_body,
            crate::ui::shortcut_settings::scroll_shortcut_settings,
        )
            .chain(),
    )
    .add_systems(
        PostUpdate,
        (
            sync_settings_panel,
            crate::ui::shortcut_settings::sync_shortcut_settings_panel,
        )
            .chain()
            .before(bevy::ui::UiSystems::Prepare),
    );
    crate::ui::theme::install(&mut app);
    crate::ui::context_help::install(&mut app);
    let font_source = "embedded://feathrise_runtime/default_font";
    let font_bytes = feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec();
    let font_sha = Sha256::digest(&font_bytes).iter().map(|byte| format!("{byte:02x}")).collect();
    let handle = app.world_mut().resource_mut::<Assets<Font>>().add(Font::from_bytes(font_bytes));
    app.insert_resource(GpuFont { handle, source: font_source, sha256: font_sha });
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
        .set_physical_resolution(896, 720);
    let target = app.world_mut().resource_mut::<Assets<Image>>().add({
        let mut image = Image::new_target_texture(896, 720, TextureFormat::Bgra8UnormSrgb, None);
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        image
    });
    let camera = app
        .world_mut()
        .spawn((Camera2d, bevy::camera::RenderTarget::from(target.clone())))
        .id();
    let root = app
        .world_mut()
        .spawn((
            crate::components::SceneEditorRoot,
            bevy::ui::UiTargetCamera(camera),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
            BackgroundColor(colors::PANEL_BG),
        ))
        .id();
    // 三套配色截图沿用 F12 面板的设置分组。
    app.world_mut()
        .run_system_once::<_, (), _>((|| settings_panel(false)).spawn())
        .unwrap();
    let panel = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<SceneEditorSettingsPanel>>()
            .single(world)
            .unwrap()
    };
    app.world_mut().entity_mut(panel).insert(ChildOf(root));
    let help_popup = app.world_mut().spawn_scene(crate::ui::context_help::popup()).unwrap().id();
    app.world_mut().entity_mut(help_popup).insert(ChildOf(root)); // 唯一根浮层，设置与帮助复用正式父级关系。
    frames(&mut app, 30);
    assert!(app.world().resource::<EditorSettings>().editor_show_terrain_backfaces);
    click(&mut app, window, camera, EditorSettingsAction::ToggleTerrainBackfaces);
    assert!(!app.world().resource::<EditorSettings>().editor_show_terrain_backfaces);
    snapshot(&mut app, target.clone(), "editor-settings-terrain-backfaces-off.png");
    click(&mut app, window, camera, EditorSettingsAction::ToggleTerrainBackfaces);
    assert!(app.world().resource::<EditorSettings>().editor_show_terrain_backfaces);
    snapshot(&mut app, target.clone(), "editor-settings-terrain-backfaces-on.png");
    for theme in EditorColorTheme::ALL {
        click(
            &mut app,
            window,
            camera,
            EditorSettingsAction::SetColorTheme(theme),
        );
        assert_eq!(
            app.world().resource::<EditorSettings>().editor_color_theme,
            theme
        );
        assert_eq!(
            app.world()
                .resource::<feathrise_editor_ui::EditorPalette>()
                .name(),
            theme.as_str()
        );
        assert!(!app.world().resource::<SceneEditorState>().save_requested);
        snapshot(
            &mut app,
            target.clone(),
            &format!("editor-settings-{}.png", theme.as_str()),
        );
        click(
            &mut app,
            window,
            camera,
            EditorSettingsAction::OpenShortcuts,
        );
        assert!(app.world().resource::<SceneEditorSettingsState>().open);
        snapshot(
            &mut app,
            target.clone(),
            &format!("editor-settings-{}-shortcuts.png", theme.as_str()),
        );
        click(
            &mut app,
            window,
            camera,
            EditorSettingsAction::RebindShortcut(EditorShortcutAction::FocusSelected),
        );
        capture_keys(&mut app, &[KeyCode::Home]);
        assert_eq!(
            app.world().resource::<SceneEditorSettingsState>().listening,
            Some(EditorShortcutAction::FocusSelected)
        );
        assert_eq!(
            app.world()
                .resource::<SceneEditorSettingsState>()
                .shortcut_notice_kind,
            EditorShortcutNoticeKind::Warning
        );
        assert_eq!(
            app.world().resource::<EditorSettings>().editor_keybindings,
            EditorKeyBindings::default()
        );
        snapshot(
            &mut app,
            target.clone(),
            &format!("editor-settings-{}-conflict.png", theme.as_str()),
        );
        capture_keys(
            &mut app,
            &[
                KeyCode::ControlLeft,
                KeyCode::ShiftLeft,
                KeyCode::AltLeft,
                KeyCode::KeyG,
            ],
        );
        assert_eq!(
            app.world()
                .resource::<EditorSettings>()
                .editor_keybindings
                .get(EditorShortcutAction::FocusSelected)
                .unwrap()
                .display(),
            "Ctrl+Shift+Alt+G"
        );
        click(
            &mut app,
            window,
            camera,
            EditorSettingsAction::UnbindShortcut(EditorShortcutAction::FocusSelected),
        );
        assert!(
            app.world()
                .resource::<EditorSettings>()
                .editor_keybindings
                .get(EditorShortcutAction::FocusSelected)
                .is_none()
        );
        click(
            &mut app,
            window,
            camera,
            EditorSettingsAction::ResetShortcuts,
        );
        assert_eq!(
            app.world().resource::<EditorSettings>().editor_keybindings,
            EditorKeyBindings::default()
        );
        scroll_shortcuts_to_end(&mut app);
        let (help_button, help_body) = click_shortcut_help(&mut app, window, camera, help_popup);
        snapshot(
            &mut app,
            target.clone(),
            &format!("editor-settings-{}-help.png", theme.as_str()),
        );
        escape_help_only(&mut app, help_popup, help_button, help_body);
        click(&mut app, window, camera, EditorSettingsAction::Open);
        assert!(!app.world().resource::<SceneEditorState>().dirty);
    }
    click(
        &mut app,
        window,
        camera,
        EditorSettingsAction::SetScale(150),
    );
    app.world_mut().resource_mut::<UiScale>().0 = 1.5;
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .resolution
        .set_physical_resolution(384, 360);
    app.world_mut()
        .resource_mut::<Assets<Image>>()
        .get_mut(&target)
        .unwrap()
        .resize(bevy::render::render_resource::Extent3d {
            width: 384,
            height: 360,
            depth_or_array_layers: 1,
        });
    frames(&mut app, 8);
    click(&mut app, window, camera, EditorSettingsAction::ToggleTerrainBackfaces);
    assert!(!app.world().resource::<EditorSettings>().editor_show_terrain_backfaces);
    click(&mut app, window, camera, EditorSettingsAction::ToggleTerrainBackfaces);
    snapshot(&mut app, target.clone(), "editor-settings-narrow-150.png");
    click(
        &mut app,
        window,
        camera,
        EditorSettingsAction::OpenShortcuts,
    );
    snapshot(
        &mut app,
        target.clone(),
        "editor-settings-narrow-150-shortcuts.png",
    );
    click(
        &mut app,
        window,
        camera,
        EditorSettingsAction::RebindShortcut(EditorShortcutAction::FocusSelected),
    );
    capture_keys(&mut app, &[KeyCode::Home]);
    snapshot(
        &mut app,
        target.clone(),
        "editor-settings-narrow-150-conflict.png",
    );
    capture_keys(&mut app, &[KeyCode::Escape]);
    click(&mut app, window, camera, EditorSettingsAction::Open);
    click(
        &mut app,
        window,
        camera,
        EditorSettingsAction::OpenShortcuts,
    );
    scroll_shortcuts_to_end(&mut app);
    snapshot(
        &mut app,
        target.clone(),
        "editor-settings-narrow-150-shortcuts-bottom.png",
    );
    let (help_button, help_body) = click_shortcut_help(&mut app, window, camera, help_popup);
    snapshot(&mut app, target.clone(), "editor-settings-narrow-150-help.png");
    escape_help_only(&mut app, help_popup, help_button, help_body);
    click(&mut app, window, camera, EditorSettingsAction::Open);
}
