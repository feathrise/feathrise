//! 资源动画时间轴的片段操作，名称输入绑定当前文件会话和片段身份。

use super::{components::AnimationWriteControls, context::AnimationContext, resources::{AnimationEditorState, PendingAction}, types::Command};
use bevy::{input_focus::{FocusCause, InputFocus}, prelude::*, text::{EditableText, FontSourceTemplate, TextEdit}, ui::InteractionDisabled};
use feathrise_editor_animation_model::media::{AssetPreviewState, AssetPreviewWorkspace};
use feathrise_editor_core::{EditorTextInput, FONT_PATH};
use feathrise_editor_ui::{colors, localization::label_scene, metrics, EditorButtonClick, InteractionButton};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action { Rename, Duplicate, Delete, Confirm, Cancel }

#[derive(Clone, Copy, Component)]
pub(super) struct ClipButton { action: Action, revision: u64 }
#[derive(Clone, Copy, Component, Default)]
pub(super) struct NameRow;
#[derive(Clone, Copy, Component, Default)]
pub(super) struct NameField;

#[derive(Clone, Copy, PartialEq, Eq)]
struct ClipTarget { source_revision: u64, id: usize }

struct Rename {
    target: ClipTarget,
    row: Entity,
    field: Entity,
}

#[derive(Resource, Default)]
pub(super) struct ClipManagement {
    rename: Option<Rename>,
}

fn target(state: &AnimationEditorState) -> Option<ClipTarget> {
    let session = state.current()?;
    if !state.editable() || state.busy() || session.pose.is_some() {
        return None;
    }
    Some(ClipTarget { source_revision: state.source_revision, id: session.document.clips.get(session.clip)?.source_index })
}

fn button(action: Action, text: &'static str) -> impl Scene {
    let danger = action == Action::Delete;
    let background = if danger { colors::DANGER_BG } else { colors::SLOT_BG };
    let border = if danger { colors::DANGER_BORDER } else { colors::COPPER_BORDER_WEAK };
    let foreground = if danger { colors::DANGER_TEXT } else { colors::TEXT_PRIMARY };
    bsn! {
        InteractionButton ~{ClipButton { action, revision: 0 }}
        Node { height: px(26), padding: UiRect::horizontal(px(6)), flex_shrink: 0.0,
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            border: px(1), border_radius: BorderRadius::all(px(2)) }
        BackgroundColor(background) BorderColor::all(border)
        Children [
            Text(text) @label_scene(text) Pickable::IGNORE
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
                font_size: px(metrics::FONT_CONTROL) } TextColor(foreground)
        ]
    }
}

pub(super) fn scene() -> impl Scene {
    bsn! {
        AnimationWriteControls
        Node { display: Display::None, width: percent(100), min_width: px(0), flex_wrap: FlexWrap::Wrap,
            align_items: AlignItems::Center, column_gap: px(4), row_gap: px(3) }
        Children [
            @button(Action::Rename, "重命名动画")--
            @button(Action::Duplicate, "复制动画")--
            @button(Action::Delete, "删除动画")--
            NameRow Node { display: Display::None, width: percent(100), min_width: px(0),
                align_items: AlignItems::Center, column_gap: px(4) }
            Children [
                NameField EditorTextInput ~{EditableText { allow_newlines: false, ..EditableText::new("") }}
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
                    font_size: px(metrics::FONT_CONTROL) }
                TextColor(colors::TEXT_PRIMARY) TextLayout::no_wrap()
                Node { flex_grow: 1.0, min_width: px(0), height: px(26), padding: UiRect::horizontal(px(6)),
                    border: UiRect::all(px(1)), overflow: Overflow::clip() }
                BackgroundColor(colors::INPUT_BG) BorderColor::all(colors::INPUT_BORDER)--
                @button(Action::Confirm, "确认")-- @button(Action::Cancel, "取消")
            ]
        ]
    }
}

