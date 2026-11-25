//! 连续动作表与姿势工具栏：时间视区和骨骼过滤复用固定控件池，不截断关键帧数据。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use super::{components::*, docking::{
    BindModelButton, BindModelHome, BindModelStatus, InlinePropertyHost, PropertySection,
}};
use feathrise_editor_core::FONT_PATH;
use feathrise_editor_ui::{animation_timeline as timeline, colors};
use bevy::{
    asset::HandleTemplate,
    prelude::*,
    text::FontSourceTemplate,
    ui::FocusPolicy,
};
use feathrise_editor_ui::{
    NumericInputScrubConfig, NumericInputSpec, NumericScrubFieldOptions, NumericScrubFieldStyle,
    NumericValue, metrics as ui_metrics, spawn_numeric_scrub_field,
};

fn label(text: &str, kind: AnimationLabel) -> impl Scene {
    let wraps = matches!(kind, AnimationLabel::Clip | AnimationLabel::ClipParameters);
    let layout = if wraps {
        TextLayout::linebreak(bevy::text::LineBreak::WordOrCharacter)
    } else {
        TextLayout::no_wrap()
    };
    let width = if wraps { percent(100) } else { Val::Auto };
    let max_width = if wraps { percent(100) } else { Val::Auto };
    bsn! { ~{kind} Text(text) ~{FocusPolicy::Pass}
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
        TextColor(colors::TEXT_PRIMARY) ~{layout}
        Node { width, min_width: px(0), max_width, overflow: Overflow::clip_x() }
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

fn bind_button() -> impl Scene {
    bsn! { BindModelButton AnimationDirectSourceControls @button("绑定所选模型", AnimationAction::BindSelectedModel) }
}

fn bind_status() -> impl Scene {
    bsn! { BindModelStatus @label("", AnimationLabel::Model) }
}

/// 动画编辑沿用场景视口左侧的图标和尺寸。轨道占用鼠标命中，避免按钮间隙选中骨骼或拖动画布。
pub fn pose_toolbar() -> impl Scene {
    bsn! { AnimationPoseToolbar UiInteraction::default() ~{FocusPolicy::Block} ZIndex(25)
        Node { display: Display::None, position_type: PositionType::Absolute,
            left: px(8), top: px(8), width: px(36), flex_direction: FlexDirection::Column,
            row_gap: px(2), padding: UiRect::all(px(2)), border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(2)) }
        BackgroundColor(colors::PANEL_BG_STRONG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [@pose_icon("移动 (G)", "icons/editor/tool_move.png", AnimationAction::MovePose)--
            @pose_icon("旋转 (R)", "icons/editor/tool_rotate.png", AnimationAction::RotatePose)]
    }
}

fn pose_icon(label: &'static str, path: &'static str, action: AnimationAction) -> impl Scene {
    let icon = HandleTemplate::Path(path.into());
    let name = Name::new(label);
    bsn! { InteractionButton ~{AnimationButton { action, binding: 0 }} ~{name} ~{FocusPolicy::Block}
        Node { width: px(30), height: px(30), flex_shrink: 0.0,
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(2)) }
        BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [ImageNode { image: icon, color: colors::TEXT_PRIMARY }
            Node { width: px(20), height: px(20), flex_shrink: 0.0 }
            ~{FocusPolicy::Pass} Pickable::IGNORE]
    }
}

fn playhead() -> impl Scene {
    bsn! { AnimationPlayhead @timeline::playhead() }
}

fn tick(index: usize) -> impl Scene {
    let node = timeline::tick_node(index);
    bsn! { ~{node} ~{AnimationRulerTick(index)}
        BorderColor::all(colors::COPPER_BORDER_WEAK) ~{FocusPolicy::Pass}
        Children [@label("0", AnimationLabel::Tick(index))]
    }
}

fn key(row: usize, slot: usize) -> impl Scene {
    bsn! { ~{AnimationKeyButton { row, slot, ..default() }} @timeline::key_marker() }
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
            ~{AnimationTrack { binding: 0, row: Some(slot) }} @timeline::lane(false)
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

fn wrap_row() -> impl Scene {
    bsn! {
        Node { min_width: px(0), max_width: percent(100), flex_wrap: FlexWrap::Wrap,
            align_items: AlignItems::Center, column_gap: px(3), row_gap: px(3) }
    }
}

/// 模式入口挂在 GLB 属性上，无动作的模型也可发起新建。
pub(super) fn mode_controls() -> impl Scene {
    bsn! { @wrap_row()
        Children [@button("新建动画", AnimationAction::CreateClip)--
            @button("编辑动画", AnimationAction::ToggleEditing)]
    }
}

fn property_home(section: PropertySection) -> impl Scene {
    bsn! { ~{InlinePropertyHost(section)} @wrap_row() }
}

pub fn panel() -> impl Scene {
    bsn! { AnimationPanel
        Node { display: Display::None, width: percent(100), min_width: px(0), flex_shrink: 0.0,
            flex_direction: FlexDirection::Column, row_gap: px(3), padding: UiRect::all(px(4)),
            border: UiRect::top(px(1)) }
        BackgroundColor(colors::PANEL_BG_SECTION) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            Node { width: percent(100), min_width: px(0), align_items: AlignItems::Center,
                column_gap: px(3), row_gap: px(3), flex_wrap: FlexWrap::Wrap,
                padding: UiRect::right(px(22)) }
            Children [
                ~{InlinePropertyHost(PropertySection::Clips)} Node { width: percent(100), min_width: px(0) }
                Children [
                    // 标题和参数各用完整栏宽，长名称可换行。前后动作按钮独立排布，避免挤窄标题。
                    ~{PropertySection::Clips}
                    Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(3) }
                    Children [
                        @label("动作", AnimationLabel::Clip)--
                        @label("", AnimationLabel::ClipParameters)--
                        AnimationDirectSourceControls @wrap_row()
                        Children [@button("前动作", AnimationAction::PreviousClip)--
                            @button("后动作", AnimationAction::NextClip)]
                    ]
                ]
            --
                @super::clip_management::scene()
            --
                ~{AnimationWriteControls::History} @wrap_row()
                Children [@button("保存", AnimationAction::Save)-- @button("撤销", AnimationAction::Undo)--
                    @button("重做", AnimationAction::Redo)-- @button("重载动作", AnimationAction::Reload)]
            --
                AnimationWriteControls
                @feathrise_editor_ui::context_help::help_button("动画编辑", "点选骨节，用移动轴或旋转环摆姿势。自动插帧在松开时记录；关闭后需插入姿势或还原。时间轴滚轮缩放，中键或Shift滚轮平移；空白拖动框选，Ctrl增减选区，拖菱形整组移动。悬停时间轴可用Delete、Ctrl+C/V/Z、空格；Esc取消拖动。旋转数值为XYZ角度，左键单击精确输入。密集关键帧显示为窄条，放大后可逐键选择。中央动画标签保持打开，在资产列表选择同骨架角色GLB后绑定；模型只用于预览，保存仅写当前动画GLB。外置GLTF只读。", FONT_PATH)
            ]
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
            Children [
                @property_home(PropertySection::Pose)
                Children [
                    ~{PropertySection::Pose} @wrap_row()
                    Children [@label("", AnimationLabel::Bone)-- @button("上骨骼", AnimationAction::PreviousBone)--
                        @button("下骨骼", AnimationAction::NextBone)--
                        AnimationWriteControls @button("还原姿势", AnimationAction::ResetPose)]
                ]
            --
                @button("所选骨骼", AnimationAction::ToggleBoneFilter)--
                AnimationWriteControls @button("自动插帧", AnimationAction::ToggleAutoKey)--
                AnimationWriteControls @button("插入姿势", AnimationAction::InsertPose)
            ]
        --
            @property_home(PropertySection::Binding)
            Children [
                ~{PropertySection::Binding} BindModelHome @wrap_row()
                Children [@bind_button()-- @button("仅骨架", AnimationAction::UnbindModel)-- @bind_status()]
            ]
        --
            Node { width: percent(100), min_width: px(0), height: px(24), flex_shrink: 0.0 }
            Children [
                Node { width: px(152), min_width: px(92), max_width: percent(42), height: percent(100),
                    flex_shrink: 0.0, align_items: AlignItems::Center, padding: UiRect::left(px(5)) }
                Children [@label("通道 / 关键帧", AnimationLabel::Pages)]
            --
                AnimationTrack::default() @timeline::lane(true)
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
                AnimationWriteControls @button("复制", AnimationAction::CopyKeys)--
                AnimationWriteControls @button("粘贴", AnimationAction::PasteKeys)--
                @button("前键", AnimationAction::PreviousKey)-- @button("后键", AnimationAction::NextKey)--
                AnimationWriteControls @button("插入关键帧", AnimationAction::InsertKey)--
                AnimationWriteControls @button("删除关键帧", AnimationAction::DeleteKey)--
                AnimationWriteControls @button("LINEAR", AnimationAction::CycleInterpolation)]
        --
            AnimationWriteControls @super::curve_editor::panel()
        --
            AnimationWriteControls @label("", AnimationLabel::Hint)
        --
            AnimationWriteControls
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
