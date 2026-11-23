//! 属性面板在源 GLB、场景和世界坐标中查看与编辑同一节点的源变换。

use super::{owns_selection, properties, resolve_source_target};
use super::spaces::{EditFrame, Frames, Space};
use crate::{
    SceneEditorSystemSet,
    authoring::{EditorAuthoringSession, source_glbs::{self, SourceMeshProperties}},
    resources::SceneEditorState,
    ui::{
        gltf_browser::EditorGltfBrowserState,
        placement_transform::PlacementTransformField as Field,
        scene_context_menu::{SceneObjectKey, SceneObjectTarget},
        theme,
    },
};
use bevy::{
    asset::AssetId,
    prelude::*,
    tasks::{IoTaskPool, Task, futures::check_ready},
    text::FontSourceTemplate,
    ui::InteractionDisabled,
    world_serialization::WorldAsset,
};
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, EditorButtonClick, NumericInputConfig,
    NumericInputDisabled, NumericInputScrubConfig, NumericInputScrubMapping, NumericInputSetValue,
    NumericInputSpec, NumericInputValue, NumericScrubFieldOptions, NumericValue, UiInteraction,
    normalize_numeric_value, spawn_numeric_scrub_field,
};

#[derive(Component, Clone, Copy, Default)]
struct Panel;
#[derive(Component, Clone, Copy)]
struct Slot(Field);
#[derive(Component, Clone, Copy)]
pub(super) struct Control(pub(super) Field);

#[cfg(feature = "validation")]
pub(crate) fn source_position_field_for_validation(world: &mut World) -> Option<(Entity, &'static str)> {
    let space = world.resource::<Inspector>().frame?.space.label();
    let mut fields = world.query_filtered::<(Entity, &Control), Without<NumericInputDisabled>>();
    fields.iter(world).find_map(|(entity, field)| (field.0 == Field::TranslationX).then_some((entity, space)))
}

impl Control {
    pub(super) fn apply(self, pose: &mut Transform, next: f32) {
        if let Some(axis) = self.0.translation_axis() {
            pose.translation[axis] = next;
        } else if let Some(axis) = self.0.scale_axis() {
            pose.scale[axis] = next;
        } else {
            let (x, y, z) = pose.rotation.to_euler(EulerRot::XYZ);
            let mut angles = [x, y, z];
            angles[self.0.animation_rotation_axis().unwrap()] = next.to_radians();
            pose.rotation = Quat::from_euler(EulerRot::XYZ, angles[0], angles[1], angles[2]);
        }
    }
}

#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct AdoptNodeSnapshot;

type NodeRead = (Transform, Mat4, SourceMeshProperties, String);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SourcePublication {
    Draft,
    Scene(AssetId<WorldAsset>, u64),
}

#[derive(Resource, Default)]
pub(crate) struct Inspector {
    visible: bool,
    displayed: Option<SceneObjectTarget>,
    requested: Option<SceneObjectTarget>,
    requested_publication: Option<SourcePublication>,
    pose: Option<Transform>,
    pub(super) parent_in_glb: Mat4,
    pub(super) space: Space,
    pub(super) frame: Option<EditFrame>,
    pub(super) space_notice: &'static str,
    content_hash: Option<String>,
    pub(super) mesh_properties: Option<SourceMeshProperties>,
    pub(super) read_error: Option<String>,
    pub(super) selection_changed: bool,
    read: Option<(SceneObjectTarget, SourcePublication, Task<Result<NodeRead, String>>)>,
}

impl Inspector {
    /// 视口只借用已从源文件或当前草稿读到的节点局部姿势。
    pub(crate) fn selected_pose(
        &self,
        browser: &EditorGltfBrowserState,
        editor: &SceneEditorState,
        session: &EditorAuthoringSession,
    ) -> Option<(&SceneObjectTarget, Transform, &str)> {
        Some((self.editable_target(browser, editor, session)?, self.pose?, self.content_hash.as_deref()?))
    }

    pub(super) fn editable_target(
        &self,
        browser: &EditorGltfBrowserState,
        editor: &SceneEditorState,
        session: &EditorAuthoringSession,
    ) -> Option<&SceneObjectTarget> {
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
            && self.requested_publication.is_some()
            && source_publication(object, browser, session) == self.requested_publication
            && browser.selection.as_ref() == Some(mesh)
            && browser.selected_placement_id.as_ref() == Some(&mesh.placement_id))
        .then_some(object)
    }

    #[cfg(test)]
    pub(super) fn test_snapshot(target: SceneObjectTarget, pose: Transform, properties: SourceMeshProperties,
        content_hash: &str, browser: &EditorGltfBrowserState, session: &EditorAuthoringSession) -> Self {
        Self { visible: true, requested: Some(target.clone()), requested_publication: source_publication(&target, browser, session),
            displayed: Some(target), pose: Some(pose), content_hash: Some(content_hash.to_owned()),
            frame: Some(EditFrame::glb(Mat4::IDENTITY)),
            mesh_properties: Some(properties), ..Self::default() }
    }
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<Inspector>()
        .add_systems(
            Update,
            (setup, refresh.in_set(AdoptNodeSnapshot), preview)
                .chain()
                .in_set(SceneEditorSystemSet::Presentation),
        );
    properties::install(app);
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

