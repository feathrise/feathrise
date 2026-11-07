//! 工程菜单中的关于浮层：共用编辑器对话框外壳，版本取自当前宿主包，输入在关闭释放帧继续隔离。

use super::{components::*, resources::{AboutState, Presentation}};
use bevy::{input::mouse::{MouseScrollUnit, MouseWheel}, input_focus::InputFocus, prelude::*,
    tasks::{IoTaskPool, Task, futures::check_ready},
    ui::{FocusPolicy, RelativeCursorPosition}, ui_widgets::{ControlOrientation, Scrollbar, ScrollbarThumb},
    window::PrimaryWindow};
use feathrise_editor_map::workbench::WorkbenchHeader;
use feathrise_editor_ui::{EditorButtonClick, colors as c};

const LICENSE_DOCUMENT: &str = include_str!(concat!(env!("OUT_DIR"), "/distribution-licenses.txt"));
const LICENSE_OFFSETS: &[usize] = include!(concat!(env!("OUT_DIR"), "/distribution-license-pages.rs"));
pub(super) const LICENSE_PAGE_COUNT: usize = LICENSE_OFFSETS.len() - 1;
pub(super) const REPOSITORY_URL: &str = "https://github.com/feathrise/feathrise";

#[derive(Component)]
pub(super) struct AboutRepository;

pub(super) fn license_page(index: usize) -> &'static str {
    &LICENSE_DOCUMENT[LICENSE_OFFSETS[index]..LICENSE_OFFSETS[index + 1]]
}

fn step_page(page: Option<usize>, step: isize) -> Option<usize> {
    page?.checked_add_signed(step).filter(|&index| index < LICENSE_PAGE_COUNT)
}

use feathrise_editor_ui::localization::{EditorLocale, LocalizedText};

fn caption(font: &Handle<Font>, text: &str, size: f32) -> impl Bundle {
    (Text::new(text), TextFont { font: font.clone().into(), font_size: size.into(), ..default() },
        TextColor(c::TEXT_PRIMARY), Node { min_width: px(0), ..default() }, Pickable::IGNORE)
}

