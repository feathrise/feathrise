//! 属性面板用原专业数值框编辑当前 GLB 节点的源局部 TRS。

use super::{SourceGlbRequest, owns_selection, target};
use crate::{
    SceneEditorSystemSet,
    authoring::{EditorAuthoringSession, source_glbs},
    resources::SceneEditorState,
    ui::{
        gltf_browser::EditorGltfBrowserState,
        placement_transform::PlacementTransformField as Field,
        scene_context_menu::{SceneObjectKey, SceneObjectTarget},
        theme,
    },
};
use bevy::{
    prelude::*,
    tasks::{IoTaskPool, Task, futures::check_ready},
    text::FontSourceTemplate,
};
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, NumericInputCommitOutcome, NumericInputCommitted,
    NumericInputDisabled, NumericInputSetValue, NumericInputValue, NumericScrubFieldOptions,
    NumericValue, spawn_numeric_scrub_field,
};

#[derive(Component, Clone, Copy, Default)]
struct Panel;
#[derive(Component, Clone, Copy)]
struct Slot(Field);
#[derive(Component, Clone, Copy)]
struct Control(Field);

#[derive(Resource, Default)]
pub(crate) struct Inspector {
    visible: bool,
    displayed: Option<SceneObjectTarget>,
    requested: Option<SceneObjectTarget>,
    pose: Option<Transform>,
    read: Option<(SceneObjectTarget, Task<Result<Transform, String>>)>,
}

impl Inspector {
    /// 视口只借用已从源文件或当前草稿读到的节点局部姿势。
    pub(crate) fn selected_pose(
        &self,
        browser: &EditorGltfBrowserState,
        editor: &SceneEditorState,
        session: &EditorAuthoringSession,
    ) -> Option<(&SceneObjectTarget, Transform)> {
        let object = self.displayed.as_ref()?;
        let SceneObjectKey::Placement {
            mesh: Some(mesh), ..
        } = &object.key
        else {
            return None;
        };
        (self.visible
            && !object.hidden
            && !object.locked
            && object.current(editor, session)
            && browser.selection.as_ref() == Some(mesh)
            && browser.selected_placement_id.as_ref() == Some(&mesh.placement_id))
        .then_some((object, self.pose?))
    }
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<Inspector>()
        .add_systems(
            Update,
            (setup, refresh, preview)
                .chain()
                .in_set(SceneEditorSystemSet::Presentation),
        )
        .add_systems(Update, commit.in_set(SceneEditorSystemSet::Input));
}

fn row(label: &str, fields: [Field; 3]) -> impl Scene {
    let slots = fields
        .into_iter()
        .map(|field| {
            bsn! { ~{Slot(field)}
                Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0), height: px(24) }
            }
        })
        .collect::<Vec<_>>();
    bsn! { Node { width: percent(100), min_width: px(0), column_gap: px(3), align_items: AlignItems::Center }
        Children [
            Text(label) TextColor(theme::TEXT_MUTED) TextLayout::no_wrap()
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(crate::ui::scene::FONT_PATH)), font_size: px(10) }
                Node { min_width: px(35), flex_shrink: 0.0 }--
            {slots}
        ]
    }
}

struct AppliedPose {
    entity: Entity,
    index: usize,
    before: Transform,
    current: Transform,
}

