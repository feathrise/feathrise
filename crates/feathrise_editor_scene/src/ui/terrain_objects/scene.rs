//! 网格创建、左侧场景树与右侧连续属性；三处共用同一稳定对象与作者事务。
use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use super::components::*;
use crate::ui::{
    scene::FONT_PATH,
    sidebars::{EditorSidebarHelp, sidebar_help_body, sidebar_help_button},
    theme as colors,
};
use bevy::{prelude::*, text::FontSourceTemplate};
use feathrise_editor_ui::{
    NumericInputScrubConfig, NumericInputSpec, NumericScrubFieldOptions, NumericScrubFieldStyle,
    NumericValue, spawn_localized_numeric_scrub_field,
};

fn button(label: &str, action: Action) -> impl Scene {
    let lod_binding = matches!(action, Action::AddLod(_) | Action::RemoveLod(_))
        .then(|| bsn! { LodActionBinding::default() });
    (bsn! { InteractionButton ActionButton(action) Node { height: px(25), flex_grow: 1.0, min_width: px(0),
    border: px(1), border_radius: BorderRadius::all(px(2)), align_items: AlignItems::Center,
    justify_content: JustifyContent::Center, padding: UiRect::horizontal(px(3)) }
    BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL) BorderColor::all(colors::COPPER_BORDER_WEAK)
    Children [ActionLabel(action) Text(label) @feathrise_editor_ui::localization::label_scene(label) TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
        TextColor(colors::TEXT_PRIMARY)] }, lod_binding)
}
fn caption(label: &str) -> impl Scene {
    bsn! { Text(label) @feathrise_editor_ui::localization::label_scene(label) TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
    TextColor(colors::TEXT_MUTED) TextLayout::linebreak(LineBreak::AnyCharacter)
    Node { width: percent(100), min_width: px(0) } }
}
fn row(index: usize) -> impl Scene {
    bsn! { InteractionButton Row(index) Node { display: Display::None, position_type: PositionType::Absolute, width: percent(100), height: px(24),
    align_items: AlignItems::Center, padding: UiRect::horizontal(px(4)), flex_shrink: 0.0,
    column_gap: px(2), border: px(0), border_radius: BorderRadius::all(px(2)) }
    BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
    // 裁切由文本父容器承担；Text自身的overflow不会裁掉本节点绘制的超长字形。
    Children [@crate::ui::scene::tree_icon("icons/editor/tree_mesh.png")--
        Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0), overflow: Overflow::clip_x() }
        Children [RowText(index) Text("") TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_VALUE) TextLayout::no_wrap()
            Node { width: percent(100), min_width: px(0) } Pickable::IGNORE]--
        RowKind(index) Text("") TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
            TextColor(colors::TEXT_MUTED) TextLayout::no_wrap() Pickable::IGNORE Node { flex_shrink: 0.0 }--
        @row_action(index, Action::Enabled)-- @row_action(index, Action::Visible)-- @row_action(index, Action::Lock)] }
}
fn row_action(index: usize, action: Action) -> impl Scene {
    use crate::ui::tooltips::{EditorTooltipTarget, TooltipAction};
    let tooltip = match action {
        Action::Enabled => TooltipAction::TerrainParticipation,
        Action::Visible => TooltipAction::TerrainVisibility,
        _ => TooltipAction::TerrainLock,
    };
    bsn! { InteractionButton ActionButton(action) RowAction { slot: index, target: None }
    EditorTooltipTarget(tooltip)
    Node { width: px(18), height: px(21), flex_shrink: 0.0, border: px(0),
        border_radius: BorderRadius::all(px(3)), align_items: AlignItems::Center, justify_content: JustifyContent::Center }
    BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL) BorderColor::all(colors::COPPER_BORDER_WEAK)
    Children [RowIcon { slot: index, action } ImageNode::default()
        Node { width: px(14), height: px(14), flex_shrink: 0.0 } Pickable::IGNORE] }
}
fn lod_row(index: usize, display: bool) -> impl Scene {
    let action = Action::SelectLod(index, display);
    bsn! { InteractionButton LodRow(index, display) LodRowBinding::default() ActionButton(action)
    Node { display: Display::None, position_type: PositionType::Absolute, top: px(0), left: px(0), width: percent(100), height: px(25), min_width: px(0),
        border: UiRect::bottom(px(1)), align_items: AlignItems::Center,
        padding: UiRect::horizontal(px(4)), column_gap: px(3), flex_shrink: 0.0 }
    BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL) BorderColor::all(colors::COPPER_BORDER_WEAK)
    Children [@lod_layer(index, display)-- @lod_value_cell(index, LodColumn::Multiplier, display)--
        @lod_value_cell(index, LodColumn::Range, display)] }
}