pub(super) fn mount(
    mut commands: Commands,
    headers: Query<Entity, Added<WorkbenchHeader>>,
    assets: Res<AssetServer>,
    brand: Option<Res<crate::branding::BrandImages>>,
) {
    let font = feathrise_editor_core::editor_font(&assets, feathrise_editor_core::FONT_PATH);
    for header in &headers {
        commands.entity(header).with_children(|header| {
            header.spawn((AboutOverlay, GlobalZIndex(5000), FocusPolicy::Block, Interaction::None,
                Node { display: Display::None, position_type: PositionType::Absolute,
                    left: px(0), top: px(0), width: Val::Vw(100.), height: Val::Vh(100.),
                    align_items: AlignItems::Center, justify_content: JustifyContent::Center, ..default() },
                BackgroundColor(c::DIALOG_BACKDROP),
            )).with_children(|overlay| {
                overlay.spawn((AboutPanel, FocusPolicy::Block,
                    Node { width: px(440), max_width: percent(94), max_height: Val::Vh(92.),
                        ..feathrise_editor_ui::dialog_panel_node() },
                    BackgroundColor(c::DIALOG_BG), BorderColor::all(c::DIALOG_BORDER),
                )).with_children(|panel| {
                    panel.spawn((AboutHeader, feathrise_editor_ui::dialog_header_node(),
                        BackgroundColor(c::DIALOG_HEADER_BG), BorderColor::all(c::DIALOG_BORDER),
                    )).with_children(|header| {
                        // 标题文字直接参与标题栏布局，避免空容器的零正文尺寸裁掉实际字形。
                        header.spawn(caption(&font, "关于 Feathrise", 13.))
                            .insert((AboutTitle, Node { flex_grow: 1., min_width: px(0), ..default() }));
                        feathrise_editor_ui::spawn_custom_text_button(header, Action::CloseAbout,
                            font.clone(), "×", feathrise_editor_ui::dialog_close_button_style());
                    });
                    panel.spawn((AboutBody::Overview, RelativeCursorPosition::default(), ScrollPosition::default(),
                        Node { overflow: Overflow::scroll_y(), ..feathrise_editor_ui::dialog_body_node() },
                    )).with_children(|body| {
                        if let Some(brand) = &brand {
                            body.spawn((ImageNode::new(brand.mark.clone()), Pickable::IGNORE,
                                Node { width: px(52), height: px(52), flex_shrink: 0., ..default() }));
                        }
                        body.spawn((caption(&font, "Feathrise（羽立）", 22.), LocalizedText::label("Feathrise（羽立）")));
                        // 编译期读取宿主的实际版本，升级包版本后关于内容自动同步。
                        body.spawn((caption(&font, concat!("版本 ", env!("CARGO_PKG_VERSION")), 13.),
                            LocalizedText::with_args("about.version", vec![("version", env!("CARGO_PKG_VERSION").into())])));
                        // 显示完整仓库地址，点击时交给系统默认浏览器，离线查看关于仍可读到来源。
                        feathrise_editor_ui::spawn_custom_text_button(body, AboutRepository,
                            font.clone(), REPOSITORY_URL, feathrise_editor_ui::dialog_button_style(false));
                        for copy in [
                            "基于 Bevy 的通用 3D 编辑器。",
                            "在统一工作台中创建工程、编辑场景，并预览与运行。",
                            "羽翼升起，让想法成形。",
                        ] {
                            body.spawn((caption(&font, copy, 13.), LocalizedText::label(copy)));
                        }
                    });
                    if LICENSE_PAGE_COUNT > 0 {
                        panel.spawn((AboutLicenseRegion, Node { display: Display::None,
                            width: percent(100), min_height: px(0), flex_direction: FlexDirection::Column, ..default() },
                        )).with_children(|region| {
                            let body = region.spawn((AboutBody::Licenses, RelativeCursorPosition::default(), ScrollPosition::default(),
                                Node { overflow: Overflow::scroll_y(),
                                    ..feathrise_editor_ui::dialog_body_node() },
                            )).with_children(|body| {
                                body.spawn(caption(&font, "", 13.)).insert((AboutLicenseText,
                                    TextLayout::linebreak(bevy::text::LineBreak::WordOrCharacter),
                                    // 正文高度按阅读区的换行宽度测量，滑到末尾时仍能读到最后一行。
                                    Node { width: percent(100), flex_shrink: 0., min_width: px(0), ..default() }));
                            }).id();
                            // 轨道固定在正文外侧的预留边距中，不参与正文高度测量，也不随内容滚走。
                            region.spawn((AboutLicenseScrollbar, Scrollbar::new(body, ControlOrientation::Vertical, 24.),
                                RelativeCursorPosition::default(), FocusPolicy::Block, Visibility::Hidden,
                                Node { position_type: PositionType::Absolute, width: px(8), top: px(6), bottom: px(6), right: px(6),
                                    border_radius: BorderRadius::all(px(4)), ..default() },
                                BackgroundColor(c::PANEL_BG_SECTION),
                            )).with_child((ScrollbarThumb { border_radius: BorderRadius::all(px(4)), ..default() },
                                BackgroundColor(c::COPPER_BORDER)));
                        });
                    }
                    panel.spawn((AboutFooter, feathrise_editor_ui::dialog_footer_node(),
                        BackgroundColor(c::DIALOG_HEADER_BG), BorderColor::all(c::DIALOG_BORDER),
                    )).with_children(|footer| {
                        if LICENSE_PAGE_COUNT > 0 {
                            for (step, label) in [(-1, "上一页"), (1, "下一页")] {
                                let mut style = feathrise_editor_ui::dialog_button_style(false);
                                style.node.display = Display::None;
                                feathrise_editor_ui::spawn_custom_text_button_with_text_marker(
                                    footer, Action::StepLicensePage(step), LocalizedText::label(label),
                                    font.clone(), label, style);
                            }
                            feathrise_editor_ui::spawn_custom_text_button_with_text_marker(footer, Action::OpenLicenses,
                                AboutLicenseToggleLabel, font.clone(), "许可与署名", feathrise_editor_ui::dialog_button_style(false));
                        }
                        feathrise_editor_ui::spawn_custom_text_button_with_text_marker(footer, Action::CloseAbout,
                            LocalizedText::label("关闭 (Esc)"), font.clone(), "关闭 (Esc)", feathrise_editor_ui::dialog_button_style(true));
                    });
                });
            });
        });
    }
}

