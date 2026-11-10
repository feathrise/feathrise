//! 连续动作表与姿势工具栏：时间视区和骨骼过滤复用固定控件池，不截断关键帧数据。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use super::components::*;
use crate::ui::{scene::FONT_PATH, theme as colors};
use bevy::{
    prelude::*,
    text::FontSourceTemplate,
    ui::{FocusPolicy, RelativeCursorPosition},
};
use feathrise_editor_ui::{
    NumericInputScrubConfig, NumericInputSpec, NumericScrubFieldOptions, NumericScrubFieldStyle,
    NumericValue, metrics as ui_metrics, spawn_numeric_scrub_field,
};

fn label(text: &str, kind: AnimationLabel) -> impl Scene {
    bsn! { ~{kind} Text(text) ~{FocusPolicy::Pass}
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
        TextColor(colors::TEXT_PRIMARY) TextLayout::no_wrap()
        Node { min_width: px(0), overflow: Overflow::clip_x() }
    }
}

fn button(text: &str, action: AnimationAction) -> impl Scene {
    bsn! { InteractionButton ~{AnimationButton { action, binding: 0 }}
        Node { height: px(26), padding: UiRect::horizontal(px(6)), flex_shrink: 0.0,
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            border: px(1), border_radius: BorderRadius::all(px(2)) }
        BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [@label(text, AnimationLabel::Button(action))]
    }
}

/// 姿势工具紧邻骨架画布，避免藏在时间轴按钮中；固定两个入口不占画布布局高度。
pub(in super::super) fn pose_toolbar() -> impl Scene {
    bsn! { AnimationPoseToolbar ~{FocusPolicy::Pass} ZIndex(1)
        Node { display: Display::None, position_type: PositionType::Absolute,
            left: px(8), top: px(8), width: px(76), flex_direction: FlexDirection::Column,
            row_gap: px(3) }
        Children [@button("移动 G", AnimationAction::MovePose)-- @button("旋转 R", AnimationAction::RotatePose)]
    }
}

fn playhead() -> impl Scene {
    bsn! { AnimationPlayhead ~{FocusPolicy::Pass}
        Node { position_type: PositionType::Absolute, left: percent(0), top: px(0),
            width: px(1), height: percent(100) }
        BackgroundColor(colors::TEXT_ACCENT)
    }
}

fn tick(index: usize) -> impl Scene {
    // 末刻度把文字放回尺内，边界线仍在真实100%位置，不能把终帧标签裁到画布外。
    let last = index == 8;
    let node = Node {
        position_type: PositionType::Absolute,
        left: if last {
            Val::Auto
        } else {
            percent(index as f32 * 12.5)
        },
        right: if last { px(0) } else { Val::Auto },
        top: px(0),
        bottom: px(0),
        width: px(34),
        border: if last {
            UiRect::right(px(1))
        } else {
            UiRect::left(px(1))
        },
        padding: UiRect::horizontal(px(3)),
        justify_content: if last {
            JustifyContent::FlexEnd
        } else {
            JustifyContent::FlexStart
        },
        overflow: Overflow::clip_x(),
        ..default()
    };
    bsn! { ~{node}
        BorderColor::all(colors::COPPER_BORDER_WEAK) ~{FocusPolicy::Pass}
        Children [@label("0", AnimationLabel::Tick(index))]
    }
}

fn key(row: usize, slot: usize) -> impl Scene {
    bsn! { UiInteraction::default() ~{FocusPolicy::Pass} ~{AnimationKeyButton { row, slot, ..default() }}
        Node { display: Display::None, position_type: PositionType::Absolute,
            left: percent(0), top: px(2), width: px(14), height: px(20),
            margin: UiRect::left(px(-7)), align_items: AlignItems::Center,
            justify_content: JustifyContent::Center }
        BackgroundColor(Color::NONE)
        Children [
            Text("◆") ~{FocusPolicy::Pass}
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_VALUE)
        ]
    }
}

/// 四个8键子池限制BSN临时树大小；实体上限固定，不限制实际动作长度。
fn keys(row: usize, start: usize) -> Box<dyn Scene> {
    Box::new(bsn! { ~{FocusPolicy::Pass}
        Node { position_type: PositionType::Absolute, left: px(0), top: px(0),
            width: percent(100), height: percent(100) }
        Children [@key(row, start)-- @key(row, start + 1)-- @key(row, start + 2)--
            @key(row, start + 3)-- @key(row, start + 4)-- @key(row, start + 5)--
            @key(row, start + 6)-- @key(row, start + 7)]
    })
}

