//! 完整摆放实例的右栏属性面板与公共数值输入槽。

use feathrise_editor_ui::{InteractionButton, UiInteraction};
use feathrise_editor_ui::metrics as ui_metrics;
use crate::ui::theme as colors;
use bevy::prelude::*; // BSN静态场景、Commands和字体资产。
use bevy::text::FontSourceTemplate; // 静态标题与按钮沿用项目中文字体。
use feathrise_editor_ui::{
    NumericScrubFieldOptions, NumericScrubFieldStyle, NumericValue, spawn_numeric_scrub_field,
}; // 一次复用字符输入、左右拖动、进度填充和统一水墨控件视觉。

use super::{
    SceneEditorPlacementTransformActionButton, SceneEditorPlacementTransformPanel,
    SceneEditorPlacementTransformSummary, PlacementTransformAction, PlacementTransformField,
    PlacementTransformInputSlot, PlacementPropertiesSection, SceneEditorCuboidGeometryRow, SceneEditorAnimationRotationRow,
}; // 当前功能域静态marker、字段和动作。
use crate::ui::scene::FONT_PATH;

const NUMERIC_CONTROL_WIDTH: f32 = 164.0; // 公共builder的初始像素基准；创建帧将根改为整栏百分比宽度。
const NUMERIC_FIELD_HEIGHT: f32 = 24.0; // 数值行沿用原命中高度，不随26px动作按钮改变输入树。
const NUMERIC_FIELD_FONT_SIZE: f32 = 9.5; // 紧凑作者栏仍保持数值清晰可读。

pub(in crate::ui) fn property_column() -> Node {
    Node { width: percent(100), min_width: px(0), flex_shrink: 0.0,
        flex_direction: FlexDirection::Column, row_gap: px(3), ..default() }
}

pub(in crate::ui) fn property_panel_layout() -> Node {
    Node { display: Display::None, padding: UiRect::vertical(px(3)), ..property_column() }
}

/// 单个字段的空槽位；Presentation阶段只在Added帧挂载一次公共输入根。
fn numeric_input_slot(field: PlacementTransformField) -> impl Scene {
    bsn! {
        PlacementTransformInputSlot(field)
        Node {
            width: percent(100), // 每个字段独占一行，坐标不会被低分辨率挤压。
            min_width: px(0),
            height: px(NUMERIC_FIELD_HEIGHT), // 固定命中高度与同页网格水属性一致。
            flex_shrink: 0.0, // 右栏整体滚动，字段自身不能被压扁。
        }
    }
}

/// XYZ共用一行组标题，三个原数字根各占剩余空间的三分之一；不合并作者字段或精确输入事务。
pub(in crate::ui) fn axis_row(label: &str, fields: [impl Component + Clone + Unpin; 3]) -> impl Scene {
    let cells = fields.into_iter().map(|field| bsn! {
        ~{field}
        Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0), height: px(NUMERIC_FIELD_HEIGHT) }
    }).collect::<Vec<_>>();
    bsn! { Node { width: percent(100), min_width: px(0), column_gap: px(3), align_items: AlignItems::Center, flex_shrink: 0.0 }
        Children [
            // 侧栏变窄时三轴共享剩余空间，双字标签预留字宽和缩放取整余量。
            Text(label) TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                TextColor(colors::TEXT_MUTED) TextLayout::no_wrap()
                Node { min_width: px(ui_metrics::FONT_HINT * 2.0 + 4.0), flex_shrink: 0.0 }--
            {cells}
        ] }
}

fn placement_axis_row(label: &str, fields: [PlacementTransformField; 3]) -> impl Scene {
    axis_row(label, fields.map(PlacementTransformInputSlot))
}

/// 首次选中灯具后才创建这组输入；以后切换对象只改显隐，保留公共数值控件和输入焦点。
#[derive(Component)]
pub(crate) struct LightControlsInstalled;

pub(super) fn light_controls() -> impl Scene {
    use super::LightField::*;
    use PlacementTransformField::Light;
    bsn! { ~{PlacementPropertiesSection::Light} ~{property_panel_layout()}
        Children [
            @crate::ui::context_help::section_header("局部灯光", "颜色为线性 RGB，光强使用坎德拉。聚光灯朝本地 −Z 轴照射，内、外锥角均为半角。")--
            @placement_axis_row("颜色", [Light(Red), Light(Green), Light(Blue)])--
            @numeric_input_slot(Light(Intensity))--
            @numeric_input_slot(Light(Range))--
            @numeric_input_slot(Light(InnerAngle))--
            @numeric_input_slot(Light(OuterAngle))
        ] }
}

