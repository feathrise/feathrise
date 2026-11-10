//! 中央工作区底部分隔与尺寸分配。只移动唯一面板的布局和鼠标所有权。

use bevy::{
    prelude::*,
    text::FontSourceTemplate,
    ui::{FocusPolicy, RelativeCursorPosition},
};
use feathrise_editor_ui::{InteractionButton, UiInteraction, metrics};

use crate::{
    components::MapEditorPresetWorkspaceScroll,
    resources::MapEditorNumericScrubMouseGuard,
    ui::{scene::FONT_PATH, theme as colors},
};

use super::{
    super::{
        AssetPreviewNavigation, AssetPreviewWorkspace,
        components::AssetPreviewCenterHost,
        types::AssetPreviewState,
    },
    context::AnimationContext,
    docking,
    resources::AnimationEditorState,
};

#[cfg(feature = "validation")]
use super::super::types::AssetPreviewCanvas;

const DEFAULT_HEIGHT: f32 = 280.0;
const MIN_HEIGHT: f32 = 96.0;
const SPLITTER_HEIGHT: f32 = 6.0;

#[derive(Component, Clone, Copy, Default)]
pub(super) struct Dock;

#[derive(Component, Clone, Copy, Default)]
pub(super) struct Body;

#[derive(Component, Clone, Copy, Default)]
pub(super) struct Splitter;

#[derive(Component, Clone, Copy, Default)]
pub(super) struct Waiting;

#[derive(Clone, Copy)]
struct Drag {
    tab: u64,
    origin_y: f32,
    origin_height: f32,
    origin_preferred_height: f32,
    scale: f32,
}

impl Drag {
    fn height_at(self, y: f32, available: f32) -> f32 {
        let delta = (self.origin_y - y) * self.scale;
        if delta == 0.0 {
            // 点击或拖回原位时保留原偏好，即使当前窗口暂时压缩了显示高度。
            return self.origin_preferred_height;
        }
        let (minimum, maximum) = height_bounds(available);
        (self.origin_height + delta).clamp(minimum, maximum)
    }
}

#[derive(Resource)]
pub(super) struct Layout {
    preferred_height: f32,
    drag: Option<Drag>,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            preferred_height: DEFAULT_HEIGHT,
            drag: None,
        }
    }
}

pub(in crate::ui::asset_preview) fn scene() -> impl Scene {
    bsn! {
        Dock
        Node {
            display: Display::None, width: percent(100), min_width: px(0), min_height: px(0),
            height: px(DEFAULT_HEIGHT), flex_shrink: 0.0, flex_direction: FlexDirection::Column,
        }
        BackgroundColor(colors::PANEL_BG_SECTION)
        Children [
            Splitter InteractionButton
            Node { width: percent(100), height: px(SPLITTER_HEIGHT), flex_shrink: 0.0,
                align_items: AlignItems::Center, justify_content: JustifyContent::Center }
            BackgroundColor(colors::PANEL_BG_STRONG)
            Children [
                ~{FocusPolicy::Pass}
                Node { width: px(36), height: px(2), border_radius: BorderRadius::all(px(1)) }
                BackgroundColor(colors::COPPER_BORDER)
            ]
        --
            Body MapEditorPresetWorkspaceScroll ScrollPosition::default() RelativeCursorPosition::default()
            ~{FocusPolicy::Block}
            Node { width: percent(100), min_width: px(0), min_height: px(0), flex_grow: 1.0, flex_basis: px(0),
                flex_direction: FlexDirection::Column, overflow: Overflow::scroll_y() }
            Children [
                Waiting
                Text("")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
                    font_size: px(metrics::FONT_CONTROL) }
                TextColor(colors::TEXT_MUTED)
                Node { display: Display::None, padding: UiRect::all(px(8)) }
            ]
        ]
    }
}

fn height_bounds(available: f32) -> (f32, f32) {
    let available = (available - 12.0).max(0.0); // 扣除资产正文外边与行间距。
    // 小窗口按比例给画布保留空间；高窗口至少给工具条和模型保留约两百像素。
    let maximum = (available - (available * 0.42).min(200.0)).max(SPLITTER_HEIGHT);
    (MIN_HEIGHT.min(maximum), maximum)
}

