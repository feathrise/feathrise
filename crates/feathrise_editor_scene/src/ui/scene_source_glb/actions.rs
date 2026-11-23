//! 源网格与文件夹的名称输入、源网格应用变换。实际写入经原源 GLB 后台事务。

use super::{
    SourceGlbRequest,
    pose::{AdoptNodeSnapshot, Inspector, is_mesh},
};
use crate::{
    SceneEditorSystemSet,
    authoring::EditorAuthoringSession,
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState},
    ui::{
        gltf_browser::EditorGltfBrowserState,
        placement_transform::{
            apply_transform::{ApplyPart, transform_apply_menu},
            property_button_visual,
        },
        scene_context_menu::SceneObjectTarget,
        scene_objects, text_input,
    },
};
use bevy::{
    input_focus::{InputFocus, tab_navigation::TabIndex},
    prelude::*,
    text::EditableText,
    ui::InteractionDisabled,
};
use feathrise_editor_ui::{ActiveNumericInput, EditorButtonClick, UiInteraction};

#[derive(Component, Clone, Copy, Default)]
struct NameInput;

#[derive(Component, Clone, Copy)]
enum Action {
    Rename,
    Apply(Option<ApplyPart>),
}

#[derive(Component, Clone, Copy, Default)]
struct ApplyItems;

#[derive(Resource, Default)]
struct Controls {
    bound: Option<(SceneObjectTarget, String)>,
    open: bool,
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<Controls>()
        .add_systems(Update, input.after(crate::ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard)
            .in_set(SceneEditorSystemSet::Input))
        .add_systems(Update, sync.after(AdoptNodeSnapshot).in_set(SceneEditorSystemSet::Presentation));
}

pub(super) fn name_row() -> impl Scene {
    scene_objects::name_row(NameInput, Action::Rename)
}

pub(super) fn apply_menu() -> impl Scene {
    transform_apply_menu(Action::Apply, ApplyItems)
}

#[cfg(feature = "validation")]
pub(crate) fn source_name_input_for_validation(world: &mut World) -> Option<Entity> {
    world
        .query_filtered::<Entity, (With<NameInput>, Without<InteractionDisabled>)>()
        .single(world)
        .ok()
}

fn input(
    mut editor: ResMut<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    browser: Res<EditorGltfBrowserState>,
    inspector: Res<Inspector>,
    guard: Res<SceneEditorNumericScrubMouseGuard>,
    keys: Res<ButtonInput<KeyCode>>,
    mut focus: ResMut<InputFocus>,
    mut controls: ResMut<Controls>,
    names: Query<(Entity, &EditableText), (With<NameInput>, Without<InteractionDisabled>)>,
    buttons: Query<
        (&Action, &EditorButtonClick),
        (Changed<EditorButtonClick>, Without<InteractionDisabled>),
    >,
    mut requests: MessageWriter<SourceGlbRequest>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        controls.open = false;
    }
    if guard.blocked()
        || editor.new_scene_dialog_open
        || editor.scene_switch_confirmation_open()
        || session.source_glb_busy
    {
        return;
    }
    let Some((target, hash)) = inspector.identity() else {
        return;
    };
    let Some((bound, bound_hash)) = &controls.bound else {
        return;
    };
    if target != bound
        || hash != bound_hash
        || inspector.editable_target(&browser, &editor, &session) != Some(target)
    {
        return;
    }
    let Some((action, _)) = buttons.iter().find(|(_, click)| click.is_clicked()) else {
        return;
    };
    match *action {
        Action::Rename => {
            let Ok((entity, text)) = names.single() else {
                return;
            };
            if text.is_composing() {
                editor.status = "请先确认名称的输入法候选".into();
                return;
            }
            let value = text.value().to_string();
            let name = value.trim();
            if name.is_empty() {
                editor.status = "名称不能为空".into();
                return;
            }
            if let Err(error) = target.validate_name(name) {
                editor.status = error.into();
                return;
            }
            if focus.get() == Some(entity) {
                focus.clear();
            }
            if name != target.name {
                requests.write(SourceGlbRequest::Rename {
                    target: target.clone(),
                    expected_hash: hash.into(),
                    name: name.into(),
                });
            }
        }
        Action::Apply(None) if is_mesh(target) => controls.open = !controls.open,
        Action::Apply(Some(part)) if controls.open => {
            let Some((_, mut pose, _)) = inspector.selected_pose(&browser, &editor, &session)
            else {
                return;
            };
            controls.open = false;
            // 显示档只改变回显。应用始终归零源节点的对应局部分量，与 GLB 文件中的姿势一致。
            if matches!(part, ApplyPart::Position | ApplyPart::All) {
                pose.translation = Vec3::ZERO;
            }
            if matches!(part, ApplyPart::Rotation | ApplyPart::All) {
                pose.rotation = Quat::IDENTITY;
            }
            if matches!(part, ApplyPart::Scale | ApplyPart::All) {
                pose.scale = Vec3::ONE;
            }
            requests.write(SourceGlbRequest::ApplyTransform {
                target: target.clone(),
                expected_hash: hash.into(),
                remaining: pose,
            });
        }
        _ => {}
    }
}

