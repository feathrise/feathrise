//! 资产预览固定UI结构。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use bevy::prelude::*; // BSN Scene、Node和ImageNode。
use bevy::text::FontSourceTemplate; // 使用项目中文字体。
use bevy::ui::{FocusPolicy, RelativeCursorPosition}; // 画布接收完整鼠标命中，图像子节点允许事件到达父画布。
use crate::ui::theme as colors; // 沿用F12水墨铜木配色。

use super::types::*; // 预览与导航表现只按明确marker原位更新。
use crate::ui::scene::FONT_PATH; // 与资产列表共享同一字体。

/// 预览按钮复用紧凑铜边风格，不携带地图作者动作。
fn preview_button(label: &str, action: AssetPreviewAction, width: f32) -> impl Scene {
    bsn! {
        InteractionButton // 使用Bevy标准点击状态。
        AssetPreviewNavigationButton // 只让导航表现系统维护这三个按钮。
        ~{action} // BSN动态组件值显式走Template，不把动作误认成Scene。
        Node {
            width: px(width), // 固定短按钮不挤压画布宽度。
            height: px(26), // 与资产浏览器普通动作统一，画布导航仍独立绑定。
            flex_shrink: 0.0, // 宽度由工具条明确安排。
            align_items: AlignItems::Center, // 文字与按钮垂直居中。
            justify_content: JustifyContent::Center, // 短标签水平居中。
            border: UiRect::all(px(1)), // 沿用F12铜色细边。
            border_radius: BorderRadius::all(px(3)), // 与同栏其他按钮一致。
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL) // 默认暗墨底。
        BorderColor::all(colors::COPPER_BORDER_WEAK) // 低对比边框。
        Children [
            Text(label) @feathrise_editor_ui::localization::label_scene(label) // 使用中文重置说明或普通ASCII缩放符号。
            ~{FocusPolicy::Pass} // 枚举组件显式取值，文字不能挡住父按钮。
            TextFont {
                font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), // 项目统一中文字体。
                font_size: px(ui_metrics::FONT_CONTROL), // 小画布工具条保持可读且紧凑。
            }
            TextColor(colors::TEXT_PRIMARY) // 米白正文色。
        ]
    }
}

/// “完整场景”是固定首项，不占用primitive五槽行池。
fn scene_row() -> impl Scene {
    bsn! {
        InteractionButton // 使用真实Interaction边沿切回Scene0。
        AssetPreviewSceneRow // 表现系统按当前目标维持选中底色。
        ~{AssetPreviewAction::ShowScene0} // 只修改临时预览目标。
        Node {
            width: percent(100), // 完整填满300px右栏可用宽度。
            height: px(20), // 与资产列表紧凑行高一致。
            min_width: px(0), // 长文件状态不撑宽侧栏。
            flex_shrink: 0.0, // 当前真实行不会被压成零高。
            align_items: AlignItems::Center, // 标签垂直居中。
            padding: UiRect::horizontal(px(5)), // 保留小型点击留白。
            border: UiRect::all(px(1)), // 铜色细边表达可选择项。
            border_radius: BorderRadius::all(px(3)), // 与F12其他行按钮一致。
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL) // 默认暗墨底。
        BorderColor::all(colors::COPPER_BORDER_WEAK) // 非活动时弱铜边。
        Children [
            Text("完整场景 · Scene0") @feathrise_editor_ui::localization::label_scene("完整场景 · Scene0") // 明确默认项不是文件内全部Scene。
            ~{FocusPolicy::Pass} // 字形点击仍交给父按钮。
            TextFont {
                font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), // 项目统一中文字体。
                font_size: px(ui_metrics::FONT_CONTROL), // 与资产行正文一致。
            }
            TextColor(colors::TEXT_PRIMARY) // 米白正文。
            TextLayout::no_wrap() // 右栏内单行裁切，不增高行池。
        ]
    }
}