fn lod_layer(index: usize, display: bool) -> impl Scene {
    bsn! { Node { flex_basis: px(28), min_width: px(0), height: px(23),
        align_items: AlignItems::Center, padding: UiRect::horizontal(px(4)),
        overflow: Overflow::clip_x() }
    Children [~{LodRowText(index, LodColumn::Layer, display)} Text("")
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
        TextColor(colors::TEXT_VALUE) TextLayout::no_wrap()
        Node { width: percent(100), min_width: px(0) } Pickable::IGNORE] }
}

/// 每组64行各两个常驻公共数值框，首次点击直接接管；用途固定，作者层数只改变本组行绑定。
fn lod_value_cell(index: usize, column: LodColumn, display: bool) -> impl Scene {
    let field = Field::Lod(display, 8 + index * 2 + usize::from(matches!(column, LodColumn::Multiplier)));
    bsn! { Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0), height: px(23),
        align_items: AlignItems::Center, border_radius: BorderRadius::all(px(2)), overflow: Overflow::clip_x() }
        BackgroundColor(colors::INPUT_BG)
        Children [
            Slot(field) Node { width: percent(100), height: px(23), min_width: px(0), flex_shrink: 0.0 }--
            LodValueFallback(field) ~{LodRowText(index, column, display)} Text("")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                TextColor(colors::TEXT_MUTED) TextLayout::no_wrap()
                Node { display: Display::None, width: percent(100), min_width: px(0), padding: UiRect::horizontal(px(4)) } Pickable::IGNORE
        ] }
}

fn lod_heading(label: &str) -> impl Scene {
    let grow = if label == "LOD" { 0.0_f32 } else { 1.0_f32 };
    let basis = if label == "LOD" { px(28) } else { px(0) };
    bsn! { Text(label) @feathrise_editor_ui::localization::label_scene(label) TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
        TextColor(colors::TEXT_MUTED) TextLayout::no_wrap()
        Node { flex_grow: grow, flex_basis: basis, min_width: px(0) } }
}
fn lod_list(display: bool) -> impl Scene {
    // 作者层数没有固定上限；常驻行池只覆盖可见区域，完整高度让既有属性栏直接连续滚动。
    let rows = (0..super::lod_list::ROW_POOL_SIZE).map(|index| lod_row(index, display)).collect::<Vec<_>>();
    bsn! { LodList(display) Node { width: percent(100), min_width: px(0), height: px(0), flex_shrink: 0.0 }
        Children [{rows}] }
}
fn slot(field: Field) -> impl Scene {
    bsn! { Slot(field) Node { width: percent(100), height: px(24), min_width: px(0), flex_shrink: 0.0 } }
}

fn compact_slot(field: Field) -> impl Scene {
    bsn! { Slot(field) Node { flex_grow: 1.0, flex_basis: px(0), height: px(24), min_width: px(0), flex_shrink: 0.0 } }
}

/// 三行共用固定标签列与三个等分轨道，位置、完整旋转和缩放保持相同的轴列边缘。
fn axis_row_node() -> Node {
    Node { display: Display::Grid, width: percent(100), min_width: px(0),
        grid_template_columns: vec![RepeatedGridTrack::px(1, 32.0), RepeatedGridTrack::flex(3, 1.0)],
        column_gap: px(3), align_items: AlignItems::Center, flex_shrink: 0.0, ..default() }
}

