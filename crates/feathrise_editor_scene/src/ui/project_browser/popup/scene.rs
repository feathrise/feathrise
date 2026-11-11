//! 目录浮层按实际布局锚定，九种类型和少量动作槽位复用同一组固定节点。

use super::{Choices, PopupKind, ProjectAnimationClip, ProjectAnimationExpand, ProjectBrowserPopup, ProjectTypeFilterOption};
use crate::{
    asset_catalog::{AssetTypeFilter, EditorAssetCatalog},
    components::{AssetRowBinding, SceneEditorRoot},
    ui::{asset_preview::PickProjectAnimationClip, project_browser::{ProjectBrowserState, filter}, scene::FONT_PATH, theme as colors},
};
use bevy::{prelude::*, ui::{FocusPolicy, RelativeCursorPosition}};
use feathrise_editor_core::{AssetBrowserViewMode, EditorSettings};
use feathrise_editor_ui::{EditorButtonClick, InteractionButton, UiInteraction, localization::EditorLocale, metrics};

const OPTION_HEIGHT: f32 = 24.0;
const OPTION_GAP: f32 = 2.0;
pub(super) const OPTION_PITCH: f32 = OPTION_HEIGHT + OPTION_GAP;
const MENU_INSET: f32 = 5.0; // 四像素内边距加一像素边框。
const CLIP_SLOTS: usize = 8;

#[derive(Component)]
pub(in crate::ui::project_browser) struct Backdrop;

pub(in crate::ui::project_browser) fn mount(
    mut commands: Commands,
    roots: Query<Entity, Added<SceneEditorRoot>>,
    assets: Res<AssetServer>,
) {
    if roots.is_empty() { return; }
    let font = feathrise_editor_core::editor_font(&assets, FONT_PATH);
    for root in &roots {
        commands.entity(root).with_children(|parent| {
            // 浮层挂在编辑器根，避免被目录滚动框或窄侧栏裁剪。
            parent.spawn((Backdrop, GlobalZIndex(429), FocusPolicy::Block,
                Node { display: Display::None, position_type: PositionType::Absolute, left: px(0), top: px(0),
                    width: percent(100), height: percent(100), ..default() }))
                .with_children(|overlay| {
                    overlay.spawn((Choices, RelativeCursorPosition::default(), FocusPolicy::Block,
                        Node { position_type: PositionType::Absolute, flex_direction: FlexDirection::Column,
                            row_gap: px(OPTION_GAP), padding: UiRect::all(px(MENU_INSET - 1.0)), border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(4)), overflow: Overflow::clip(), ..default() },
                        BackgroundColor(colors::PANEL_BG_STRONG), BorderColor::all(colors::COPPER_BORDER_WEAK)))
                        .with_children(|menu| {
                            for kind in filter::FILTERS { spawn_option(menu, ProjectTypeFilterOption(kind), &font); }
                            for slot in 0..CLIP_SLOTS {
                                spawn_option(menu, ProjectAnimationClip { slot, clip_index: None }, &font);
                            }
                        });
                });
        });
    }
}

fn spawn_option(parent: &mut ChildSpawnerCommands, marker: impl Component, font: &Handle<Font>) {
    parent.spawn((InteractionButton, marker,
        Node { display: Display::None, height: px(OPTION_HEIGHT), width: percent(100), min_width: px(0),
            align_items: AlignItems::Center, flex_shrink: 0.0, padding: UiRect::horizontal(px(6)),
            overflow: Overflow::clip(), border_radius: BorderRadius::all(px(2)), ..default() },
        BackgroundColor(colors::PANEL_BG_STRONG)))
        .with_child((Text::new(""), TextLayout::no_wrap(), Pickable::IGNORE, FocusPolicy::Pass,
            TextFont { font: font.clone().into(), font_size: metrics::FONT_CONTROL.into(), ..default() },
            TextColor(colors::TEXT_PRIMARY)));
}

pub(crate) fn spawn_expander(row: &mut ChildSpawnerCommands, font: &Handle<Font>) {
    row.spawn((InteractionButton, ProjectAnimationExpand::default(),
        Node { display: Display::None, width: px(22), height: px(20), flex_shrink: 0.0,
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(2)), ..default() },
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL), BorderColor::all(colors::COPPER_BORDER_WEAK)))
        .with_child((Text::new("▶"), Pickable::IGNORE, FocusPolicy::Pass,
            TextFont { font: font.clone().into(), font_size: metrics::FONT_HINT.into(), ..default() },
            TextColor(colors::TEXT_PRIMARY)));
}

