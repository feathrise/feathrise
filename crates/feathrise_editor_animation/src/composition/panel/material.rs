//! 同文件对象是替换范围，图元行是放下目标。切到材质分类时保留该对象。

use super::view::{button, wrap};
use super::*;
use crate::{data::MaterialSlot, resources::Edit};
use feathrise_editor_ui::localization::EditorLocale;

pub(super) const SLOTS: usize = 3;
#[derive(Component, Clone, Copy, Default)]
pub(super) struct Details;
#[derive(Component, Clone, Copy, Default)]
pub(super) struct Hint;
#[derive(Component, Clone, Copy)]
pub(super) struct SlotText(usize);

fn slot(i: usize) -> impl Scene {
    bsn! { InteractionButton ~{Button::Slot(i)}
    Node { width: percent(100), height: px(24), padding: UiRect::horizontal(px(4)), align_items: AlignItems::Center,
        border: UiRect::all(px(1)), overflow: Overflow::clip() }
    BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
    Children [~{SlotText(i)} Text("") TextLayout::no_wrap() Pickable::IGNORE TextColor(colors::TEXT_PRIMARY)
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(metrics::FONT_CONTROL) }] }
}

pub(super) fn scene() -> impl Scene {
    bsn! { Details Node { display: Display::None, width: percent(100), flex_direction: FlexDirection::Column, row_gap: px(2) }
    Children [Hint Text("") Pickable::IGNORE TextColor(colors::TEXT_PRIMARY)
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(metrics::FONT_CONTROL) }--
        @slot(0)-- @slot(1)-- @slot(2)--
        ~{wrap()} Children [@button(Button::SlotsPrevious,"上一组图元")-- @button(Button::SlotsNext,"下一组图元")--
            @button(Button::Undo,"撤销")-- @button(Button::Redo,"重做")-- @button(Button::Save,"保存 GLB")] ] }
}

pub(super) fn action(
    action: Button,
    doc: &AnimationDocument,
    panel: &mut State,
    state: &mut AnimationEditorState,
) {
    match action {
        Button::Slots => {
            panel.slots_open = !panel.slots_open;
            if panel.slots_open {
                panel.kind = ContentKind::Node;
                panel.page = 0;
                panel.anchor = None;
            }
        }
        Button::Slot(i) => {
            panel.active_slot = Some(panel.slot_page * SLOTS + i);
            panel.kind = ContentKind::Material;
            panel.page = 0;
            panel.anchor = None;
        }
        Button::SlotsPrevious => panel.slot_page = panel.slot_page.saturating_sub(1),
        Button::SlotsNext => {
            if let Some(n) = panel.slot_node {
                panel.slot_page =
                    (panel.slot_page + 1).min(doc.material_slot_count(n).saturating_sub(1) / SLOTS);
            }
        }
        Button::Undo | Button::Redo | Button::Save => {
            let op = match action {
                Button::Undo => PendingAction::Undo,
                Button::Redo => PendingAction::Redo,
                _ => PendingAction::Save,
            };
            panel.show_feedback = true;
            if let Some(s) = state.current().filter(|s| {
                s.material_action(&op) || matches!(op, PendingAction::Save) && s.dirty()
            }) {
                if s.pose.is_some() {
                    state.status = "请先插入或还原当前临时姿势".into();
                } else if matches!(op, PendingAction::Save) {
                    if let Some(path) = &panel.path
                        && let Err(error) = crate::drive::launch_save(state, path)
                    {
                        state.status = error;
                    }
                } else {
                    state.pending = Some(op);
                }
            }
        }
        _ => {}
    }
}

pub(super) fn drop_material(
    panel: &mut State,
    state: &mut AnimationEditorState,
    index: usize,
    row: usize,
) {
    let Some((doc, node)) = panel.shown.as_ref().zip(panel.slot_node) else {
        return;
    };
    if state.author_path() != panel.path.as_deref()
        || !state
            .current()
            .is_some_and(|s| Arc::ptr_eq(doc, &s.document))
    {
        return;
    }
    panel.show_feedback = true;
    let slot = MaterialSlot {
        node,
        primitive: panel.slot_page * SLOTS + row,
    };
    panel.active_slot = Some(slot.primitive);
    if state.current().is_some_and(|s| s.pose.is_some()) {
        state.status = "请先插入或还原当前临时姿势".into();
    } else {
        match doc.plan_material(slot, index) {
            Ok(change) => state.pending = Some(PendingAction::Edit(Edit::MaterialSlot(change))),
            Err(error) => state.status = error,
        }
    }
    state.changed();
}

pub(super) fn sync(
    panel: Res<State>,
    locale: Option<Res<EditorLocale>>,
    mut block: Query<&mut Node, With<Details>>,
    mut hints: Query<&mut Text, With<Hint>>,
    mut labels: Query<(&SlotText, &mut Text), Without<Hint>>,
) {
    if !panel.is_changed() && !locale.as_ref().is_some_and(|l| l.is_changed()) {
        return;
    }
    let locale = locale.as_deref().copied().unwrap_or_default();
    for mut n in &mut block {
        n.display = if panel.slots_open {
            Display::Flex
        } else {
            Display::None
        };
    }
    let Some(doc) = panel.shown.as_ref().filter(|_| panel.slots_open) else {
        return;
    };
    for mut text in &mut hints {
        let hint = panel
            .slot_node
            .and_then(|n| doc.content_at(ContentKind::Node, n))
            .map_or_else(
                || locale.editor_label("先选择含网格的对象").to_owned(),
                |(_, name)| {
                    let tip = if let Some((_, material)) = &panel.slot_drag {
                        match panel.slot_hover {
                            Some(row)
                                if panel
                                    .slot_node
                                    .and_then(|node| {
                                        doc.slot_material(MaterialSlot {
                                            node,
                                            primitive: panel.slot_page * SLOTS + row,
                                        })
                                    })
                                    .flatten()
                                    == Some(*material) =>
                            {
                                "该图元已经使用所选材质"
                            }
                            Some(_) => "松开以替换此图元材质",
                            None => "拖到图元材质槽后松开",
                        }
                    } else {
                        "点击图元，从材质列表拖入槽"
                    };
                    format!("{name} · {}", locale.editor_label(tip))
                },
            );
        text.set_if_neq(Text::new(hint));
    }
    for (row, mut text) in &mut labels {
        let primitive = panel.slot_page * SLOTS + row.0;
        let material = panel
            .slot_node
            .and_then(|node| doc.slot_material(MaterialSlot { node, primitive }))
            .flatten();
        let name = material
            .and_then(|m| doc.content_at(ContentKind::Material, m))
            .map(|(_, n)| n)
            .unwrap_or_else(|| locale.editor_label("默认材质").to_owned());
        text.set_if_neq(Text::new(format!(
            "{} {} · {name}",
            locale.editor_label("图元"),
            primitive + 1
        )));
    }
}
