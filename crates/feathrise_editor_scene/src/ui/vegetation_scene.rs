//! 植被侧栏分清全局库、场景图层与所选属性；原目录、六行池和作者事务保持唯一。

use feathrise_editor_ui::metrics as ui_metrics;
use bevy::prelude::*; // BSN Scene与上下文Display布局。
use bevy::text::FontSourceTemplate; // 摘要与状态使用项目中文字体。
use crate::ui::theme as colors; // 植被页沿用水墨铜木配色。

use crate::components::{
    SceneEditorVegetationLayerList, SceneEditorVegetationLayerListTitle, SceneEditorVegetationPresetList,
    SceneEditorVegetationPresetListTitle, SceneEditorVegetationStatusText,
}; // 连续预设列表、作者固定行池、短摘要和植被专属状态marker。
use crate::types::EditorAction; // 所有静态按钮继续使用结构化动作。

use super::scene::{FONT_PATH, flex_editor_button}; // 全部操作行显式等宽，不依赖自动换行。
use super::sidebars::{
    SceneEditorVegetationListActions, SceneEditorVegetationSelectionProperties,
}; // 空层与有效选择上下文及默认折叠帮助。
use super::vegetation_layers::vegetation_numeric_slots; // 五项连续参数复用公共专业数值框。
use super::vegetation_preset_editor::{vegetation_preset_editor_panel, vegetation_preset_feedback};
use super::vegetation_sidebar::{
    VegetationSidebarFeedbackKind, VegetationSidebarFeedbackText, VegetationSidebarRoot,
};

/// 分区只使用弱分隔线和标题，不给每个参数组叠加卡片背景。
fn section_heading(label: &str) -> impl Scene {
    bsn! {
        Text(label) @feathrise_editor_ui::localization::label_scene(label)
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
        TextColor(colors::TEXT_ACCENT)
        Node {
            width: percent(100), min_width: px(0), flex_shrink: 0.0,
            margin: UiRect::top(px(4)), padding: UiRect::top(px(8)), border: UiRect::top(px(1)),
        }
        BorderColor::all(colors::COPPER_BORDER_WEAK)
    }
}

fn feedback_text(kind: VegetationSidebarFeedbackKind, initial: &str) -> impl Scene {
    let source = match kind {
        VegetationSidebarFeedbackKind::CreationTarget => None,
        VegetationSidebarFeedbackKind::Availability => Some("植被工具"),
        VegetationSidebarFeedbackKind::Compile => Some("植被编译"),
        VegetationSidebarFeedbackKind::Preview => Some("植被预览"),
        VegetationSidebarFeedbackKind::Catalog => Some("植被目录"),
        VegetationSidebarFeedbackKind::Message => Some("植被最近操作"),
    };
    let node = bsn! {
        VegetationSidebarFeedbackText(kind)
        Text(initial)
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
        TextColor(colors::TEXT_MUTED)
        TextLayout { linebreak: LineBreak::WordOrCharacter }
        Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
    };
    // 原反馈身份与完整文字保留，底栏统一采集；当前新建目标仍属于可见上下文。
    (node, source.map(|source| bsn! { ~{crate::ui::status_bar::PanelStatusText(source)} }))
}

/// 构造严格等宽三列按钮行。
fn three_button_row(
    first_label: &str,           // 第一列标签。
    first_action: EditorAction,  // 第一列结构化动作。
    second_label: &str,          // 第二列标签。
    second_action: EditorAction, // 第二列结构化动作。
    third_label: &str,           // 第三列标签。
    third_action: EditorAction,  // 第三列结构化动作。
) -> impl Scene {
    bsn! {
        Node {
            width: percent(100),
            column_gap: px(3),
            flex_shrink: 0.0,
        }
        Children [
            @flex_editor_button(first_label, first_action)--
            @flex_editor_button(second_label, second_action)--
            @flex_editor_button(third_label, third_action)
        ]
    }
}