fn channel(slot: usize) -> Box<dyn Scene> {
    Box::new(bsn! { ~{AnimationRow(slot)}
        Node { display: Display::None, width: percent(100), min_width: px(0), height: px(24),
            flex_shrink: 0.0, border: UiRect::bottom(px(1)) }
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            InteractionButton ~{AnimationChannelButton { slot, ..default() }}
            Node { width: px(152), min_width: px(92), max_width: percent(42), height: percent(100),
                flex_shrink: 0.0, align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(5)), overflow: Overflow::clip_x(),
                border: UiRect::right(px(1)) }
            BackgroundColor(colors::PANEL_BG_STRONG) BorderColor::all(colors::COPPER_BORDER_WEAK)
            Children [@label("", AnimationLabel::Channel(slot))]
        --
            ~{AnimationTrack { binding: 0, row: Some(slot) }} RelativeCursorPosition::default() UiInteraction::default()
            ~{FocusPolicy::Block}
            Node { position_type: PositionType::Relative, flex_grow: 1.0, flex_basis: px(0),
                min_width: px(0), height: percent(100), overflow: Overflow::clip() }
            BackgroundColor(colors::TRACK_BG)
            Children [@playhead()-- @keys(slot, 0)-- @keys(slot, 8)-- @keys(slot, 16)-- @keys(slot, 24)-- @keys(slot, 32)-- @keys(slot, 40)-- @keys(slot, 48)-- @keys(slot, 56)--
                ~{AnimationSelectionBox(slot)} ~{FocusPolicy::Pass}
                Node { display: Display::None, position_type: PositionType::Absolute, top: px(0), height: percent(100), border: px(1) }
                BackgroundColor(Color::srgba(0.3, 0.65, 0.95, 0.2)) BorderColor::all(Color::srgb(0.3,0.65,0.95))
            ]
        ]
    })
}

fn numeric_slot(field: AnimationNumericField) -> impl Scene {
    bsn! { ~{AnimationNumericSlot(field)}
        Node { width: px(110), min_width: px(90), height: px(26), flex_grow: 1.0, flex_basis: px(90) }
    }
}

pub(in super::super) fn panel() -> impl Scene {
    bsn! { AnimationPanel
        Node { display: Display::None, width: percent(100), min_width: px(0), flex_shrink: 0.0,
            flex_direction: FlexDirection::Column, row_gap: px(3), padding: UiRect::all(px(4)),
            border: UiRect::top(px(1)) }
        BackgroundColor(colors::PANEL_BG_SECTION) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            Node { width: percent(100), min_width: px(0), align_items: AlignItems::Center,
                column_gap: px(3), row_gap: px(3), flex_wrap: FlexWrap::Wrap,
                padding: UiRect::right(px(22)) }
            Children [@label("动作", AnimationLabel::Clip)--
                @button("前动作", AnimationAction::PreviousClip)-- @button("后动作", AnimationAction::NextClip)--
                @button("保存", AnimationAction::Save)-- @button("撤销", AnimationAction::Undo)--
                @button("重做", AnimationAction::Redo)--
                @button("重载动作", AnimationAction::Reload)--
                @crate::ui::context_help::help_button("动画编辑", "点选骨节，用移动轴或旋转环摆姿势。自动插帧在松开时记录；关闭后需插入姿势或还原。时间轴滚轮缩放，中键或Shift滚轮平移；空白拖动框选，Ctrl增减选区，拖菱形整组移动。悬停时间轴可用Delete、Ctrl+C/V/Z、空格；Esc取消拖动。旋转数值为XYZ角度，中键精确输入。密集关键帧显示为窄条，放大后可逐键选择。中央动画标签保持打开，在资产列表选择同骨架角色GLB后绑定；模型只用于预览，保存仅写当前动画GLB。外置GLTF只读。")]
        --
            Node { width: percent(100), min_width: px(0), align_items: AlignItems::Center,
                column_gap: px(3), row_gap: px(3), flex_wrap: FlexWrap::Wrap }
            Children [@button("首帧", AnimationAction::FirstFrame)--
                @button("前帧", AnimationAction::PreviousFrame)-- @button("播放", AnimationAction::TogglePlayback)--
                @button("后帧", AnimationAction::NextFrame)-- @button("末帧", AnimationAction::LastFrame)--
                @button("循环", AnimationAction::ToggleLoop)-- @button("30 fps", AnimationAction::ToggleFps)--
                @button("1×", AnimationAction::CycleSpeed)-- @label("0 / 0 帧", AnimationLabel::Time)]
        --
            Node { width: percent(100), min_width: px(0), column_gap: px(3), row_gap: px(3), flex_wrap: FlexWrap::Wrap, align_items: AlignItems::Center }
            Children [@label("", AnimationLabel::Bone)-- @button("上骨骼", AnimationAction::PreviousBone)--
                @button("下骨骼", AnimationAction::NextBone)-- @button("所选骨骼", AnimationAction::ToggleBoneFilter)--
                @button("自动插帧", AnimationAction::ToggleAutoKey)-- @button("插入姿势", AnimationAction::InsertPose)--
                @button("还原姿势", AnimationAction::ResetPose)]
        --
            Node { width: percent(100), min_width: px(0), column_gap: px(3), row_gap: px(3), flex_wrap: FlexWrap::Wrap, align_items: AlignItems::Center }
            Children [@button("绑定所选模型", AnimationAction::BindSelectedModel)-- @button("仅骨架", AnimationAction::UnbindModel)-- @label("", AnimationLabel::Model)]
        --
            Node { width: percent(100), min_width: px(0), height: px(24), flex_shrink: 0.0 }
            Children [
                Node { width: px(152), min_width: px(92), max_width: percent(42), height: percent(100),
                    flex_shrink: 0.0, align_items: AlignItems::Center, padding: UiRect::left(px(5)) }
                Children [@label("通道 / 关键帧", AnimationLabel::Pages)]
            --
                AnimationTrack::default() RelativeCursorPosition::default() UiInteraction::default()
                ~{FocusPolicy::Block}
                Node { position_type: PositionType::Relative, flex_grow: 1.0, flex_basis: px(0), min_width: px(0),
                    height: percent(100), overflow: Overflow::clip(), border: UiRect::bottom(px(1)) }
                BackgroundColor(colors::PANEL_BG_STRONG) BorderColor::all(colors::COPPER_BORDER_WEAK)
                Children [@tick(0)-- @tick(2)-- @tick(4)-- @tick(6)-- @tick(8)-- @playhead()]
            ]
        --
            Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column,
                min_height: px(24), flex_shrink: 0.0 }
            Children [@channel(0)-- @channel(1)-- @channel(2)-- @channel(3)-- @channel(4)--
                @channel(5)-- @channel(6)-- @channel(7)]
        --
            Node { width: percent(100), min_width: px(0), column_gap: px(3), row_gap: px(3),
                align_items: AlignItems::Center, flex_wrap: FlexWrap::Wrap }
            Children [@button("前通道", AnimationAction::PreviousChannelPage)--
                @button("后通道", AnimationAction::NextChannelPage)--
                @button("适配", AnimationAction::FitTimeline)-- @button("＋", AnimationAction::ZoomIn)-- @button("－", AnimationAction::ZoomOut)--
                @button("复制", AnimationAction::CopyKeys)-- @button("粘贴", AnimationAction::PasteKeys)--
                @button("前键", AnimationAction::PreviousKey)-- @button("后键", AnimationAction::NextKey)--
                @button("插入关键帧", AnimationAction::InsertKey)-- @button("删除关键帧", AnimationAction::DeleteKey)--
                @button("LINEAR", AnimationAction::CycleInterpolation)]
        -- @label("", AnimationLabel::Hint)--
            Node { width: percent(100), min_width: px(0), column_gap: px(3), row_gap: px(3), flex_wrap: FlexWrap::Wrap }
            Children [@numeric_slot(AnimationNumericField::Frame)-- @numeric_slot(AnimationNumericField::Value(0))--
                @numeric_slot(AnimationNumericField::Value(1))-- @numeric_slot(AnimationNumericField::Value(2))]
        --
            ~{AnimationLabel::Status} Text("") ~{FocusPolicy::Pass}
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
            TextColor(colors::TEXT_MUTED) TextLayout { linebreak: bevy::text::LineBreak::AnyCharacter }
            Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
        ]
    }
}

