//! 地图资料模态仅编辑名称和备注；几何参数与LOD统一由所选网格属性负责。

use feathrise_editor_ui::UiInteraction;
use crate::components::{
    NewMapConfirmText, NewMapDialog, NewMapDialogBodyScroll, NewMapDialogHint, NewMapDialogTitle,
    NewMapStatusText,
};
use crate::resources::MapEditorState;
use crate::ui::theme as colors;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;

mod components;
mod scene;
mod systems;

#[cfg(test)]
mod layout_tests;
#[cfg(test)]
mod workbench_tests;

pub(crate) fn install(app: &mut App) {
    app.add_systems(
        Update,
        systems::scroll_feedback.in_set(crate::MapEditorSystemSet::Presentation),
    );
}

pub(super) fn new_map_dialog() -> impl Scene {
    bsn! {
        NewMapDialog
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
pub(crate) fn scroll_new_map_dialog_body(
    editor: Res<MapEditorState>,
    mut was_open: Local<bool>,
    mut wheels: MessageReader<MouseWheel>,
    feedback: Query<&RelativeCursorPosition, With<components::DialogFeedbackScroll>>,
    mut bodies: Query<
        (&RelativeCursorPosition, &ComputedNode, &mut ScrollPosition),
        With<NewMapDialogBodyScroll>,
    >,
) {
    let visible = editor.open && editor.new_map_dialog_open;
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

/// 名称、备注与确认动作是地图资料唯一作者入口，不读取地形类型或LOD快照。
pub(crate) fn sync_new_map_dialog_status(
    editor: Res<MapEditorState>,
    mut texts: Query<
        (
            &mut Text,
            Has<NewMapDialogTitle>,
            Has<NewMapDialogHint>,
            Has<NewMapConfirmText>,
        ),
        Or<(
            With<NewMapDialogTitle>,
            With<NewMapDialogHint>,
            With<NewMapConfirmText>,
            With<NewMapStatusText>,
        )>,
    >,
) {
    if !editor.is_changed() {
        return;
    }
    let editing = editor.new_map_edit_target.is_some();
    for (mut text, title, hint, confirm) in &mut texts {
        let value = if title {
            if editing {
                "编辑地图资料"
            } else {
                "新建地图"
            }
        } else if hint {
            if editing {
                "名称和备注在此保存；从左侧场景树选中网格后，在右侧属性编辑尺寸、表示参数和显示/碰撞LOD。"
            } else {
                "Tab切换名称和备注；创建后在“网格”页添加网格并设置尺寸与LOD。"
            }
        } else if confirm {
            if editing {
                "保存资料"
            } else {
                "创建空白地图"
            }
        } else {
            &editor.status
        };
        if text.0 != value {
            text.0 = value.to_owned();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::{EditorStaticTextInputState, MapBrowserState};
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
        .insert_resource(MapEditorState {
            open: true,
            new_map_dialog_open: true,
            ..default()
        })
        .init_resource::<MapBrowserState>()
        .init_resource::<VegetationPresetEditorState>()
        .init_resource::<ActiveNumericInput>()
        .init_resource::<ActiveNumericInputScrub>()
        .init_resource::<InputFocus>()
        .init_resource::<EditorStaticTextInputState>()
        .add_message::<NumericInputSetValue>()
        .add_message::<MouseWheel>()
        .add_systems(Startup, new_map_dialog.spawn())
        .add_systems(Update, sync_new_map_dialog_status);
        install(&mut app);
        crate::ui::text_input::install(&mut app);
        app
    }
}