impl AppliedPose {
    fn without_preview(&self, mut live: Transform) -> Transform {
        // 只归还预览仍持有的分量。运行时只刷新高度时，不能把已有水平差量加第二次。
        live.translation = Vec3::select(
            live.translation.cmpeq(self.current.translation), self.before.translation, live.translation,
        );
        live.scale = Vec3::select(live.scale.cmpeq(self.current.scale), self.before.scale, live.scale);
        if live.rotation == self.current.rotation {
            live.rotation = self.before.rotation;
        }
        live
    }
}

fn preview_pose(live: Transform, original: Transform, edited: Transform) -> Transform {
    // 在实际实例姿态上叠加源位移，保留运行时已经算出的贴地修正。
    let mut next = live;
    next.translation += edited.translation - original.translation;
    if edited.rotation != original.rotation {
        next.rotation = edited.rotation;
    }
    if edited.scale != original.scale {
        next.scale = edited.scale;
    }
    next
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
        let node_index = mesh.node_index?;
        // 碰撞、LOD 和其它节点的草稿不会获得当前节点的变换预览权。
        let original = draft.source_glb_edits
            .version(&record.asset_path)?
            .original_node_pose(node_index)?;
        let primitive = selection.exact_primitive(&draft.scene_id, mesh)?;
        Some((
            parents.get(primitive).ok()?.parent(),
            node_index,
            original,
            state.pose?,
        ))
    })();
    if wanted.as_ref().is_some_and(|(entity, ..)| {
        movement
            .as_ref()
            .is_some_and(|movement| movement.owns_source_node(*entity))
    }) {
        return; // 本次手柄持有同一实时节点，属性预览在松手或取消后再继续。
    }
    if let Some(old) = applied.as_ref()
        && wanted.is_none_or(|(entity, index, ..)| (entity, index) != (old.entity, old.index))
    {
        let old = applied.take().unwrap();
        if let Ok((index, mut transform)) = nodes.get_mut(old.entity)
            && index.0 == old.index
        {
            let restored = old.without_preview(*transform);
            transform.set_if_neq(restored);
        }
    }
    if let Some((entity, index, original, edited)) = wanted
        && let Ok((marker, mut transform)) = nodes.get_mut(entity)
        && marker.0 == index
    {
        let before = applied.as_ref().map_or(*transform, |old| old.without_preview(*transform));
        let next = preview_pose(before, original, edited);
        transform.set_if_neq(next);
        *applied = (next != before).then_some(AppliedPose { entity, index, before, current: next });
    }
}

#[test]
fn source_pose_preview_applies_authored_changes_to_settled_height() {
    let original = Transform::from_xyz(1.0, 0.0, 2.0);
    let live = Transform::from_xyz(1.0, 7.25, 2.0);
    assert_eq!(preview_pose(live, original, original), live);

    let edited = Transform::from_xyz(4.0, 0.0, 2.0)
        .with_rotation(Quat::from_rotation_y(0.5))
        .with_scale(Vec3::splat(2.0));
    let visual = preview_pose(live, original, edited);
    assert_eq!(visual.translation, Vec3::new(4.0, 7.25, 2.0));
    assert_eq!(visual.rotation, edited.rotation);
    assert_eq!(visual.scale, edited.scale);

    let preview = AppliedPose { entity: Entity::PLACEHOLDER, index: 0, before: live, current: visual };
    let mut refreshed = visual;
    refreshed.translation.y = 9.0;
    refreshed.rotation = Quat::from_rotation_x(0.75);
    let restored = preview.without_preview(refreshed);
    assert_eq!(restored.translation, Vec3::new(1.0, 9.0, 2.0));
    assert_eq!(restored.rotation, refreshed.rotation);
    assert_eq!(restored.scale, live.scale);
    assert_eq!(preview_pose(restored, original, edited).translation, Vec3::new(4.0, 9.0, 2.0));
}