pub(super) fn input(
    mut contexts: ParamSet<(AnimationContext, ResMut<MapEditorNumericScrubMouseGuard>)>,
    preview: Res<AssetPreviewState>,
    media: Res<AssetPreviewWorkspace>,
    animation: Res<AnimationEditorState>,
    navigation: Option<Res<AssetPreviewNavigation>>,
    mouse: Res<ButtonInput<MouseButton>>,
    center: Query<&ComputedNode, With<AssetPreviewCenterHost>>,
    dock: Query<&ComputedNode, With<Dock>>,
    splitter: Query<&UiInteraction, With<Splitter>>,
    mut layout: ResMut<Layout>,
) {
    let sample = (|| {
        let context = contexts.p0();
        if !context.available(&preview, &media)
            || !media.active(context.workspace.active_tab)
            || !(animation.visible() || animation.in_place_rig_preview())
        {
            return None;
        }
        let tab = media.expanded.as_ref()?.id;
        let (_, window) = context.windows.single().ok()?;
        let y = window.physical_cursor_position()?.y;
        let dock = dock.single().ok()?;
        let scale = dock.inverse_scale_factor();
        let available = center.single().ok()?.size().y * scale;
        if !y.is_finite() || !scale.is_finite() || scale <= 0.0
            || !available.is_finite() || dock.size().y <= 0.0
        {
            return None;
        }
        let can_start = !context.guard.blocked()
            && context.numeric.as_ref().is_none_or(|numeric| numeric.entity.is_none())
            && context.layout.as_ref().is_none_or(|state| state.drag.is_none() && !state.owns_mouse_this_frame)
            && context.map_drag.as_ref().is_none_or(|drag| !drag.blocks_other_mouse() && drag.press.is_none())
            && !animation.owns_mouse()
            && navigation.as_ref().is_none_or(|preview| !preview.dragging());
        Some((tab, y, dock.size().y * scale, scale, available, can_start))
    })();

    if let Some(drag) = layout.drag {
        // 按下、跨区拖动和释放帧都交给同一个编辑器鼠标门。
        contexts.p1().advance(true, false, false);
        let Some((_, y, _, _, available, _)) = sample.filter(|sample| {
            sample.0 == drag.tab && sample.3 == drag.scale
        }) else {
            layout.drag = None;
            return; // 换标签、失焦或DPI改变都放弃旧手势。
        };
        let height = drag.height_at(y, available);
        if height != layout.preferred_height {
            layout.preferred_height = height;
        }
        if !mouse.pressed(MouseButton::Left) {
            layout.drag = None;
        }
        return;
    }
    if !mouse.just_pressed(MouseButton::Left)
        || [MouseButton::Right, MouseButton::Middle]
            .into_iter()
            .any(|other| mouse.pressed(other) || mouse.just_released(other))
        || !splitter.single().is_ok_and(|interaction| *interaction != UiInteraction::None)
    {
        return;
    }
    if let Some((tab, origin_y, origin_height, scale, _, true)) = sample {
        layout.drag = Some(Drag {
            tab,
            origin_y,
            origin_height,
            origin_preferred_height: layout.preferred_height,
            scale,
        });
        contexts.p1().advance(true, false, false);
    }
}