/// 标签只占固定窄列，三轴沿用独立字段身份与原数值输入根，拖动或缩放不会重建控件。
fn axis_row(label: &str, first: usize) -> impl Scene {
    bsn! { ~{axis_row_node()}
        Children [
            // 位置/缩放的组标签作为每行独立模板值绑定。
            Text(label) @feathrise_editor_ui::localization::label_scene(label) TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                TextColor(colors::TEXT_MUTED) Node { width: px(32), flex_shrink: 0.0 }--
            @compact_slot(Field::Selected(first))-- @compact_slot(Field::Selected(first + 1))--
            @compact_slot(Field::Selected(first + 2))
        ] }
}

/// 旋转使用独立轴用途，不能占用源格距或LOD数字槽；整体姿态由同一后台事务采用。
fn rotation_row() -> impl Scene {
    bsn! { ~{axis_row_node()}
        Children [
            Text("旋转") @feathrise_editor_ui::localization::label_scene("旋转") TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                TextColor(colors::TEXT_MUTED) Node { width: px(32), flex_shrink: 0.0 }--
            @compact_slot(Field::Rotation(0))-- @compact_slot(Field::Rotation(1))-- @compact_slot(Field::Rotation(2))
        ] }
}

/// 网格工具页不复制对象行池；从场景列表和视口点选都更新唯一稳定选择。
pub(crate) fn panel() -> impl Scene {
    bsn! { MeshPageRoot Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column,
    flex_shrink: 0.0, row_gap: px(5), padding: UiRect::top(px(4)) }
    Children [
        // 中文正文字体包含这组实心三角；小三角▸/▾缺字会显示方框。
        @button("添加网格 ▶", Action::ToggleCreation)--
        PanelSection(Section::Creation) Node { display: Display::None, width: percent(100), min_width: px(0),
            flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0,
            padding: UiRect::all(px(4)), border: UiRect::bottom(px(1)) }
            BorderColor::all(colors::COPPER_BORDER_WEAK)
            Children [
                @caption("基础形状")--
                @button("立方体", Action::Cuboid)--
                @caption("导入模型")--
                @button("原始 GLB · 使用所选资产", Action::ImportGlb)--
                @caption("可塑形网格")--
                Node { width: percent(100), column_gap: px(3) } Children [
                    @button("高度网格", Action::Glb)-- @button("Clipmap", Action::Clipmap)-- @button("体素", Action::Voxel)]--
                @caption("放置方式")--
                Node { width: percent(100), column_gap: px(3) } Children [
                    @button("视口点放", Action::PlacementViewport)-- @button("指定坐标", Action::PlacementCoordinates)]--
                @slot(Field::Creation(0))-- @slot(Field::Creation(1))-- @slot(Field::Creation(2))--
                @slot(Field::Creation(3))-- @slot(Field::Creation(4))-- @slot(Field::Creation(5))--
                @slot(Field::Creation(6))-- @slot(Field::Creation(7))--
                PanelSection(Section::CreationCuboid) Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column,
                    row_gap: px(3), flex_shrink: 0.0 } Children [
                    @caption("几何")-- @slot(Field::CuboidCreation(0))-- @slot(Field::CuboidCreation(1))-- @slot(Field::CuboidCreation(2))--
                    @caption("材质")-- @slot(Field::CuboidCreation(3))-- @slot(Field::CuboidCreation(4))-- @slot(Field::CuboidCreation(5))-- @slot(Field::CuboidCreation(6))--
                    @button("盒碰撞：开", Action::CuboidCollision)
                ]--
                CreationFeedback Text("") TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                    TextColor(colors::TEXT_MUTED) TextLayout::linebreak(LineBreak::AnyCharacter)
                    Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }--
                @button("开始放置", Action::StartCreation)--
                @button("取消点放", Action::CancelPlacement)--
                InteractionButton ~{feathrise_editor_ui::context_help::ContextHelpButton("初始规格 · 仅用于下一次添加".to_owned())}
                    Node { position_type: PositionType::Absolute, right: px(0), top: px(0), width: px(18), height: px(18),
                        align_items: AlignItems::Center, justify_content: JustifyContent::Center }
                    BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
                    Children [
                        Text("?") TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                            TextColor(colors::TEXT_HINT) Pickable::IGNORE--
                        CreationSummary feathrise_editor_ui::context_help::ContextHelpBody
                            Text("高度网格高度域不是平面厚度；Clipmap高度不受此项限制；体素高为初始实心盒高。原始 GLB 使用资产几何。") @feathrise_editor_ui::localization::label_scene("高度网格高度域不是平面厚度；Clipmap高度不受此项限制；体素高为初始实心盒高。原始 GLB 使用资产几何。")
                            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                            TextColor(colors::TEXT_MUTED) TextLayout::linebreak(LineBreak::AnyCharacter)
                            Node { display: Display::None, width: percent(100), min_width: px(0), flex_shrink: 0.0 }
                    ] // 动态规格解释复用原唯一CreationSummary，借到根浮层，不增加正文高度。
            ]
    ] }
}

