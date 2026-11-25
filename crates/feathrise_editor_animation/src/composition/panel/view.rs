//! 窗口复用同一组内容行，文件和选择变化时才刷新可见资源。

use super::*;

#[derive(Component, Clone, Copy, Default)]
pub(super) struct Feedback;

fn label(value: &'static str) -> impl Scene {
    bsn! { Text(value) @label_scene(value) Pickable::IGNORE TextColor(colors::TEXT_PRIMARY)
    TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(metrics::FONT_CONTROL) } }
}
pub(super) fn button(action: Button, value: &'static str) -> impl Scene {
    bsn! { InteractionButton ~{action}
    Node { min_height: px(24), padding: UiRect::horizontal(px(5)), align_items: AlignItems::Center,
        border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(2)) }
    BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
    Children [@label(value)] }
}
fn row(i: usize) -> impl Scene {
    bsn! { InteractionButton ~{Button::Row(i)} Node { width: percent(100), height: px(23), padding: UiRect::horizontal(px(4)),
        overflow: Overflow::clip(), align_items: AlignItems::Center } BackgroundColor(colors::SLOT_BG)
    Children [~{RowText(i)} Text("") TextLayout::no_wrap() Pickable::IGNORE TextColor(colors::TEXT_PRIMARY)
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(metrics::FONT_CONTROL) }] }
}
pub(super) fn wrap() -> Node {
    Node {
        width: percent(100),
        flex_wrap: FlexWrap::Wrap,
        column_gap: px(3),
        row_gap: px(2),
        ..default()
    }
}

pub(crate) fn scene() -> impl Scene {
    use ContentKind as K;
    bsn! { Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(2) }
    Children [@button(Button::Open, "GLB 内容")--
        Body RelativeCursorPosition::default() Node { display: Display::None, width: percent(100), min_width: px(0),
                flex_direction: FlexDirection::Column, row_gap: px(2) }
        Children [
            ~{wrap()} Children [@button(Button::Kind(K::Animation), "动画")-- @button(Button::Kind(K::Node), "对象")--
                @button(Button::Kind(K::Skin), "骨架")-- @button(Button::Kind(K::Material), "材质")-- @button(Button::Kind(K::Texture), "纹理")--
                @button(Button::Kind(K::Image), "图像")-- @button(Button::Kind(K::Camera), "相机")]--
            @row(0)-- @row(1)-- @row(2)-- @row(3)-- @row(4)-- @row(5)--
            ~{wrap()} Children [@button(Button::Previous, "上一页")-- @button(Button::Next, "下一页")--
                @button(Button::Copy, "复制动作")-- @button(Button::Paste, "粘贴动作")-- @button(Button::Adapt, "按预览适配粘贴")--
                @button(Button::Export, "所选另存为…")-- @button(Button::Slots, "替换材质槽")]--
            @material::scene()--
            Output Node { display: Display::None, width: percent(100), flex_wrap: FlexWrap::Wrap, column_gap: px(3) }
            Children [OutputName EditorTextInput ~{EditableText { allow_newlines: false, ..EditableText::new("") }}
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(metrics::FONT_CONTROL) }
                TextColor(colors::TEXT_PRIMARY) TextLayout::no_wrap() Node { width: percent(100), height: px(25), overflow: Overflow::clip() }
                    BackgroundColor(colors::INPUT_BG)-- @button(Button::Confirm, "保存副本")-- @button(Button::Overwrite, "确认覆盖副本")-- @button(Button::Cancel, "取消")]--
                Feedback Text("") Pickable::IGNORE Node { display: Display::None, width: percent(100), min_width: px(0) }
                    TextColor(colors::TEXT_PRIMARY) TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(metrics::FONT_CONTROL) }
        ]
    ] }
}

