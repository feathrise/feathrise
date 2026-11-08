//! 视口笔刷类型与侧栏半高、整平反馈共用会话参数；作者任务仍由雕刻域拥有。
use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::EditorButtonClick;
use feathrise_editor_ui::metrics as ui_metrics;
use super::{components::*, systems::TerrainSidebarInputGate};
use crate::ui::sidebars::{EditorLeftSidebarTab, MapEditorSidebarState};
use crate::{
    resources::MapEditorState,
    types::EditorTool,
    ui::{scene::FONT_PATH, theme as colors},
};
use bevy::ui::FocusPolicy;
use bevy::{prelude::*, text::FontSourceTemplate};
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, NumericInputChanged, NumericInputCommitted,
    NumericInputDisabled, NumericInputScrubConfig, NumericInputSetValue, NumericInputSpec,
    NumericInputValue, NumericScrubFieldOptions, NumericScrubFieldStyle, NumericValue,
    spawn_numeric_scrub_field,
};
use feathrise_map_terrain::volume::VolumeBrushShape;

pub(crate) fn spawn_types(parent: &mut ChildSpawnerCommands, font: &Handle<Font>) {
    // 【需求设计】类型紧跟模式行，共用视口浮层的鼠标占用范围；不能在按钮或行间空隙落笔。
    parent
        .spawn((
            VolumeBrushTypeRow,
            Node {
                display: Display::None,
                width: percent(100),
                min_width: px(0),
                column_gap: px(3),
                row_gap: px(3),
                flex_wrap: FlexWrap::Wrap,
                flex_shrink: 0.0, // 笔刷类型同属模式浮层，完整折行高度必须参与背景和鼠标范围。
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_children(|row| {
            row.spawn((
                feathrise_editor_ui::localization::editor_label("笔刷类型"),
                TextFont {
                    font: font.clone().into(),
                    font_size: 12.0.into(),
                    ..default()
                },
                TextColor(colors::TEXT_MUTED),
                TextLayout::no_wrap(),
                Node {
                    width: px(68),
                    flex_shrink: 0.0,
                    ..default()
                },
                FocusPolicy::Pass,
                Pickable::IGNORE,
            ));
            for (label, shape) in [
                ("球形", VolumeBrushShape::Sphere),
                ("方盒", VolumeBrushShape::Box),
                ("圆柱", VolumeBrushShape::Cylinder),
            ] {
                row.spawn((
                    InteractionButton,
                    VolumeBrushShapeButton(shape),
                    FocusPolicy::Block,
                    Node {
                        width: px(48),
                        height: px(26),
                        flex_shrink: 0.0,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border: UiRect::all(px(1)),
                        ..default()
                    },
                    BackgroundColor(colors::SLOT_BG),
                    BorderColor::all(colors::COPPER_BORDER_WEAK),
                ))
                .with_child((
                    Text::new(label),
                    TextFont {
                        font: font.clone().into(),
                        font_size: 12.0.into(),
                        ..default()
                    },
                    TextColor(colors::TEXT_PRIMARY),
                    TextLayout::no_wrap(),
                    FocusPolicy::Pass,
                    Pickable::IGNORE,
                ));
            }
        });
}

pub(super) fn panel() -> impl Scene {
    bsn! {
        VolumeBrushControls
        Node { display: Display::None, width: percent(100), flex_direction: FlexDirection::Column, row_gap: px(5), flex_shrink: 0.0 }
        Children [
            (
                VolumeBrushHeightRow
                Node { display: Display::None, width: percent(100), height: px(26), flex_shrink: 0.0 }
                Children [(
                    VolumeBrushHeightSlot
                    Node { width: percent(100), min_width: px(0) }
                )]
            ),
            (
                InteractionButton
                VolumeBrushHeightLockButton
                Node { height: px(26), width: percent(100), align_items: AlignItems::Center, justify_content: JustifyContent::Center, border: UiRect::all(px(1)) }
                BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
                BorderColor::all(colors::COPPER_BORDER_WEAK)
                Children [(
                    Text("锁定本笔高度") @feathrise_editor_ui::localization::label_scene("锁定本笔高度")
                    TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                    TextColor(colors::TEXT_PRIMARY)
                )]
            ),
            (
                VolumeBrushHintText
                Text("")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                TextColor(colors::TEXT_MUTED)
                TextLayout { linebreak: LineBreak::AnyCharacter }
            ),
        ]
    }
}

pub(super) fn setup(
    mut commands: Commands,
    slots: Query<Entity, Added<VolumeBrushHeightSlot>>,
    assets: Res<AssetServer>,
    editor: Res<MapEditorState>,
) {
    for slot in &slots {
        let options = NumericScrubFieldOptions::new(
            "竖直半高",
            "m",
            NumericValue::F32(editor.volume_brush_half_height),
            NumericInputSpec::f32(1)
                .with_min(NumericValue::F32(0.5))
                .with_max(NumericValue::F32(8.))
                .with_step(NumericValue::F32(0.1)),
            NumericInputScrubConfig::linear(0.02, 0.1, 0.5, 8., 1.),
        )
        .with_style(NumericScrubFieldStyle {
            width: 160.,
            height: 26.,
            border_radius: 3.,
            column_gap: 4.,
            label_font_size: 10.,
            value_font_size: 11.,
            unit_font_size: 10.,
        });
        let font = feathrise_editor_core::editor_font(&assets, FONT_PATH);
        commands.entity(slot).with_children(|slot| {
            let root = spawn_numeric_scrub_field(slot, VolumeBrushHeightField, font, options);
            slot.commands()
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

pub(super) fn handle(
    mut editor: ResMut<MapEditorState>,
    gate: TerrainSidebarInputGate,
    sidebar: Res<MapEditorSidebarState>,
    spatial: Option<Res<crate::viewport::spatial_brush::SpatialBrushState>>,
    shapes: Query<(&EditorButtonClick, &VolumeBrushShapeButton), Changed<EditorButtonClick>>,
    locks: Query<&EditorButtonClick, (With<VolumeBrushHeightLockButton>, Changed<EditorButtonClick>)>,
    heights: Query<Has<NumericInputDisabled>, With<VolumeBrushHeightField>>,
    mut changed: MessageReader<NumericInputChanged>,
    mut committed: MessageReader<NumericInputCommitted>,
) {
    let enabled = editor.open
        && !editor.close_requested
        && !editor.new_map_dialog_open
        && editor.tool.is_volume_sculpt()
        && sidebar.tool_page_visible(EditorLeftSidebarTab::Terrain)
        && !gate.modal_blocked();
    for event in changed.read() {
        if enabled
            && editor.volume_brush_shape != VolumeBrushShape::Sphere
            && heights.get(event.entity) == Ok(false)
            && let Some(value) = event.value.as_f32().filter(|v| v.is_finite())
        {
            editor.volume_brush_half_height = value.clamp(0.5, 8.);
        }
    }
    // 公共控件Esc/拒绝提交返回原值；参数不属于地图历史，但必须跟随完整输入事务恢复。
    for event in committed.read() {
        if enabled
            && editor.volume_brush_shape != VolumeBrushShape::Sphere
            && heights.get(event.entity) == Ok(false)
            && let Some(value) = event.value.as_f32().filter(|v| v.is_finite())
        {
            editor.volume_brush_half_height = value.clamp(0.5, 8.);
        }
    }
    if !enabled || gate.blocked() {
        return;
    }
    for (interaction, shape) in &shapes {
        if interaction.is_clicked() {
            if editor.volume_brush_shape != shape.0 {
                editor.volume_brush_lock_height = shape.0 != VolumeBrushShape::Sphere;
            } // 规则刷默认固定高度推进；需要继续向内挖时可显式关闭，球形恢复原自由深度手感。
            editor.volume_brush_shape = shape.0;
        }
    }
    for interaction in &locks {
        if interaction.is_clicked()
            && editor.tool != EditorTool::VolumeFlatten
            && !spatial.as_ref().is_some_and(|s| s.enabled())
        {
            editor.volume_brush_lock_height = !editor.volume_brush_lock_height;
        }
    }
}

pub(super) fn sync(
    mut commands: Commands,
    editor: Res<MapEditorState>,
    sidebar: Res<MapEditorSidebarState>,
    gate: TerrainSidebarInputGate,
    volume: Option<Res<crate::authoring::volume_sculpt::VolumeAuthoringState>>,
    spatial: Option<Res<crate::viewport::spatial_brush::SpatialBrushState>>,
    active_input: Res<ActiveNumericInput>,
    active_scrub: Res<ActiveNumericInputScrub>,
    mut nodes: Query<
        (&mut Node, Has<VolumeBrushHeightRow>),
        Or<(
            With<VolumeBrushControls>,
            With<VolumeBrushHeightRow>,
            With<VolumeBrushTypeRow>,
        )>,
    >,
    mut shapes: Query<
        (
            &VolumeBrushShapeButton,
            &Interaction,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        Without<VolumeBrushHeightLockButton>,
    >,
    mut locks: Query<
        (
            Entity,
            &mut BackgroundColor,
            &mut BorderColor,
            Has<bevy::ui::InteractionDisabled>,
        ),
        With<VolumeBrushHeightLockButton>,
    >,
    heights: Query<
        (Entity, &NumericInputValue, Has<NumericInputDisabled>),
        With<VolumeBrushHeightField>,
    >,
    mut hints: Query<&mut Text, With<VolumeBrushHintText>>,
    mut set_value: MessageWriter<NumericInputSetValue>,
) {
    let path = spatial.as_ref().is_some_and(|s| s.path_active());
    let visible = editor.open
        && editor.tool.is_volume_sculpt()
        && sidebar.tool_page_visible(EditorLeftSidebarTab::Terrain)
        && !path;
    let enabled = visible
        && !editor.close_requested
        && !editor.new_map_dialog_open
        && editor.volume_brush_shape != VolumeBrushShape::Sphere
        && !gate.modal_blocked();
    for (mut node, height) in &mut nodes {
        let display =
            if visible && (!height || editor.volume_brush_shape != VolumeBrushShape::Sphere) {
                Display::Flex
            } else {
                Display::None
            };
        if node.display != display {
            node.display = display;
        }
    }
    for (shape, interaction, mut background, mut border) in &mut shapes {
        let color = match (*interaction, shape.0 == editor.volume_brush_shape) {
            (Interaction::Pressed, _) => colors::SELECT_BG_PRESSED,
            (Interaction::Hovered, _) => colors::SELECT_BG_HOVER,
            (Interaction::None, true) => colors::SELECT_BG,
            (Interaction::None, false) => colors::SLOT_BG,
        };
        if background.0 != color {
            background.0 = color;
        }
        // 笔刷几何类型和锁高同属普通工具选择，使用填色与弱边框，不冒充输入焦点。
        let next = BorderColor::all(colors::COPPER_BORDER_WEAK);
        if *border != next {
            *border = next;
        }
    }
    for (entity, mut background, mut border, disabled) in &mut locks {
        let space = spatial.as_ref().is_some_and(|s| s.enabled());
        let locked =
            !space && (editor.tool == EditorTool::VolumeFlatten || editor.volume_brush_lock_height);
        let color = if locked {
            colors::SELECT_BG
        } else {
            colors::BUTTON_DEV_SMALL_NORMAL
        };
        if background.0 != color {
            background.0 = color;
        }
        let next = BorderColor::all(colors::COPPER_BORDER_WEAK);
        if *border != next {
            *border = next;
        }
        let disable =
            space || editor.tool == EditorTool::VolumeFlatten || !visible || gate.modal_blocked();
        if disabled != disable {
            if disable {
                commands
                    .entity(entity)
                    .insert(bevy::ui::InteractionDisabled);
            } else {
                commands
                    .entity(entity)
                    .remove::<bevy::ui::InteractionDisabled>();
            }
        }
    }
    for (entity, value, disabled) in &heights {
        if enabled == disabled {
            if enabled {
                commands.entity(entity).remove::<NumericInputDisabled>();
            } else {
                commands.entity(entity).insert(NumericInputDisabled);
            }
        }
        if active_input.entity != Some(entity)
            && active_scrub.entity != Some(entity)
            && value.0.as_f32() != Some(editor.volume_brush_half_height)
        {
            set_value.write(NumericInputSetValue {
                entity,
                value: NumericValue::F32(editor.volume_brush_half_height),
                cancel_edit: true,
            });
        }
    }
    let hint = if path {
        "路径节点控制通道底部高程；宽高与截面在视口路径控件中设置。".to_owned()
    } else if spatial.as_ref().is_some_and(|s| s.enabled()) {
        if editor.tool == EditorTool::VolumeFlatten {
            "空间整平以刷心或直线起点Y为平面；调整位置后点击执行雕刻。".to_owned()
        } else {
            "空间笔刷位置固定；调整XYZ或设置起终点后点击执行雕刻。".to_owned()
        }
    } else if editor.tool == EditorTool::VolumeFlatten {
        volume
            .as_deref()
            .and_then(|v| v.flatten_height())
            .map_or_else(
                || "按下锁定落点高度；松开后下一笔重新取高度。".to_owned(),
                |y| format!("本笔整平高度：{y:.2}m（已锁定）"),
            )
    } else if editor.volume_brush_shape == VolumeBrushShape::Sphere {
        "球形三轴同半径；适合自然洞穴。".into()
    } else {
        format!(
            "水平半径 {:.1}m · 总高度 {:.1}m；{}。",
            editor.brush_radius,
            editor.volume_brush_half_height * 2.,
            if editor.volume_brush_lock_height {
                "本笔高度固定，平底推进"
            } else {
                "跟随命中，可继续深入"
            }
        )
    };
    for mut text in &mut hints {
        if text.0 != hint {
            text.0.clone_from(&hint);
        }
    }
}