/// 完整网格列表连续滚动；固定十行只覆盖可见窗口，参数在右侧下方独立滚动。
pub(crate) fn scene_list_panel() -> impl Scene {
    bsn! { MeshSceneListRoot Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column,
    flex_shrink: 0.0, row_gap: px(3), padding: UiRect::top(px(4)) }
    Children [
        ListSummary Text("当前场景暂无独立网格。") @feathrise_editor_ui::localization::label_scene("当前场景暂无独立网格。")
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
            TextColor(colors::TEXT_MUTED) TextLayout::linebreak(LineBreak::AnyCharacter)
            Node { width: percent(100), min_width: px(0) }--
        ListViewport crate::ui::list_layout::SceneObjectListScroll bevy::ui::RelativeCursorPosition::default() ScrollPosition::default()
            Node { width: percent(100), min_width: px(0), height: px(0), flex_shrink: 0.0, overflow: Overflow::scroll_y() }
            Children [ListContent Node { width: percent(100), min_width: px(0), height: px(0), flex_shrink: 0.0 }
                Children [@row(0)-- @row(1)-- @row(2)-- @row(3)-- @row(4)-- @row(5)-- @row(6)-- @row(7)-- @row(8)-- @row(9)]]
    ] }
}

pub(super) fn setup(
    mut commands: Commands,
    slots: Query<(Entity, &Slot, &ChildOf), Added<Slot>>,
    parents: Query<&ChildOf>,
    server: Res<AssetServer>,
) {
    for (entity, slot, parent) in &slots {
        let field = slot.0;
        let index = field.index();
        let lod_cell = matches!(field, Field::Lod(_, _));
        let lod_row = lod_cell.then(|| parents.get(parent.parent()).expect("LOD数值槽属于固定行单元格").parent());
        let multiplier = index >= 8 && index % 2 == 1;
        let (mut label, mut unit, minimum, step) = if matches!(field, Field::CuboidCreation(_)) {
            match index {
                0..=2 => (["尺寸 X", "尺寸 Y", "尺寸 Z"][index], "m", 0.001, 0.001),
                3..=5 => (["颜色 R", "颜色 G", "颜色 B"][index - 3], "", 0.0, 1.0),
                _ => ("粗糙度", "", 0.0, 0.001),
            }
        } else if let Field::Rotation(axis) = field {
            (["X", "Y", "Z"][axis], "°", -1_000_000.0, 0.01)
        } else if field == Field::AuthorHeightDomain {
            ("作者高度域", "m", 0.01, 0.01)
        } else if field == Field::VoxelHeight {
            ("体素高", "m", 0.01, 0.01)
        } else if matches!(field, Field::Policy(_)) {
            [
                ("屏幕误差", "px", 0.1, 0.1),
                ("切换迟滞", "比例", 0.0, 0.01),
                ("安全近区", "m", 32.0, 1.0),
                ("碰撞误差", "m", 0.0001, 0.001),
            ][index]
        } else if matches!(field, Field::Selected(index) if (3..6).contains(&index)) {
            (
                ["缩放 X", "缩放 Y", "缩放 Z"][index - 3],
                "×",
                0.000001,
                0.001,
            )
        } else if index < 8 {
            (
                [
                    "位置 X",
                    "位置 Y",
                    "位置 Z",
                    "宽度 X",
                    "高度域/体素高",
                    "长度 Z",
                    "朝向 Y",
                    "源格距",
                ][index],
                if index == 6 { "°" } else { "m" },
                if index == 7 {
                    if matches!(field, Field::Creation(_)) { super::creation::MIN_SPACING } else { 0.01 }
                } else if (3..6).contains(&index) {
                    0.01
                } else {
                    -1_000_000.0
                },
                if index == 7 { super::creation::SPACING_STEP } else { 0.01 },
            )
        } else if multiplier {
            ("格距倍率", "×", 1.0, 1.0)
        } else {
            ("覆盖距离", "m", 0.01, 1.0)
        };
        if matches!(field, Field::Selected(index) if index < 6) {
            label = ["X", "Y", "Z"][index % 3]; // 位置/缩放由同行组标签说明，字段保留各轴单位与完整精度。
        }
        if field == Field::Selected(6) { label = "Y"; }
        if field == Field::Selected(7) { label = ""; } // 表示分组在控件外显示源格距标签，值与类型列保持一致。
        if lod_cell { label = ""; unit = ""; } // 列标题已表达用途和单位，行内只显示可直接编辑的值。
        let maximum = if matches!(field, Field::CuboidCreation(_)) {
            if index < 3 { 10_000.0 } else if index < 6 { 255.0 } else { 1.0 }
        } else if matches!(field, Field::Policy(_)) {
            [16.0, 0.5, 4096.0, 0.25][index]
        } else if multiplier {
            32768.0
        } else {
            1_000_000.0
        };
        let spec = NumericInputSpec::f32(
            if matches!(field, Field::CuboidCreation(3..=5)) { 0
            } else if matches!(field, Field::Selected(index) if (3..6).contains(&index)) {
                6
            } else if matches!(field, Field::Policy(3)) {
                4
            } else if multiplier {
                0
            } else {
                3
            },
        )
        .with_min(NumericValue::F32(minimum))
        .with_max(NumericValue::F32(maximum))
        .with_step(NumericValue::F32(step))
        .with_live_update(false)
        .with_step_buttons(false)
        .with_replace_on_first_key(true);
        let options = NumericScrubFieldOptions::new(
            label,
            unit,
            NumericValue::F32(0.0),
            spec,
            NumericInputScrubConfig::linear(step * 10.0, step, minimum, maximum, 1.0),
        )
        .with_style(NumericScrubFieldStyle {
            width: 164.0,
            height: if lod_cell { 23.0 } else { 24.0 },
            border_radius: 2.0,
            column_gap: if matches!(field, Field::Rotation(_) | Field::Selected(0..=5)) { 1.0 } else { 3.0 },
            label_font_size: ui_metrics::FONT_HINT,
            value_font_size: ui_metrics::FONT_CONTROL,
            unit_font_size: 9.5,
        });
        commands.entity(entity).with_children(|parent| {
            let root = spawn_localized_numeric_scrub_field(parent, field, feathrise_editor_core::editor_font(&server, FONT_PATH), options);
            if let Some(row) = lod_row { parent.commands().entity(root).insert(LodFieldRow(row)); }
            parent
                .commands()
                .entity(root)
                .insert(NumericFieldBinding::default())
                .entry::<Node>()
                .and_modify(|mut node| {
                    node.width = percent(100);
                    node.min_width = px(0);
                    node.max_width = percent(100);
                }); // 三轴同排时数值根随槽位收缩；长精确值由原输入树裁切，不撑开相邻轴。
        });
    }
}

