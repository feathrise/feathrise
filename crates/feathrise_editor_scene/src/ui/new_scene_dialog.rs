//! 场景资料模态仅编辑名称和备注；几何参数与LOD统一由所选网格属性负责。

use feathrise_editor_ui::UiInteraction;
use crate::components::{
    NewSceneConfirmText, NewSceneDialog, NewSceneDialogBodyScroll, NewSceneDialogHint, NewSceneDialogTitle,
    NewSceneStatusText, SceneEditorActionButton,
};
use crate::resources::{SceneCreationTask, SceneEditorState};
use crate::types::EditorAction;
use crate::ui::theme as colors;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;

mod components;
pub(crate) mod copy;
mod scene;
mod systems;

#[cfg(test)]
mod layout_tests;
#[cfg(test)]
mod workbench_tests;

pub(crate) fn install(app: &mut App) {
    app.add_systems(
        Update,
        systems::scroll_feedback.in_set(crate::SceneEditorSystemSet::Presentation),
    );
}

pub(super) fn new_scene_dialog() -> impl Scene {
    bsn! {
        NewSceneDialog
        UiInteraction::default()
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            left: px(0), top: px(0), width: percent(100), height: percent(100),
            flex_direction: FlexDirection::Column, align_items: AlignItems::Center,
            justify_content: JustifyContent::Center, padding: UiRect::all(px(8)),
        }
        GlobalZIndex(430)
        BackgroundColor(colors::SCENE_OVERLAY_BG)
        Children [@scene::panel()]
    }
}

/// 正文与独立错误反馈各自滚动；资料表单不再创建内层LOD列表。
pub(crate) fn scroll_new_scene_dialog_body(
    editor: Res<SceneEditorState>,
    mut was_open: Local<bool>,
    mut wheels: MessageReader<MouseWheel>,
    feedback: Query<&RelativeCursorPosition, With<components::DialogFeedbackScroll>>,
    mut bodies: Query<
        (&RelativeCursorPosition, &ComputedNode, &mut ScrollPosition),
        With<NewSceneDialogBodyScroll>,
    >,
) {
    let visible = editor.open && editor.new_scene_dialog_open;
    let opening = visible && !*was_open;
    *was_open = visible;
    let delta: f32 = wheels
        .read()
        .map(|wheel| {
            -wheel.y
                * if wheel.unit == MouseScrollUnit::Line {
                    24.0
                } else {
                    1.0
                }
        })
        .sum();
    if !visible {
        return;
    }
    let feedback_hovered = feedback.iter().any(RelativeCursorPosition::cursor_over);
    for (cursor, computed, mut scroll) in &mut bodies {
        let maximum = ((computed.content_size().y - computed.size().y)
            * computed.inverse_scale_factor())
        .max(0.0);
        if opening {
            scroll.y = 0.0;
        } else if !feedback_hovered && delta != 0.0 && cursor.cursor_over() {
            scroll.y = (scroll.y + delta).clamp(0.0, maximum);
        } else {
            scroll.y = scroll.y.clamp(0.0, maximum);
        }
    }
}

/// 名称、备注与确认动作是场景资料唯一作者入口，不读取地形类型或LOD快照。
pub(crate) fn sync_new_scene_dialog_status(
    editor: Res<SceneEditorState>,
    creation: Option<Res<SceneCreationTask>>,
    locale: Option<Res<feathrise_editor_ui::localization::EditorLocale>>,
    mut controls: Query<(
        &mut Node,
        Option<&mut SceneEditorActionButton>,
        Has<components::SceneNotesFields>,
        Has<components::CopySavedVersionAction>,
    ), Or<(
        With<components::SceneNotesFields>,
        With<components::CopySavedVersionAction>,
        With<components::SceneDialogPrimaryAction>,
    )>>,
    mut texts: Query<
        (
            &mut Text,
            Has<NewSceneDialogTitle>,
            Has<NewSceneDialogHint>,
            Has<NewSceneConfirmText>,
        ),
        Or<(
            With<NewSceneDialogTitle>,
            With<NewSceneDialogHint>,
            With<NewSceneConfirmText>,
            With<NewSceneStatusText>,
        )>,
    >,
) {
    if !editor.is_changed()
        && !(editor.new_scene_copy_target.is_some() && creation.as_ref().is_some_and(|task| task.is_changed()))
        && !locale.as_ref().is_some_and(|locale| locale.is_changed())
    {
        return;
    }
    let editing = editor.new_scene_edit_target.is_some();
    let copying = editor.new_scene_copy_target.as_ref();
    let busy = copying.is_some() && creation.as_ref().is_some_and(|task| task.busy());
    for (mut node, action, notes, saved_version) in &mut controls {
        let shown = if notes {
            copying.is_none()
        } else if saved_version {
            copying.is_some_and(|copy| copy.dirty) && !busy
        } else {
            if let Some(mut action) = action {
                let wanted = if copying.is_some() { EditorAction::ConfirmSceneCopy } else { EditorAction::ConfirmNewScene };
                if action.0 != wanted { action.0 = wanted; }
            }
            !busy
        };
        let display = if shown { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
    }
    let locale = locale.as_deref().copied().unwrap_or_default();
    let source_hint = copying.map(|copy| format!("源场景“{}”", copy.display_name));
    for (mut text, title, hint, confirm) in &mut texts {
        let value = if title {
            if copying.is_some() {
                "复制为新场景"
            } else if editing {
                "编辑场景资料"
            } else {
                "新建场景"
            }
        } else if hint {
            if let Some(source) = source_hint.as_deref() {
                source
            } else if editing {
                "名称和备注在此保存；从左侧场景树选中网格后，在右侧属性编辑尺寸、表示参数和显示/碰撞LOD。"
            } else {
                "Tab切换名称和备注；创建后在“网格”页添加网格并设置尺寸与LOD。"
            }
        } else if confirm {
            if let Some(copy) = copying {
                if copy.dirty { "保存并复制" } else { "复制" }
            } else if editing {
                "保存资料"
            } else {
                "创建空白场景"
            }
        } else {
            &editor.status
        };
        let value = locale.editor_notice(value);
        if text.as_str() != value.as_ref() {
            text.0 = value.into_owned();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::{EditorStaticTextInputState, SceneBrowserState};
    use crate::ui::vegetation_preset_editor::VegetationPresetEditorState;
    use bevy::input_focus::InputFocus;
    use feathrise_editor_ui::{ActiveNumericInput, ActiveNumericInputScrub, NumericInputSetValue};

    pub(super) fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin {
                file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets").into(),
                watch_for_changes_override: Some(false),
                ..default()
            },
            bevy::scene::ScenePlugin,
        ))
        .init_asset::<Font>()
        .insert_resource(SceneEditorState {
            open: true,
            new_scene_dialog_open: true,
            ..default()
        })
        .init_resource::<SceneBrowserState>()
        .init_resource::<VegetationPresetEditorState>()
        .init_resource::<ActiveNumericInput>()
        .init_resource::<ActiveNumericInputScrub>()
        .init_resource::<InputFocus>()
        .init_resource::<EditorStaticTextInputState>()
        .add_message::<NumericInputSetValue>()
        .add_message::<MouseWheel>()
        .add_systems(Startup, new_scene_dialog.spawn())
        .add_systems(Update, sync_new_scene_dialog_status);
        install(&mut app);
        crate::ui::text_input::install(&mut app);
        app
    }
}