pub(super) fn sync(
    context: docking::Context,
    animation: Res<AnimationEditorState>,
    layout: Res<Layout>,
    locale: Option<Res<feathrise_editor_ui::localization::EditorLocale>>,
    center: Query<&ComputedNode, With<AssetPreviewCenterHost>>,
    mut nodes: Query<(&mut Node, Has<Dock>), Or<(With<Dock>, With<Waiting>)>>,
    mut splitter: Query<(&UiInteraction, &mut BackgroundColor), With<Splitter>>,
    mut status: Query<&mut Text, With<Waiting>>,
) {
    let visible = context.timeline_visible(&animation);
    let waiting = visible && !animation.visible();
    let available = center.single().ok().map(|node| node.size().y * node.inverse_scale_factor());
    let height = available.filter(|height| height.is_finite() && *height > 0.0)
        .map_or(layout.preferred_height, |available| {
            let (minimum, maximum) = height_bounds(available);
            layout.preferred_height.clamp(minimum, maximum)
        });
    for (mut node, dock) in &mut nodes {
        // 中央画布继续由资产预览表现系统分配剩余高度，分隔器只写下方宿主尺寸。
        let shown = if dock { visible } else { waiting };
        let display = if shown {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        if dock && node.height != px(height) {
            node.height = px(height);
        }
    }
    if waiting && let Ok(mut status) = status.single_mut() {
        // 载入失败仍使用同一位置回显实际错误，不把失败显示成永久载入。
        let locale = locale.as_deref().copied().unwrap_or_default();
        let wanted = locale.editor_notice(&animation.status);
        if status.as_str() != wanted.as_ref() {
            **status = wanted.into_owned();
        }
    }
    if let Ok((interaction, mut color)) = splitter.single_mut() {
        color.set_if_neq(BackgroundColor(if layout.drag.is_some() {
            colors::SELECT_BG_PRESSED
        } else if *interaction != UiInteraction::None {
            colors::SELECT_BG_HOVER
        } else {
            colors::PANEL_BG_STRONG
        }));
    }
}

#[cfg(feature = "validation")]
pub(in crate::ui::asset_preview) struct AnimationTimelineDockProbe {
    pub splitter: Entity,
    pub splitter_center: Vec2,
    pub canvas_height: f32,
    pub timeline_height: f32,
    pub canvas_bottom: f32,
    pub timeline_top: f32,
    pub dragging: bool,
}

#[cfg(feature = "validation")]
pub(in crate::ui::asset_preview) fn timeline_dock_probe(world: &mut World) -> Option<AnimationTimelineDockProbe> {
    let (splitter, splitter_node, splitter_transform) = world
        .query_filtered::<(Entity, &ComputedNode, &bevy::ui::UiGlobalTransform), With<Splitter>>()
        .single(world).ok()?;
    let splitter_center = splitter_transform.affine().translation;
    if splitter_node.size().min_element() <= 1.0 {
        return None;
    }
    let (canvas, canvas_transform) = world
        .query_filtered::<(&ComputedNode, &bevy::ui::UiGlobalTransform), With<AssetPreviewCanvas>>()
        .single(world).ok()?;
    let canvas_height = canvas.size().y;
    let canvas_bottom = canvas_transform.affine().translation.y + canvas_height * 0.5;
    let (timeline, timeline_transform) = world
        .query_filtered::<(&ComputedNode, &bevy::ui::UiGlobalTransform), With<Body>>()
        .single(world).ok()?;
    let timeline_height = timeline.size().y;
    let timeline_top = timeline_transform.affine().translation.y - timeline_height * 0.5;
    (canvas_height > 1.0 && timeline_height > 1.0).then(|| AnimationTimelineDockProbe {
        splitter,
        splitter_center,
        canvas_height,
        timeline_height,
        canvas_bottom,
        timeline_top,
        dragging: world.resource::<Layout>().drag.is_some(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_windows_preserve_both_panes_and_do_not_replace_preferred_height() {
        let preferred = Layout::default().preferred_height;
        let (min, max) = height_bounds(150.0);
        assert!(min > SPLITTER_HEIGHT && max < 150.0);
        assert!(preferred.clamp(min, max) < preferred);
        let (_, compressed) = height_bounds(300.0);
        let drag = Drag {
            tab: 1,
            origin_y: 240.0,
            origin_height: compressed,
            origin_preferred_height: preferred,
            scale: 0.5,
        };
        assert_eq!(drag.height_at(280.0, 300.0), compressed - 20.0);
        assert_eq!(drag.height_at(240.0, 300.0), preferred);
        let (min, max) = height_bounds(720.0);
        assert_eq!(preferred.clamp(min, max), preferred);
        assert!(720.0 - max >= 200.0);
    }
}