/// 折叠标题使用一条弱分隔线；展开只影响本组，不再切换整个属性页。
fn accordion(label: &str, action: Action) -> impl Scene {
    bsn! { InteractionButton ActionButton(action)
        Node { width: percent(100), height: px(29), min_width: px(0), flex_shrink: 0.0,
            align_items: AlignItems::Center, padding: UiRect::horizontal(px(3)), border: UiRect::top(px(1)) }
        BackgroundColor(colors::PANEL_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [ActionLabel(action) Text(label)
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY) TextLayout::no_wrap()]
    }
}

/// 两种用途各挂载一次完整编辑器；折叠只隐藏本组，输入根和滚动绑定始终各自稳定。
fn native_lod_editor(display: bool) -> impl Scene {
    bsn! { PanelSection(Section::NativeLods) NativeLodEditor(display) Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column,
        row_gap: px(3), flex_shrink: 0.0 }
        Children [
            NativeLodSummary(display) Text("") TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                TextColor(colors::TEXT_MUTED) Node { width: percent(100), min_width: px(0) }--
            Node { width: percent(100), min_width: px(0), column_gap: px(3), padding: UiRect::horizontal(px(5)), flex_shrink: 0.0 }
                Children [@lod_heading("LOD")-- @lod_heading("采样倍率")-- @lod_heading("范围 m")]--
            @lod_list(display)--
            Node { width: percent(100), column_gap: px(3), flex_shrink: 0.0 } Children [
                @button("添加层", Action::AddLod(display))-- @button("删除所选层", Action::RemoveLod(display))]
        ]
    }
}