fn preview(
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    state: Res<Inspector>,
    selection: Res<crate::SelectedPlacementViewportState>,
    movement: Option<Res<crate::viewport::placement_move::PlacementMoveState>>,
    parents: Query<&ChildOf>,
    mut nodes: Query<(
        &feathrise_editor_animation_model::types::EmbeddedNodeIndex,
        &mut Transform,
    )>,
    mut applied: Local<Option<AppliedPose>>,
) {
    let wanted = (|| {
        let object = state
            .displayed
            .as_ref()
            .filter(|object| object.current(&editor, &session))?;
        let SceneObjectKey::Placement {
            index,
            mesh: Some(mesh),
            ..
        } = &object.key
        else {
            return None;
        };
        let draft = session.draft.as_ref()?;
        let record = draft.placements.placements.get(*index)?;
        draft.source_glb_edits.version(&record.asset_path)?;
        let primitive = selection.exact_primitive(&draft.scene_id, mesh)?;
        Some((
            parents.get(primitive).ok()?.parent(),
            mesh.node_index?,
            state.pose?,
        ))
    })();
    if wanted.as_ref().is_some_and(|(entity, _, _)| {
        movement
            .as_ref()
            .is_some_and(|movement| movement.owns_source_node(*entity))
    }) {
        return; // 本次手柄持有同一实时节点，属性预览在松手或取消后再继续。
    }
    if let Some(old) = applied.as_ref()
        && wanted.is_none_or(|(entity, index, _)| (entity, index) != (old.entity, old.index))
    {
        let old = applied.take().unwrap();
        if let Ok((index, mut transform)) = nodes.get_mut(old.entity)
            && index.0 == old.index
            && *transform == old.current
        {
            *transform = old.before; // 只有仍由本预览持有的姿势可恢复，热更后的实体保持权威数据。
        }
    }
    if let Some((entity, index, next)) = wanted
        && let Ok((marker, mut transform)) = nodes.get_mut(entity)
        && marker.0 == index
        && *transform != next
    {
        let before = applied.as_ref().map_or(*transform, |old| old.before);
        *transform = next;
        *applied = Some(AppliedPose {
            entity,
            index,
            before,
            current: next,
        });
    }
}

pub(in crate::ui) fn panel() -> impl Scene {
    bsn! { Panel Node { display: Display::None, width: percent(100), flex_direction: FlexDirection::Column, row_gap: px(3) }
        Children [
            @crate::ui::context_help::section_header("源 GLB 节点", "位置、旋转和缩放是此节点在 GLB 中的局部变换。保存后该源文件的所有引用实例一起更新。")--
            @row("位置", [Field::TranslationX, Field::TranslationY, Field::TranslationZ])--
            @row("旋转", [Field::RotationX, Field::RotationY, Field::RotationZ])--
            @row("缩放", [Field::ScaleX, Field::ScaleY, Field::ScaleZ])
        ]
    }
}

fn setup(
    mut commands: Commands,
    slots: Query<(Entity, &Slot), Added<Slot>>,
    assets: Res<AssetServer>,
    settings: Res<feathrise_editor_core::EditorSettings>,
) {
    let font = feathrise_editor_core::editor_font(&assets, crate::ui::scene::FONT_PATH);
    for (slot, marker) in &slots {
        let field = marker.0;
        let axis = field
            .translation_axis()
            .or_else(|| field.animation_rotation_axis())
            .or_else(|| field.scale_axis())
            .unwrap();
        let options = NumericScrubFieldOptions::new(
            ["X", "Y", "Z"][axis],
            field.unit(),
            NumericValue::F32(0.0),
            field.spec(None, true),
            field.scrub_config(&settings, None, true),
        );
        commands.entity(slot).with_children(|parent| {
            let root = spawn_numeric_scrub_field(parent, Control(field), font.clone(), options);
            parent
                .commands()
                .entity(root)
                .insert(NumericInputDisabled)
                .entry::<Node>()
                .and_modify(|mut node| {
                    node.width = percent(100);
                    node.min_width = px(0);
                    node.height = px(24);
                });
        });
    }
}

fn selected(
    browser: &EditorGltfBrowserState,
    editor: &SceneEditorState,
    session: &EditorAuthoringSession,
) -> Option<SceneObjectTarget> {
    if !editor.open || editor.scene_transition_in_progress || !owns_selection(browser) {
        return None;
    }
    let mesh = browser
        .selection
        .as_ref()
        .filter(|mesh| !mesh.is_source_folder())?;
    let draft = session.draft.as_ref()?;
    let index =
        browser.placement_record_index(&mesh.placement_id, session.placement_structure_revision)?;
    let record = draft.placements.placements.get(index)?;
    if !record.asset_target.is_scene0() || record.id != mesh.placement_id {
        return None;
    }
    Some(SceneObjectTarget {
        scene_id: draft.scene_id.clone(),
        revision: session.revision,
        key: SceneObjectKey::Placement {
            index,
            id: record.id.clone(),
            mesh: Some(mesh.clone()),
        },
        name: mesh.node_path.last().cloned().unwrap_or_default(),
        hidden: record.editor.hidden,
        locked: record.editor.locked,
        copyable: false,
    })
}