/// 整件与子网格共用按钮壳，业务标记和较窄侧栏的最小列宽由调用处提供。
pub(in crate::ui) fn property_button(
    label: &str,
    marker: impl Component + Clone + Unpin,
    minimum_width: f32,
) -> impl Scene {
    bsn! {
        InteractionButton
        ~{marker}
        Node {
            height: px(26), // 与其他作者动作按钮同高。
            flex_shrink: 0.0,
            flex_grow: 1.0, // 同一操作行内严格等宽，单列动作也会填满整行。
            flex_basis: px(0), // 中文长度不参与列宽。
            min_width: px(minimum_width),
            border: px(1), // 铜边表达可交互性。
            border_radius: BorderRadius::all(px(3)), // 紧凑作者工具小圆角。
            align_items: AlignItems::Center, // 文字垂直居中。
            justify_content: JustifyContent::Center, // 文字水平居中。
            padding: UiRect::horizontal(px(4)), // 保留短标签呼吸空间。
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            Text(label) @feathrise_editor_ui::localization::label_scene(label)
            TextFont {
                font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), // 统一中文字体避免按钮乱码。
                font_size: px(ui_metrics::FONT_CONTROL), // 与同页网格属性按钮一致。
            }
            TextColor(colors::TEXT_PRIMARY)
        ]
    }
}

fn action_button(label: &str, action: PlacementTransformAction) -> impl Scene {
    property_button(label, SceneEditorPlacementTransformActionButton(action), 0.0)
}

pub(in crate::ui) fn property_button_visual(interaction: UiInteraction, selected: bool) -> (BackgroundColor, BorderColor) {
    let background = match (interaction, selected) {
        (UiInteraction::Pressed, _) => colors::BUTTON_DEV_SMALL_PRESSED,
        (UiInteraction::Hovered, _) => colors::BUTTON_DEV_SMALL_HOVERED,
        (UiInteraction::None, true) => colors::SELECT_BG,
        (UiInteraction::None, false) => colors::BUTTON_DEV_SMALL_NORMAL,
    };
    let border = if selected || interaction != UiInteraction::None {
        colors::COPPER_BORDER
    } else {
        colors::COPPER_BORDER_WEAK
    };
    (BackgroundColor(background), BorderColor::all(border))
}