/// 一个固定primitive槽；翻页只改文字和真实下标绑定，不增删实体。
fn primitive_row(slot: usize) -> impl Scene {
    bsn! {
        InteractionButton // 每槽始终使用同一个Bevy实体。
        ~{AssetPreviewPrimitiveRow(slot)} // 当前页相对槽位。
        ~{AssetPreviewAction::ShowPrimitive(slot)} // 输入系统再映射完整候选下标。
        Node {
            display: Display::None, // 候选完整Ready且该槽有真实项时才进入布局。
            width: percent(100), // 五槽共用同一行宽。
            height: px(20), // 固定紧凑行高。
            min_width: px(0), // Blender长名称只能裁切。
            flex_shrink: 0.0, // 已绑定行保持可点击高度。
            align_items: AlignItems::Center, // 文字与铜边垂直居中。
            padding: UiRect::horizontal(px(5)), // 与Scene0首项一致。
            overflow: Overflow::clip_x(), // 不让长名称扩大侧栏。
            border: UiRect::all(px(1)), // 当前选中态强化此边框。
            border_radius: BorderRadius::all(px(3)), // 水墨铜木统一圆角。
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL) // 未选中暗墨底。
        BorderColor::all(colors::COPPER_BORDER_WEAK) // 未选中弱铜边。
        Children [
            ~{AssetPreviewPrimitiveRowText(slot)} // 不依赖Children顺序更新标签。
            Text("Mesh?/Primitive?") // 首帧占位永远保持隐藏。
            ~{FocusPolicy::Pass} // 鼠标命中归父按钮。
            TextFont {
                font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), // 支持Blender中文网格名。
                font_size: px(8.8), // 五槽文字在窄栏保持清晰。
            }
            TextColor(colors::TEXT_PRIMARY) // 普通候选米白色。
            TextLayout::no_wrap() // 固定单行池不因名称换行增高。
            Node {
                min_width: px(0), // 允许长文本被父行裁切。
                overflow: Overflow::clip_x(), // 不遮挡侧栏滚动区域。
            }
        ]
    }
}

/// 当前引用文字和复制按钮共用紧凑横行；具体marker由调用位置声明。
fn reference_copy_button(label: &str, action: AssetPreviewAction) -> impl Scene {
    bsn! {
        InteractionButton // 只有显式Pressed才访问系统剪贴板。
        ~{action} // 主引用与材质引用使用不同动作。
        Node {
            width: px(62), // 固定短按钮把主要宽度留给完整引用。
            height: px(26), // 引用动作统一普通按钮高度，五槽候选仍保持20px。
            flex_shrink: 0.0, // 引用再长也保留复制入口。
            align_items: AlignItems::Center, // 文字垂直居中。
            justify_content: JustifyContent::Center, // 动作标签水平居中。
            border: UiRect::all(px(1)), // 弱铜边按钮。
            border_radius: BorderRadius::all(px(3)), // 与子列表统一。
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL) // 暗墨底。
        BorderColor::all(colors::COPPER_BORDER_WEAK) // 默认弱铜边。
        Children [
            Text(label) @feathrise_editor_ui::localization::label_scene(label) // 分别显示复制引用或复制材质。
            ~{FocusPolicy::Pass} // 字形不截获父按钮。
            TextFont {
                font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), // 统一中文字体。
                font_size: px(8.6), // 紧凑动作字号。
            }
            TextColor(colors::TEXT_PRIMARY) // 米白动作文字。
        ]
    }
}

/// 子资产分页按钮不属于模型缩放工具，使用动作枚举直接维护悬停与边界态。
fn subasset_page_button(label: &str, action: AssetPreviewAction) -> impl Scene {
    bsn! {
        InteractionButton // 真实Interaction只在当前固定分页行内产生。
        ~{action} // Previous与Next由输入系统安全钳制页码。
        Node {
            width: px(62), // 两侧按钮为中间页码保留空间。
            height: px(26), // 分页动作与复制按钮一致，不改变primitive行池高度。
            flex_shrink: 0.0, // 页码变化不改变按钮宽度。
            align_items: AlignItems::Center, // 文字垂直居中。
            justify_content: JustifyContent::Center, // 动作标签水平居中。
            border: UiRect::all(px(1)), // 弱铜边。
            border_radius: BorderRadius::all(px(3)), // 统一圆角。
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL) // 暗墨底。
        BorderColor::all(colors::COPPER_BORDER_WEAK) // 普通边界色。
        Children [
            Text(label) @feathrise_editor_ui::localization::label_scene(label) // 上一组或下一组。
            ~{FocusPolicy::Pass} // 点击字形归父按钮。
            TextFont {
                font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), // 项目中文字体。
                font_size: px(8.6), // 紧凑分页字号。
            }
            TextColor(colors::TEXT_PRIMARY) // 米白动作色。
        ]
    }
}

