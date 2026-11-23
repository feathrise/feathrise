//! 将普通内置网格的选定实例变换烘焙到作者几何，并在显示与碰撞完备后交接。

use bevy::{
    ecs::system::SystemParam,
    math::{DAffine3, DVec3},
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, futures::check_ready},
    ui::InteractionDisabled,
};
use feathrise_editor_ui::{EditorButtonClick, InteractionButton, metrics as ui_metrics};
use feathrise_scene::ScenePlacementInstance;
use feathrise_scene_content::{SceneId, ScenePlacementHeightMode, ScenePlacementRecord};

use super::{PlacementTransformInputState, PlacementTransformTarget, selected_target};
use crate::{
    authoring::{
        EditorAuthoringSession, EditorCommand, EditorSceneDraft,
        placement_pose::{placement_rotation, set_placement_rotation},
    },
    components::EditorPlacedAsset,
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState},
    ui::{gltf_browser::EditorGltfBrowserState, scene::FONT_PATH, scene_motion::Workbench, theme},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) enum ApplyPart {
    #[default]
    Position,
    Rotation,
    Scale,
    All,
}

impl ApplyPart {
    pub(in crate::ui) const fn label(self) -> &'static str {
        match self {
            Self::Position => "位置",
            Self::Rotation => "旋转",
            Self::Scale => "缩放",
            Self::All => "全部变换",
        }
    }
}

#[derive(Component, Clone, Default)]
pub(crate) struct ApplyMenuButton(Option<ApplyPart>);

#[derive(Component, Clone, Default)]
pub(crate) struct ApplyMenuItems;

#[derive(Resource, Default)]
pub(crate) struct ApplyTransformState {
    menu: Option<PlacementTransformTarget>,
    pending: Option<Pending>,
}

struct Candidate {
    record: ScenePlacementRecord,
    transform: Transform,
}

enum Pending {
    Bake {
        target: PlacementTransformTarget,
        part: ApplyPart,
        task: Task<Result<Option<Candidate>, String>>,
    },
    Visual {
        target: PlacementTransformTarget,
        part: ApplyPart,
        candidate: Candidate,
        root: Entity,
        prepared: Option<Result<(), String>>,
    },
}

#[derive(SystemParam)]
pub(crate) struct ApplyGuards<'w> {
    browser: Res<'w, EditorGltfBrowserState>,
    transforms: Res<'w, PlacementTransformInputState>,
    numeric_mouse: Res<'w, SceneEditorNumericScrubMouseGuard>,
    batch: Res<'w, crate::authoring::placement_batch::PlacementBatchState>,
    selection: Res<'w, crate::viewport::multi_select::PlacementSelectionSet>,
    viewport_move: Res<'w, crate::viewport::placement_move::PlacementMoveState>,
    mesh_edit: Res<'w, crate::authoring::mesh_edit::MeshEditState>,
    animation: Res<'w, Workbench>,
}

impl ApplyGuards<'_> {
    fn target(
        &self,
        editor: &SceneEditorState,
        session: &EditorAuthoringSession,
    ) -> Option<PlacementTransformTarget> {
        if !editor.open
            || editor.close_requested
            || editor.new_scene_dialog_open
            || session.saving
            || session.model_vegetation_busy
            || session.vegetation_cross_busy
            || self.batch.busy()
            || self.transforms.transaction_active()
            || self.transforms.relocation_armed()
            || self.viewport_move.exit_pending()
            || self.selection.members.len() > 1
            || self.mesh_edit.active()
        {
            return None;
        }
        let target = selected_target(&self.browser, session)?;
        if self.animation.owns(&target.scene_id, &target.placement_id) {
            return None;
        }
        current_record(session, &target)
            .is_some_and(|(_, record)| {
                record.asset_target.is_builtin() && record.courtyard.is_none()
            })
            .then_some(target)
    }
}

fn current_record<'a>(
    session: &'a EditorAuthoringSession,
    target: &PlacementTransformTarget,
) -> Option<(&'a EditorSceneDraft, &'a ScenePlacementRecord)> {
    let draft = session.draft.as_ref()?;
    let record = draft.placements.placements.get(target.record_index)?;
    (session.revision == target.authoring_revision
        && session.placement_structure_revision == target.structure_revision
        && draft.scene_id == target.scene_id
        && record.id == target.placement_id)
        .then_some((draft, record))
}