pub(super) fn input(
    mut context: ParamSet<(AnimationContext, Option<ResMut<InputFocus>>)>,
    preview: Res<AssetPreviewState>,
    media: Res<AssetPreviewWorkspace>,
    mut state: ResMut<AnimationEditorState>,
    mut management: ResMut<ClipManagement>,
    buttons: Query<(&ClipButton, &EditorButtonClick, &ChildOf), Changed<EditorButtonClick>>,
    rows: Query<(Entity, &ChildOf), With<NameRow>>,
    mut fields: Query<(Entity, &mut EditableText, &ChildOf), With<NameField>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
) {
    let (available, composing) = {
        let context = context.p0();
        (context.available(&preview, &media) && !context.host.weights_own_viewport
            && !context.guard.blocked() && !context.host.scene_dragging
            && context.numeric.as_ref().is_none_or(|input| input.entity.is_none())
            && context.numeric_text.as_ref().is_none_or(|input| input.entity.is_none())
            && state.visible && context.source_matches(&state, &preview, &media, context.host.asset_root()),
            context.text.as_ref().is_some_and(|guard| guard.block_shortcuts))
    };
    let selected = available.then(|| target(&state)).flatten();
    let mut focus = context.p1();
    if management.rename.as_ref().is_some_and(|rename| selected != Some(rename.target)
        || fields.get(rename.field).is_err()) {
        close(&mut management, focus.as_deref_mut());
    }
    let Some(selected) = selected else { return; };
    let clicked = buttons.iter().find_map(|(button, click, parent)|
        (click.is_clicked() && button.revision == state.revision).then_some((button.action, parent.parent())));
    if let Some(rename) = management.rename.as_ref() {
        let hit = clicked.filter(|(_, parent)| *parent == rename.row).map(|(action, _)| action);
        let keyboard = focus.as_deref().is_some_and(|focus| focus.get() == Some(rename.field))
            .then_some(keys.as_deref()).flatten();
        if hit == Some(Action::Cancel) || !composing && keyboard.is_some_and(|keys| keys.just_pressed(KeyCode::Escape)) {
            close(&mut management, focus.as_deref_mut());
            state.mouse_owned = true;
        } else if !composing && (hit == Some(Action::Confirm) || keyboard.is_some_and(|keys|
            keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter))) {
            let Ok((_, field, _)) = fields.get(rename.field) else { return; };
            if field.is_composing() { return; }
            let name = field.value().to_string();
            let session = state.current().expect("片段已复验");
            match session.document.renamed_clip_name(selected.id, &name) {
                Ok(name) => {
                    if name != session.document.clips[session.clip].name {
                        state.pending = Some(PendingAction::Command(Command::RenameClip { id: selected.id, name }));
                    }
                    close(&mut management, focus.as_deref_mut());
                }
                Err(error) => state.status = error,
            }
            state.mouse_owned = true;
            state.changed();
        }
        return;
    }
    let Some((action, controls)) = clicked else { return; };
    let command = match action {
        Action::Rename => {
            let Some((row, _)) = rows.iter().find(|(_, parent)| parent.parent() == controls) else { return; };
            let Some((field_id, mut field, _)) = fields.iter_mut().find(|(_, _, parent)| parent.parent() == row)
                else { return; };
            let session = state.current().expect("片段已复验");
            field.clear();
            field.editor.set_text(&session.document.clips[session.clip].name);
            field.queue_edit(TextEdit::SelectAll);
            management.rename = Some(Rename { target: selected, row, field: field_id });
            if let Some(focus) = focus.as_deref_mut() { focus.set(field_id, FocusCause::Navigated); }
            None
        }
        Action::Duplicate => Some(Command::DuplicateClip { id: selected.id }),
        Action::Delete => Some(Command::DeleteClip { id: selected.id }),
        Action::Confirm | Action::Cancel => return,
    };
    state.pending = command.map(PendingAction::Command);
    state.playing = false;
    state.mouse_owned = true;
    state.changed();
}

fn close(management: &mut ClipManagement, focus: Option<&mut InputFocus>) {
    if let Some(rename) = management.rename.take()
        && let Some(focus) = focus.filter(|focus| focus.get() == Some(rename.field))
    {
        focus.clear();
    }
}

pub(super) fn sync(
    state: Res<AnimationEditorState>,
    management: Res<ClipManagement>,
    mut commands: Commands,
    mut rows: Query<(Entity, &mut Node), With<NameRow>>,
    mut buttons: Query<(Entity, &mut ClipButton, Has<InteractionDisabled>)>,
) {
    for (entity, mut row) in &mut rows {
        let show = management.rename.as_ref().is_some_and(|rename| rename.row == entity);
        let desired = if show { Display::Flex } else { Display::None };
        if row.display != desired { row.display = desired; }
    }
    let author = target(&state).is_some();
    for (entity, mut button, disabled) in &mut buttons {
        let lock = !author || (management.rename.is_some() && !matches!(button.action, Action::Confirm | Action::Cancel));
        if lock != disabled {
            if lock { commands.entity(entity).insert(InteractionDisabled); }
            else { commands.entity(entity).remove::<InteractionDisabled>(); }
        }
        if button.revision != state.revision { button.revision = state.revision; }
    }
}

#[cfg(feature = "validation")]
pub(super) fn original_duplicate_button(world: &mut World) -> Option<Entity> {
    use bevy::ui::ComputedNode;

    let binding = world.resource::<AnimationEditorState>().revision;
    world.query_filtered::<(Entity, &ClipButton, &ComputedNode, &InheritedVisibility), Without<InteractionDisabled>>()
        .iter(world)
        .find_map(|(entity, button, layout, visibility)| {
            (button.revision == binding && button.action == Action::Duplicate
                && layout.size().min_element() > 1.0 && visibility.get()).then_some(entity)
        })
}
