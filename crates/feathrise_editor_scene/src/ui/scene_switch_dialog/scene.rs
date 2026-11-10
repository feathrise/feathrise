//! 未保存切换场景模态的静态结构、冻结目标说明与增量显隐，复用编辑器水墨铜木主题。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::metrics as ui_metrics;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::FontSourceTemplate;
use bevy::ui::{FocusPolicy, InteractionDisabled, RelativeCursorPosition};

use crate::authoring::GlobalEnvironmentAuthoringState;
use crate::components::{
    SceneEditorActionButton, SceneSwitchDialog, SceneSwitchDialogBody, SceneSwitchDialogText,
};
use crate::resources::SceneEditorState;
use crate::types::{EditorAction, SceneSwitchStage};
use crate::ui::scene::{FONT_PATH, editor_button, section_title};
use crate::ui::theme as colors;

pub(crate) fn scene_switch_dialog() -> impl Scene {
    bsn! {
        SceneSwitchDialog
        UiInteraction::default()
        FocusPolicy::default()
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            left: px(0), top: px(0),
            width: percent(100), height: percent(100),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
        }
        GlobalZIndex(2000) // 确认属于整个工作台，覆盖离屏视口和浮动工具栏。
        BackgroundColor(colors::OVERLAY_BG)
        Children [
            Node {
                width: percent(90), max_width: px(440), min_width: px(0), max_height: percent(90),
                flex_direction: FlexDirection::Column,
                row_gap: px(9), padding: UiRect::all(px(14)),
                border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(6)),
            }
            BackgroundColor(colors::PANEL_BG_STRONG)
            BorderColor::all(colors::COPPER_BORDER_WEAK)
            Children [
                @section_title("处理未保存修改")--

                    SceneSwitchDialogBody
                    RelativeCursorPosition::default()
                    ScrollPosition::default()
                    Node {
                        width: percent(100), min_width: px(0), min_height: px(0),
                        flex_direction: FlexDirection::Column,
                        max_height: px(190), overflow: Overflow::scroll_y(),
                    } // 长目标名或错误原因只滚动正文，不把确认和取消按钮推出窗口。
                    Children [
                        SceneSwitchDialogText
                        Text("当前场景有未保存修改") @feathrise_editor_ui::localization::label_scene("当前场景有未保存修改")
                        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                        TextColor(colors::TEXT_PRIMARY)
                        Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
                    ]
                --

                    Node {
                        width: percent(100), flex_wrap: FlexWrap::Wrap,
                        column_gap: px(6), row_gap: px(6),
                        justify_content: JustifyContent::FlexEnd,
                        flex_shrink: 0.0,
                    } // 窄窗口自动换行，不压缩长中文操作名称。
                    Children [
                        @editor_button("保存并切换", EditorAction::SaveAndSwitchScene, 114.0)--
                        @editor_button("放弃修改并切换", EditorAction::DiscardAndSwitchScene, 140.0)--
                        @editor_button("取消切换场景", EditorAction::CancelSceneSwitch, 84.0)
                    ]

            ]
        ]
    }
}

/// 隐藏当帧主动释放根和固定三按钮的交互；保存等待态禁用重复提交，仍允许取消切换场景。
pub(crate) fn sync_scene_switch_dialog(
    mut commands: Commands,
    editor: Res<SceneEditorState>,
    global: Res<GlobalEnvironmentAuthoringState>,
    mut roots: Query<
        (Entity, &mut Node, &mut UiInteraction, &mut FocusPolicy),
        With<SceneSwitchDialog>,
    >,
    children: Query<&Children>,
    mut interactions: Query<&mut UiInteraction, Without<SceneSwitchDialog>>,
    buttons: Query<(Entity, &SceneEditorActionButton, Has<InteractionDisabled>)>,
    mut texts: Query<&mut Text, With<SceneSwitchDialogText>>,
    mut bodies: Query<&mut ScrollPosition, With<SceneSwitchDialogBody>>,
    mut focus: Option<ResMut<InputFocus>>,
    documents: Option<Res<crate::ui::scene_documents::SceneDocuments>>,
    mut button_labels: Query<&mut Text, Without<SceneSwitchDialogText>>,
) {
    let request = editor
        .scene_switch_confirmation
        .as_ref()
        .filter(|_| editor.open);
    if request.is_some()
        && let Some(focus) = focus.as_mut()
        && focus.get().is_some()
    {
        focus.clear(); // 原数值/文本输入不能在下一帧官方键盘处理阶段继续编辑模态背后的字段。
    }
    if !(editor.is_changed() || global.is_changed()) {
        return;
    }
    if request.is_none() {
        for mut body in &mut bodies {
            body.y = 0.0;
        } // 再次打开从冻结目标开始阅读，不继承上次失败信息的滚动位置。
    }
    for (entity, mut node, mut interaction, mut focus) in &mut roots {
        node.display = if request.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        *focus = if request.is_some() {
            FocusPolicy::Block
        } else {
            FocusPolicy::Pass
        };
        if request.is_none() {
            *interaction = UiInteraction::None;
            for descendant in children.iter_descendants(entity) {
                if let Ok(mut interaction) = interactions.get_mut(descendant) {
                    *interaction = UiInteraction::None;
                }
            }
        }
    }
    let closing = documents
        .as_ref()
        .is_some_and(|docs| docs.closing.is_some());
    for (entity, button, disabled) in &buttons {
        let label = match (closing, button.0) {
            (true, EditorAction::SaveAndSwitchScene) => Some("保存并关闭"),
            (true, EditorAction::DiscardAndSwitchScene) => Some("放弃场景修改并关闭"),
            (true, EditorAction::CancelSceneSwitch) => Some("取消关闭"),
            (false, EditorAction::SaveAndSwitchScene) => Some("保存并切换"),
            (false, EditorAction::DiscardAndSwitchScene) => Some("放弃修改并切换"),
            (false, EditorAction::CancelSceneSwitch) => Some("取消切换场景"),
            _ => None,
        };
        if let Some(label) = label {
            for child in children.iter_descendants(entity) {
                if let Ok(mut text) = button_labels.get_mut(child)
                    && text.0 != label
                {
                    **text = label.to_owned();
                }
            }
        }
        if !matches!(
            button.0,
            EditorAction::SaveAndSwitchScene | EditorAction::DiscardAndSwitchScene
        ) {
            continue;
        }
        let waiting = request.is_some_and(|request| request.stage != SceneSwitchStage::Choosing);
        if waiting && !disabled {
            commands.entity(entity).insert(InteractionDisabled);
        } else if !waiting && disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        }
    }
    let Some(request) = request else {
        return;
    };
    let scope = if global.dirty {
        "当前场景及全局太阳/月光设置"
    } else {
        "当前场景"
    };
    let message = if closing {
        format!(
            "准备关闭场景“{}” [{}]。\n保存成功后关闭；放弃只丢弃这个场景的修改；取消则保留标签。其他场景草稿与共享全局环境继续保留。\n\n{}",
            request.target_display_name,
            request.target_scene_id,
            request.diagnostic.as_deref().unwrap_or(""),
        )
    } else {
        format!(
            "准备进入“{}” [{}]。\n{}的未保存修改需要先处理。保存成功后自动切换；放弃会丢弃这些修改；取消则继续编辑。\n\n{}",
            request.target_display_name,
            request.target_scene_id,
            scope,
            request.diagnostic.as_deref().unwrap_or(&editor.status),
        )
    };
    for mut text in &mut texts {
        if text.0 != message {
            text.0 = message.clone();
        }
    }
}
