//! 外部 GLB 网格节点的四组独立属性，读取和提交都冻结当前源节点身份。

use super::{
    SourceGlbRequest,
    pose::{AdoptNodeSnapshot, Control as PoseControl, Inspector, SourcePublication, mount_numeric, source_publication},
};
use crate::{
    SceneEditorSystemSet,
    authoring::{
        EditorAuthoringSession,
        source_glbs::{SourceMeshHeight, SourceMeshProperties},
    },
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState},
    ui::{
        context_help::section_header, gltf_browser::EditorGltfBrowserState,
        placement_transform::PlacementTransformField as Field, scene::FONT_PATH,
        scene_context_menu::SceneObjectTarget, theme,
    },
};
use bevy::{prelude::*, text::FontSourceTemplate, ui::InteractionDisabled};
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, EditorButtonClick, InteractionButton,
    NumericInputCommitOutcome, NumericInputCommitted, NumericInputDisabled, NumericInputSetValue,
    NumericInputValue, NumericScrubFieldOptions, NumericValue, UiInteraction, metrics,
    round_to_decimals,
};
use feathrise_scene_content::{MeshLodMode, MeshLodPolicy};
use std::mem::discriminant;

pub(super) fn install(app: &mut App) {
    app.add_systems(
        Update,
        (setup, sync)
            .chain()
            .after(AdoptNodeSnapshot)
            .in_set(SceneEditorSystemSet::Presentation),
    )
    .add_systems(
        Update,
        input
            .after(
                crate::ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard,
            )
            .in_set(SceneEditorSystemSet::Input),
    );
}

#[derive(Component, Clone, Copy)]
enum Mode {
    Height(Option<SourceMeshHeight>),
    Collision(Option<bool>),
    DisplayLod(Lod),
    CollisionLod(Lod),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lod {
    Placement,
    Global,
    Original,
    Custom,
}

impl Lod {
    const ALL: [Self; 4] = [Self::Placement, Self::Global, Self::Original, Self::Custom];

    fn from_mode<T>(mode: Option<MeshLodMode<T>>) -> Self {
        match mode {
            None => Self::Placement,
            Some(MeshLodMode::Inherit) => Self::Global,
            Some(MeshLodMode::Disabled) => Self::Original,
            Some(MeshLodMode::Override(_)) => Self::Custom,
        }
    }

    fn mode<T: Default>(self) -> Option<MeshLodMode<T>> {
        match self {
            Self::Placement => None,
            Self::Global => Some(MeshLodMode::Inherit),
            Self::Original => Some(MeshLodMode::Disabled),
            Self::Custom => Some(MeshLodMode::Override(T::default())),
        }
    }
}

impl Mode {
    fn label(self) -> &'static str {
        match self {
            Self::Height(None)
            | Self::Collision(None)
            | Self::DisplayLod(Lod::Placement)
            | Self::CollisionLod(Lod::Placement) => "继承摆放",
            Self::Height(Some(SourceMeshHeight::FollowTerrain { .. })) => "跟随地形",
            Self::Height(Some(SourceMeshHeight::FixedHeight)) => "固定高度",
            Self::Height(Some(SourceMeshHeight::SnapToGround)) => "吸附地面",
            Self::Collision(Some(true)) => "开启",
            Self::Collision(Some(false)) => "关闭",
            Self::DisplayLod(Lod::Global) | Self::CollisionLod(Lod::Global) => "继承全局",
            Self::DisplayLod(Lod::Original) | Self::CollisionLod(Lod::Original) => "保持原始",
            Self::DisplayLod(Lod::Custom) | Self::CollisionLod(Lod::Custom) => "自定义",
        }
    }

    fn selected(self, properties: SourceMeshProperties) -> bool {
        match self {
            Self::Height(mode) => {
                mode.as_ref().map(discriminant) == properties.height.as_ref().map(discriminant)
            }
            Self::Collision(mode) => mode == properties.collision,
            Self::DisplayLod(mode) => mode == Lod::from_mode(properties.display_lod),
            Self::CollisionLod(mode) => mode == Lod::from_mode(properties.collision_lod),
        }
    }

