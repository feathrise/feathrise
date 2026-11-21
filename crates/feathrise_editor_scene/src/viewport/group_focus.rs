//! 逻辑组定位复用单对象的活动根和世界包围盒，分帧完成整组取景。

use bevy::{
    camera::{primitives::Aabb, visibility::NoAutoAabb},
    ecs::{entity_disabling::Disabled, system::SystemParam},
    prelude::*,
    world_serialization::{InstanceId, WorldAssetRoot, WorldInstance, WorldInstanceSpawner},
};
use feathrise_scene::{ScenePlacementInstance, ScenePlacementRuntimePhase, ScenePlacementRuntimeStatus};
use feathrise_scene_content::SceneId;

use super::{
    camera_math::framing_extent,
    framing::framing_available,
    placement_selection::{PlacementWorldBounds, resolve_active_root, transformed_placement_bounds},
};
use crate::{
    authoring::{EditorAuthoringSession, WaterNetworkEditState, placement_deletion::PlacementDeletionIdentity},
    components::SceneEditorSelectedPlacementHighlight,
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState, SceneEditorViewportState},
    ui::{
        gltf_browser::EditorGltfBrowserState,
        placement_water::PlacementInstanceIndex,
        scene_objects::{SceneObjectState, effective_metadata, effective_participation},
        vegetation_preset_editor::VegetationPresetEditorState,
    },
};

const GROUP_VISITS_PER_FRAME: usize = 256; // 成员记录、层级节点和返回上级各占一次预算。

/// 一次显式聚焦占一个可替换的槽。稳定帧无需计算组范围。
#[derive(Resource, Default)]
pub(crate) struct GroupFocusState(Option<GroupScan>);

struct GroupScan {
    scene: SceneId,
    group: String,
    document: PlacementDeletionIdentity,
    revision: u64,
    terrain_revision: u64,
    pose_revision: u64,
    camera: (Vec3, f32, f32, f32, UVec2, bool),
    geometry_revision: (u64, u64),
    cursor: usize,
    branch: Option<(Entity, Option<InstanceId>)>,
    stack: Vec<(Entity, usize)>,
    bounds: Option<PlacementWorldBounds>,
}

fn camera(view: &SceneEditorViewportState, orthographic: bool) -> (Vec3, f32, f32, f32, UVec2, bool) {
    (view.center, view.extent, view.yaw, view.pitch, view.render_size, orthographic)
}

impl GroupFocusState {
    pub(super) fn request(&mut self, group: &str, scene: &SceneId, session: &EditorAuthoringSession,
        view: &SceneEditorViewportState, orthographic: bool) {
        self.0 = Some(GroupScan {
            scene: scene.clone(), group: group.to_owned(),
            document: session.placement_deletions.identity(),
            revision: session.revision, terrain_revision: session.terrain_revision,
            pose_revision: session.placement_derived_pose_revision,
            camera: camera(view, orthographic),
            geometry_revision: (0, 0), cursor: 0, branch: None, stack: Vec::new(), bounds: None,
        });
    }

    pub(crate) fn cancel(&mut self) { self.0 = None; }
}

#[derive(SystemParam)]
pub(crate) struct GroupGeometry<'w, 's> {
    index: Res<'w, PlacementInstanceIndex>,
    runtime: Option<Res<'w, ScenePlacementRuntimeStatus>>,
    spawner: Option<Res<'w, WorldInstanceSpawner>>,
    roots: Query<'w, 's, (Entity, &'static ScenePlacementInstance)>,
    instances: Query<'w, 's, &'static WorldInstance>,
    nodes: Query<'w, 's, (
        Option<&'static Children>, Option<&'static Visibility>, Option<&'static InheritedVisibility>,
        Option<&'static GlobalTransform>, Option<&'static Aabb>,
        Has<Mesh3d>, Has<WorldAssetRoot>, Has<NoAutoAabb>,
    ), (Without<Disabled>, Without<SceneEditorSelectedPlacementHighlight>)>,
}

impl GroupScan {
    fn restart(&mut self, revision: (u64, u64)) {
        self.geometry_revision = revision;
        self.cursor = 0;
        self.branch = None;
        self.stack.clear();
        self.bounds = None; // 运行时换代则之前已扫描成员的范围也重新验收。
    }