fn button(label: &str, marker: impl Component + Clone + Unpin) -> impl Scene {
    bsn! {
        InteractionButton
        ~{marker}
        Node {
            width: percent(100), height: px(26), min_width: px(0), flex_shrink: 0.0,
            border: px(1), border_radius: BorderRadius::all(px(3)),
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
        }
        BackgroundColor(theme::BUTTON_DEV_SMALL_NORMAL)
        BorderColor::all(theme::COPPER_BORDER_WEAK)
        Children [
            Text(label) @feathrise_editor_ui::localization::label_scene(label)
            TextFont {
                font: bevy::text::FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
                font_size: px(ui_metrics::FONT_CONTROL),
            }
            TextColor(theme::TEXT_PRIMARY)
        ]
    }
}

pub(super) fn apply_menu() -> impl Scene {
    transform_apply_menu(ApplyMenuButton, ApplyMenuItems)
}

pub(in crate::ui) fn transform_apply_menu<T: Component + Clone + Unpin>(
    marker: impl Fn(Option<ApplyPart>) -> T,
    items: impl Component + Clone + Unpin,
) -> impl Scene {
    bsn! {
        Node { width: percent(100), flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0 }
        Children [
            @button("应用", marker(None))--
            ~{items}
            Node { display: Display::None, width: percent(100), flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0 }
            Children [
                @button(ApplyPart::Position.label(), marker(Some(ApplyPart::Position)))--
                @button(ApplyPart::Rotation.label(), marker(Some(ApplyPart::Rotation)))--
                @button(ApplyPart::Scale.label(), marker(Some(ApplyPart::Scale)))--
                @button(ApplyPart::All.label(), marker(Some(ApplyPart::All)))
            ]
        ]
    }
}

/// 输入阶段只冻结这一条记录。GLB 目录、顶点量化和重新拓扑校验均由同一个后台任务完成。
pub(crate) fn handle_apply_transform(
    buttons: Query<(&EditorButtonClick, &ApplyMenuButton), Changed<EditorButtonClick>>,
    keys: Res<ButtonInput<KeyCode>>,
    guards: ApplyGuards,
    mut editor: ResMut<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    mut state: ResMut<ApplyTransformState>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        state.menu = None;
    }
    let Some(action) = buttons
        .iter()
        .find_map(|(click, button)| click.is_clicked().then_some(button.0))
    else {
        return;
    };
    if state.pending.is_some() || guards.numeric_mouse.blocked() {
        return;
    }
    let Some(target) = guards.target(&editor, &session) else {
        return;
    };
    let Some(part) = action else {
        state.menu = (state.menu.as_ref() != Some(&target)).then_some(target);
        return;
    };
    if state.menu.take().as_ref() != Some(&target) {
        return;
    }
    let Some((draft, record)) = current_record(&session, &target) else {
        return;
    };
    let work = (|| {
        if draft.motion.get(&record.id).is_some() {
            return Err("此对象已有场景动画关键帧".to_owned());
        }
        let bounds = draft.manifest.world_bounds().ok_or("场景世界范围无效")?;
        let path = if draft.placement_glb_hash.is_some() && !draft.motion.clip_names().is_empty() {
            Some(feathrise_editor_assets::files::safe_asset_path(
                &crate::authoring::workspace_asset_root(),
                &draft.placements_path(),
            )?)
        } else {
            None
        };
        let before = record.clone();
        let scene = target.scene_id.clone();
        Ok(move || {
            if let Some(path) = path {
                let glb = feathrise_editor_assets::glb_metadata::read_directory(&path)?;
                if has_animation_channels(&glb, &before.id) {
                    return Err("此对象已绑定到场景动画".to_owned());
                }
            }
            bake_record(&before, &scene, bounds, part)
        })
    })();
    match work {
        Ok(work) => {
            let task = AsyncComputeTaskPool::get().spawn(async move { work() });
            state.pending = Some(Pending::Bake { target, part, task });
            editor.status = format!("正在应用{}…", part.label());
        }
        Err(error) => editor.status = format!("应用变换失败：{error}"),
    }
}

fn has_animation_channels(glb: &serde_json::Value, id: &str) -> bool {
    let Some(nodes) = glb["nodes"].as_array() else {
        return false;
    };
    glb["animations"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|clip| clip["channels"].as_array().into_iter().flatten())
        .filter_map(|channel| usize::try_from(channel["target"]["node"].as_u64()?).ok())
        .any(|index| {
            nodes
                .get(index)
                .is_some_and(|node| node["extras"]["feathrise_id"].as_str() == Some(id))
        })
}

