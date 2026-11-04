//! 工程菜单中的关于浮层：共用编辑器对话框外壳，版本取自当前宿主包，输入在关闭释放帧继续隔离。

use super::{components::*, resources::{AboutState, Presentation}};
use bevy::{input::mouse::{MouseScrollUnit, MouseWheel}, input_focus::InputFocus, prelude::*,
    ui::{FocusPolicy, RelativeCursorPosition}, window::PrimaryWindow};
use feathrise_editor_map::workbench::WorkbenchHeader;
use feathrise_editor_ui::{EditorButtonClick, colors as c};

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
                            .insert(Node { flex_grow: 1., min_width: px(0), ..default() });
                        feathrise_editor_ui::spawn_custom_text_button(header, Action::CloseAbout,
                            font.clone(), "×", feathrise_editor_ui::dialog_close_button_style());
                    });
                    panel.spawn((AboutBody, RelativeCursorPosition::default(), ScrollPosition::default(),
                        Node { overflow: Overflow::scroll_y(), ..feathrise_editor_ui::dialog_body_node() },
                    )).with_children(|body| {
                        if let Some(brand) = &brand {
                            body.spawn((ImageNode::new(brand.mark.clone()), Pickable::IGNORE,
                                Node { width: px(52), height: px(52), flex_shrink: 0., ..default() }));
                        }
                        body.spawn(caption(&font, "Feathrise（羽立）", 22.));
                        // 编译期读取宿主的实际版本，升级包版本后关于内容自动同步。
                        body.spawn(caption(&font, concat!("版本 ", env!("CARGO_PKG_VERSION")), 13.));
                        body.spawn(caption(&font, "基于 Bevy 的通用 3D 编辑器。", 13.));
                        body.spawn(caption(&font, "在统一工作台中创建工程、编辑场景，并预览与运行。", 13.));
                        body.spawn(caption(&font, "羽翼升起，让想法成形。", 13.));
                    });
                    panel.spawn((AboutFooter, feathrise_editor_ui::dialog_footer_node(),
                        BackgroundColor(c::DIALOG_HEADER_BG), BorderColor::all(c::DIALOG_BORDER),
                    )).with_children(|footer| {
                        feathrise_editor_ui::spawn_custom_text_button(footer, Action::CloseAbout,
                            font.clone(), "关闭 (Esc)", feathrise_editor_ui::dialog_button_style(true));
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
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut focus: ResMut<InputFocus>,
) {
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
                _ => {}
            }
        }
        if was_open && keys.just_pressed(KeyCode::Escape) { state.open = false; }
    }
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
    mut nodes: Query<(&mut Node, Has<AboutOverlay>, Has<AboutHeader>, Has<AboutBody>, Has<AboutFooter>),
        (Without<Action>, Or<(With<AboutOverlay>, With<AboutHeader>, With<AboutBody>, With<AboutFooter>)>)>,
    mut buttons: Query<(&Action, &ChildOf, &mut Node)>,
    headers: Query<(), With<AboutHeader>>,
    mut bodies: Query<(&RelativeCursorPosition, &ComputedNode, &mut ScrollPosition), With<AboutBody>>,
    mut wheels: MessageReader<MouseWheel>,
    windows: Query<&Window, With<PrimaryWindow>>,
    scale: Option<Res<UiScale>>,
) {
    let available = windows.single().map_or(Vec2::new(1280., 720.), |window| window.size())
        / scale.map_or(1., |scale| scale.0).max(0.25);
    let compact = feathrise_editor_ui::dialog_uses_compact_layout(available);
    for (mut node, overlay, header, body, footer) in &mut nodes {
        if overlay {
            node.display = if state.open { Display::Flex } else { Display::None };
        } else {
            // 极小视口继续使用公共紧凑配方，正文独立滚动，关闭操作始终留在固定底栏。
            let recipe = if header {
                if compact { feathrise_editor_ui::dialog_compact_header_node() }
                else { feathrise_editor_ui::dialog_header_node() }
            } else if body {
                if compact { feathrise_editor_ui::dialog_compact_body_node() }
                else { feathrise_editor_ui::dialog_body_node() }
            } else if footer {
                if compact { feathrise_editor_ui::dialog_compact_footer_node() }
                else { feathrise_editor_ui::dialog_footer_node() }
            } else { continue; };
            node.padding = recipe.padding;
            node.min_height = recipe.min_height;
        }
    }
    for (action, parent, mut node) in &mut buttons {
        if *action != Action::CloseAbout { continue; }
        let style = if headers.contains(parent.parent()) {
            if compact { feathrise_editor_ui::dialog_compact_close_button_style() }
            else { feathrise_editor_ui::dialog_close_button_style() }
        } else if compact { feathrise_editor_ui::dialog_compact_button_style(true) }
        else { feathrise_editor_ui::dialog_button_style(true) };
        node.height = style.node.height;
        node.width = style.node.width;
        node.min_width = style.node.min_width;
        node.padding = style.node.padding;
    }
    for wheel in wheels.read() {
        if !state.open { continue; }
        for (cursor, node, mut offset) in &mut bodies {
            if !cursor.cursor_over() || node.is_empty() { continue; }
            let amount = if wheel.unit == MouseScrollUnit::Line { wheel.y * 24. }
                else { wheel.y * node.inverse_scale_factor() };
            let max = ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.);
            offset.y = (offset.y - amount).clamp(0., max);
        }
    }
}