/// 所选对象固定为名称、变换、表示、显示LOD、碰撞LOD与来源；默认可同时看到常用参数。
pub(crate) fn properties_panel() -> impl Scene {
    bsn! { ~{crate::ui::scene_inspector::InspectorSection::Terrain}
        Node { display: Display::None, width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0 }
        Children [
            Summary Text("") TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                TextColor(colors::TEXT_PRIMARY) TextLayout::linebreak(LineBreak::AnyCharacter)
                Node { width: percent(100), min_width: px(0), padding: UiRect::vertical(px(5)) }--
            PanelSection(Section::Selection) Node { display: Display::None, width: percent(100), min_width: px(0),
                flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0 } Children [
                Node { width: percent(100), min_width: px(0), height: px(26),
                    align_items: AlignItems::Center, column_gap: px(4), flex_shrink: 0.0 } Children [
                    Text("名称") @feathrise_editor_ui::localization::label_scene("名称") TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                        TextColor(colors::TEXT_MUTED) TextLayout::no_wrap() Node { width: px(32), flex_shrink: 0.0 }--
                    NameInput crate::ui::text_input::EditorTextInput
                        ~{crate::ui::text_input::single_line_input("", 128)}
                        bevy::text::EditableTextFilter::new(|character| !character.is_control())
                        bevy::text::TextCursorStyle::default() TextLayout::no_wrap()
                        Node { height: px(26), flex_grow: 1.0, flex_basis: px(0), min_width: px(0),
                            padding: UiRect::horizontal(px(4)), border: px(1), overflow: Overflow::clip() }
                        BackgroundColor(colors::INPUT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
                        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                        TextColor(colors::TEXT_VALUE)--
                    Node { width: px(36), height: px(26), flex_shrink: 0.0 } Children [@button("确认", Action::Rename)]
                ]--
                @accordion("▼ 变换", Action::BasicProperties)--
                PanelSection(Section::Basic) Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column,
                    row_gap: px(3), flex_shrink: 0.0 } Children [
                    @axis_row("位置", 0)--
                    @rotation_row()--
                    @axis_row("缩放", 3)
                ]--
                @accordion("▼ 几何", Action::TypeProperties)--
                PanelSection(Section::Type) Node { width: percent(100), min_width: px(0),
                    flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0 } Children [
                    Node { width: percent(100), min_width: px(0), height: px(26), column_gap: px(4), align_items: AlignItems::Center, flex_shrink: 0.0 } Children [
                        Text("类型") @feathrise_editor_ui::localization::label_scene("类型") TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                            TextColor(colors::TEXT_MUTED) Node { width: px(67), flex_shrink: 0.0 }--
                        RepresentationSummary Text("") TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                            TextColor(colors::TEXT_VALUE) Node { flex_grow: 1.0, min_width: px(0), height: px(26), padding: UiRect::axes(px(5), px(4)), border: px(1) }
                            BackgroundColor(colors::INPUT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
                    ]--
                    Node { width: percent(100), min_width: px(0), height: px(24), column_gap: px(4), align_items: AlignItems::Center, flex_shrink: 0.0 } Children [
                        Text("源格距") @feathrise_editor_ui::localization::label_scene("源格距") TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                            TextColor(colors::TEXT_MUTED) Node { width: px(67), flex_shrink: 0.0 }--
                        @compact_slot(Field::Selected(7))
                    ]--
                    @slot(Field::AuthorHeightDomain)-- @slot(Field::VoxelHeight)
                ]--
                @accordion("▼ 显示 LOD", Action::DisplayLods)--
                PanelSection(Section::DisplayLods) Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column,
                    row_gap: px(3), flex_shrink: 0.0 } Children [
                    PanelSection(Section::PolicyLods) Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0 } Children [
                        @button("策略：继承全局", Action::DisplayPolicy)--
                        PanelSection(Section::DisplayOverride) Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0 }
                            Children [@slot(Field::Policy(0))-- @slot(Field::Policy(1))]
                    ]--
                    @native_lod_editor(true)
                ]--
                @accordion("▼ 碰撞 LOD", Action::CollisionLods)--
                PanelSection(Section::CollisionLods) Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column,
                    row_gap: px(3), flex_shrink: 0.0 } Children [
                    PanelSection(Section::PolicyLods) Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0 } Children [
                        @button("策略：继承全局", Action::CollisionPolicy)--
                        PanelSection(Section::CollisionOverride) Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0 }
                            Children [@slot(Field::Policy(2))-- @slot(Field::Policy(3))]
                    ]--
                    @native_lod_editor(false)
                ]--
                @accordion("▶ 拼接", Action::StitchProperties)--
                PanelSection(Section::Stitch) Node { display: Display::None, width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column,
                    row_gap: px(3), flex_shrink: 0.0 } Children [
                    Node { width: percent(100), column_gap: px(3) } Children [
                        @button("预览完整边（2m内）", Action::StitchNearest)-- @button("确认拼接", Action::ConfirmStitch)]--
                    Node { width: percent(100), column_gap: px(3) } Children [
                        @button("取消预览", Action::CancelStitch)-- @button("解除拼接", Action::Unstitch)]--
                    @caption("支持矩形Clipmap及二次幂格距比例；移动旋转分离后暂停，回到原位恢复。")
                ]--
                @accordion("▶ 来源与高级", Action::SourceProperties)--
                PanelSection(Section::Source) Node { display: Display::None, width: percent(100), min_width: px(0),
                    flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0 } Children [
                    SourceSummary Text("") TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                        TextColor(colors::TEXT_MUTED) TextLayout::linebreak(LineBreak::AnyCharacter)
                        Node { width: percent(100), min_width: px(0) }--
                    LodSummary Text("") TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                        TextColor(colors::TEXT_MUTED) TextLayout::linebreak(LineBreak::AnyCharacter)
                        Node { width: percent(100), min_width: px(0) }--
                    @sidebar_help_button(EditorSidebarHelp::MeshLods)--
                    @sidebar_help_body(EditorSidebarHelp::MeshLods,
                        "在表格内直接点击输入或拖动修改倍率与范围。Clipmap末显示层覆盖网格边界且范围只读。倍率只接受1、2、4、8…，越大越粗。")--
                    @button("删除所选网格", Action::Remove)
                ]
            ]
        ]
    }
}