pub(super) fn sync(
    mut panel: ResMut<State>,
    state: Res<AnimationEditorState>,
    host: Res<feathrise_editor_animation_model::host::AnimationHost>,
    mut body: Query<&mut Node, (With<Body>, Without<Output>, Without<Button>)>,
    mut out: Query<&mut Node, (With<Output>, Without<Button>, Without<Body>)>,
    mut buttons: Query<(
        &Button,
        &mut Node,
        &mut BackgroundColor,
        &mut EditorButtonClick,
    )>,
    mut labels: Query<(&RowText, &mut Text)>,
) {
    let current = state
        .active
        .as_ref()
        .and_then(|p| state.sessions.get(p))
        .map(|s| &s.document);
    let refreshed = panel.path != state.active
        || !match (current, &panel.shown) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
    if refreshed {
        if panel.path != state.active {
            panel.show_feedback = false;
        }
        if panel.path != state.active || !current.zip(panel.shown.as_ref()).is_some_and(|(a,b)| Arc::ptr_eq(&a.source_identity, &b.source_identity)) {
            panel.slot_node = None;
            panel.slot_page = 0;
            panel.active_slot = None;
            panel.slot_drag = None;
            if panel.slots_open { panel.kind = ContentKind::Node; }
        }
        panel.path.clone_from(&state.active);
        panel.shown = current.cloned();
        panel.page = 0;
        panel.spans.clear();
        panel.anchor = None;
        panel.export = false;
        panel.exporting = false;
    }
    if panel.exporting && !state.busy() {
        panel.exporting = false;
        panel.export = !state.selection_written;
    }
    let confirming = state.export_overwrite.as_ref().is_some_and(|(from, _, _)| {
        current.is_some_and(|doc| std::sync::Weak::ptr_eq(from, &Arc::downgrade(doc)))
    });
    if panel.confirming != confirming {
        panel.confirming = confirming;
    }
    // 拖放只授权这次标签切换。转到第三个文件或目标读取失败后不保留迟到动作。
    if panel.pending_drop.as_ref().is_some_and(|(target, source)| {
        state.failed.as_ref() == Some(target)
            || state
                .active
                .as_ref()
                .is_some_and(|p| p != &source.origin && p != target)
    }) {
        panel.pending_drop = None;
    }
    if panel.root != host.asset_root {
        panel.root.clone_from(&host.asset_root);
        panel.copied = None;
        panel.drag = None;
        panel.pending_drop = None;
        panel.slot_drag = None;
    }
    let controls = state.current().filter(|_| panel.slots_open).map_or((false,false,false), |s|
        (s.material_action(&PendingAction::Undo),s.material_action(&PendingAction::Redo),s.dirty()));
    if panel.slot_controls != controls { panel.slot_controls = controls; }
    if !panel.is_changed() && !refreshed {
        return;
    }
    let display = |show| if show { Display::Flex } else { Display::None };
    for mut n in &mut body {
        n.display = display(panel.open && current.is_some());
    }
    for mut n in &mut out {
        n.display = display(panel.export);
    }
    let count = current.map_or(0, |d| d.content_count(panel.kind));
    for (button, mut node, mut color, mut click) in &mut buttons {
        let (show, active) = match *button {
            Button::Row(i) => (
                panel.page * ROWS + i < count,
                selected(&panel.spans, panel.kind, panel.page * ROWS + i),
            ),
            Button::Kind(kind) => (
                current.is_some_and(|d| d.content_count(kind) > 0),
                kind == panel.kind,
            ),
            Button::Previous => (panel.page > 0, false),
            Button::Next => ((panel.page + 1) * ROWS < count, false),
            Button::Copy => (panel.kind == ContentKind::Animation, false),
            Button::Paste | Button::Adapt => (panel.copied.is_some(), false),
            Button::Open => (current.is_some_and(|d| !d.read_only), panel.open),
            Button::Overwrite => (confirming, false),
            Button::Confirm => (!confirming, false),
            Button::Slots => (current.is_some_and(|d| d.can_replace_material()), panel.slots_open),
            Button::Slot(i) => {
                let slot = panel.slot_page * material::SLOTS + i;
                (current.zip(panel.slot_node).is_some_and(|(d,n)| slot < d.material_slot_count(n)),
                    if panel.slot_drag.is_some() { panel.slot_hover == Some(i) } else { panel.active_slot == Some(slot) })
            }
            Button::SlotsPrevious => (panel.slot_page > 0, false),
            Button::SlotsNext => (current.zip(panel.slot_node).is_some_and(|(d,n)| (panel.slot_page + 1) * material::SLOTS < d.material_slot_count(n)), false),
            Button::Undo => (controls.0, false),
            Button::Redo => (controls.1, false),
            Button::Save => (controls.2, false),
            _ => (true, false),
        };
        if refreshed || node.display != display(show) {
            click.invalidate();
        }
        node.display = display(show);
        color.0 = if active {
            colors::PANEL_BG_STRONG
        } else {
            colors::SLOT_BG
        };
    }
    for (slot, mut text) in &mut labels {
        let i = panel.page * ROWS + slot.0;
        let value = current
            .and_then(|d| d.content_at(panel.kind, i))
            .map_or_else(String::new, |(_, name)| format!("{} · {name}", i + 1));
        text.set_if_neq(Text::new(value));
    }
}

/// 无动作角色也能在原内容目录看到映射失败、实际副本路径与覆盖确认。
pub(super) fn feedback(
    panel: Res<State>,
    state: Res<AnimationEditorState>,
    locale: Option<Res<feathrise_editor_ui::localization::EditorLocale>>,
    mut lines: Query<(&mut Text, &mut Node), With<Feedback>>,
) {
    if !panel.is_changed() && !state.is_changed() && !locale.as_ref().is_some_and(|l| l.is_changed()) {
        return;
    }
    let locale = locale.as_deref().copied().unwrap_or_default();
    for (mut text, mut node) in &mut lines {
        let visible = panel.open && panel.show_feedback && !state.status.is_empty();
        let display = if visible {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        if visible {
            let message = locale.editor_notice(&state.status);
            if text.as_str() != message.as_ref() { **text = message.into_owned(); }
        }
    }
}