pub(super) fn input(
    mut state: ResMut<AboutState>,
    mut page: ResMut<Presentation>,
    close: Option<Res<crate::native_project_close::NativeProjectCloseState>>,
    export: Option<Res<crate::project_export::ui::ExportOverlayState>>,
    plugins: Option<Res<crate::native_plugins::management::PluginManagerState>>,
    buttons: Query<(&Action, &EditorButtonClick, &InheritedVisibility), Changed<EditorButtonClick>>,
    repository_buttons: Query<(&EditorButtonClick, &InheritedVisibility), (With<AboutRepository>, Changed<EditorButtonClick>)>,
    mut repository_task: Local<Option<Task<Result<(), String>>>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut focus: ResMut<InputFocus>,
) {
    if let Some(task) = repository_task.as_mut() && let Some(result) = check_ready(task) {
        *repository_task = None;
        if let Err(error) = result { error!("{error}"); }
    }
    let was_open = state.open;
    let close_active = close.is_some_and(|close| close.captures_input());
    if close_active {
        state.open = false; // 工程关闭／退出确认优先，不能让关于挡住保存或取消选择。
    } else if plugins.is_some_and(|plugins| plugins.captures_input()) {
        // 同窗浮层互斥；管理器关闭释放帧中的旧按钮不得打开另一个浮层。
        state.open = false;
    } else {
        for (action, click, visible) in &buttons {
            if !visible.get() || !click.is_clicked() { continue; }
            match action {
                // 关于先于导出接收本帧输入；已有导出及其释放帧仍拥有旧菜单点击，不能叠开。
                Action::ShowAbout if !state.captured && !export.as_ref().is_some_and(|export| export.captures_input()) => {
                    state.open = true;
                    page.menu = false;
                    focus.clear(); // 清除原文本焦点，正文内容和草稿仍保留。
                }
                Action::CloseAbout if was_open => state.open = false,
                Action::OpenLicenses if was_open && state.open && LICENSE_PAGE_COUNT > 0 => {
                    state.license_page = state.license_page.is_none().then_some(0);
                }
                Action::StepLicensePage(step) if was_open && state.open => {
                    if let Some(page) = step_page(state.license_page, *step) {
                        state.license_page = Some(page);
                    }
                }
                _ => {}
            }
        }
        if was_open && state.open && state.license_page.is_none() && repository_task.is_none()
            && repository_buttons.iter().any(|(click, visible)| visible.get() && click.is_clicked())
        {
            // 系统打开操作与现有目录入口一致，在单个后台槽执行，主线程只轮询结果。
            *repository_task = Some(IoTaskPool::get().spawn(async {
                #[cfg(target_os = "windows")]
                let mut command = std::process::Command::new("explorer.exe");
                #[cfg(target_os = "macos")]
                let mut command = std::process::Command::new("open");
                #[cfg(not(any(target_os = "windows", target_os = "macos")))]
                let mut command = std::process::Command::new("xdg-open");
                command.arg(REPOSITORY_URL).spawn().map(|_| ())
                    .map_err(|error| format!("无法打开 GitHub 仓库：{error}"))
            }));
        }
        if was_open && keys.just_pressed(KeyCode::Escape) { state.open = false; }
    }
    if !state.open { state.license_page = None; }
    let mouse_busy = mouse.get_pressed().next().is_some()
        || mouse.get_just_pressed().next().is_some() || mouse.get_just_released().next().is_some();
    // 松开关闭按钮及按Esc的当前帧仍归浮层；鼠标静默后才允许背景接收新的动作。
    state.captured = state.open || was_open || state.captured && mouse_busy;
    if state.captured && !close_active {
        let edges: Vec<_> = keys.get_just_pressed().chain(keys.get_just_released()).copied().collect();
        for key in edges { keys.clear_just_pressed(key); keys.clear_just_released(key); }
    }
}