/// 构造植被作者页；预设按三列连续展示，零作者层仍可直接创建。
pub(super) fn vegetation_authoring_panel() -> impl Scene {
    bsn! {
        VegetationSidebarRoot
        Node {
            width: percent(100),
            min_width: px(0),
            flex_direction: FlexDirection::Column,
            row_gap: px(4), // 用统一紧凑行距区分控件，分区另用细线标识。
            flex_shrink: 0.0, // 内容交给外层滚动，不压缩按钮和提示。
        }
        Children [
            @crate::ui::context_help::help_button("植被预设库", "使用已有预设：在全局库选择。新建预设：先在左侧资产选择GLB，再点击新建。预设库与当前场景图层是不同的编辑对象。")--
            @vegetation_preset_editor_panel()--

                SceneEditorVegetationPresetListTitle
                Text("预设库 · 加载中") @feathrise_editor_ui::localization::label_scene("预设库 · 加载中") // 标题只表达当前目录状态。
                TextFont {
                    font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
                    font_size: px(ui_metrics::FONT_CONTROL), // 提升目录标题可读性。
                }
                TextColor(colors::TEXT_VALUE)
                TextLayout { linebreak: LineBreak::WordOrCharacter }
                Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
            --

                SceneEditorVegetationPresetList
                Node {
                    display: Display::None, // 目录未就绪或有界重绑期间不暴露错位按钮。
                    width: percent(100),
                    min_width: px(0),
                    flex_wrap: FlexWrap::Wrap, // 三列连续换行，不再建立页码或翻页动作。
                    column_gap: px(3),
                    row_gap: px(3),
                    flex_shrink: 0.0,
                }
            --
            // GLB与内置静态模型共用种植路径；实验状态只标注表面绘制，不扩大到普通模型操作。
            @crate::ui::context_help::section_header("模型表面绘制（实验）", "在场景树选择已摆放的静态GLB或内置模型，再用植被工具绘制草、树和灌木。植被随模型移动、旋转和缩放，使用场景图层保存；拓扑变化会保留样本并停用旧锚点，重新绑定工具尚未提供。")--
            @crate::ui::context_help::section_header("当前场景图层", "绘制：在全局库选预设 → 用此预设新建空白层 → 切到视口编辑后涂绘；新建不会自动填满，Shift+左键擦除。\n选层：选择模式左键选择整层；植被工具用Ctrl+左键吸取图层，重复点击或Alt+点击轮换重叠层。复制、删除仅影响当前场景作者层；全局预设删除仍检查所有场景引用。\n分组：组内可共存、共享容量或高优先级覆盖；跨组可取最大、相加封顶或非零替换。组容量与覆盖阈值仅在对应规则下可编辑。\n数值：左拖实时回显，松开形成一次撤销；中键精确输入，右拖只调本字段灵敏度。\n预览：作者源、最终结果与组内冲突使用原覆盖图；编译和密度发布完成不代表GPU与模型已经显示。半径和强度仍在视图同排工具条。")--
            @super::vegetation_paint::panel()--
            @feedback_text(VegetationSidebarFeedbackKind::CreationTarget, "新建层预设：尚未选择")--
            @flex_editor_button("用此预设新建空白层", EditorAction::VegetationAddLayer)-- // 显式场景事务，不创建或修改全局预设。

                SceneEditorVegetationLayerListTitle
                Text("作者图层 · 加载中") @feathrise_editor_ui::localization::label_scene("作者图层 · 加载中") // 与下面的新建图层操作统一称呼。
                Node {
                    width: percent(100), // 分区线与列表同宽。
                    min_width: px(0),
                    flex_shrink: 0.0, // 多行空态提示自然撑高。
                }
                TextLayout { linebreak: LineBreak::WordOrCharacter }
                TextFont {
                    font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
                    font_size: px(ui_metrics::FONT_CONTROL), // 与预设库标题建立一致层级。
                }
                TextColor(colors::TEXT_VALUE)
            --

                SceneEditorVegetationLayerList
                Node {
                    display: Display::None, // 零作者层时固定六行池完全退出布局。
                    width: percent(100),
                    height: px(0), // 一项起按当前页真实行数增长。
                    flex_direction: FlexDirection::Column,
                    row_gap: px(2),
                    padding: UiRect::all(px(0)),
                    flex_shrink: 0.0,
                }
            --

                SceneEditorVegetationListActions
                Node {
                    display: Display::None, // 有作者层后才显示分页直选与删除所选。
                    width: percent(100),
                    column_gap: px(3),
                    flex_shrink: 0.0,
                }
                Children [
                    @flex_editor_button("前页首项", EditorAction::VegetationListPreviousPage)--
                    @flex_editor_button("后页首项", EditorAction::VegetationListNextPage)--
                    @flex_editor_button("删除图层", EditorAction::VegetationDeleteSelected) // 与全局“删除所选预设”区分。
                ]
            --
            @vegetation_preset_feedback() // 保留唯一原诊断，底栏集中回显并提供完整详情。
        ]
    }
}

