//! 问号输入、静默释放和原正文移挂；只遍历固定帮助节点，不扫描地图或资产。

use feathrise_editor_ui::EditorButtonClick;
use super::{components::*, resources::EditorContextHelpState, types::*};
use crate::components::MapEditorSettingsPanel;
use crate::resources::{
    MapEditorLayoutState, MapEditorNumericScrubMouseGuard, MapEditorSettingsState, MapEditorState,
};
use crate::ui::{
    sidebars::{
        EditorSidebarTab, MapEditorSidebarHelpBody, MapEditorSidebarHelpButton,
        MapEditorSidebarPage, MapEditorSidebarState,
    },
    theme as colors,
    vegetation_preset_editor::VegetationPresetEditorState,
};
use bevy::{
    ecs::system::SystemParam,
    input::mouse::{MouseScrollUnit, MouseWheel},
    prelude::*,
    ui::RelativeCursorPosition,
};

/// 正式管理模态优先于帮助；可选资源兼容只安装公共帮助的布局与输入测试。
#[derive(SystemParam)]
pub(super) struct HelpModalInputs<'w> {
    browser: Option<Res<'w, crate::resources::MapBrowserState>>,
    deletion: Option<Res<'w, crate::resources::MapDeletionTask>>,
    terrain: Option<Res<'w, crate::authoring::TerrainExternalChangeState>>,
}

impl HelpModalInputs<'_> {
    fn open(&self) -> bool {
        self.browser.as_ref().is_some_and(|state| state.dialog.is_some())
            || self.deletion.as_ref().is_some_and(|state| state.confirmation.is_some())
            || self.terrain.as_ref().is_some_and(|state| state.modal_open())
    }
}

fn page(
    editor: &MapEditorState,
    sidebar: &MapEditorSidebarState,
    workspace: Option<&VegetationPresetEditorState>,
    settings: Option<&MapEditorSettingsState>,
) -> HelpPage {
    HelpPage {
        navigator: sidebar.navigator,
        module: sidebar.left,
        right: sidebar.right,
        workspace: workspace.map(|workspace| workspace.active_tab),
        map: editor.selected_map_id.clone(),
        settings: settings
            .filter(|settings| settings.open)
            .map(|settings| settings.shortcuts),
    }
}

fn page_matches(
    saved: &HelpPage,
    editor: &MapEditorState,
    sidebar: &MapEditorSidebarState,
    workspace: Option<&VegetationPresetEditorState>,
    settings: Option<&MapEditorSettingsState>,
) -> bool {
    saved.navigator == sidebar.navigator
        && saved.module == sidebar.left
        && saved.right == sidebar.right
        && saved.workspace == workspace.map(|workspace| workspace.active_tab)
        && saved.map == editor.selected_map_id
        && saved.settings
            == settings
                .filter(|settings| settings.open)
                .map(|settings| settings.shortcuts)
}

fn close(state: &mut EditorContextHelpState, sidebar: &mut MapEditorSidebarState) {
    if state.active.take().is_some() {
        state.generation = state.generation.wrapping_add(1);
    }
    state.page = None;
    state.hovered = false;
    state.from_settings = false;
    state.pointer = None;
    sidebar.close_help();
}

fn capture(
    state: &mut EditorContextHelpState,
    guard: &mut MapEditorNumericScrubMouseGuard,
    mouse: &ButtonInput<MouseButton>,
) {
    state.captured_this_frame = true;
    guard.advance(
        true,
        mouse.get_pressed().next().is_some(),
        mouse.get_just_released().next().is_some(),
    );
}

/// InheritedVisibility不表示Display；旧Pressed必须核对按钮原父链，不能借已移挂正文取得资格。
fn trigger_visible(
    entity: Entity,
    sidebar: &MapEditorSidebarState,
    layout: Option<&MapEditorLayoutState>,
    geometry: &Query<
        (
            &Node,
            Option<&ComputedNode>,
            Option<&ChildOf>,
            Option<&MapEditorSidebarPage>,
            Option<&MapEditorSettingsPanel>,
        ),
        (Without<ContextHelpBody>, Without<MapEditorSidebarHelpBody>, Without<ContextHelpPopup>),
    >,
) -> Option<bool> {
    let mut current = Some(entity);
    let mut from_settings = false;
    for _ in 0..64 {
        let Some(entity) = current else {
            return Some(from_settings);
        };
        let Ok((node, computed, parent, page, settings)) = geometry.get(entity) else {
            return None;
        };
        from_settings |= settings.is_some();
        if node.display == Display::None
            || computed.is_some_and(|node| node.size().min_element() <= 0.0)
        {
            return None;
        }
        if let Some(page) = page {
            if !sidebar.page_visible(page.0) {
                return None;
            }
            if layout.is_some_and(|layout| match page.0 {
                EditorSidebarTab::Navigator(_) => !layout.content_shown(),
                EditorSidebarTab::Left(_) | EditorSidebarTab::Right(_) => !layout.inspector_shown(),
            }) {
                return None;
            }
        }
        current = parent.map(ChildOf::parent);
    }
    None // 损坏父链或异常深层UI失败关闭，不能在输入帧扫描无界层级。
}