/// 数值框只在固定槽位首次出现时挂载；切动作、翻页和撤销保留控件实体。
pub(super) fn setup_numeric_inputs(
    mut commands: Commands,
    slots: Query<(Entity, &AnimationNumericSlot), Added<AnimationNumericSlot>>,
    assets: Res<AssetServer>,
) {
    for (slot, marker) in &slots {
        let field = marker.0;
        let (label, step, minimum, maximum, decimals) = match field {
            AnimationNumericField::Frame => ("帧", 1.0, 0.0, 10_000_000.0, 3),
            AnimationNumericField::Value(axis) => (
                ["X", "Y", "Z", "W"][axis],
                0.001,
                -1_000_000.0,
                1_000_000.0,
                5,
            ),
        };
        let spec = NumericInputSpec::f32(decimals)
            .with_step_buttons(false)
            .with_live_update(false)
            .with_min(NumericValue::F32(minimum))
            .with_max(NumericValue::F32(maximum));
        let options = NumericScrubFieldOptions::new(
            label,
            "",
            NumericValue::F32(0.0),
            spec,
            NumericInputScrubConfig::linear(step, step, minimum, maximum, 1.0),
        )
        .with_style(NumericScrubFieldStyle {
            width: 110.0,
            height: 26.0,
            ..default()
        });
        let font: Handle<Font> = feathrise_editor_core::editor_font(&assets, FONT_PATH);
        commands.entity(slot).with_children(|parent| {
            let root = spawn_numeric_scrub_field(
                parent,
                AnimationNumericInput { field, binding: 0 },
                font,
                options,
            );
            parent
                .commands()
                .entity(root)
                .entry::<Node>()
                .and_modify(|mut node| {
                    node.width = percent(100);
                    node.min_width = px(0);
                    node.max_width = percent(100);
                });
        });
    }
}

#[cfg(test)]
#[path = "ui_tests.rs"]
mod ui_tests;