pub(in crate::ui) fn panel() -> impl Scene {
    let spaces: Vec<_> = Space::ALL.into_iter()
        .map(|space| properties::property_button(space.label(), space)).collect();
    bsn! { Panel Node { display: Display::None, width: percent(100), flex_direction: FlexDirection::Column, row_gap: px(3) }
        Children [
            @crate::ui::context_help::section_header("节点变换", "切换位置、旋转、缩放共用的参考坐标。源 GLB 以所属模型根为准，当前场景以所在场景为准，世界包含外层引用。修改后换算为直属父节点的局部变换并保存到源 GLB。")--
            Node { width: percent(100), flex_wrap: FlexWrap::Wrap, column_gap: px(3), row_gap: px(3) }
                Children [{spaces}]--
            @row("位置", [Field::TranslationX, Field::TranslationY, Field::TranslationZ])--
            @row("旋转", [Field::RotationX, Field::RotationY, Field::RotationZ])--
            @row("缩放", [Field::ScaleX, Field::ScaleY, Field::ScaleZ])--
            @properties::panel()
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
            node_input_spec(field),
            field.scrub_config(&settings, None, true),
        );
        mount_numeric(&mut commands, slot, Control(field), font.clone(), options);
    }
}

fn node_input_spec(field: Field) -> NumericInputSpec {
    let mut spec = field.spec(None, true);
    if field.scale_axis().is_some() {
        // 父级组合后可超过整件摆放的范围，GLB 节点还可以镜像或缩到零。
        spec.min = None;
        spec.max = None;
    }
    spec
}

fn node_scale_scrub(mut config: NumericInputScrubConfig, shown: f32) -> NumericInputScrubConfig {
    // 保留常用正倍率的指数手感。超出普通范围时从实际值起拖，避免首帧跳变。
    config.maximum = config.maximum.max((2.0 * shown.abs()).min(f32::MAX));
    if shown > 0.0 {
        config.minimum = config.minimum.min(shown * 0.5);
    } else if let NumericInputScrubMapping::Exponential { exponent_per_pixel } = config.mapping {
        config.minimum = -config.maximum;
        config.mapping = NumericInputScrubMapping::Linear {
            units_per_pixel: exponent_per_pixel * shown.abs().max(1.0),
        };
    }
    config
}

#[test]
fn source_glb_scale_input_preserves_combined_and_mirrored_scale() {
    let field = Field::ScaleX;
    let spec = node_input_spec(field);
    let base = field.scrub_config(&feathrise_editor_core::EditorSettings::default(), None, true);
    assert_eq!(node_scale_scrub(base, 1.0), base);
    for shown in [-2500.0, -0.5, 0.0, 0.0006, 2500.0] {
        let expected = if shown == 0.0006 { 0.001 } else { shown };
        let proposed = NumericValue::F32(shown);
        let actual = normalize_numeric_value(proposed, spec).as_f32().unwrap();
        assert_eq!(actual, expected);
        let config = node_scale_scrub(base, actual);
        assert!(config.minimum < actual && actual < config.maximum);
        assert_eq!(matches!(config.mapping, NumericInputScrubMapping::Exponential { .. }), actual > 0.0);
    }
}