/// 右栏场景树下方的完整实例变换区；没有稳定根选择时完全退出布局。
pub(in crate::ui) fn placement_transform_panel() -> impl Scene {
    bsn! {
        SceneEditorPlacementTransformPanel
        ~{property_panel_layout()}
        Children [

                ~{PlacementPropertiesSection::Transform}
                ~{property_column()}
                Children [
                    @crate::ui::context_help::section_header("变换", "数值框支持左拖实际值、右拖本字段灵敏度、左键单击精确输入。普通资产使用世界变换，三维地形使用关联Clipmap的源局部变换；位置使用米，缩放为无量纲倍率。")--
                    @placement_axis_row("位置", [PlacementTransformField::TranslationX, PlacementTransformField::TranslationY, PlacementTransformField::TranslationZ])--
                    @numeric_input_slot(PlacementTransformField::Yaw)--
                    SceneEditorAnimationRotationRow
                        Node { display: Display::None, width: percent(100), min_width: px(0), flex_shrink: 0.0 }
                        Children [@placement_axis_row("旋转", [PlacementTransformField::RotationX, PlacementTransformField::RotationY, PlacementTransformField::RotationZ])]--
                    @action_button("建筑旋转 90°", PlacementTransformAction::RotateQuarterTurn)-- // 建筑必须先后台重检占地、门口和五点地基。
                    @placement_axis_row("缩放", [PlacementTransformField::ScaleX, PlacementTransformField::ScaleY, PlacementTransformField::ScaleZ])--
                    @super::apply_transform::apply_menu()
                ]--

            // 高度策略独立成组，位置和姿态字段在上方变换标题下连续排列。
            ~{property_column()} Children [
            @crate::ui::context_help::section_header("高度与落点", "吸附地面会等待中央视口单击，并按首次摆放规则命中真实地形。跟随地形会归零离地偏移；固定高度保留模式并采用命中Y。")--

                SceneEditorPlacementTransformSummary
                Text("请选择场景树中的资产实例") @feathrise_editor_ui::localization::label_scene("请选择场景树中的资产实例")
                TextFont {
                    font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), // 摘要支持中文资产路径。
                    font_size: px(ui_metrics::FONT_CONTROL), // 低于正式数值一层。
                }
                TextColor(colors::TEXT_VALUE)
                TextLayout { linebreak: bevy::text::LineBreak::AnyCharacter }
                Node {
                    width: percent(100), // 长路径按可用宽度自然换行。
                    min_width: px(0),
                    flex_shrink: 0.0,
                    overflow: Overflow::clip(), // 不撑宽固定右栏。
                    padding: UiRect::all(px(4)), // 与外框保持可读间距。
                }
            --

                Node {
                    width: percent(100), // 两种持久高度模式与一次性地面吸附在同一行直接选择。
                    flex_direction: FlexDirection::Row, // 严格三列而不自动换行。
                    column_gap: px(3), // 三个铜边按钮之间保留清晰间距。
                    flex_shrink: 0.0, // 固定按钮高度不能被右栏滚动布局压扁。
                }
                Children [
                    @action_button("跟随地形", PlacementTransformAction::FollowTerrain)-- // 持续保持当前离地偏移。
                    @action_button("固定高度", PlacementTransformAction::FixedHeight)-- // 持续保持当前世界Y。
                    @action_button("吸附地面", PlacementTransformAction::Relocate) // 复用首次摆放的真实三角命中选择新落点。
                ]
                ]
            --

                ~{PlacementPropertiesSection::Geometry}
                Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0 }
                Children [
                    @crate::ui::context_help::section_header("几何", "立方体尺寸按局部 X/Y/Z 轴使用米，最小 1 毫米，与三轴缩放独立。外部模型的三维地形与洞口使用其原作者源，按当前对象能力显示。")--
                    SceneEditorCuboidGeometryRow
                        Node { display: Display::None, width: percent(100), min_width: px(0), flex_shrink: 0.0 }
                        Children [@placement_axis_row("尺寸", [PlacementTransformField::CuboidSizeX, PlacementTransformField::CuboidSizeY, PlacementTransformField::CuboidSizeZ])]--

                        Node { width:percent(100),flex_direction:FlexDirection::Row,column_gap:px(3),flex_shrink:0.0 }
                        Children [
                            @action_button("三维地形",PlacementTransformAction::ToggleTerrainPatch)--
                            @action_button("洞口开关",PlacementTransformAction::ToggleTerrainCutout)
                        ]
                    --
                    @numeric_input_slot(PlacementTransformField::CutoutMinX)--
                    @numeric_input_slot(PlacementTransformField::CutoutMaxX)--
                    @numeric_input_slot(PlacementTransformField::CutoutMinZ)--
                    @numeric_input_slot(PlacementTransformField::CutoutMaxZ)
                ]
            --

                ~{PlacementPropertiesSection::Material}
                Node { display: Display::None, width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0 }
                Children [
                    @crate::ui::context_help::section_header("材质", "颜色使用 sRGB 的 0–255，粗糙度使用 0–1；属性随场景保存，单次编辑可统一撤销。")--
                    @placement_axis_row("颜色", [PlacementTransformField::CuboidColorR, PlacementTransformField::CuboidColorG, PlacementTransformField::CuboidColorB])--
                    @numeric_input_slot(PlacementTransformField::CuboidRoughness)
                ]
            --

                ~{PlacementPropertiesSection::Collision}
                Node { display: Display::None, width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0 }
                Children [
                    @crate::ui::context_help::section_header("碰撞", "参数立方体使用盒碰撞。编辑网格和普通 GLB 使用模型的三角网格。按钮选中表示开启。内置对象隐藏或删除时暂停碰撞，恢复后按原状态生效。普通 GLB 的碰撞设置在保存场景后生效。")--
                    @action_button("碰撞", PlacementTransformAction::ToggleCuboidCollision)
                ]
            --
                ~{PlacementPropertiesSection::DisplayLod}
                Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0 }
                Children [
                    @crate::ui::context_help::section_header("显示 LOD", "按屏幕误差自动选档。预览 LOD 显示实际三角线、档位和面数；关闭三角线，可在同一镜头用“对照原始”比较显示开销。全局开关控制简化。骨骼和形态目标保持原始。")--
                    Node { width: percent(100), column_gap: px(3), flex_shrink: 0.0 }
                    Children [
                        @action_button("继承全局", PlacementTransformAction::DisplayLodInherit)--
                        @action_button("保持原始", PlacementTransformAction::DisplayLodOriginal)--
                        @action_button("自定义", PlacementTransformAction::DisplayLodCustom)
                    ]--
                    @numeric_input_slot(PlacementTransformField::LodScreenError)--
                    @numeric_input_slot(PlacementTransformField::LodHysteresis)--
                    Node { width: percent(100), column_gap: px(3), flex_shrink: 0.0 }
                    Children [
                        @action_button("预览 LOD", PlacementTransformAction::ToggleLodPreview)--
                        @action_button("三角线", PlacementTransformAction::ToggleLodTriangles)--
                        @action_button("对照原始", PlacementTransformAction::CompareLodOriginal)
                    ]--
                    super::lod_preview::LodPreviewSummary Text("")
                        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                        TextColor(colors::TEXT_VALUE)
                        TextLayout { linebreak: bevy::text::LineBreak::AnyCharacter }
                        Node { display: Display::None, width: percent(100), min_width: px(0), flex_shrink: 0.0 }
                ]
            --
                ~{PlacementPropertiesSection::CollisionLod}
                Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0 }
                Children [
                    @crate::ui::context_help::section_header("碰撞 LOD", "碰撞精度独立于显示和相机，安全近区至少32米。保持原始只停止减面，不关闭碰撞；自定义也受全局碰撞LOD开关约束，默认关闭简化。盒碰撞、建筑专用碰撞等安全形状保持原实现。")--
                    Node { width: percent(100), column_gap: px(3), flex_shrink: 0.0 }
                    Children [
                        @action_button("继承全局", PlacementTransformAction::CollisionLodInherit)--
                        @action_button("保持原始", PlacementTransformAction::CollisionLodOriginal)--
                        @action_button("自定义", PlacementTransformAction::CollisionLodCustom)
                    ]--
                    @numeric_input_slot(PlacementTransformField::LodSafeNear)--
                    @numeric_input_slot(PlacementTransformField::LodCollisionError)
                ]

        ]
    }
}