    fn advance(&mut self, session: &EditorAuthoringSession, objects: Option<&SceneObjectState>,
        geometry: &GroupGeometry) -> Result<bool, String> {
        let records = &session.draft.as_ref().expect("已验收组所属草稿").placements.placements;
        let runtime = geometry.runtime.as_ref().filter(|runtime| runtime.scene_id.as_ref() == Some(&self.scene));
        let revision = (geometry.index.membership_revision(), runtime.map_or(0, |runtime| runtime.revision));
        if self.geometry_revision != revision
            || self.branch.is_some_and(|(root, ticket)| {
                geometry.instances.get(root).ok().map(|instance| **instance) != ticket
            })
        {
            self.restart(revision);
        }
        for _ in 0..GROUP_VISITS_PER_FRAME {
            if let Some((entity, child_index)) = self.stack.last_mut() {
                // 每个节点首次进入时取真实世界 Aabb，随后按游标访问孩子，避免大层级同步展开。
                let Ok((children, visibility, inherited, transform, aabb, mesh, world_root, no_auto_aabb)) =
                    geometry.nodes.get(*entity) else { self.stack.pop(); continue; };
                let nested_world = self.branch.is_some_and(|(root, _)| root != *entity) && world_root;
                if nested_world || visibility == Some(&Visibility::Hidden) || inherited.is_some_and(|state| !state.get()) {
                    self.stack.pop();
                    continue;
                }
                if *child_index == 0 && mesh {
                    match (transform, aabb) {
                        (Some(transform), Some(aabb)) => {
                            let bounds = transformed_placement_bounds(transform, aabb);
                            let frame = bounds.frame();
                            if frame.center.is_finite() && frame.size.is_finite() {
                                self.bounds = Some(self.bounds.map_or(bounds, |old| old.merge(bounds)));
                            }
                        }
                        _ if !no_auto_aabb => return Ok(false), // 下一帧可见网格完成 Aabb 后继续。
                        _ => {},
                    }
                }
                if let Some(child) = children.and_then(|list| list.get(*child_index)).copied() {
                    *child_index += 1;
                    self.stack.push((child, 0));
                } else {
                    self.stack.pop();
                }
                continue;
            }
            self.branch = None;
            let Some(record) = records.get(self.cursor) else { return Ok(true); };
            let metadata = effective_metadata(objects, session, record);
            if metadata.group != self.group || metadata.hidden || !effective_participation(objects, session, record) {
                self.cursor += 1;
                continue;
            } // 锁定只限制编辑，仍可对可见锁定成员取景。
            let Some((root, _)) = resolve_active_root(&self.scene, &record.id, session, &geometry.index, &geometry.roots)? else {
                if runtime.is_some_and(|state| state.phase == ScenePlacementRuntimePhase::Failed) {
                    return Err("组内对象加载失败".into());
                }
                return Ok(false);
            };
            let ticket = geometry.instances.get(root).ok().map(|instance| **instance);
            let scene_instance = geometry.roots.get(root).is_ok_and(|(_, instance)| instance.asset_target().is_scene0());
            if scene_instance && !geometry.spawner.as_ref().zip(ticket).is_some_and(|(spawner, id)| spawner.instance_is_ready(id)) {
                return Ok(false); // 完整 GLB 在 World 齐备前不能用部分后代取景。
            }
            self.branch = Some((root, ticket));
            self.stack.push((root, 0));
            self.cursor += 1;
        }
        Ok(false)
    }
}

/// 仅在正式组选择、作者身份和相机都与请求一致时采用完整范围。
pub(crate) fn frame_group_selection(
    mut state: ResMut<GroupFocusState>,
    mut editor: ResMut<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    viewport: Option<ResMut<SceneEditorViewportState>>,
    guard: Res<SceneEditorNumericScrubMouseGuard>,
    water_edit: Res<WaterNetworkEditState>,
    workspace: Res<VegetationPresetEditorState>,
    browser: Res<EditorGltfBrowserState>,
    objects: Option<Res<SceneObjectState>>,
    geometry: GroupGeometry,
) {
    if state.0.is_none() { return; }
    let Some(mut scan) = state.0.take() else { return; };
    let Some(mut view) = viewport else { return; };
    if !framing_available(&editor, &guard, &water_edit, &workspace)
        || session.saving || objects.as_ref().is_some_and(|objects| objects.busy())
        || editor.selected_scene_id.as_ref() != Some(&scan.scene)
        || session.draft.as_ref().is_none_or(|draft| draft.scene_id != scan.scene)
        || view.framed_scene_id.as_ref() != Some(&scan.scene)
        || browser.selected_group.as_deref() != Some(scan.group.as_str())
        || browser.selected_placement_id.is_some()
        || !scan.document.matches(&session.placement_deletions)
        || scan.revision != session.revision || scan.terrain_revision != session.terrain_revision
        || scan.pose_revision != session.placement_derived_pose_revision
        || scan.camera != camera(&view, editor.orthographic)
    {
        return; // 切换对象或主动导航时作废，旧请求不能随后夺回镜头。
    }
    match scan.advance(&session, objects.as_deref(), &geometry) {
        Ok(false) => state.0 = Some(scan),
        Ok(true) => {
            if let Some(bounds) = scan.bounds {
                let frame = bounds.frame();
                view.center = frame.center;
                view.extent = framing_extent(frame.size * 0.5, view.yaw, view.pitch, view.render_size, editor.orthographic);
                editor.status = format!("已聚焦组「{}」", scan.group);
            } else {
                editor.status = format!("组「{}」没有可定位的可见对象", scan.group);
            }
        }
        Err(reason) => editor.status = format!("无法聚焦组「{}」：{reason}", scan.group),
    }
}
