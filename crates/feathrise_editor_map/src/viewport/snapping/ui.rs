//! 地图同排工具条的吸附偏好；复用输入所有权、主题和现有设置自动保存链。
use feathrise_editor_ui::EditorButtonClick;
use super::components::{SnapAction, SnapButton, SnapLabel};
use crate::{
    resources::MapEditorState,
    ui::{scene::FONT_PATH, theme as colors},
    viewport::placement_move::MoveInputContext,
};
use bevy::{prelude::*, text::FontSourceTemplate};
use feathrise_editor_core::EditorSettings;
use feathrise_editor_core::EditorSnapMode;

fn button(label: &str, action: SnapAction) -> impl Scene {
    bsn! {
        Button SnapButton(action)
        Node { height: px(26), padding: UiRect::horizontal(px(8)), flex_shrink: 0.0, align_items: AlignItems::Center, justify_content: JustifyContent::Center, border: UiRect::bottom(px(2)) }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [(
            SnapLabel(action) Text(label)
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(11) }
            TextColor(colors::TEXT_PRIMARY) TextLayout::no_wrap()
        )]
    }
}

pub(crate) fn toolbar() -> impl Scene {
    bsn! {
        Node { height: px(26), column_gap: px(2), align_items: AlignItems::Center, flex_shrink: 0.0 }
        Children [(button("网格", SnapAction::Grid)), (button("吸附", SnapAction::Enabled)), (button("网格点", SnapAction::Mode)), (button("1米", SnapAction::Step))]
    }
}

pub(crate) fn handle_buttons(
    mut editor: ResMut<MapEditorState>,
    mut context: ParamSet<(MoveInputContext, ResMut<EditorSettings>)>,
    buttons: Query<(&SnapButton, &EditorButtonClick), Changed<EditorButtonClick>>,
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
) {
    if !context.p0().map_commands_available(&editor) {
        return;
    }
    let Some(action) = buttons.iter().find_map(|(action, interaction)| {
        (interaction.is_clicked()).then_some(action.0)
    }) else {
        return;
    };
    let mut settings = context.p1();
    let mut snap = settings.editor_snapping.sanitized();
    match action {
        SnapAction::Grid => snap.show_grid = !snap.show_grid,
        SnapAction::Enabled => snap.enabled = !snap.enabled,
        SnapAction::Mode => {
            snap.mode = match snap.mode {
                EditorSnapMode::WorldGrid => EditorSnapMode::Relative,
                EditorSnapMode::Relative => EditorSnapMode::WorldGrid,
            }
        }
        SnapAction::Step => {
            const STEPS: [f32; 7] = [0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0];
            snap.translation_meters = STEPS
                .into_iter()
                .find(|step| *step > snap.translation_meters + 0.00001)
                .unwrap_or(STEPS[0]);
        }
    }
    settings.editor_snapping = snap;
    editor.status = format!(
        "{} · {:.3}米 · {}；Ctrl临时反转，精确步长在设置中调整",
        snap.mode.label(),
        snap.translation_meters,
        if snap.enabled {
            "吸附常开"
        } else {
            "吸附关闭"
        }
    );
    editor.suppress_world_pick_this_frame = true; // 工具条点击不能同帧触发摆放、拾取或笔刷。
    keyboard.clear();
}

pub(crate) fn sync_buttons(
    settings: Res<EditorSettings>,
    mut labels: Query<(&SnapLabel, &mut Text)>,
    mut buttons: Query<(
        &SnapButton,
        &Interaction,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
) {
    let snap = settings.editor_snapping.sanitized();
    for (label, mut text) in &mut labels {
        let wanted = match label.0 {
            SnapAction::Grid => "网格".into(),
            SnapAction::Enabled => if snap.enabled {
                "吸附开"
            } else {
                "吸附关"
            }
            .into(),
            SnapAction::Mode => snap.mode.label().into(),
            SnapAction::Step => format!("{}米", snap.translation_meters),
        };
        if text.as_str() != wanted {
            **text = wanted;
        }
    }
    for (action, interaction, mut background, mut border) in &mut buttons {
        let selected = match action.0 {
            SnapAction::Grid => snap.show_grid,
            SnapAction::Enabled => snap.enabled,
            _ => false,
        };
        let color = match (interaction, selected) {
            (Interaction::Pressed, _) => colors::SELECT_BG_PRESSED,
            (Interaction::Hovered, _) => colors::BUTTON_DEV_SMALL_HOVERED,
            (_, true) => colors::SELECT_BG,
            _ => colors::BUTTON_DEV_SMALL_NORMAL,
        };
        background.set_if_neq(BackgroundColor(color));
        border.set_if_neq(BorderColor::all(if selected {
            colors::COPPER_BORDER
        } else {
            colors::COPPER_BORDER_WEAK
        }));
    }
}
