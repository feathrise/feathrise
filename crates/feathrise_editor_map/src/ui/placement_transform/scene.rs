//! 完整摆放实例的右栏属性面板与七个公共专业数值框槽。

use feathrise_editor_ui::metrics as ui_metrics;
use crate::ui::theme as colors;
use bevy::prelude::*; // BSN静态场景、Commands和字体资产。
use bevy::text::FontSourceTemplate; // 静态标题与按钮沿用项目中文字体。
use feathrise_editor_ui::{
    NumericScrubFieldOptions, NumericScrubFieldStyle, NumericValue, spawn_numeric_scrub_field,
}; // 一次复用字符输入、左右拖动、进度填充和统一水墨控件视觉。

use super::{
    MapEditorPlacementTransformActionButton, MapEditorPlacementTransformPanel, MapEditorPlacementTransformLocate,
    MapEditorPlacementTransformSummary, PlacementTransformAction, PlacementTransformField,
    PlacementTransformInputSlot, PlacementPropertiesSection, MapEditorCuboidGeometryRow,
}; // 当前功能域静态marker、字段和动作。
use crate::types::EditorAction;
use crate::ui::scene::FONT_PATH; // 紧凑分组与原定位动作共享编辑器控件。

/// 使用原全局导航动作并保留专属marker，禁用门只影响此属性入口，不改变视口工具条。
fn locate_button() -> impl Scene {
    bsn! {
        Button
        MapEditorPlacementTransformLocate
        crate::components::MapEditorActionButton(EditorAction::FocusSelected)
        Node {
            height: px(26), flex_grow: 1.0, flex_basis: px(0), min_width: px(0), flex_shrink: 0.0,
            border: px(1), border_radius: BorderRadius::all(px(3)),
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(px(4)),
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [(
            Text("定位") feathrise_editor_ui::localization::label_scene("定位")
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
        )]
    }
}

const NUMERIC_CONTROL_WIDTH: f32 = 164.0; // 公共builder的初始像素基准；创建帧将根改为整栏百分比宽度。
const NUMERIC_FIELD_HEIGHT: f32 = 24.0; // 数值行沿用原命中高度，不随26px动作按钮改变输入树。
const NUMERIC_FIELD_FONT_SIZE: f32 = 9.5; // 紧凑作者栏仍保持数值清晰可读。

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
fn axis_row(label: &str, fields: [PlacementTransformField; 3]) -> impl Scene {
    let cells = fields.into_iter().map(|field| bsn! {
        template_value(PlacementTransformInputSlot(field))
        Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0), height: px(NUMERIC_FIELD_HEIGHT), flex_shrink: 0.0 }
    }).collect::<Vec<_>>();
    bsn! { Node { width: percent(100), min_width: px(0), column_gap: px(3), align_items: AlignItems::Center, flex_shrink: 0.0 }
        Children [
            (template_value(Text(label.to_owned())) TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                TextColor(colors::TEXT_MUTED) Node { width: px(22), flex_shrink: 0.0 }),
            {cells},
        ] }
}

/// 构造一个紧凑完整实例动作按钮。
fn action_button(
    label: &str,                      // 用户可见短动作名。
    action: PlacementTransformAction, // 独立结构化动作。
    danger: bool,                     // 删除使用危险色，其余保持开发工具铜边色。
) -> impl Scene {
    let background = if danger {
        colors::DANGER_BG // 删除常态使用危险底色。
    } else {
        colors::BUTTON_DEV_SMALL_NORMAL // 普通动作使用暗墨开发工具底色。
    };
    let border = if danger {
        colors::DANGER_BORDER // 删除使用红铜危险边。
    } else {
        colors::COPPER_BORDER_WEAK // 普通动作使用弱铜边。
    };
    let text_color = if danger {
        colors::DANGER_TEXT // 删除文字保持高可读危险色。
    } else {
        colors::TEXT_PRIMARY // 普通动作使用主要文字色。
    };
    bsn! {
        Button
        MapEditorPlacementTransformActionButton(action)
        Node {
            height: px(26), // 与其他作者动作按钮同高。
            flex_shrink: 0.0,
            flex_grow: 1.0, // 同一操作行内严格等宽，单列动作也会填满整行。
            flex_basis: px(0), // 中文长度不参与列宽。
            min_width: px(0), // 低分辨率允许安全收缩。
            border: px(1), // 铜边或危险边界表达可交互性。
            border_radius: BorderRadius::all(px(3)), // 紧凑作者工具小圆角。
            align_items: AlignItems::Center, // 文字垂直居中。
            justify_content: JustifyContent::Center, // 文字水平居中。
            padding: UiRect::horizontal(px(4)), // 保留短标签呼吸空间。
        }
        BackgroundColor(background)
        BorderColor::all(border)
        Children [(
            Text(label) feathrise_editor_ui::localization::label_scene(label)
            TextFont {
                font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), // 统一中文字体避免按钮乱码。
                font_size: px(ui_metrics::FONT_CONTROL), // 与同页网格属性按钮一致。
            }
            TextColor(text_color)
        )]
    }
}

