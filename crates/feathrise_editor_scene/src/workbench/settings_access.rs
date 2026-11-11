//! 无工程设置的临时访问门：取消旧按钮手势并灰显，恢复时只撤销本门拥有的禁用。
use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use super::{WorkbenchSettingsAccess, components::SettingsAccessDisabled};
use crate::{
    components::{
        SceneEditorLayoutButton, SceneEditorPaneSplitter, SceneEditorSettingsButton,
        SceneEditorSettingsPanel, SceneEditorShortcutSettingsPanel,
    },
    resources::SceneEditorLayoutState,
    types::EditorSettingsAction,
};
use bevy::{prelude::*, ui::InteractionDisabled};
use feathrise_editor_ui::EditorButtonClick;

pub(super) fn prepare(
    mut commands: Commands,
    access: Res<WorkbenchSettingsAccess>,
    settings: Res<feathrise_editor_core::EditorSettings>,
    mut layout: ResMut<SceneEditorLayoutState>,
    mut previous: Local<Option<bool>>,
    parents: Query<&ChildOf>,
    panels: Query<
        (),
        Or<(
            With<SceneEditorSettingsPanel>,
            With<SceneEditorShortcutSettingsPanel>,
        )>,
    >,
    mut texts: Query<&mut TextColor>,
    mut buttons: Query<
        (
            Entity,
            Option<&SceneEditorSettingsButton>,
            Has<SceneEditorLayoutButton>,
            Has<SceneEditorPaneSplitter>,
            Option<&mut EditorButtonClick>,
            Option<&mut UiInteraction>,
            Has<InteractionDisabled>,
            Option<&SettingsAccessDisabled>,
            Option<&Children>,
            Option<&mut BackgroundColor>,
            Option<&mut BorderColor>,
        ),
        Or<(With<InteractionButton>, With<SceneEditorPaneSplitter>)>,
    >,
) {
    // 首次可编辑与交接恢复必须先还原最新偏好；否则默认/旧LayoutState会被remember反写并保存。
    if access.editable && *previous != Some(true) {
        layout.apply_preferences(settings.editor_layout);
        layout.owns_mouse_this_frame = false;
    }
    *previous = Some(access.editable);
    for (
        entity,
        action,
        layout_button,
        splitter,
        click,
        interaction,
        disabled,
        saved,
        children,
        background,
        border,
    ) in &mut buttons
    {
        if access.editable {
            let Some(saved) = saved else {
                continue;
            };
            // 不修改本来已禁用的控件；只有带本门标记的禁用才会被撤销。
            commands
                .entity(entity)
                .remove::<(SettingsAccessDisabled, InteractionDisabled)>();
            if let (Some(mut current), Some(value)) = (background, saved.background) {
                current.0 = value;
            }
            if let (Some(mut current), Some(value)) = (border, saved.border) {
                *current = value;
            }
            for (label, color) in &saved.labels {
                if let Ok(mut current) = texts.get_mut(*label) {
                    *current = *color;
                }
            }
            if let Some(mut click) = click {
                click.invalidate();
            }
            continue;
        }
        // 设置关闭、Esc及页签导航继续可用；其他设置动作与面板内专业控件均暂停。
        if action.is_some_and(|button| {
            matches!(
                button.0,
                EditorSettingsAction::Open
                    | EditorSettingsAction::OpenShortcuts
                    | EditorSettingsAction::Close
            )
        }) {
            continue;
        }
        if action.is_none()
            && !layout_button
            && !splitter
            && !parents
                .iter_ancestors(entity)
                .any(|parent| panels.contains(parent))
        {
            continue;
        }
        if let Some(mut click) = click {
            click.invalidate();
        }
        if let Some(mut interaction) = interaction {
            if *interaction != UiInteraction::None {
                *interaction = UiInteraction::None;
            }
        }
        if saved.is_none() && !disabled {
            let labels = children
                .into_iter()
                .flat_map(|children| children.iter())
                .filter_map(|child| texts.get(child).ok().map(|color| (child, *color)))
                .collect();
            commands.entity(entity).insert((
                InteractionDisabled,
                SettingsAccessDisabled {
                    background: background.as_deref().map(|value| value.0),
                    border: border.as_deref().copied(),
                    labels,
                },
            ));
        } else if saved.is_some() && !disabled {
            commands.entity(entity).insert(InteractionDisabled);
        }
    }
}

pub(super) fn present(
    access: Res<WorkbenchSettingsAccess>,
    mut buttons: Query<(
        &SettingsAccessDisabled,
        Option<&mut BackgroundColor>,
        Option<&mut BorderColor>,
    )>,
    mut texts: Query<&mut TextColor>,
) {
    if access.editable {
        return;
    }
    // 固定设置控件由原系统继续回显当前值；最后统一弱化，不改变业务语义色或主题色板。
    for (saved, background, border) in &mut buttons {
        if let Some(mut background) = background {
            background.0 = feathrise_editor_ui::colors::DIALOG_BG;
        }
        if let Some(mut border) = border {
            *border = BorderColor::all(feathrise_editor_ui::colors::DIALOG_BORDER.with_alpha(0.35));
        }
        for (label, _) in &saved.labels {
            if let Ok(mut color) = texts.get_mut(*label) {
                color.0 = feathrise_editor_ui::colors::TEXT_MUTED.with_alpha(0.45);
            }
        }
    }
}