/// 所选属性只挂载在右侧；复用原控件与作者事务。
pub(crate) fn vegetation_properties_panel() -> impl Scene {
    bsn! { ~{crate::ui::scene_inspector::InspectorSection::Vegetation} Node { display: Display::None, width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(4), flex_shrink: 0.0 }
        Children [

                SceneEditorVegetationSelectionProperties
                Node {
                    display: Display::None, // 只有有效作者层选择时显示复制、规则和数值属性。
                    width: percent(100),
                    min_width: px(0),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(4), // 属性内部沿用紧凑统一行距。
                    flex_shrink: 0.0,
                }
                Children [
                    @section_heading("所选层属性")--

                        SceneEditorVegetationStatusText
                        Text("所选层 · 等待选择") @feathrise_editor_ui::localization::label_scene("所选层 · 等待选择") // 状态先于操作，明确按钮作用对象。
                        TextFont {
                            font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
                            font_size: px(ui_metrics::FONT_CONTROL),
                        }
                        TextColor(colors::TEXT_VALUE)
                        TextLayout { linebreak: LineBreak::WordOrCharacter }
                        Node {
                            width: percent(100),
                            min_width: px(0),
                            flex_shrink: 0.0, // 长名称换行后不得被父容器压缩。
                        }
                    --
                    @three_button_row("复制图层", EditorAction::VegetationDuplicateLayer, "上移图层", EditorAction::VegetationMoveUp, "下移图层", EditorAction::VegetationMoveDown)-- // 结构操作保持同一行。
                    @three_button_row("显示/隐藏", EditorAction::VegetationToggleEnabled, "锁定/解锁", EditorAction::VegetationToggleLocked, "切换预览", EditorAction::VegetationPreviewNext)-- // 常用显示操作同组，当前预览见上方状态。
                    @three_button_row("新建分组", EditorAction::VegetationNewGroup, "移入下组", EditorAction::VegetationNextGroup, "组内规则", EditorAction::VegetationCycleGroupMode)-- // 将分组操作集中，减少上下寻找。
                    @flex_editor_button("切换跨组合并规则", EditorAction::VegetationCycleMergeMode)-- // 明确这是循环切换，不是打开规则编辑器。
                    @vegetation_numeric_slots() // 参数紧随规则，避免状态打断表单。
                ]
            --
            @feedback_text(VegetationSidebarFeedbackKind::Availability, "等待当前场景作者数据")--
            @feedback_text(VegetationSidebarFeedbackKind::Compile, "编译：等待作者数据")--
            @feedback_text(VegetationSidebarFeedbackKind::Preview, "预览：等待完整密度快照")--
            @feedback_text(VegetationSidebarFeedbackKind::Catalog, "")--
            @feedback_text(VegetationSidebarFeedbackKind::Message, "")
        ]
    }
}