    fn apply(self, properties: &mut SourceMeshProperties) {
        // 模式已选中时保留用户编辑的偏移或精度。继承摆放只清除当前一项覆盖。
        if self.selected(*properties) {
            return;
        }
        match self {
            Self::Height(mode) => properties.height = mode,
            Self::Collision(mode) => properties.collision = mode,
            Self::DisplayLod(mode) => properties.display_lod = mode.mode(),
            Self::CollisionLod(mode) => properties.collision_lod = mode.mode(),
        }
    }
}

#[derive(Component, Clone, Copy)]
struct ValueField(Field);

impl ValueField {
    fn label(self) -> &'static str {
        if self.0 == Field::TranslationY {
            "离地偏移"
        } else {
            self.0.label()
        }
    }

    fn slot(self, properties: &mut SourceMeshProperties) -> Option<&mut f32> {
        match (
            self.0,
            &mut properties.height,
            &mut properties.display_lod,
            &mut properties.collision_lod,
        ) {
            (Field::TranslationY, Some(SourceMeshHeight::FollowTerrain { offset }), _, _) => {
                Some(offset)
            }
            (Field::LodScreenError, _, Some(MeshLodMode::Override(display)), _) => {
                Some(&mut display.max_error_pixels)
            }
            (Field::LodHysteresis, _, Some(MeshLodMode::Override(display)), _) => {
                Some(&mut display.hysteresis_ratio)
            }
            (Field::LodSafeNear, _, _, Some(MeshLodMode::Override(collision))) => {
                Some(&mut collision.safe_near_meters)
            }
            (Field::LodCollisionError, _, _, Some(MeshLodMode::Override(collision))) => {
                Some(&mut collision.max_error_meters)
            }
            _ => None,
        }
    }

    fn value(self, mut properties: SourceMeshProperties) -> Option<f32> {
        self.slot(&mut properties).copied()
    }
}

#[derive(Component, Clone, Copy)]
struct NumericSlot(ValueField);

#[derive(Component, Clone, Copy, Default)]
struct ReadStatus;

fn mode_row(modes: impl IntoIterator<Item = Mode>) -> impl Scene {
    let buttons = modes.into_iter().map(mode_button).collect::<Vec<_>>();
    bsn! { Node { width: percent(100), flex_wrap: FlexWrap::Wrap, column_gap: px(3), row_gap: px(3), flex_shrink: 0.0 }
    Children [{buttons}] }
}

fn mode_button(mode: Mode) -> impl Scene {
    let label = mode.label();
    bsn! {
        InteractionButton InteractionDisabled ~{mode}
        Node {
            height: px(26), flex_grow: 1.0, flex_basis: px(0), min_width: px(metrics::FONT_CONTROL * 4.0 + 4.0),
            flex_shrink: 0.0, border: px(1), border_radius: BorderRadius::all(px(3)),
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
        }
        BackgroundColor(theme::BUTTON_DEV_SMALL_NORMAL)
        BorderColor::all(theme::COPPER_BORDER_WEAK)
        Children [
            Text(label) @feathrise_editor_ui::localization::label_scene(label)
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(metrics::FONT_CONTROL) }
            TextColor(theme::TEXT_PRIMARY) TextLayout::no_wrap()
        ]
    }
}

fn numeric_slot(field: Field) -> impl Scene {
    bsn! { ~{NumericSlot(ValueField(field))}
    Node { display: Display::None, width: percent(100), min_width: px(0), height: px(24), flex_shrink: 0.0 } }
}