fn sync(
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    browser: Res<EditorGltfBrowserState>,
    inspector: Res<Inspector>,
    active: Res<ActiveNumericInput>,
    mut controls: ResMut<Controls>,
    mut focus: ResMut<InputFocus>,
    mut names: Query<(Entity, &mut EditableText, Has<InteractionDisabled>), With<NameInput>>,
    mut items: Query<&mut Node, With<ApplyItems>>,
    mut buttons: Query<(
        Entity,
        &Action,
        &UiInteraction,
        Has<InteractionDisabled>,
        &mut EditorButtonClick,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
    mut commands: Commands,
) {
    let identity = inspector.identity();
    let changed = identity
        != controls
            .bound
            .as_ref()
            .map(|(target, hash)| (target, hash.as_str()));
    let enabled = !session.source_glb_busy
        && inspector
            .editable_target(&browser, &editor, &session)
            .is_some();
    let apply = enabled && identity.is_some_and(|(target, _)| is_mesh(target));
    if changed {
        controls.bound = identity.map(|(target, hash)| (target.clone(), hash.to_owned()));
        controls.open = false;
    }
    if !apply || active.entity.is_some() {
        controls.open = false;
    }
    for (entity, mut text, disabled) in &mut names {
        if (changed || !enabled) && focus.get() == Some(entity) {
            focus.clear();
        }
        if changed {
            text_input::replace_single_line_value(
                &mut text,
                identity.map_or("", |(target, _)| &target.name),
                feathrise_scene_content::MAX_PLACEMENT_EDITOR_NAME_CHARS,
            );
        }
        set_enabled(&mut commands, entity, enabled, disabled);
        if enabled == disabled {
            if enabled {
                commands.entity(entity).insert(TabIndex(0));
            } else {
                commands.entity(entity).remove::<TabIndex>();
            }
        }
    }
    for mut node in &mut items {
        let display = if controls.open {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    for (entity, action, hover, disabled, mut click, mut background, mut border) in &mut buttons {
        let available = match action {
            Action::Rename => enabled,
            Action::Apply(None) => apply,
            Action::Apply(Some(_)) => apply && controls.open,
        };
        if changed {
            click.invalidate();
        }
        set_enabled(&mut commands, entity, available, disabled);
        let (bg, edge) = property_button_visual(
            if available {
                *hover
            } else {
                UiInteraction::None
            },
            matches!(action, Action::Apply(None)) && controls.open,
        );
        background.set_if_neq(bg);
        border.set_if_neq(edge);
    }
}

fn set_enabled(commands: &mut Commands, entity: Entity, enabled: bool, disabled: bool) {
    if enabled == disabled {
        if enabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        } else {
            commands.entity(entity).insert(InteractionDisabled);
        }
    }
}