/// UI焦点命中后、所有世界作者输入前取得鼠标；移入帮助首帧也不会把滚轮传给地图。
pub(super) fn track_pointer(
    mut state: ResMut<EditorContextHelpState>,
    editor: Res<MapEditorState>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut guard: ResMut<MapEditorNumericScrubMouseGuard>,
    popups: Query<
        (
            &RelativeCursorPosition,
            &ComputedNode,
            Option<&InheritedVisibility>,
        ),
        With<ContextHelpPopup>,
    >,
) {
    if state.captured_this_frame {
        state.captured_this_frame = false;
    }
    let hovered = editor.open
        && state.active.is_some()
        && popups.iter().any(|(cursor, node, visible)| {
            visible.is_none_or(|visible| visible.get())
                && node.size().min_element() > 0.0
                && cursor.cursor_over()
        });
    if state.hovered != hovered {
        state.hovered = hovered;
    }
    if hovered {
        guard.advance(
            true,
            mouse.get_pressed().next().is_some(),
            mouse.get_just_released().next().is_some(),
        );
    }
}

pub(super) fn handle_input(
    editor: Res<MapEditorState>,
    mut sidebar: ResMut<MapEditorSidebarState>,
    workspace: Option<Res<VegetationPresetEditorState>>,
    settings: Option<Res<MapEditorSettingsState>>,
    mut state: ResMut<EditorContextHelpState>,
    mut guard: ResMut<MapEditorNumericScrubMouseGuard>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    buttons: Query<
        (
            Entity,
            &EditorButtonClick,
            Option<&MapEditorSidebarHelpButton>,
            Option<&ContextHelpButton>,
            Option<&InheritedVisibility>,
        ),
        (
            Changed<EditorButtonClick>,
            Or<(With<MapEditorSidebarHelpButton>, With<ContextHelpButton>)>,
        ),
    >,
    close_buttons: Query<&EditorButtonClick, (Changed<EditorButtonClick>, With<ContextHelpClose>)>,
    mut wheels: MessageReader<MouseWheel>,
    mut scrolls: Query<
        (&RelativeCursorPosition, &ComputedNode, &mut ScrollPosition),
        With<ContextHelpScroll>,
    >,
    mut observed_generation: Local<u64>,
    layout: Option<Res<MapEditorLayoutState>>,
    geometry: Query<
        (
            &Node,
            Option<&ComputedNode>,
            Option<&ChildOf>,
            Option<&MapEditorSidebarPage>,
            Option<&MapEditorSettingsPanel>,
        ),
        (Without<ContextHelpBody>, Without<MapEditorSidebarHelpBody>, Without<ContextHelpPopup>),
    >,
    owners: (
        HelpModalInputs,
        Option<Res<feathrise_editor_ui::ActiveNumericInput>>,
        Option<Res<feathrise_editor_ui::ActiveNumericInputScrub>>,
        Query<&Window, With<bevy::window::PrimaryWindow>>,
    ),
) {
    let delta: f32 = wheels
        .read()
        .map(|event| {
            -event.y
                * if event.unit == MouseScrollUnit::Line {
                    24.0
                } else {
                    1.0
                }
        })
        .sum();
    state.numeric_mouse.advance(
        owners
            .2
            .as_ref()
            .is_some_and(|active| active.entity.is_some()),
        mouse.get_pressed().next().is_some(),
        mouse.get_just_released().next().is_some(),
    );
    let numeric_owned = state.numeric_mouse.blocked()
        || owners
            .1
            .as_ref()
            .is_some_and(|active| active.entity.is_some());
    let settings_open = settings.as_ref().is_some_and(|settings| settings.open);
    let suppressed = !editor.open
        || editor.close_requested
        || editor.new_map_dialog_open
        || editor.pending_map_transition.is_some()
        || editor.map_transition_in_progress
        || editor.map_switch_confirmation.is_some()
        || owners.0.open();
    if state.active.is_some() && settings_open != state.from_settings {
        close(&mut state, &mut sidebar);
        capture(&mut state, &mut guard, &mouse);
    }
    if suppressed {
        if state.active.is_some() {
            close(&mut state, &mut sidebar);
            capture(&mut state, &mut guard, &mouse);
        }
        return;
    }
    let available = !numeric_owned
        && (!guard.blocked() || state.owns_mouse() || (settings_open && state.from_settings));
    let pressed = buttons
        .iter()
        .find_map(|(entity, interaction, legacy, _, visible)| {
            if !interaction.is_clicked() || visible.is_some_and(|visible| !visible.get())
            {
                return None;
            }
            trigger_visible(entity, &sidebar, layout.as_deref(), &geometry)
                .filter(|from_settings| settings_open == *from_settings)
                .map(|from_settings| (entity, legacy, from_settings))
        });
    if let Some((entity, legacy, from_settings)) = pressed
        && settings_open == from_settings
        && !numeric_owned
        && (available || (settings_open && from_settings))
    {
        let target = legacy.map_or(HelpTarget::Section(entity), |legacy| {
            HelpTarget::Sidebar(legacy.0)
        });
        if state.active == Some(target) {
            close(&mut state, &mut sidebar);
        } else {
            sidebar.close_help();
            if let HelpTarget::Sidebar(help) = target {
                sidebar.toggle_help(help);
            }
            state.active = Some(target);
            state.from_settings = from_settings;
            state.pointer = owners.3.single().ok()
                .and_then(Window::physical_cursor_position)
                .filter(|point| point.is_finite());
            state.page = Some(page(
                &editor,
                &sidebar,
                workspace.as_deref(),
                settings.as_deref(),
            ));
            state.generation = state.generation.wrapping_add(1);
        }
        capture(&mut state, &mut guard, &mouse);
    } else if state.active.is_some() && available {
        let escape = keys.just_pressed(KeyCode::Escape);
        let close_pressed = close_buttons
            .iter()
            .any(|interaction| interaction.is_clicked());
        let outside_pressed = !state.hovered && mouse.get_just_pressed().next().is_some();
        if escape || close_pressed || outside_pressed {
            close(&mut state, &mut sidebar);
            capture(&mut state, &mut guard, &mouse); // 关闭点击和最后release由公共门静默释放，不点透地图。
            if escape {
                keys.clear_just_pressed(KeyCode::Escape);
            }
        }
    }
    for (cursor, node, mut scroll) in &mut scrolls {
        let maximum =
            ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.0);
        let next = if *observed_generation != state.generation || state.active.is_none() {
            0.0
        } else if available && state.hovered && cursor.cursor_over() {
            (scroll.y + delta).clamp(0.0, maximum)
        } else {
            scroll.y.min(maximum)
        };
        if scroll.y != next {
            scroll.y = next;
        }
    }
    *observed_generation = state.generation;
}