pub(super) fn panel() -> impl Scene {
    let height_modes = [
        Mode::Height(None),
        Mode::Height(Some(SourceMeshHeight::FollowTerrain { offset: 0.0 })),
        Mode::Height(Some(SourceMeshHeight::FixedHeight)),
        Mode::Height(Some(SourceMeshHeight::SnapToGround)),
    ];
    let collision_modes = [
        Mode::Collision(None),
        Mode::Collision(Some(true)),
        Mode::Collision(Some(false)),
    ];
    let display_lod_modes = Lod::ALL.map(Mode::DisplayLod);
    let collision_lod_modes = Lod::ALL.map(Mode::CollisionLod);
    bsn! { Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(3) }
        Children [
            ReadStatus Text("")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(metrics::FONT_HINT) }
                TextColor(theme::TEXT_MUTED) TextLayout { linebreak: bevy::text::LineBreak::AnyCharacter }
                Node { display: Display::None, width: percent(100), min_width: px(0) }--
            @section_header("高度与落点", "继承该 GLB 摆放的高度设置。跟随地形使用本节点的离地偏移，吸附地面以零偏移贴地，固定高度保留源节点姿态。")--
            @mode_row(height_modes)--
            @numeric_slot(Field::TranslationY)--
            @section_header("碰撞", "可只为当前节点启用或关闭碰撞。该设置与碰撞 LOD 分开保存。")--
            @mode_row(collision_modes)--
            @section_header("显示 LOD", "继承摆放沿用所属 GLB 实例的策略。继承全局直接使用全局设置，自定义精度仍受全局开关约束。")--
            @mode_row(display_lod_modes)--
            @numeric_slot(Field::LodScreenError)--
            @numeric_slot(Field::LodHysteresis)--
            @section_header("碰撞 LOD", "继承摆放沿用所属 GLB 实例的策略。保持原始只关闭碰撞简化，碰撞开关单独控制。")--
            @mode_row(collision_lod_modes)--
            @numeric_slot(Field::LodSafeNear)--
            @numeric_slot(Field::LodCollisionError)
        ]
    }
}

fn setup(
    mut commands: Commands,
    slots: Query<(Entity, &NumericSlot), Added<NumericSlot>>,
    assets: Res<AssetServer>,
    settings: Res<feathrise_editor_core::EditorSettings>,
) {
    if slots.is_empty() {
        return;
    }
    let font = feathrise_editor_core::editor_font(&assets, FONT_PATH);
    for (slot, marker) in &slots {
        let field = marker.0;
        let options = NumericScrubFieldOptions::new(
            field.label(),
            field.0.unit(),
            NumericValue::F32(0.0),
            field.0.spec(None, false),
            field.0.scrub_config(&settings, None, false),
        );
        mount_numeric(&mut commands, slot, field, font.clone(), options);
    }
}