pub(in crate::ui) fn numeric_field_style(axis: bool) -> NumericScrubFieldStyle {
    NumericScrubFieldStyle {
        width: NUMERIC_CONTROL_WIDTH, // 建立公共控件壳，根宽度随后跟随检查器。
        height: NUMERIC_FIELD_HEIGHT,
        border_radius: 2.0,
        column_gap: if axis { 1.0 } else { 3.0 },
        label_font_size: NUMERIC_FIELD_FONT_SIZE,
        value_font_size: NUMERIC_FIELD_FONT_SIZE,
        unit_font_size: 9.0,
    }
}

/// 在BSN槽位内挂载完整公共NumericInput子树。
pub(crate) fn setup_placement_transform_numeric_inputs(
    mut commands: Commands, // 新增槽位只创建一次固定子树。
    slots: Query<(Entity, &PlacementTransformInputSlot), Added<PlacementTransformInputSlot>>, // 每个字段只处理Added帧。
    asset_server: Res<AssetServer>, // 动态文字持有统一字体强句柄。
    settings: Res<feathrise_editor_core::EditorSettings>, // 初始ScrubConfig读取三类持久倍率。
) {
    for (slot, marker) in &slots {
        let field = marker.0; // 冻结当前槽位字段身份供整个子树复用。
        let axis_label = match field {
            PlacementTransformField::TranslationX | PlacementTransformField::RotationX | PlacementTransformField::ScaleX | PlacementTransformField::CuboidSizeX => Some("X"),
            PlacementTransformField::TranslationY | PlacementTransformField::RotationY | PlacementTransformField::ScaleY | PlacementTransformField::CuboidSizeY => Some("Y"),
            PlacementTransformField::TranslationZ | PlacementTransformField::RotationZ | PlacementTransformField::ScaleZ | PlacementTransformField::CuboidSizeZ => Some("Z"),
            PlacementTransformField::CuboidColorR | PlacementTransformField::Light(super::LightField::Red) => Some("R"),
            PlacementTransformField::CuboidColorG | PlacementTransformField::Light(super::LightField::Green) => Some("G"),
            PlacementTransformField::CuboidColorB | PlacementTransformField::Light(super::LightField::Blue) => Some("B"),
            _ => None,
        }; // 组标签已说明位置/缩放；短轴名给完整精度的数字保留更多宽度。
        let font: Handle<Font> = feathrise_editor_core::editor_font(&asset_server, FONT_PATH); // 与静态摘要使用同一字体资源。
        let style = numeric_field_style(axis_label.is_some());
        let options = NumericScrubFieldOptions::new(
            axis_label.unwrap_or(field.label()), // 保留原字段的单位、精度和输入协议。
            field.unit(),                        // 米、度或倍率符号。
            NumericValue::F32(0.0),              // 尚无选择时由同步系统禁用并保持安全零值。
            field.spec(None, false),                    // 首帧壳使用安全范围，随后同步当前场景和动画状态。
            field.scrub_config(&settings, None, false), // 初始持久手感随后同样按当前场景同步。
        )
        .with_style(style); // 只覆盖尺寸，交互树继续由公共builder拥有。
        commands.entity(slot).with_children(|slot| {
            let root = spawn_numeric_scrub_field(
                slot,         // 当前固定槽位的唯一子项。
                field,        // 字段枚举本身直接作为业务marker。
                font.clone(), // 标签、数值和单位共享字体强句柄。
                options,      // 生成进度、文字、精确输入和左右拖动结构。
            );
            slot.commands()
                .entity(root)
                .remove::<feathrise_editor_ui::NumericInputScrubSettingsKey>() // 三轴继续共用原平移/缩放速度槽。
                .entry::<Node>()
                .and_modify(|mut node| {
                    node.width = percent(100);
                    node.min_width = px(0);
                    node.max_width = percent(100);
                }); // 只在创建帧拓宽原控件；保留实体、规格、值、拖动与精确输入缓冲。
        });
    }
}