/// 原家节点只记一次；正文借出时隐藏原位占用，关闭后归位，不复制动态字段或中文内容。
pub(super) fn sync_popup(
    mut commands: Commands,
    locale: Option<Res<feathrise_editor_ui::localization::EditorLocale>>,
    editor: Res<MapEditorState>,
    mut sidebar: ResMut<MapEditorSidebarState>,
    workspace: Option<Res<VegetationPresetEditorState>>,
    settings: Option<Res<MapEditorSettingsState>>,
    mut state: ResMut<EditorContextHelpState>,
    mut bodies: Query<
        (
            Entity,
            Option<&MapEditorSidebarHelpBody>,
            Option<&ContextHelpBody>,
            Option<&ChildOf>,
            Option<&ContextHelpHome>,
            Option<&ContextHelpMounted>,
            &mut Node,
        ),
        Or<(With<MapEditorSidebarHelpBody>, With<ContextHelpBody>)>,
    >,
    mut popups: Query<
        &mut Node,
        (
            With<ContextHelpPopup>,
            Without<MapEditorSidebarHelpBody>,
            Without<ContextHelpBody>,
        ),
    >,
    hosts: Query<Entity, With<ContextHelpScroll>>,
    mut titles: Query<&mut Text, With<ContextHelpTitle>>,
    mut buttons: Query<(&ContextHelpButton, &Interaction, &mut BackgroundColor)>,
    entities: Query<()>,
    modals: HelpModalInputs,
    layout: Option<Res<MapEditorLayoutState>>,
    geometry: Query<
        (
            &Node,
            Option<&ComputedNode>,
            Option<&ChildOf>,
            Option<&MapEditorSidebarPage>,
            Option<&MapEditorSettingsPanel>,
        ),
        (Without<ContextHelpBody>, Without<MapEditorSidebarHelpBody>, Without<ContextHelpPopup>),
    >,
) {
    let suppressed = !editor.open
        || editor.close_requested
        || editor.new_map_dialog_open
        || editor.pending_map_transition.is_some()
        || editor.map_transition_in_progress
        || editor.map_switch_confirmation.is_some()
        || modals.open()
        || (settings.as_ref().is_some_and(|settings| settings.open) && !state.from_settings);
    // 正文移到根之后不再继承来源显隐，必须仍核对原问号父链；设置帮助只受设置自身显隐限制。
    let source_hidden = match state.active {
        Some(HelpTarget::Section(source)) => trigger_visible(source, &sidebar, layout.as_deref(), &geometry)
            .is_none_or(|from_settings| from_settings != state.from_settings),
        _ => false,
    };
    if state.active.is_some()
        && (suppressed
            || source_hidden
            || state.page.as_ref().is_some_and(|saved| {
                !page_matches(
                    saved,
                    &editor,
                    &sidebar,
                    workspace.as_deref(),
                    settings.as_deref(),
                )
            }))
    {
        close(&mut state, &mut sidebar);
    }
    // 旧帮助可由多个原动态字段组成；检查记住的家节点，而非已借到根的正文父级。
    // 至少一个原来源仍可见才接受请求，栏目收起后也不能由旧toggle状态重新打开浮层。
    let requested_legacy = sidebar.active_help().filter(|help| {
        bodies.iter().any(|(_, legacy, _, parent, home, _, _)| {
            legacy.is_some_and(|legacy| legacy.0 == *help)
                && home.map(|home| home.parent).or_else(|| parent.map(ChildOf::parent))
                    .is_some_and(|source| trigger_visible(source, &sidebar, layout.as_deref(), &geometry) == Some(false))
        })
    });
    if sidebar.active_help().is_some() && requested_legacy.is_none() {
        close(&mut state, &mut sidebar);
    }
    // 保留原toggle_help测试/只读动态正文入口；它现在请求根浮层，不恢复正文中的高度。
    if !suppressed && let Some(help) = requested_legacy {
        if state.active != Some(HelpTarget::Sidebar(help)) {
            state.active = Some(HelpTarget::Sidebar(help));
            state.pointer = None; // 非鼠标请求按原问号位置定位，不能复用上一次点击的锚点。
            state.page = Some(page(
                &editor,
                &sidebar,
                workspace.as_deref(),
                settings.as_deref(),
            ));
            state.generation = state.generation.wrapping_add(1);
        }
    } else if matches!(state.active, Some(HelpTarget::Sidebar(_))) {
        close(&mut state, &mut sidebar);
    }
    let host = hosts.single().ok();
    let mut has_body = false;
    for (entity, legacy, generic, parent, home, mounted, mut node) in &mut bodies {
        let remembered = home.map(|home| (home.parent, home.target)).or_else(|| {
            parent.map(|parent| {
                (
                    parent.parent(),
                    legacy.map_or(HelpTarget::Section(parent.parent()), |legacy| {
                        HelpTarget::Sidebar(legacy.0)
                    }),
                )
            })
        });
        let Some((home_parent, target)) = remembered else {
            if node.display != Display::None {
                node.display = Display::None;
            }
            continue;
        };
        if !entities.contains(home_parent) {
            commands.entity(entity).try_despawn(); // 原面板已重建的旧正文不能成为孤立的第二份帮助。
            continue;
        }
        if home.is_none() {
            commands.entity(entity).insert(ContextHelpHome {
                parent: home_parent,
                target,
            });
        }
        let shown = host.is_some()
            && state.active == Some(target)
            && (legacy.is_some() || generic.is_some());
        let wanted_parent = if shown { host.unwrap() } else { home_parent };
        if parent.is_some_and(|parent| parent.parent() != wanted_parent) {
            commands.entity(entity).insert(ChildOf(wanted_parent));
        }
        if shown && mounted.is_none() {
            commands.entity(entity).insert(ContextHelpMounted);
        }
        if !shown && mounted.is_some() {
            commands.entity(entity).remove::<ContextHelpMounted>();
        }
        let display = if shown { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
        has_body |= shown;
    }
    if !has_body && state.active.is_some() {
        close(&mut state, &mut sidebar);
    }
    let title = match state.active {
        Some(HelpTarget::Sidebar(help)) => help.title(),
        Some(HelpTarget::Section(entity)) => buttons
            .get(entity)
            .map_or("操作说明", |(button, _, _)| button.0.as_str()),
        None => "操作说明",
    };
    let locale = locale.as_deref().copied().unwrap_or_default();
    let title = locale.editor_label(title);
    for mut text in &mut titles {
        if text.as_str() != title {
            **text = title.to_owned();
        }
    }
    for mut popup in &mut popups {
        let display = if has_body {
            Display::Flex
        } else {
            Display::None
        };
        if popup.display != display {
            popup.display = display;
        }
    }
    for (_, interaction, mut background) in &mut buttons {
        let color = match interaction {
            Interaction::Pressed => colors::SELECT_BG_PRESSED,
            Interaction::Hovered => colors::SELECT_BG_HOVER,
            Interaction::None => colors::BUTTON_DEV_SMALL_NORMAL,
        };
        if background.0 != color {
            background.0 = color;
        }
    }
}

/// 打开首帧就按根的真实UI/DPI倍率定位；上方使用bottom锚定，正文高度变化也不需要跳回固定角落。
pub(super) fn position_popup(
    state: Res<EditorContextHelpState>,
    roots: Query<(&ComputedNode, &UiGlobalTransform), With<crate::components::MapEditorRoot>>,
    buttons: Query<
        (Entity, &ComputedNode, &UiGlobalTransform, Option<&MapEditorSidebarHelpButton>),
        Or<(With<ContextHelpButton>, With<MapEditorSidebarHelpButton>)>,
    >,
    mut popups: Query<&mut Node, With<ContextHelpPopup>>,
) {
    let Some(target) = state.active else { return };
    let Ok((root, transform)) = roots.single() else { return };
    let inverse = root.inverse_scale_factor();
    let viewport = root.size() * inverse;
    if !viewport.is_finite() || viewport.min_element() <= 0.0 { return; }
    let root_min = transform.translation - root.size() * 0.5;
    let pointer = state.pointer.or_else(|| {
        buttons.iter().find_map(|(entity, node, transform, legacy)| {
            let matches = match target {
                HelpTarget::Section(source) => entity == source,
                HelpTarget::Sidebar(source) => legacy.is_some_and(|button| button.0 == source),
            };
            (matches && node.size().min_element() > 0.0).then_some(transform.translation)
        })
    }).unwrap_or(root_min);
    let anchor = ((pointer - root_min) * inverse).clamp(Vec2::ZERO, viewport);
    let margin = Vec2::splat(6.0).min(viewport * 0.5);
    let gap = 12.0;
    let width = 420.0_f32.min((viewport.x - margin.x * 2.0).max(0.0));
    // 优先放在点击位置右边，空间不足则翻到左边；窄窗内收，保持全部内容可达。
    let preferred_left = if anchor.x + gap + width <= viewport.x - margin.x {
        anchor.x + gap
    } else {
        anchor.x - gap - width
    };
    let left = preferred_left.clamp(margin.x, (viewport.x - margin.x - width).max(margin.x));
    let below = (viewport.y - margin.y - anchor.y - gap).max(0.0);
    let above = (anchor.y - gap - margin.y).max(0.0);
    let upwards = above > below;
    let max_height = (viewport.y * 0.65).min(if upwards { above } else { below });
    let (top, bottom) = if upwards {
        (Val::Auto, px((viewport.y - anchor.y + gap).max(margin.y)))
    } else {
        (px((anchor.y + gap).min(viewport.y - margin.y)), Val::Auto)
    };
    for mut popup in &mut popups {
        let next = (px(left), top, Val::Auto, bottom, px(width), px(max_height));
        if (popup.left, popup.top, popup.right, popup.bottom, popup.width, popup.max_height) != next {
            (popup.left, popup.top, popup.right, popup.bottom, popup.width, popup.max_height) = next;
        }
    }
}