pub(super) fn present(
    state: Res<AboutState>,
    locale: Option<Res<EditorLocale>>,
    mut nodes: Query<(&mut Node, Has<AboutOverlay>, Has<AboutPanel>, Has<AboutHeader>, Option<&AboutBody>,
        Has<AboutLicenseRegion>, Has<AboutFooter>), (Without<Action>, Without<AboutRepository>,
        Or<(With<AboutOverlay>, With<AboutPanel>, With<AboutHeader>, With<AboutBody>, With<AboutLicenseRegion>, With<AboutFooter>)>)>,
    mut buttons: Query<(Option<&Action>, &ChildOf, &mut Node), Or<(With<Action>, With<AboutRepository>)>>,
    mut text: Query<(&mut Text, Has<AboutTitle>, Has<AboutLicenseToggleLabel>),
        Or<(With<AboutTitle>, With<AboutLicenseToggleLabel>, With<AboutLicenseText>)>>,
    headers: Query<(), With<AboutHeader>>,
    mut bodies: Query<(&AboutBody, &RelativeCursorPosition, &ComputedNode, &mut ScrollPosition)>,
    mut tracks: Query<(&Scrollbar, &RelativeCursorPosition, &mut Visibility), With<AboutLicenseScrollbar>>,
    mut previous_page: Local<Option<usize>>,
    mut wheels: MessageReader<MouseWheel>,
    windows: Query<&Window, With<PrimaryWindow>>,
    scale: Option<Res<UiScale>>,
) {
    let available = windows.single().map_or(Vec2::new(1280., 720.), |window| window.size())
        / scale.map_or(1., |scale| scale.0).max(0.25);
    let compact = feathrise_editor_ui::dialog_uses_compact_layout(available);
    let active_body = if state.license_page.is_some() { AboutBody::Licenses } else { AboutBody::Overview };
    for (mut node, overlay, panel, header, body, license_region, footer) in &mut nodes {
        if overlay {
            node.display = if state.open { Display::Flex } else { Display::None };
        } else if panel {
            node.width = px(if active_body == AboutBody::Licenses { 760. } else { 440. });
        } else if license_region {
            node.display = if active_body == AboutBody::Licenses { Display::Flex } else { Display::None };
        } else {
            // 极小视口继续使用公共紧凑配方，正文独立滚动，关闭操作始终留在固定底栏。
            let recipe = if header {
                if compact { feathrise_editor_ui::dialog_compact_header_node() }
                else { feathrise_editor_ui::dialog_header_node() }
            } else if let Some(body) = body {
                node.display = if *body == active_body { Display::Flex } else { Display::None };
                if compact { feathrise_editor_ui::dialog_compact_body_node() }
                else { feathrise_editor_ui::dialog_body_node() }
            } else if footer {
                if compact { feathrise_editor_ui::dialog_compact_footer_node() }
                else { feathrise_editor_ui::dialog_footer_node() }
            } else { continue; };
            node.padding = recipe.padding;
            if body == Some(&AboutBody::Licenses) { node.padding.right = px(22); }
            node.min_height = recipe.min_height;
        }
    }
    for (action, parent, mut node) in &mut buttons {
        let primary = match action {
            Some(Action::CloseAbout) => true,
            Some(Action::OpenLicenses) | None => false,
            Some(Action::StepLicensePage(step)) => {
                node.display = if step_page(state.license_page, *step).is_some() { Display::Flex } else { Display::None };
                false
            }
            _ => continue,
        };
        let style = if headers.contains(parent.parent()) {
            if compact { feathrise_editor_ui::dialog_compact_close_button_style() }
            else { feathrise_editor_ui::dialog_close_button_style() }
        } else if compact { feathrise_editor_ui::dialog_compact_button_style(primary) }
        else { feathrise_editor_ui::dialog_button_style(primary) };
        node.height = style.node.height;
        node.width = style.node.width;
        node.min_width = style.node.min_width;
        node.padding = style.node.padding;
    }
    let page_changed = *previous_page != state.license_page;
    let language_changed = locale.as_ref().is_some_and(|locale| locale.is_changed());
    let language = locale.as_deref().copied().unwrap_or_default();
    for (mut text, title, toggle) in &mut text {
        if !page_changed && !text.is_added() && !(language_changed && (title || toggle)) { continue; }
        text.0 = if title {
            state.license_page.map_or_else(|| language.editor_label("关于 Feathrise").into(),
                |page| language.format("about.licenses.page", &[("page", (page + 1).to_string()), ("total", LICENSE_PAGE_COUNT.to_string())]))
        } else if toggle {
            language.text(if state.license_page.is_some() { "about.return" } else { "about.licenses" }).into()
        } else {
            state.license_page.map_or("", license_page).to_owned()
        };
    }
    if page_changed {
        *previous_page = state.license_page;
        for (body, _, _, mut offset) in &mut bodies {
            if *body == AboutBody::Licenses { offset.y = 0.; }
        }
    }
    let mut pointer_over_track = false;
    for (bar, cursor, mut visibility) in &mut tracks {
        let mut scrollable = false;
        if state.open && active_body == AboutBody::Licenses
            && let Ok((_, _, node, mut offset)) = bodies.get_mut(bar.target)
            && !node.is_empty()
        {
            let max = vertical_scroll_range(node);
            if offset.y > max { offset.y = max; }
            scrollable = max > 0.;
        }
        // 隐藏短页的轨道但保留边距，正文换行宽度不随轨道显示反复变化。
        visibility.set_if_neq(if scrollable { Visibility::Inherited } else { Visibility::Hidden });
        pointer_over_track |= scrollable && cursor.cursor_over();
    }
    for wheel in wheels.read() {
        if !state.open { continue; }
        for (body, cursor, node, mut offset) in &mut bodies {
            let pointer_over = cursor.cursor_over() || *body == AboutBody::Licenses && pointer_over_track;
            if *body != active_body || !pointer_over || node.is_empty() { continue; }
            let amount = if wheel.unit == MouseScrollUnit::Line { wheel.y * 24. }
                else { wheel.y * node.inverse_scale_factor() };
            offset.y = (offset.y - amount).clamp(0., vertical_scroll_range(node));
        }
    }
}

fn vertical_scroll_range(node: &ComputedNode) -> f32 {
    ((node.content_size().y - node.size().y + node.scrollbar_size.y) * node.inverse_scale_factor()).max(0.)
}