/// 位置、离地偏移和 LOD 共用公共数字框，保持相同的宽度、输入和拖动行为。
pub(super) fn mount_numeric(
    commands: &mut Commands,
    slot: Entity,
    marker: impl Component,
    font: Handle<Font>,
    options: NumericScrubFieldOptions,
) {
    commands.entity(slot).with_children(|parent| {
        let root = spawn_numeric_scrub_field(parent, marker, font, options);
        parent.commands().entity(root).insert(NumericInputDisabled)
            .entry::<Node>()
            .and_modify(|mut node| {
                node.width = percent(100);
                node.min_width = px(0);
                node.height = px(24);
            });
    });
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
    if !record.asset_target.is_scene0() || record.id != mesh.placement_id
        || record.asset_path == draft.placements_path()
    {
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

pub(super) fn source_publication(
    object: &SceneObjectTarget,
    browser: &EditorGltfBrowserState,
    session: &EditorAuthoringSession,
) -> Option<SourcePublication> {
    let SceneObjectKey::Placement { index, .. } = &object.key else { return None; };
    let draft = session.draft.as_ref()?;
    let path = &draft.placements.placements.get(*index)?.asset_path;
    if draft.source_glb_edits.version(path).is_some() {
        return Some(SourcePublication::Draft);
    }
    let (asset, revision) = browser.ready_source_revision(path)?;
    Some(SourcePublication::Scene(asset, revision))
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
    settings: Res<feathrise_editor_core::EditorSettings>,
    active: Res<ActiveNumericInput>,
    scrub: Res<ActiveNumericInputScrub>,
    mut panel_nodes: Query<&mut Node, With<Panel>>,
    mut fields: Query<(
        Entity,
        &Control,
        &NumericInputValue,
        &NumericInputConfig,
        &mut NumericInputScrubConfig,
        Has<NumericInputDisabled>,
    )>,
    frames: Frames,
    mut buttons: Query<(Entity, &Space, &UiInteraction, Has<InteractionDisabled>, &mut BackgroundColor, &mut EditorButtonClick)>,
    mut state: ResMut<Inspector>,
    mut values: MessageWriter<NumericInputSetValue>,
    mut commands: Commands,
) {
    let chosen = selected(&browser, &editor, &session);
    let publication = chosen.as_ref().and_then(|object| source_publication(object, &browser, &session));
    let changed = state.requested != chosen || state.requested_publication != publication;
    state.selection_changed = changed;
    if changed {
        state.requested = chosen.clone();
        state.requested_publication = publication;
        state.displayed = None;
        state.pose = None;
        state.content_hash = None;
        state.mesh_properties = None;
        state.read_error = None;
    }
    if let Some((_, _, task)) = state.read.as_mut()
        && let Some(result) = check_ready(task)
    {
        let (read, read_publication, _) = state.read.take().unwrap();
        if chosen.as_ref() == Some(&read) && publication == Some(read_publication) {
            state.displayed = Some(read);
            match result {
                Ok((pose, parent, mesh_properties, content_hash)) => {
                    state.pose = Some(pose);
                    state.parent_in_glb = parent;
                    state.mesh_properties = Some(mesh_properties);
                    state.content_hash = Some(content_hash);
                }
                Err(error) => state.read_error = Some(error),
            }
        }
    }
    if state.read.is_none()
        && state.displayed.is_none()
        && let Some(publication) = publication
        && let Some(object) = chosen.as_ref().filter(|object| object.current(&editor, &session))
        && let Some(draft) = session.draft.as_ref()
    {
        match resolve_source_target(object, draft, false) {
            Ok(owner) => {
                let root = feathrise_editor_core::workspace_asset_root();
                let edits = draft.source_glb_edits.clone();
                // 局部姿态和四项属性来自同一份源或草稿，只解析一次 GLB。
                let task = IoTaskPool::get().spawn(async move {
                    source_glbs::read_node(&root, &owner, &edits)
                });
                state.read = Some((object.clone(), publication, task));
            }
            Err(error) => {
                state.displayed = Some(object.clone());
                state.read_error = Some(error);
            }
        }
    }
    let show = chosen.is_some();
    state.visible = show;
    for mut node in &mut panel_nodes {
        let display = if show { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
    for (entity, space, hover, disabled, mut background, mut click) in &mut buttons {
        if changed { click.invalidate(); }
        if show == disabled {
            if show { commands.entity(entity).remove::<InteractionDisabled>(); }
            else { commands.entity(entity).insert(InteractionDisabled); }
        }
        background.set_if_neq(BackgroundColor(properties::button_color(if show { *hover } else { UiInteraction::None }, *space == state.space)));
    }
    let frame = state.pose.and_then(|_| chosen.as_ref().and_then(|object| frames.get(state.space, object, state.parent_in_glb)));
    let reference_changed = changed || frame != state.frame;
    state.frame = frame;
    let position = frame.zip(state.pose).map(|(frame, pose)| frame.position(pose)).filter(|point| point.is_finite());
    let displayed = frame.zip(state.pose).and_then(|(frame, pose)| frame.pose(pose));
    state.space_notice = if state.pose.is_none() { "" }
        else if frame.is_none() && state.space == Space::Glb { "源 GLB 父级坐标无法换算" }
        else if frame.is_none() { "当前引用的坐标尚未就绪" }
        else if displayed.is_none() { "当前坐标系的旋转和缩放无法精确表示，可编辑位置" }
        else { "" };
    let enabled = !reference_changed && !session.source_glb_busy
        && state.editable_target(&browser, &editor, &session).is_some();
    for (entity, field, current, input, mut drag, disabled) in &mut fields {
        let next = match field.0.translation_axis() {
            Some(axis) => position.map(|position| position[axis]),
            None => displayed.map(|pose| value(pose, field.0)),
        };
        // 让上一代拖动先走完共享捕获的取消/松手，再把同一数值框交给新节点。
        let field_enabled = enabled && next.is_some() && (!disabled || scrub.entity != Some(entity));
        if field_enabled == disabled {
            if field_enabled {
                commands.entity(entity).remove::<NumericInputDisabled>();
            } else {
                commands.entity(entity).insert(NumericInputDisabled);
            }
        }
        if let Some(next) = next {
            let next = normalize_numeric_value(NumericValue::F32(next), input.0);
            let idle = active.entity != Some(entity) && scrub.entity != Some(entity);
            if idle && field.0.scale_axis().is_some() {
                let mut base = field.0.scrub_config(&settings, None, true);
                base.sensitivity = drag.sensitivity;
                drag.set_if_neq(node_scale_scrub(base, next.as_f32().unwrap()));
            }
            if reference_changed || idle && current.0 != next {
                values.write(NumericInputSetValue {
                    entity,
                    value: next,
                    cancel_edit: true,
                });
            }
        } else if reference_changed {
            values.write(NumericInputSetValue {
                entity,
                value: current.0,
                cancel_edit: true,
            });
        }
    }
}
