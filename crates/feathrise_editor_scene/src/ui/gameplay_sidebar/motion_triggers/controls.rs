//! 动画触发工具的固定控件。数值框按当前实体身份复用，菜单与视口共用同一编辑草稿。

use super::super::components::GameplaySidebarPanel;
use super::*;
use bevy::text::FontSourceTemplate;
use feathrise_editor_core::FONT_PATH;
use feathrise_editor_ui::localization::EditorLocale;
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, InteractionButton, NumericInputScrubConfig,
    NumericInputSetValue, NumericInputSpec, NumericInputValue, NumericScrubFieldOptions, colors,
    localization::label_scene, metrics, spawn_numeric_scrub_field,
};

#[derive(Component, Clone, Default)]
pub(super) struct Summary;
#[derive(Component, Clone, Copy)]
pub(super) struct FieldSlot(Dimension);
fn button(text: &'static str, action: Action) -> impl Scene {
    bsn! { InteractionButton ~{action}
        Node { height: px(24), padding: UiRect::horizontal(px(4)), align_items: AlignItems::Center, border: px(1) }
        BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [Text(text) @label_scene(text) Pickable::IGNORE
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)]
    }
}
fn row() -> impl Scene {
    bsn! { Node { width: percent(100), flex_wrap: FlexWrap::Wrap, column_gap: px(3), row_gap: px(3), flex_shrink: 0.0 } }
}

pub(in super::super) fn panel() -> impl Scene {
    bsn! { GameplaySidebarPanel(GameplaySidebarPage::MotionTrigger)
        Node { display: Display::None, width: percent(100), flex_direction: FlexDirection::Column, row_gap: px(4), flex_shrink: 0.0 }
        Children [
            @crate::ui::context_help::section_header("动画触发", "选择主场景的动画片段。新增时先点「绘制新区域」，再设置新区的高度和动作，在场景视口点选两个对角。直接修改下方参数会更新当前选中的区域。")--
            Summary Text("") TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(metrics::FONT_CONTROL) }
                TextColor(colors::TEXT_VALUE)--
            @row() Children [@button("上一片段", Action::PreviousClip)-- @button("下一片段", Action::NextClip)]--
            @row() Children [@button("上一区域", Action::PreviousRegion)-- @button("下一区域", Action::NextRegion)]--
            @row() Children [@button("绘制新区域", Action::Draw)-- @button("调整区域", Action::Adjust)]--
            @row() Children [@button("重画区域", Action::Redraw)-- @button("删除区域", Action::Remove)]--
            @row() Children [@button("切换进入动作", Action::Enter)-- @button("切换离开动作", Action::Exit)]--
            ~{FieldSlot(Dimension::Floor)} Node { width: percent(100) }--
            ~{FieldSlot(Dimension::Height)} Node { width: percent(100) }
        ]
    }
}

pub(super) fn setup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    slots: Query<(Entity, &FieldSlot), Added<FieldSlot>>,
) {
    for (entity, slot) in &slots {
        let field = slot.0;
        let (label, value, minimum) = match field {
            Dimension::Floor => ("底面高度", 0.0, f32::MIN),
            Dimension::Height => ("区域高度", 2.0, f32::MIN_POSITIVE),
        };
        let spec = NumericInputSpec::f32(3)
            .with_min(NumericValue::F32(minimum))
            .with_step_buttons(false)
            .with_live_update(false)
            .with_replace_on_first_key(true);
        let options = NumericScrubFieldOptions::new(
            label,
            "m",
            NumericValue::F32(value),
            spec,
            NumericInputScrubConfig::linear(4.0, 0.1, minimum, f32::MAX, 1.0),
        );
        commands.entity(entity).with_children(|children| {
            spawn_numeric_scrub_field(
                children,
                field,
                feathrise_editor_core::editor_font(&assets, FONT_PATH),
                options,
            );
        });
    }
}

pub(super) fn sync(
    mut tool: ResMut<Tool>,
    session: Res<EditorAuthoringSession>,
    mut summaries: Query<&mut Text, With<Summary>>,
    fields: Query<(Entity, &Dimension, &NumericInputValue)>,
    active: Res<ActiveNumericInput>,
    scrub: Res<ActiveNumericInputScrub>,
    mut set: MessageWriter<NumericInputSetValue>,
    locale: Res<EditorLocale>,
    mut last: Local<
        Option<(
            usize,
            usize,
            MotionRegionAction,
            MotionRegionAction,
            bool,
            EditorLocale,
        )>,
    >,
) {
    let draft = session.draft.as_ref();
    if tool
        .bound
        .as_ref()
        .is_some_and(|b| Some(&b.scene) != draft.map(|d| &d.scene_id))
    {
        *tool = Tool::default();
    }
    let count = draft.map_or(0, |d| d.motion.clip_count());
    if count > 0 && tool.clip >= count {
        tool.clip = count - 1;
    }
    let rebound = !tool.bound_is_current(&session);
    if rebound {
        let bound = tool.binding(&session);
        tool.drawing = None;
        tool.drag = None;
        if let Some(b) = &bound {
            tool.region = b.index;
            if let Some(r) = draft.and_then(|d| d.motion.clip_regions_at(tool.clip).get(b.index)) {
                tool.floor = r.center.y - r.half_extents.y;
                tool.height = r.half_extents.y * 2.0;
                tool.enter = r.enter;
                tool.exit = r.exit;
            }
        }
        tool.bound = bound;
    }
    let drawing_new = tool.drawing.as_ref().is_some_and(|drawing| !drawing.replace);
    let stamp = (count, tool.clip, tool.enter, tool.exit, drawing_new, *locale);
    if rebound || last.as_ref() != Some(&stamp) {
        *last = Some(stamp);
        let text = if let Some(d) = draft
            && let Some((_, name)) = d.motion.clip_at(tool.clip)
        {
            let n = d.motion.clip_regions_at(tool.clip).len();
            format!(
                "{} / {} · {name}\n{} {} / {} · {} {} · {} {}",
                tool.clip + 1,
                count,
                locale.editor_label("区域"),
                if n == 0 { 0 } else { tool.region + 1 },
                n,
                locale.editor_label("进入"),
                locale.editor_label(tool.enter.label()),
                locale.editor_label("离开"),
                locale.editor_label(tool.exit.label())
            )
        } else {
            locale
                .editor_label("请先在主场景 GLB 中创建动画")
                .to_owned()
        };
        for mut t in &mut summaries {
            let caption = if drawing_new {
                if *locale == EditorLocale::English {
                    format!("{text}\nEditing settings for a new region")
                } else {
                    format!("{text}\n当前参数用于正在绘制的新区域")
                }
            } else {
                text.clone()
            };
            if t.as_str() != caption {
                **t = caption;
            }
        }
    }
    for (entity, field, current) in &fields {
        if !rebound && (active.entity == Some(entity) || scrub.entity == Some(entity)) {
            continue;
        }
        let value = match field {
            Dimension::Floor => tool.floor,
            Dimension::Height => tool.height,
        };
        if rebound || current.0.as_f32() != Some(value) {
            set.write(NumericInputSetValue {
                entity,
                value: NumericValue::F32(value),
                cancel_edit: true,
            });
        }
    }
}