fn value(pose: Transform, field: Field) -> f32 {
    if let Some(axis) = field.translation_axis() {
        return pose.translation[axis];
    }
    if let Some(axis) = field.scale_axis() {
        return pose.scale[axis];
    }
    let (x, y, z) = pose.rotation.to_euler(EulerRot::XYZ);
    [x, y, z][field.animation_rotation_axis().unwrap()].to_degrees()
}

fn refresh(
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    browser: Res<EditorGltfBrowserState>,
    active: Res<ActiveNumericInput>,
    scrub: Res<ActiveNumericInputScrub>,
    mut panel_nodes: Query<&mut Node, With<Panel>>,
    fields: Query<(
        Entity,
        &Control,
        &NumericInputValue,
        Has<NumericInputDisabled>,
    )>,
    mut state: ResMut<Inspector>,
    mut values: MessageWriter<NumericInputSetValue>,
    mut commands: Commands,
) {
    let chosen = selected(&browser, &editor, &session);
    let changed = state.requested != chosen;
    if changed {
        state.requested = chosen.clone();
        state.displayed = None;
        state.pose = None;
    }
    if let Some((_, task)) = state.read.as_mut()
        && let Some(result) = check_ready(task)
    {
        let (read, _) = state.read.take().unwrap();
        if chosen.as_ref() == Some(&read) {
            state.displayed = Some(read);
            state.pose = result.ok();
        }
    }
    if state.read.is_none()
        && state.displayed.is_none()
        && let Some(object) = chosen.as_ref()
        && let Some(draft) = session.draft.as_ref()
        && let Ok(owner) = target(object, &editor, &session, false)
    {
        let root = feathrise_editor_core::workspace_asset_root();
        let edits = draft.source_glb_edits.clone();
        let task =
            IoTaskPool::get().spawn(async move { source_glbs::read_pose(&root, &owner, &edits) });
        state.read = Some((object.clone(), task));
    }
    let show = chosen.is_some();
    state.visible = show;
    for mut node in &mut panel_nodes {
        let display = if show { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
    let enabled = show && state.pose.is_some() && !session.saving && !session.source_glb_busy;
    for (entity, field, current, disabled) in &fields {
        if enabled == disabled {
            if enabled {
                commands.entity(entity).remove::<NumericInputDisabled>();
            } else {
                commands.entity(entity).insert(NumericInputDisabled);
            }
        }
        if let Some(pose) = state.pose {
            let next = value(pose, field.0);
            if changed
                || (active.entity != Some(entity)
                    && scrub.entity != Some(entity)
                    && current
                        .0
                        .as_f32()
                        .is_none_or(|previous| (previous - next).abs() > 0.0005))
            {
                values.write(NumericInputSetValue {
                    entity,
                    value: NumericValue::F32(next),
                    cancel_edit: true,
                });
            }
        } else if changed {
            values.write(NumericInputSetValue {
                entity,
                value: NumericValue::F32(0.0),
                cancel_edit: true,
            });
        }
    }
}

fn commit(
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    browser: Res<EditorGltfBrowserState>,
    fields: Query<&Control>,
    state: Res<Inspector>,
    mut commits: MessageReader<NumericInputCommitted>,
    mut requests: MessageWriter<SourceGlbRequest>,
) {
    for event in commits.read() {
        let Ok(field) = fields.get(event.entity) else {
            continue;
        };
        let Some(next) = event.value.as_f32() else {
            continue;
        };
        if !event.edited
            || event.outcome != NumericInputCommitOutcome::Accepted
            || session.source_glb_busy
        {
            continue;
        }
        let (Some(object), Some(mut pose)) = (state.displayed.as_ref(), state.pose) else {
            continue;
        };
        if !state.visible
            || selected(&browser, &editor, &session).as_ref() != Some(object)
            || !object.current(&editor, &session)
        {
            continue;
        }
        if let Some(axis) = field.0.translation_axis() {
            pose.translation[axis] = next;
        } else if let Some(axis) = field.0.scale_axis() {
            pose.scale[axis] = next;
        } else {
            let (x, y, z) = pose.rotation.to_euler(EulerRot::XYZ);
            let mut angles = [x, y, z];
            angles[field.0.animation_rotation_axis().unwrap()] = next.to_radians();
            pose.rotation = Quat::from_euler(EulerRot::XYZ, angles[0], angles[1], angles[2]);
        }
        requests.write(SourceGlbRequest::Pose {
            target: object.clone(),
            pose,
        });
    }
}