pub(in crate::ui::project_browser) fn bind_expanders(
    browser: Res<ProjectBrowserState>,
    catalog: Res<EditorAssetCatalog>,
    settings: Option<Res<EditorSettings>>,
    preview: Option<Res<Messages<PickProjectAnimationClip>>>,
    popup: Res<ProjectBrowserPopup>,
    rows: Query<&AssetRowBinding>,
    mut buttons: Query<(Entity, &ChildOf, &Children, &mut ProjectAnimationExpand, &mut EditorButtonClick,
        &mut Node, &UiInteraction, &mut BackgroundColor)>,
    mut labels: Query<&mut Text>,
) {
    let active = preview.is_some() && browser.assets_visible() && browser.type_filter == AssetTypeFilter::Animation
        && catalog.adopted_type_filter == AssetTypeFilter::Animation && !catalog.is_loading();
    let thumbnails = settings.as_ref().is_some_and(|settings| settings.asset_browser_view_mode == AssetBrowserViewMode::Thumbnails);
    for (entity, parent, children, mut button, mut click, mut node, interaction, mut background) in &mut buttons {
        let binding = active.then(|| rows.get(parent.parent()).ok()).flatten().copied().filter(|binding| {
            binding.index.and_then(|index| catalog.entries.get(index))
                .is_some_and(|entry| !catalog.animation_clips(&entry.asset_path).is_empty())
        });
        if button.0 != binding {
            click.invalidate(); // 虚拟行换绑时，旧按下不能触发展开新文件。
            button.0 = binding;
        }
        let display = if binding.is_some() { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
        if binding.is_none() { continue; }
        // 列表给名称留下独立宽度，卡片把展开按钮放在右上角。
        let position = if thumbnails { PositionType::Absolute } else { PositionType::Relative };
        let top = if thumbnails { px(2) } else { Val::Auto };
        let right = if thumbnails { px(3) } else { Val::Auto };
        if node.position_type != position { node.position_type = position; }
        if node.top != top { node.top = top; }
        if node.right != right { node.right = right; }
        let expanded = popup.open.as_ref().is_some_and(|open| open.anchor == entity);
        background.set_if_neq(BackgroundColor(option_background(*interaction, expanded)));
        let marker = if expanded { "▼" } else { "▶" };
        for child in children {
            if let Ok(mut text) = labels.get_mut(*child) && text.0 != marker { text.0 = marker.to_owned(); }
        }
    }
}

pub(in crate::ui::project_browser) fn show(
    mut popup: ResMut<ProjectBrowserPopup>,
    browser: Res<ProjectBrowserState>,
    catalog: Res<EditorAssetCatalog>,
    locale: Option<Res<EditorLocale>>,
    roots: Query<(&ComputedNode, &UiGlobalTransform), With<SceneEditorRoot>>,
    anchors: Query<(&ComputedNode, &UiGlobalTransform, &InheritedVisibility)>,
    mut backdrop: Query<&mut Node, (With<Backdrop>, Without<Choices>)>,
    mut panel: Query<&mut Node, (With<Choices>, Without<Backdrop>)>,
    mut options: Query<(&mut Node, &mut BackgroundColor, &Children, &UiInteraction, &mut EditorButtonClick,
        Option<&ProjectTypeFilterOption>, Option<&mut ProjectAnimationClip>),
        (Or<(With<ProjectTypeFilterOption>, With<ProjectAnimationClip>)>, Without<Backdrop>, Without<Choices>)>,
    mut labels: Query<&mut Text>,
) {
    let (Ok(mut backdrop), Ok(mut panel)) = (backdrop.single_mut(), panel.single_mut()) else { return; };
    let active = popup.open.as_mut().and_then(|open| {
        let (root, transform) = roots.single().ok()?;
        let (button, button_transform, visibility) = anchors.get(open.anchor).ok()?;
        if root.is_empty() || button.is_empty() || !visibility.get() { return None; }
        let scale = root.inverse_scale_factor();
        let origin = transform.affine().translation - root.size() * 0.5;
        let anchor = Rect::from_center_size((button_transform.affine().translation - origin) * scale, button.size() * scale);
        let viewport = root.size() * scale;
        let (desired_width, length, slots) = match &open.kind {
            PopupKind::Filter(_) => (142.0, filter::FILTERS.len(), filter::FILTERS.len()),
            PopupKind::Clips(source) if source.current(&browser, &catalog) => (260.0, catalog.animation_clips(&source.path).len(), CLIP_SLOTS),
            PopupKind::Clips(_) => return None,
        };
        if length == 0 { return None; }
        let max_rows = ((viewport.y - 2.0 * MENU_INSET - 8.0 + OPTION_GAP) / OPTION_PITCH).floor().max(1.0) as usize;
        open.visible = length.min(slots).min(max_rows);
        open.scroll = open.scroll.min(length.saturating_sub(open.visible) as f32);
        open.first = open.scroll.floor() as usize;
        let size = Vec2::new(desired_width, open.visible as f32 * OPTION_PITCH - OPTION_GAP + 2.0 * MENU_INSET);
        let rect = menu_rect(anchor, viewport, size);
        if panel.left != px(rect.min.x) { panel.left = px(rect.min.x); }
        if panel.top != px(rect.min.y) { panel.top = px(rect.min.y); }
        if panel.width != px(rect.width()) { panel.width = px(rect.width()); }
        if panel.height != px(rect.height()) { panel.height = px(rect.height()); }
        Some(open)
    });
    let display = if active.is_some() { Display::Flex } else { Display::None };
    if backdrop.display == Display::None && display == Display::None { return; }
    if backdrop.display != display { backdrop.display = display; }
    let locale = locale.as_deref().copied().unwrap_or_default();
    for (mut node, mut background, children, interaction, mut click, filter, mut clip) in &mut options {
        let mut index = None;
        let mut selected = false;
        let label = active.as_ref().and_then(|open| match (&open.kind, filter, clip.as_ref()) {
            (PopupKind::Filter(_), Some(filter), _) => {
                let filters = &filter::FILTERS[open.first..open.first + open.visible];
                selected = filter.0 == browser.type_filter;
                filters.contains(&filter.0).then(|| filter::type_label(filter.0, locale))
            }
            (PopupKind::Clips(source), _, Some(clip)) if clip.slot < open.visible => {
                let info = catalog.animation_clips(&source.path).get(open.first + clip.slot)?;
                index = Some(info.index);
                Some(info.label.as_str())
            }
            _ => None,
        });
        let display = if label.is_some() { Display::Flex } else { Display::None };
        if node.display != display {
            click.invalidate();
            node.display = display;
        }
        if let Some(clip) = clip.as_mut() && clip.clip_index != index {
            click.invalidate(); // 连续滚动只改固定槽位，禁止按住原动作后松开到另一动作。
            clip.clip_index = index;
        }
        let Some(label) = label else { continue; };
        background.set_if_neq(BackgroundColor(option_background(*interaction, selected)));
        for child in children {
            if let Ok(mut text) = labels.get_mut(*child) && text.0 != label { text.0 = label.to_owned(); }
        }
    }
}

fn option_background(interaction: UiInteraction, selected: bool) -> Color {
    match (interaction, selected) {
        (UiInteraction::Pressed, _) => colors::SELECT_BG_PRESSED,
        (UiInteraction::Hovered, _) => colors::SELECT_BG_HOVER,
        (UiInteraction::None, true) => colors::SELECT_BG,
        (UiInteraction::None, false) => colors::PANEL_BG_STRONG,
    }
}

fn menu_rect(anchor: Rect, viewport: Vec2, desired: Vec2) -> Rect {
    let size = desired.min((viewport - Vec2::splat(8.0)).max(Vec2::ZERO));
    let maximum = (viewport - size - Vec2::splat(4.0)).max(Vec2::splat(4.0));
    let top = if anchor.max.y + 2.0 <= maximum.y { anchor.max.y + 2.0 } else { anchor.min.y - size.y - 2.0 };
    let position = Vec2::new(anchor.max.x - size.x, top).clamp(Vec2::splat(4.0), maximum);
    Rect::from_corners(position, position + size)
}