/// 构造资产列表下方可填满剩余高度的实际内容预览区。
pub(crate) fn asset_preview_panel(scenes: &feathrise_editor_documents_model::DocumentScenes) -> impl Scene + use<> {
    bsn! {
        AssetPreviewPanel
        Node {
            display: Display::None, // 尚未选择文件时不占用资产页高度。
            width: percent(100),
            min_width: px(0),
            min_height: px(220), // 图片保持原最小高度；模型子列表会按真实行自然增加整体高度。
            flex_grow: 1.0, // 高窗口把列表下方空白真正交给预览，而非保留无用途背景。
            flex_shrink: 0.0,
            flex_direction: FlexDirection::Column,
            row_gap: px(3),
            padding: UiRect::all(px(4)),
            border: UiRect::top(px(1)), // 预览与所选资源以弱分隔线分层，不再外套一整圈铜框。
        }
        BackgroundColor(colors::PANEL_BG_SECTION)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [

                Node { width: percent(100), flex_shrink: 0.0, column_gap: px(4), align_items: AlignItems::Center,
                    padding: UiRect::right(px(22)) } // 给绝对定位?保留横向命中区，不增加正文行。
                Children [

                        AssetPreviewTitleText Text("资产预览") @feathrise_editor_ui::localization::label_scene("资产预览")
                        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                        TextColor(colors::TEXT_ACCENT)
                        TextLayout { linebreak: bevy::text::LineBreak::AnyCharacter }
                        Node { flex_grow: 1.0, min_width: px(0) }
                    --
                    @super::workspace::expand_button()--
                    @crate::ui::context_help::help_button("资源预览", "图片：滚轮缩放，左、中、右键拖动平移。模型：滚轮缩放，右键旋转，中键平移。单击文件在右侧查看，双击在中央标签打开；复制引用只复制当前完整身份。")
                ]
            --

                AssetPreviewMetadataText Text("")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                TextColor(colors::TEXT_VALUE)
                Node { display: Display::None, width: percent(100), min_width: px(0), flex_shrink: 0.0 }
            --

                AssetPreviewControls // 加载成功后才显示可操作工具条。
                Node {
                    display: Display::None, // 不给不支持的文件伪造可用按钮。
                    width: percent(100), // 在右栏内部完整排列。
                    height: px(26), // 固定普通工具行高，不随全屏高度被拉伸。
                    flex_shrink: 0.0, // 把剩余高度留给画布。
                    align_items: AlignItems::Center, // 倍率和三个按钮保持同一基线。
                    column_gap: px(3), // 延续现有工具间距。
                }
                Children [
                    @preview_button("-", AssetPreviewAction::ZoomOut, 28.0)-- // 无滚轮设备也能缩小。
                    @preview_button("重置视图", AssetPreviewAction::Reset, 70.0)-- // 一键回到完整取景。
                    @preview_button("+", AssetPreviewAction::ZoomIn, 28.0)-- // 无滚轮设备也能放大。

                        AssetPreviewZoomText // 导航变化时原位刷新倍率。
                        Text("100%") // 表示相对完整取景的倍率，不冒充源图一比一像素。
                        TextFont {
                            font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), // 与标题共用中文字体。
                            font_size: px(ui_metrics::FONT_CONTROL), // 与缩放按钮一致。
                        }
                        TextColor(colors::TEXT_VALUE) // 工具值使用统一浅绿色。
                        Node {
                            flex_grow: 1.0, // 用剩余横向空间展示倍率。
                            min_width: px(0), // 不因窗口缩小撑开右栏。
                            margin: UiRect::left(px(3)), // 与放大按钮留少量间距。
                        }
                    --
                    @super::rigging::scene::launch()
                ]
            --

                AssetPreviewSubAssetPanel // 只有GLB/GLTF显示的固定子资产区。
                Node {
                    display: Display::None, // 图片和数据文件不占额外高度。
                    width: percent(100), // 与画布同宽。
                    flex_shrink: 0.0, // 列表、分页和引用不得被压没。
                    flex_direction: FlexDirection::Column, // Scene0、五槽与引用纵向排列。
                    row_gap: px(2), // 保持F12紧凑密度。
                    padding: UiRect::all(px(3)), // 与外框留一层暗墨间距。
                    border: UiRect::all(px(1)), // 独立弱铜边分区。
                    border_radius: BorderRadius::all(px(3)), // 统一圆角。
                }
                BackgroundColor(colors::PANEL_BG_STRONG) // 强化暗墨底区分模型画布。
                BorderColor::all(colors::COPPER_BORDER_WEAK) // 低对比铜边。
                Children [

                        AssetPreviewSubAssetSummaryText // 只显示Ready后的Scene与primitive真实数量。
                        Text("") // 枚举进度另交底栏，初始不伪造属性数量。
                        TextFont {
                            font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), // 支持中文诊断。
                            font_size: px(8.6), // 次级摘要字号。
                        }
                        TextColor(colors::TEXT_MUTED) // 弱文字不抢画布焦点。
                        Node {
                            display: Display::None, // 完整目录采用后才显示真实属性。
                            width: percent(100), // 长诊断在右栏自然换行。
                            flex_shrink: 0.0, // 保持可读高度。
                        }
                    --

                        AssetPreviewSubAssetStatusText crate::ui::status_bar::PanelStatusText("模型目录") feathrise_editor_ui::feedback::GlobalPanelStatus
                        Text("") TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                        TextColor(colors::TEXT_MUTED) Node { display: Display::None }
                    --
                    @scene_row()-- // 固定默认Scene0入口。
                    @primitive_row(0)-- // 第一槽。
                    @primitive_row(1)-- // 第二槽。
                    @primitive_row(2)-- // 第三槽。
                    @primitive_row(3)-- // 第四槽。
                    @primitive_row(4)-- // 第五槽。

                        AssetPreviewSubAssetPagination // 多于五个primitive时显示固定分页行。
                        Node {
                            display: Display::None, // 单页不占高度。
                            width: percent(100), // 三项在右栏横向排列。
                            height: px(26), // 保留五槽候选20px，分页工具单独26px。
                            flex_shrink: 0.0, // 不挤压操作入口。
                            align_items: AlignItems::Center, // 页码与按钮同一基线。
                            column_gap: px(3), // 紧凑间距。
                        }
                        Children [
                            @subasset_page_button("上一组", AssetPreviewAction::PreviousPrimitivePage)-- // 首组点击保持零页。

                                AssetPreviewPageText // 当前页码原位更新。
                                Text("1/1") // 仅分页显示时进入布局。
                                TextFont {
                                    font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), // 数字仍使用项目字体。
                                    font_size: px(8.6), // 次级信息字号。
                                }
                                TextColor(colors::TEXT_VALUE) // 浅绿值色。
                                Node {
                                    flex_grow: 1.0, // 居中占据剩余宽度。
                                    min_width: px(0), // 极窄布局可收缩。
                                }
                                TextLayout::justify(Justify::Center) // 页码水平居中。
                            --
                            @subasset_page_button("下一组", AssetPreviewAction::NextPrimitivePage) // 末组点击保持末页。
                        ]
                    --

                        Node {
                            width: percent(100), // 主引用与复制按钮同行。
                            min_height: px(26), // 长路径允许自然增高但不低于复制按钮。
                            flex_shrink: 0.0, // 引用始终可核对。
                            align_items: AlignItems::Center, // 文字与按钮垂直居中。
                            column_gap: px(3), // 保持紧凑。
                        }
                        Children [

                                AssetPreviewPrimaryReferenceText // Scene0或当前primitive完整引用。
                                Text("") // 表现系统按当前文件和目标更新。
                                TextLayout { linebreak: bevy::text::LineBreak::AnyCharacter }
                                TextFont {
                                    font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), // 路径可能含中文。
                                    font_size: px(8.2), // 长引用在300px内尽量完整显示。
                                }
                                TextColor(colors::TEXT_VALUE) // 可复制值使用浅绿色。
                                Node {
                                    flex_grow: 1.0, // 占用按钮外全部宽度。
                                    min_width: px(0), // 允许路径换行。
                                }
                            --
                            @reference_copy_button("复制引用", AssetPreviewAction::CopyPrimaryReference) // 复制完整带#标签引用。
                        ]
                    --

                        AssetPreviewMaterialReferenceRow // Scene0时整行退出布局。
                        Node {
                            display: Display::None, // 只有单primitive拥有唯一原材质。
                            width: percent(100), // 与主引用同宽。
                            min_height: px(26), // 至少与复制按钮同高。
                            flex_shrink: 0.0, // 不隐藏材质核对信息。
                            align_items: AlignItems::Center, // 同行垂直居中。
                            column_gap: px(3), // 紧凑间距。
                        }
                        Children [

                                AssetPreviewMaterialReferenceText // MaterialN/std或DefaultMaterial/std。
                                Text("") // 由当前选择原位更新。
                                TextLayout { linebreak: bevy::text::LineBreak::AnyCharacter }
                                TextFont {
                                    font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), // 路径支持中文。
                                    font_size: px(8.2), // 与主引用一致。
                                }
                                TextColor(colors::TEXT_VALUE) // 可复制值色。
                                Node {
                                    flex_grow: 1.0, // 把剩余宽度用于完整引用。
                                    min_width: px(0), // 长路径自然换行。
                                }
                            --
                            @reference_copy_button("复制材质", AssetPreviewAction::CopyMaterialReference) // 精确复制/std引用。
                        ]

                ]
            --

                AssetPreviewCanvas
                RelativeCursorPosition::default() // 尊重祖先裁剪，读取当前画布内真实光标位置。
                UiInteraction::default() // 额外验证前方模态遮挡，不能仅按矩形抢输入。
                ~{FocusPolicy::Block} // 画布接管导航，不把点击传给底下节点。
                Node {
                    position_type: PositionType::Relative,
                    width: percent(100),
                    flex_grow: 1.0,
                    min_height: px(170),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    overflow: Overflow::clip(),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(2)),
                }
                BackgroundColor(colors::MODEL_PREVIEW_BG)
                BorderColor::all(colors::COPPER_BORDER_WEAK)
                Children [

                        AssetPreviewImage
                        ~{FocusPolicy::Pass} // 图片本身不阻挡父画布捕获。
                        UiTransform::default() // 缩放与平移只改变视觉矩阵，不撑大布局或滚动范围。
                        ImageNode::default()
                        Node {
                            display: Display::None,
                            max_width: percent(100), // Auto图像模式在双轴约束内保持源宽高比。
                            max_height: percent(100),
                            flex_shrink: 1.0,
                        }
                    --
                    @super::documents::panel(scenes)-- // 同一画布按内容互斥，文本文档不创建额外三维View。
                    @super::audio::scene::panel()-- // 同一音频播放器原位迁移到中央，图片/模型/正文互斥。
                    @super::documents::sidebar_launch(scenes)-- // 放大后侧栏仍可打开另一个文档。

                        AssetPreviewModelViewport
                        Visibility::Hidden // 加载期参与布局并预分配正确RTT尺寸，但绝不显示旧画面。
                        Node {
                            display: Display::None,
                            position_type: PositionType::Absolute,
                            left: px(0),
                            top: px(0),
                            width: percent(100),
                            height: percent(100),
                        }
                    --

                        AssetPreviewPlaceholder
                        crate::ui::status_bar::PanelStatusText("资源预览") feathrise_editor_ui::feedback::GlobalPanelStatus
                        ~{FocusPolicy::Pass} // 占位说明不成为额外鼠标所有者。
                        Text("")
                        TextLayout { linebreak: bevy::text::LineBreak::AnyCharacter }
                        TextFont {
                            font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
                            font_size: px(ui_metrics::FONT_HINT),
                        }
                        TextColor(colors::TEXT_HINT)
                        Node { display: Display::None }
                    --
                    @super::animation::scene::pose_toolbar()-- // 动画移动/旋转入口留在画布旁，原位迁移到中央后仍可直接使用。
                    @super::rigging::scene::dots()
                ]
            --

                @super::rigging::scene::panel()--
                @super::animation::scene::panel()
            --

                AssetPreviewNavigationHint // 图片与模型分别显示准确的手势说明。
                Text("") // 未加载或失败时保持隐藏。
                TextLayout { linebreak: bevy::text::LineBreak::AnyCharacter }
                TextFont {
                    font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), // 使用统一中文字体。
                    font_size: px(ui_metrics::FONT_HINT), // 次要操作说明不抢占预览画布。
                }
                TextColor(colors::TEXT_HINT) // 沿用F12提示色。
                Node { display: Display::None } // 兼容原手势同步；说明由标题旁?展示。
            --

                AssetPreviewPhaseText crate::ui::status_bar::PanelStatusText("预览阶段") feathrise_editor_ui::feedback::GlobalPanelStatus
                Text("") ~{FocusPolicy::Pass}
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                TextColor(colors::TEXT_MUTED) Node { display: Display::None }
            --

                AssetPreviewStatusText
                crate::ui::status_bar::PanelStatusText("资源预览") feathrise_editor_ui::feedback::GlobalPanelStatus
                Text("")
                TextLayout { linebreak: bevy::text::LineBreak::AnyCharacter }
                TextFont {
                    font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
                    font_size: px(ui_metrics::FONT_HINT),
                }
                TextColor(colors::TEXT_MUTED)
                Node { display: Display::None }

        ]
    }
}