/// 右栏场景树下方的完整实例变换区；没有稳定根选择时完全退出布局。
pub(in crate::ui) fn placement_transform_panel() -> impl Scene {
    bsn! {
        MapEditorPlacementTransformPanel
        Node {
            display: Display::None, // 无完整实例选择时不占右栏高度。
            width: percent(100), // 填满场景页可用宽度。
            min_width: px(0),
            flex_shrink: 0.0, // 内容通过父场景页滚动访问。
            flex_direction: FlexDirection::Column, // 摘要、字段与动作纵向排列。
            row_gap: px(3), // 与同页其他分组保持紧凑间距。
            padding: UiRect::vertical(px(3)), // 上下留出组边界。
        }
        Children [
            (
                template_value(PlacementPropertiesSection::Transform)
                Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0 }
                Children [
            (crate::ui::context_help::section_header("变换", "数值框支持左拖实际值、右拖本字段灵敏度、中键精确输入。普通资产使用世界变换，三维地形使用关联Clipmap的源局部变换；位置使用米，缩放为无量纲倍率。")),
            (
                Node {
                    width: percent(100),
                    min_width: px(0),
                    column_gap: px(3),
                    flex_shrink: 0.0,
                }
                Children [
                    (locate_button()), // 只登记原相机聚焦请求，不与地面吸附混淆。
                    (action_button("复制", PlacementTransformAction::Duplicate, false)),
                    (action_button("删除", PlacementTransformAction::Delete, true)),
                ]
            ),
            (crate::ui::context_help::section_header("高度与落点", "吸附地面会等待中央视口单击，并按首次摆放规则命中真实地形。跟随地形会归零离地偏移；固定高度保留模式并采用命中Y。")),
            (
                MapEditorPlacementTransformSummary
                Text("请选择场景树中的资产实例") feathrise_editor_ui::localization::label_scene("请选择场景树中的资产实例")
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
            ),
            (
                Node {
                    width: percent(100), // 两种持久高度模式与一次性地面吸附在同一行直接选择。
                    flex_direction: FlexDirection::Row, // 严格三列而不自动换行。
                    column_gap: px(3), // 三个铜边按钮之间保留清晰间距。
                    flex_shrink: 0.0, // 固定按钮高度不能被右栏滚动布局压扁。
                }
                Children [
                    (action_button("跟随地形", PlacementTransformAction::FollowTerrain, false)), // 持续保持当前离地偏移。
                    (action_button("固定高度", PlacementTransformAction::FixedHeight, false)), // 持续保持当前世界Y。
                    (action_button("吸附地面", PlacementTransformAction::Relocate, false)), // 复用首次摆放的真实三角命中选择新落点。
                ]
            ),
            (axis_row("位置", [PlacementTransformField::TranslationX, PlacementTransformField::TranslationY, PlacementTransformField::TranslationZ])),
            (numeric_input_slot(PlacementTransformField::Yaw)),
            (action_button("建筑旋转 90°", PlacementTransformAction::RotateQuarterTurn, false)), // 建筑必须先后台重检占地、门口和五点地基。
            (axis_row("缩放", [PlacementTransformField::ScaleX, PlacementTransformField::ScaleY, PlacementTransformField::ScaleZ])),
                ]
            ),
            (
                template_value(PlacementPropertiesSection::Geometry)
                Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0 }
                Children [
                    (crate::ui::context_help::section_header("几何", "立方体尺寸按局部 X/Y/Z 轴使用米，最小 1 毫米，与三轴缩放独立。外部模型的三维地形与洞口使用其原作者源，按当前对象能力显示。")),
                    (MapEditorCuboidGeometryRow
                        Node { display: Display::None, width: percent(100), min_width: px(0), flex_shrink: 0.0 }
                        Children [(axis_row("尺寸", [PlacementTransformField::CuboidSizeX, PlacementTransformField::CuboidSizeY, PlacementTransformField::CuboidSizeZ]))]),
                    (
                        Node { width:percent(100),flex_direction:FlexDirection::Row,column_gap:px(3),flex_shrink:0.0 }
                        Children [
                            (action_button("三维地形",PlacementTransformAction::ToggleTerrainPatch,false)),
                            (action_button("洞口开关",PlacementTransformAction::ToggleTerrainCutout,false)),
                        ]
                    ),
                    (numeric_input_slot(PlacementTransformField::CutoutMinX)),
                    (numeric_input_slot(PlacementTransformField::CutoutMaxX)),
                    (numeric_input_slot(PlacementTransformField::CutoutMinZ)),
                    (numeric_input_slot(PlacementTransformField::CutoutMaxZ)),
                ]
            ),
            (
                template_value(PlacementPropertiesSection::Material)
                Node { display: Display::None, width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0 }
                Children [
                    (crate::ui::context_help::section_header("材质", "颜色使用 sRGB 的 0–255，粗糙度使用 0–1；属性随地图保存，单次编辑可统一撤销。")),
                    (axis_row("颜色", [PlacementTransformField::CuboidColorR, PlacementTransformField::CuboidColorG, PlacementTransformField::CuboidColorB])),
                    (numeric_input_slot(PlacementTransformField::CuboidRoughness)),
                ]
            ),
            (
                template_value(PlacementPropertiesSection::Collision)
                Node { display: Display::None, width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0 }
                Children [
                    (crate::ui::context_help::section_header("碰撞", "参数立方体使用盒碰撞；点边面编辑后的网格使用实际三角碰撞。按钮选中表示开启；隐藏或删除对象时暂停，恢复后按原开启状态生效。")),
                    (action_button("碰撞", PlacementTransformAction::ToggleCuboidCollision, false)),
                ]
            ),
        ]
    }
}