fn bake_record(
    before: &ScenePlacementRecord,
    scene: &SceneId,
    bounds: Rect,
    part: ApplyPart,
) -> Result<Option<Candidate>, String> {
    let mut record = before.clone();
    if matches!(part, ApplyPart::Position | ApplyPart::All) && record.translation != [0.0; 3] {
        if record.height_mode == ScenePlacementHeightMode::FollowTerrain {
            return Err("请先将跟随地形的对象设为固定高度，再应用位置".into());
        }
        record.translation = [0.0; 3];
    }
    let original_rotation = placement_rotation(before);
    if matches!(part, ApplyPart::Rotation | ApplyPart::All) {
        set_placement_rotation(&mut record, Quat::IDENTITY)?;
    }
    if matches!(part, ApplyPart::Scale | ApplyPart::All) {
        record.scale = [1.0; 3];
    }
    if record == *before {
        return Ok(None);
    }

    // 对非均匀缩放也必须用完整仿射矩阵。保留变换乘回去后，网格仍占据原来的世界位置。
    let matrix = |record: &ScenePlacementRecord, rotation: Quat| {
        DAffine3::from_scale_rotation_translation(
            Vec3::from_array(record.scale).as_dvec3(),
            rotation.as_dquat().normalize(),
            Vec3::from_array(record.translation).as_dvec3(),
        )
    };
    let bake =
        matrix(&record, placement_rotation(&record)).inverse() * matrix(before, original_rotation);
    let geometry = before
        .asset_target
        .editable_geometry()
        .ok_or("当前网格没有可应用的作者几何")?;
    let mut vertices = Vec::with_capacity(geometry.vertex_count());
    for vertex in geometry.vertices() {
        let point = DVec3::from_array(vertex.map(f64::from)) * 0.0005;
        let value = (bake.transform_point3(point) * 2000.0).round();
        if !value.is_finite()
            || value.min_element() < f64::from(i32::MIN)
            || value.max_element() > f64::from(i32::MAX)
        {
            return Err("应用后的顶点超过可保存的坐标范围".into());
        }
        vertices.push(value.to_array().map(|coordinate| coordinate as i32));
    }
    if vertices != geometry.vertices() {
        record.asset_target = before
            .asset_target
            .with_editable_geometry(geometry.with_vertices(vertices)?)?;
    }
    let transform = record
        .validated_transform(scene, bounds)?
        .with_rotation(placement_rotation(&record));
    Ok(Some(Candidate { record, transform }))
}

/// 候选使用隔离的场景身份，准备期间既不会抢占真实根，也不会被选择或参与碰撞。
fn stage_visual(commands: &mut Commands, candidate: &Candidate) -> Entity {
    let root = commands
        .spawn((
            Name::new("应用变换候选"),
            ScenePlacementInstance::new(
                &SceneId("editor-apply-transform".to_owned()),
                &candidate.record,
            ),
            candidate.transform,
            Visibility::Hidden,
            avian3d::prelude::RigidBody::Static,
        ))
        .id();
    feathrise_scene::set_builtin_placement_batch_enabled(commands, root, false);
    feathrise_scene::spawn_builtin_placement_visual(
        commands,
        root,
        candidate.record.asset_target.clone(),
    );
    root
}

pub(crate) fn observe_apply_transform_visual(
    prepared: On<feathrise_scene::BuiltinVisualPrepared>,
    mut state: ResMut<ApplyTransformState>,
) {
    if let Some(Pending::Visual {
        root,
        prepared: result,
        ..
    }) = state.pending.as_mut()
        && *root == prepared.entity
    {
        *result = Some(prepared.result.clone());
    }
}