fn sync(
    editor: Res<SceneEditorState>,
    browser: Res<EditorGltfBrowserState>,
    session: Res<EditorAuthoringSession>,
    state: Res<Inspector>,
    active: Res<ActiveNumericInput>,
    scrub: Res<ActiveNumericInputScrub>,
    mut commands: Commands,
    mut buttons: Query<(
        Entity,
        &Mode,
        &UiInteraction,
        Has<InteractionDisabled>,
        &mut BackgroundColor,
        &mut EditorButtonClick,
    )>,
    mut slots: Query<(&NumericSlot, &mut Node), Without<ReadStatus>>,
    mut status: Query<(&mut Text, &mut Node), With<ReadStatus>>,
    fields: Query<(
        Entity,
        &ValueField,
        &NumericInputValue,
        Has<NumericInputDisabled>,
    )>,
    mut values: MessageWriter<NumericInputSetValue>,
) {
    let properties = state.mesh_properties;
    let enabled = properties.is_some()
        && !session.source_glb_busy
        && state.editable_target(&browser, &editor, &session).is_some();
    for (entity, mode, hover, disabled, mut background, mut click) in &mut buttons {
        if state.selection_changed {
            click.invalidate(); // 固定面板重绑节点时，按在旧节点上的按钮不能在新节点上松手生效。
        }
        if enabled == disabled {
            if enabled {
                commands.entity(entity).remove::<InteractionDisabled>();
            } else {
                commands.entity(entity).insert(InteractionDisabled);
            }
        }
        let selected = properties.is_some_and(|properties| mode.selected(properties));
        let interaction = if enabled { *hover } else { UiInteraction::None };
        let color = match (interaction, selected) {
            (UiInteraction::Pressed, _) => theme::BUTTON_DEV_SMALL_PRESSED,
            (UiInteraction::Hovered, _) => theme::BUTTON_DEV_SMALL_HOVERED,
            (UiInteraction::None, true) => theme::SELECT_BG,
            (UiInteraction::None, false) => theme::BUTTON_DEV_SMALL_NORMAL,
        };
        background.set_if_neq(BackgroundColor(color));
    }
    for (slot, mut node) in &mut slots {
        let has_value = properties
            .and_then(|properties| slot.0.value(properties))
            .is_some();
        let display = if has_value {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    for (entity, field, current, disabled) in &fields {
        let value = properties.and_then(|properties| field.value(properties));
        let field_enabled = enabled && value.is_some() && !state.selection_changed
            && (!disabled || scrub.entity != Some(entity));
        if field_enabled == disabled {
            if field_enabled {
                commands.entity(entity).remove::<NumericInputDisabled>();
            } else {
                commands.entity(entity).insert(NumericInputDisabled);
            }
        }
        // 待加载或继承时保留显示值，也要撤销旧字段尚未提交的文本。
        let next = value.map_or(current.0, |value| NumericValue::F32(round_to_decimals(value, field.0.decimals())));
        if state.selection_changed
            || (value.is_some()
                && active.entity != Some(entity)
                && scrub.entity != Some(entity)
                && current.0 != next)
        {
            values.write(NumericInputSetValue {
                entity,
                value: next,
                cancel_edit: true,
            });
        }
    }
    let notice = if properties.is_some() {
        ""
    } else {
        state.read_error.as_deref().unwrap_or("正在读取源网格属性…")
    };
    for (mut text, mut node) in &mut status {
        if text.as_str() != notice {
            text.0 = notice.to_owned();
        }
        let display = if notice.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
        if node.display != display {
            node.display = display;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{gltf_browser::EditorGltfMeshSelection, scene_context_menu::SceneObjectKey};
    use feathrise_editor_ui::NumericInputCommitSource;

    #[test]
    fn mesh_property_modes_preserve_other_overrides_and_custom_precision() {
        let mut properties = SourceMeshProperties::default();
        let follow = Mode::Height(Some(SourceMeshHeight::FollowTerrain { offset: 0.0 }));
        let display_custom = Mode::DisplayLod(Lod::Custom);
        let collision_custom = Mode::CollisionLod(Lod::Custom);
        for mode in [
            follow,
            Mode::Collision(Some(false)),
            display_custom,
            collision_custom,
        ] {
            mode.apply(&mut properties);
        }
        let offset = ValueField(Field::TranslationY);
        let display_error = ValueField(Field::LodScreenError);
        let collision_error = ValueField(Field::LodCollisionError);
        *offset.slot(&mut properties).unwrap() = 0.625;
        *display_error.slot(&mut properties).unwrap() = 3.5;
        *collision_error.slot(&mut properties).unwrap() = 0.0002;
        for mode in [follow, display_custom, collision_custom] {
            mode.apply(&mut properties);
        }
        assert_eq!(offset.value(properties), Some(0.625));
        assert_eq!(display_error.value(properties), Some(3.5));
        assert_eq!(collision_error.value(properties), Some(0.0002));

        Mode::Height(None).apply(&mut properties);
        Mode::DisplayLod(Lod::Global).apply(&mut properties);
        assert_eq!(properties.height, None);
        assert_eq!(properties.collision, Some(false));
        assert_eq!(properties.display_lod, Some(MeshLodMode::Inherit));
        assert_eq!(collision_error.value(properties), Some(0.0002));
        Mode::DisplayLod(Lod::Placement).apply(&mut properties);
        assert_eq!(properties.display_lod, None);
        assert_eq!(collision_error.value(properties), Some(0.0002));
    }

    #[test]
    fn source_node_numeric_blur_and_mode_release_share_one_frozen_request() {
        for (fast_click, switch_node, reload) in [
            (false, false, false), (true, false, false), (false, true, false), (false, false, true),
        ] {
            let (mut app, _, _) = crate::viewport::placement_move::interaction_tests::app();
            app.add_message::<NumericInputCommitted>()
                .add_message::<NumericInputSetValue>()
                .add_message::<SourceGlbRequest>();
            let world = app.world_mut();
            let session = world.resource::<EditorAuthoringSession>();
            let draft = session.draft.as_ref().unwrap();
            let path = draft.placements.placements[0].asset_path.clone();
            let mesh = EditorGltfMeshSelection {
                placement_id: draft.placements.placements[0].id.clone(),
                node_path: vec!["测试网格".into()],
                mesh_name: "mesh".into(),
                mesh_index: Some(0),
                node_index: Some(7),
                valid: true,
                orphaned_water_binding: false,
            };
            let target = SceneObjectTarget {
                scene_id: draft.scene_id.clone(),
                revision: session.revision,
                key: SceneObjectKey::Placement {
                    index: 0,
                    id: mesh.placement_id.clone(),
                    mesh: Some(mesh.clone()),
                },
                name: mesh.node_path[0].clone(),
                hidden: false,
                locked: false,
                copyable: false,
            };
            let mut browser = world.resource_mut::<EditorGltfBrowserState>();
            browser.selection = Some(mesh);
            browser.set_test_source_revision(path.clone(), 1);
            let mut properties = SourceMeshProperties::default();
            Mode::DisplayLod(Lod::Custom).apply(&mut properties);
            let snapshot = |world: &World, hash| Inspector::test_snapshot(
                target.clone(), Transform::IDENTITY, properties, hash,
                world.resource::<EditorGltfBrowserState>(), world.resource::<EditorAuthoringSession>(),
            );
            world.insert_resource(snapshot(world, "最初源版本"));
            let run_input = world.register_system(input);
            let offset = world.spawn(PoseControl(Field::TranslationY)).id();
            let display_error = world.spawn((ValueField(Field::LodScreenError), NumericInputValue(NumericValue::F32(1.5)))).id();
            let collision_button = world
                .spawn((Mode::Collision(Some(false)), EditorButtonClick::default()))
                .id();

            for (entity, value) in [(offset, 1.5), (display_error, 2.75)] {
                world.write_message(NumericInputCommitted {
                    entity,
                    value: NumericValue::F32(value),
                    outcome: NumericInputCommitOutcome::Accepted,
                    source: NumericInputCommitSource::Text,
                    changed: true,
                    edited: true,
                });
            }
            world
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Left);
            if !fast_click {
                world.run_system(run_input).unwrap();
                assert!(
                    world.resource::<Messages<SourceGlbRequest>>().is_empty(),
                    "按下时只冻结失焦提交"
                );
            }
            if switch_node {
                world
                    .resource_mut::<EditorGltfBrowserState>()
                    .selection
                    .as_mut()
                    .unwrap()
                    .node_index = Some(8);
            }
            if reload {
                world.resource_mut::<EditorGltfBrowserState>().set_test_source_revision(path, 2);
                assert!(world.resource::<Inspector>().selected_pose(
                    world.resource::<EditorGltfBrowserState>(), world.resource::<SceneEditorState>(),
                    world.resource::<EditorAuthoringSession>(),
                ).is_none(), "同节点重载当帧已经不能使用旧数值");
                let run_sync = world.register_system(sync);
                let mut loading = Inspector::default();
                loading.selection_changed = true;
                world.insert_resource(loading);
                world.resource_mut::<ActiveNumericInput>().entity = Some(display_error);
                world.run_system(run_sync).unwrap();
                let reset = world.resource_mut::<Messages<NumericInputSetValue>>().drain().next().unwrap();
                assert_eq!(reset.entity, display_error);
                assert_eq!(reset.value, NumericValue::F32(1.5));
                assert!(reset.cancel_edit, "新快照还没读到值也应取消旧文本");
                world.insert_resource(snapshot(world, "重载后版本"));
                world.resource_mut::<ActiveNumericInputScrub>().entity = Some(display_error);
                world.run_system(run_sync).unwrap();
                assert!(world.get::<NumericInputDisabled>(display_error).is_some(), "旧鼠标捕获结束前不得启用同一控件");
            }
            world
                .resource_mut::<ButtonInput<MouseButton>>()
                .release(MouseButton::Left);
            world
                .entity_mut(collision_button)
                .insert(EditorButtonClick::triggered());
            world.run_system(run_input).unwrap();
            if reload {
                assert!(world.resource::<Messages<SourceGlbRequest>>().is_empty(), "换代前的失焦和按钮释放均不得落到新版本");
                // 公开旧 SDK 可能仍在禁用根松手时发出 Accepted，编辑器必须独立拒绝该消息。
                world.resource_mut::<ActiveNumericInputScrub>().entity = None;
                world.write_message(NumericInputCommitted { entity: display_error, value: NumericValue::F32(9.0),
                    outcome: NumericInputCommitOutcome::Accepted, source: NumericInputCommitSource::Scrub, changed: true, edited: true });
                world.run_system(run_input).unwrap();
                assert!(world.resource::<Messages<SourceGlbRequest>>().is_empty());
                continue;
            }
            let mut messages = world.resource_mut::<Messages<SourceGlbRequest>>();
            assert_eq!(messages.len(), 1);
            let Some(SourceGlbRequest::NodeEdit {
                target: actual,
                expected_hash,
                pose: Some(pose),
                properties: Some(properties),
            }) = messages.drain().next()
            else {
                panic!("局部姿态和覆盖应在一笔请求交给源文件")
            };
            assert!(actual == target, "换选不能改派已确认的数值");
            assert_eq!(expected_hash, "最初源版本");
            assert_eq!(pose.translation.y, 1.5);
            assert_eq!(
                ValueField(Field::LodScreenError).value(properties),
                Some(2.75)
            );
            assert_eq!(properties.collision, (!switch_node).then_some(false));
        }
    }
}

struct PendingNodeInput {
    target: SceneObjectTarget,
    publication: SourcePublication,
    expected_hash: String,
    pose: Option<Transform>,
    properties: Option<SourceMeshProperties>,
}

fn input(
    editor: Res<SceneEditorState>,
    browser: Res<EditorGltfBrowserState>,
    session: Res<EditorAuthoringSession>,
    state: Res<Inspector>,
    guard: Res<SceneEditorNumericScrubMouseGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    buttons: Query<
        (&Mode, &EditorButtonClick),
        (Changed<EditorButtonClick>, Without<InteractionDisabled>),
    >,
    // 兼容仍可能在禁用根上发出旧松手提交的 SDK。
    fields: Query<&ValueField, Without<NumericInputDisabled>>,
    pose_fields: Query<&PoseControl, Without<NumericInputDisabled>>,
    mut pending: Local<Option<PendingNodeInput>>,
    mut commits: MessageReader<NumericInputCommitted>,
    mut requests: MessageWriter<SourceGlbRequest>,
) {
    let clicked = (!guard.blocked())
        .then(|| {
            buttons
                .iter()
                .find_map(|(mode, click)| click.is_clicked().then_some(*mode))
        })
        .flatten();
    if !session.source_glb_busy
        && (!commits.is_empty() || clicked.is_some())
        && let Some((target, before_pose, hash)) = state.selected_pose(&browser, &editor, &session)
        && let Some(publication) = source_publication(target, &browser, &session)
        && let Some(before_properties) = state.mesh_properties
        && pending
            .as_ref()
            .is_none_or(|pending| &pending.target == target && pending.expected_hash == hash && pending.publication == publication)
    {
        let mut pose = pending
            .as_ref()
            .and_then(|pending| pending.pose)
            .unwrap_or(before_pose);
        let mut properties = pending
            .as_ref()
            .and_then(|pending| pending.properties)
            .unwrap_or(before_properties);
        for event in commits.read() {
            if !event.edited || event.outcome != NumericInputCommitOutcome::Accepted {
                continue;
            }
            let Some(value) = event.value.as_f32().filter(|value| value.is_finite()) else {
                continue;
            };
            if let Ok(field) = pose_fields.get(event.entity) {
                field.apply(&mut pose, value);
            } else if let Ok(field) = fields.get(event.entity)
                && let Some(slot) = field.slot(&mut properties)
            {
                *slot = value;
            }
        }
        if let Some(mode) = clicked {
            mode.apply(&mut properties);
        }
        let policy = MeshLodPolicy {
            display: properties.display_lod.unwrap_or_default(),
            collision: properties.collision_lod.unwrap_or_default(),
        };
        let pose = (pose != before_pose).then_some(pose);
        let properties =
            (properties != before_properties && policy.validate().is_ok()).then_some(properties);
        *pending = (pose.is_some() || properties.is_some()).then(|| PendingNodeInput {
            target: target.clone(),
            publication,
            expected_hash: hash.to_owned(),
            pose,
            properties,
        });
    } else {
        commits.clear();
    }
    // 外部点击按下时数字框先失焦，松手后按钮才通过原捕获门。等该松手，
    // 使同节点的数值和模式只冻结一次，也保留切换选择前已确认的数值身份。
    if !mouse.pressed(MouseButton::Left)
        && let Some(PendingNodeInput {
            target,
            publication,
            expected_hash,
            pose,
            properties,
        }) = pending.take()
        && !session.source_glb_busy
        && target.current(&editor, &session)
        && source_publication(&target, &browser, &session) == Some(publication)
    {
        requests.write(SourceGlbRequest::NodeEdit {
            target,
            expected_hash,
            pose,
            properties,
        });
    }
}