/// 在七个BSN槽位内挂载完整公共NumericInput子树。
pub(crate) fn setup_placement_transform_numeric_inputs(
    mut commands: Commands, // 新增槽位只创建一次固定子树。
    slots: Query<(Entity, &PlacementTransformInputSlot), Added<PlacementTransformInputSlot>>, // 每个字段只处理Added帧。
    asset_server: Res<AssetServer>, // 动态文字持有统一字体强句柄。
    settings: Res<feathrise_editor_core::EditorSettings>, // 初始ScrubConfig读取三类持久倍率。
) {
    for (slot, marker) in &slots {
        let field = marker.0; // 冻结当前槽位字段身份供整个子树复用。
        let axis_label = match field {
            PlacementTransformField::TranslationX | PlacementTransformField::ScaleX | PlacementTransformField::CuboidSizeX => Some("X"),
            PlacementTransformField::TranslationY | PlacementTransformField::ScaleY | PlacementTransformField::CuboidSizeY => Some("Y"),
            PlacementTransformField::TranslationZ | PlacementTransformField::ScaleZ | PlacementTransformField::CuboidSizeZ => Some("Z"),
            PlacementTransformField::CuboidColorR => Some("R"),
            PlacementTransformField::CuboidColorG => Some("G"),
            PlacementTransformField::CuboidColorB => Some("B"),
            _ => None,
        }; // 组标签已说明位置/缩放；短轴名给完整精度的数字保留更多宽度。
        let font: Handle<Font> = feathrise_editor_core::editor_font(&asset_server, FONT_PATH); // 与静态摘要使用同一字体资源。
        let style = NumericScrubFieldStyle {
            width: NUMERIC_CONTROL_WIDTH, // 建立公共控件壳，根宽度随后跟随检查器。
            height: NUMERIC_FIELD_HEIGHT, // 填满24像素槽位。
            border_radius: 2.0,           // 与其他专业字段一致的小圆角。
            column_gap: if axis_label.is_some() { 1.0 } else { 3.0 },
            label_font_size: NUMERIC_FIELD_FONT_SIZE, // 标签保持统一字号。
            value_font_size: NUMERIC_FIELD_FONT_SIZE, // 数值保持统一字号。
            unit_font_size: 9.0,          // 单位作为辅助信息略小。
        };
        let options = NumericScrubFieldOptions::new(
            axis_label.unwrap_or(field.label()), // 保留原字段的单位、精度和输入协议。
            field.unit(),                        // 米、度或倍率符号。
            NumericValue::F32(0.0),              // 尚无选择时由同步系统禁用并保持安全零值。
            field.spec(None),                    // 首帧壳使用安全范围，随后同步当前地图真实XZ边界。
            field.scrub_config(&settings, None), // 初始持久手感随后同样按当前地图同步。
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
                .entry::<Node>()
                .and_modify(|mut node| {
                    node.width = percent(100);
                    node.min_width = px(0);
                    node.max_width = percent(100);
                }); // 只在创建帧拓宽原控件；保留实体、规格、值、拖动与精确输入缓冲。
        });
    }
}