pub(crate) fn poll_apply_transform(
    mut commands: Commands,
    guards: ApplyGuards,
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut state: ResMut<ApplyTransformState>,
    roots: Query<(&ScenePlacementInstance, Has<EditorPlacedAsset>)>,
) {
    let Some(pending) = state.pending.as_ref() else {
        return;
    };
    let target = match pending {
        Pending::Bake { target, .. } | Pending::Visual { target, .. } => target,
    };
    let current = guards.target(&editor, &session).as_ref() == Some(target);
    match state.pending.as_mut().expect("前面已确认任务存在") {
        Pending::Bake { task, .. } => {
            let Some(result) = check_ready(task) else {
                return;
            };
            let Some(Pending::Bake { target, part, .. }) = state.pending.take() else {
                unreachable!()
            };
            if !current {
                AsyncComputeTaskPool::get()
                    .spawn(async move {
                        drop(result);
                    })
                    .detach();
                return;
            }
            match result {
                Ok(Some(candidate)) => {
                    let root = stage_visual(&mut commands, &candidate);
                    state.pending = Some(Pending::Visual {
                        target,
                        part,
                        candidate,
                        root,
                        prepared: None,
                    });
                }
                Ok(None) => editor.status = format!("{}已经是默认值", part.label()),
                Err(error) => editor.status = format!("应用变换失败：{error}"),
            }
        }
        Pending::Visual { prepared, root, .. } => {
            if current && prepared.is_none() && roots.contains(*root) {
                return;
            }
            let Some(Pending::Visual {
                target,
                part,
                candidate,
                root,
                prepared,
            }) = state.pending.take()
            else {
                unreachable!()
            };
            if !current || !matches!(prepared, Some(Ok(()))) || !roots.contains(root) {
                commands.entity(root).try_despawn();
                if current {
                    let error = prepared
                        .and_then(Result::err)
                        .unwrap_or_else(|| "候选网格已经退出".into());
                    editor.status = format!("应用变换失败：{error}");
                }
                return;
            }
            // 根身份、正式数据和历史只在同一发布边沿变化，旧有模型始终显示到这里。
            let before = current_record(&session, &target)
                .expect("采用前已核对作者身份")
                .1
                .clone();
            let after = candidate.record;
            if let Some(old) = session.placement_previews.insert(after.id.clone(), root)
                && roots.get(old).is_ok_and(|(instance, author)| {
                    author && instance.scene_id() == &target.scene_id && instance.id() == after.id
                })
            {
                commands.entity(old).try_despawn();
            }
            commands
                .entity(root)
                .remove::<ScenePlacementInstance>()
                .insert((
                    EditorPlacedAsset,
                    Name::new(format!("未保存场景实例_{}", after.id)),
                    ScenePlacementInstance::new(&target.scene_id, &after),
                    Visibility::Inherited,
                ));
            feathrise_scene::set_builtin_placement_batch_enabled(&mut commands, root, true);
            feathrise_scene::set_builtin_placement_collision_enabled(&mut commands, root, true);
            crate::authoring::placement_preview_ownership::claim_preview(
                &mut commands,
                &target.scene_id,
                &after.id,
                root,
            );

            session
                .draft
                .as_mut()
                .expect("采用前已核对场景")
                .placements
                .placements[target.record_index] = after.clone();
            session.push_bounded_water_history(EditorCommand::PlacementChanged {
                record_index: target.record_index,
                before,
                after,
            });
            session.mark_changed(&mut editor);
            editor.status = format!("已应用{}", part.label());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use feathrise_scene_content::{ScenePlacementAssetTarget, ScenePlacementsAsset};

    fn record() -> ScenePlacementRecord {
        serde_json::from_value(serde_json::json!({
            "id": "apply-mesh", "asset_target": ScenePlacementAssetTarget::Cuboid {
                size_mm: [2000, 3000, 4000], color_srgb: [21, 82, 170], roughness_milli: 416, collision: true
            },
            "editor": {"name": "平台", "group": "网格组"},
            "translation": [2.5, 1.25, -9.0], "yaw_degrees": 38.0, "scale": [1.7, 0.6, 2.4]
        })).unwrap()
    }

    fn assert_geometry_preserved(before: &ScenePlacementRecord, after: &ScenePlacementRecord) {
        let world_points = |record: &ScenePlacementRecord| {
            record
                .asset_target
                .editable_geometry()
                .unwrap()
                .vertices()
                .iter()
                .map(|point| {
                    Vec3::from_array(record.translation)
                        + placement_rotation(record)
                            * (Vec3::from_array(point.map(|value| value as f32 * 0.0005))
                                * Vec3::from_array(record.scale))
                })
                .collect::<Vec<_>>()
        };
        let points = world_points(before);
        let baked = world_points(after);
        // 每一轴最多半个半毫米刻度，误差只可能被保留的缩放放大。
        let tolerance = Vec3::from_array(after.scale).length() * 0.00025 + 1e-5;
        assert_eq!(points.len(), baked.len());
        for (original, new) in points.into_iter().zip(baked) {
            assert!(
                original.distance(new) <= tolerance,
                "{original:?} != {new:?}"
            );
        }
    }

    #[test]
    fn each_part_clears_its_fields_and_keeps_nonuniform_world_geometry_on_reload() {
        let scene = SceneId("SCENE-apply-transform".into());
        let before = record();
        for part in [
            ApplyPart::Position,
            ApplyPart::Rotation,
            ApplyPart::Scale,
            ApplyPart::All,
        ] {
            let result = bake_record(&before, &scene, Rect::default(), part)
                .unwrap()
                .unwrap()
                .record;
            assert_eq!(
                result.translation,
                if matches!(part, ApplyPart::Position | ApplyPart::All) {
                    [0.0; 3]
                } else {
                    before.translation
                }
            );
            assert_eq!(
                placement_rotation(&result),
                if matches!(part, ApplyPart::Rotation | ApplyPart::All) {
                    Quat::IDENTITY
                } else {
                    placement_rotation(&before)
                }
            );
            assert_eq!(
                result.scale,
                if matches!(part, ApplyPart::Scale | ApplyPart::All) {
                    [1.0; 3]
                } else {
                    before.scale
                }
            );
            assert_eq!(result.id, before.id);
            assert_eq!(result.editor, before.editor);
            assert_geometry_preserved(&before, &result);

            let path = "scenes/SCENE-apply-transform/scene.glb";
            let placements = ScenePlacementsAsset {
                scene_id: scene.clone(),
                placements: vec![result.clone()],
                ..default()
            };
            let bytes =
                feathrise_scene_content::write_scene_placements_glb(&placements, path, None)
                    .unwrap();
            let reloaded = feathrise_scene_content::read_scene_placements(&bytes, path).unwrap();
            let reloaded = &reloaded.placements[0];
            assert_eq!(
                reloaded.asset_target, result.asset_target,
                "材质和碰撞随几何保存"
            );
            assert_geometry_preserved(&before, reloaded);
        }
    }

    #[test]
    fn position_requires_fixed_height_and_identity_needs_no_candidate() {
        let scene = SceneId("SCENE-apply-transform".into());
        let mut before = record();
        before.height_mode = ScenePlacementHeightMode::FollowTerrain;
        assert!(bake_record(&before, &scene, Rect::default(), ApplyPart::Position).is_err());
        assert!(bake_record(&before, &scene, Rect::default(), ApplyPart::Rotation).is_ok());
        before.scale = [1.0; 3];
        assert!(
            bake_record(&before, &scene, Rect::default(), ApplyPart::Scale)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn all_saved_clips_are_considered_when_protecting_animated_targets() {
        let value = serde_json::json!({
            "nodes": [{"extras": {"feathrise_id":"mesh-a"}}, {"extras": {"feathrise_id":"mesh-b"}}],
            "animations": [
                {"channels": [{"target": {"node": 0, "path":"rotation"}}]},
                {"channels": [{"target": {"node": 1, "path":"scale"}}]}
            ]
        });
        assert!(has_animation_channels(&value, "mesh-a"));
        assert!(has_animation_channels(&value, "mesh-b"));
        assert!(!has_animation_channels(&value, "mesh-c"));
    }
}

pub(crate) fn sync_apply_menu(
    mut commands: Commands,
    guards: ApplyGuards,
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    mut state: ResMut<ApplyTransformState>,
    mut items: Query<&mut Node, With<ApplyMenuItems>>,
    buttons: Query<(Entity, &ApplyMenuButton, Has<InteractionDisabled>)>,
) {
    if items.is_empty() {
        return;
    }
    let target = guards.target(&editor, &session);
    if state.menu.is_some() && state.menu != target {
        state.menu = None;
    }
    let enabled = target.is_some() && state.pending.is_none();
    let open = enabled && state.menu.is_some();
    let display = if open { Display::Flex } else { Display::None };
    for mut node in &mut items {
        if node.display != display {
            node.display = display;
        }
    }
    for (entity, button, disabled) in &buttons {
        let available = if button.0.is_none() { enabled } else { open };
        if available == disabled {
            if available {
                commands.entity(entity).remove::<InteractionDisabled>();
            } else {
                commands.entity(entity).insert(InteractionDisabled);
            }
        }
    }
}
