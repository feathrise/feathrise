//! 外部 GLB 源节点使用场景原手柄。预览只持有当前实体，松手提交源局部姿势。

use super::{
    input::MoveInputContext,
    math::{MoveCamera, handle_point, handle_translation_mode},
    resources::PlacementMoveState,
    types::MoveHandle,
};
use crate::{
    SelectedPlacementViewportState,
    authoring::EditorAuthoringSession,
    resources::{SceneEditorState, SceneEditorViewportState},
    ui::{
        gltf_browser::EditorGltfBrowserState,
        scene_context_menu::{SceneObjectKey, SceneObjectTarget},
        scene_source_glb::{SourceGlbRequest, SourceNodeInspector},
    },
    viewport::modes::ViewportMode,
};
use bevy::{ecs::system::SystemParam, prelude::*, ui::RelativeCursorPosition};
use feathrise_editor_animation_model::types::EmbeddedNodeIndex;
use feathrise_editor_core::EditorSnapSettings;

pub(super) struct SourceNodeDisplay {
    placement_id: String,
    node_index: usize,
    revision: u64,
    pub(super) origin: Vec3,
}

impl SourceNodeDisplay {
    pub(super) fn current(&self, browser: &EditorGltfBrowserState, revision: u64) -> bool {
        self.revision == revision
            && browser.selection.as_ref().is_some_and(|selection| {
                selection.placement_id == self.placement_id
                    && selection.node_index == Some(self.node_index)
            })
    }
}

#[derive(Clone, Copy, PartialEq)]
struct NodeIdentity {
    entity: Entity,
    parent: Entity,
    index: usize,
    world_from_parent: Mat4,
}

pub(super) struct SourceNodeDrag {
    source: SceneObjectTarget,
    source_hash: String,
    identity: NodeIdentity,
    camera: MoveCamera,
    mode: ViewportMode,
    pub(super) handle: MoveHandle,
    ui_size: Vec2,
    world_origin: Vec3,
    start_cursor: Vec2,
    start_point: Vec3,
    snapping: EditorSnapSettings,
    source_before: Transform,
    visual_before: Transform,
    applied: Transform,
}

impl SourceNodeDrag {
    pub(super) fn entity(&self) -> Entity {
        self.identity.entity
    }
}

#[derive(SystemParam)]
pub(crate) struct SourceNodeWorld<'w, 's> {
    inspector: Option<Res<'w, SourceNodeInspector>>,
    selected: Option<Res<'w, SelectedPlacementViewportState>>,
    parents: Query<'w, 's, &'static ChildOf>,
    globals: Query<'w, 's, &'static GlobalTransform>,
    nodes: Query<'w, 's, (&'static EmbeddedNodeIndex, &'static mut Transform)>,
    requests: Option<ResMut<'w, Messages<SourceGlbRequest>>>,
}

impl SourceNodeWorld<'_, '_> {
    fn current_node(
        &self,
        browser: &EditorGltfBrowserState,
        editor: &SceneEditorState,
        session: &EditorAuthoringSession,
        viewport: &SceneEditorViewportState,
    ) -> Option<(&SceneObjectTarget, NodeIdentity, Transform, Transform, &str)> {
        let (source, pose, source_hash) = self
            .inspector
            .as_ref()?
            .selected_pose(browser, editor, session)?;
        if viewport.framed_scene_id.as_ref() != Some(&source.scene_id) {
            return None;
        }
        let SceneObjectKey::Placement {
            mesh: Some(mesh), ..
        } = &source.key
        else {
            return None;
        };
        let primitive = self
            .selected
            .as_ref()?
            .exact_primitive(&source.scene_id, mesh)?;
        let entity = self.parents.get(primitive).ok()?.parent();
        let parent = self.parents.get(entity).ok()?.parent();
        let world_from_parent = self.globals.get(parent).ok()?.to_matrix();
        let (marker, transform) = self.nodes.get(entity).ok()?;
        if mesh.node_index != Some(marker.0) || !world_from_parent.inverse().is_finite() {
            return None;
        }
        Some((
            source,
            NodeIdentity {
                entity,
                parent,
                index: marker.0,
                world_from_parent,
            },
            pose,
            *transform,
            source_hash,
        ))
    }

    fn restore(&mut self, drag: &SourceNodeDrag) {
        if self
            .parents
            .get(drag.identity.entity)
            .is_ok_and(|parent| parent.parent() == drag.identity.parent)
            && let Ok((marker, mut pose)) = self.nodes.get_mut(drag.identity.entity)
            && marker.0 == drag.identity.index
            && *pose == drag.applied
        {
            *pose = drag.visual_before; // 热更、重父级或其他写侧已经接管时不恢复旧姿势。
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn update(
        &mut self,
        state: &mut PlacementMoveState,
        editor: &mut SceneEditorState,
        session: &EditorAuthoringSession,
        context: &MoveInputContext,
        mouse: &ButtonInput<MouseButton>,
        keyboard: &mut ButtonInput<KeyCode>,
        available: bool,
        blocked: bool,
        viewport: Option<&SceneEditorViewportState>,
        camera: Option<MoveCamera>,
        node: Option<(&RelativeCursorPosition, &ComputedNode)>,
    ) {
        let mode = context.mode.mode;
        let ready = available
            && context.local_gizmos()
            && mode.accepts_transform(ViewportMode::Move)
            && !session.source_glb_busy;
        let view = viewport.zip(camera).zip(node);
        if state.source_drag.is_none() && state.owns_mouse {
            keyboard.clear();
        }
        if let Some(mut drag) = state.source_drag.take() {
            let escaped = keyboard.just_pressed(KeyCode::Escape)
                || mouse.any_pressed([MouseButton::Right, MouseButton::Middle]);
            keyboard.clear();
            let update = (!escaped && ready && mode == drag.mode)
                .then_some(view)
                .flatten()
                .and_then(|((viewport, camera), (cursor, node))| {
                    if camera != drag.camera || node.size() != drag.ui_size {
                        return None;
                    }
                    let (source, identity, _, pose, source_hash) =
                        self.current_node(&context.browser, editor, session, viewport)?;
                    (source == &drag.source && source_hash == drag.source_hash.as_str()
                        && identity == drag.identity && pose == drag.applied)
                        .then_some(cursor.normalized)
                        .flatten()
                })
                .and_then(|cursor| {
                    let world = drag.position(
                        cursor,
                        keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]),
                    )?;
                    let delta = drag.local_delta(world);
                    ((drag.visual_before.translation + delta).is_finite()
                        && (drag.source_before.translation + delta).is_finite())
                    .then_some((world, delta))
                });
            let Some((world, delta)) = update else {
                self.restore(&drag);
                state.source_display = None;
                editor.status = "已取消源 GLB 节点移动".into();
                return;
            };
            if mouse.pressed(MouseButton::Left) {
                drag.applied.translation = drag.visual_before.translation + delta;
                if let Ok((_, mut pose)) = self.nodes.get_mut(drag.identity.entity) {
                    *pose = drag.applied;
                }
                state.hover = Some(drag.handle);
                if let Some(display) = state.source_display.as_mut() {
                    display.origin = world;
                }
                state.source_drag = Some(drag);
            } else {
                self.restore(&drag);
                if let Some(display) = state.source_display.as_mut() {
                    display.origin = drag.world_origin;
                }
                if mouse.just_released(MouseButton::Left)
                    && delta != Vec3::ZERO
                    && let Some(requests) = self.requests.as_mut()
                {
                    let mut pose = drag.source_before;
                    pose.translation += delta;
                    requests.write(SourceGlbRequest::NodeEdit {
                        target: drag.source,
                        expected_hash: drag.source_hash,
                        pose: Some(pose),
                        properties: None,
                    });
                    editor.status = "正在准备源 GLB 节点的局部姿势".into();
                }
            }
            return;
        }
        let Some(((viewport, camera), (cursor, node))) = view.filter(|_| ready) else {
            state.source_display = None;
            return;
        };
        let Some((source, identity, source_before, visual_before, source_hash)) =
            self.current_node(&context.browser, editor, session, viewport)
        else {
            state.source_display = None;
            return;
        };
        let world_origin = identity
            .world_from_parent
            .transform_point3(visual_before.translation);
        if let Some(display) = state
            .source_display
            .as_mut()
            .filter(|display| display.current(&context.browser, session.revision))
        {
            display.origin = world_origin;
        } else if let SceneObjectKey::Placement { id, .. } = &source.key {
            state.source_display = Some(SourceNodeDisplay {
                origin: world_origin,
                revision: session.revision,
                placement_id: id.clone(),
                node_index: identity.index,
            });
        }
        if blocked || state.owns_mouse || context.pointer_over_ui() || !cursor.cursor_over() {
            return;
        }
        let Some(start_cursor) = cursor.normalized else {
            return;
        };
        let Some(handle) = camera.pick_mesh_move(world_origin, start_cursor, false) else {
            return;
        };
        state.hover = Some(handle);
        if !mouse.just_pressed(MouseButton::Left)
            || mouse.any_pressed([MouseButton::Right, MouseButton::Middle])
        {
            return;
        }
        let Some(start_point) = camera
            .ray(start_cursor)
            .and_then(|ray| handle_point(ray, world_origin, handle))
        else {
            return;
        };
        // 快照同时绑定源文件选择、活动实体代次、父矩阵和相机。其他鼠标入口沿用同一静默门。
        state.capture_mouse();
        state.source_drag = Some(SourceNodeDrag {
            source: source.clone(),
            source_hash: source_hash.to_owned(),
            identity,
            camera,
            mode,
            handle,
            ui_size: node.size(),
            world_origin,
            start_cursor,
            start_point,
            snapping: context.snap_settings(),
            source_before,
            visual_before,
            applied: visual_before,
        });
    }
}

impl SourceNodeDrag {
    fn local_delta(&self, position: Vec3) -> Vec3 {
        // 约束和吸附发生在世界坐标。逆父矩阵只换算位移，避免改变节点已有的局部旋转和缩放。
        self.identity
            .world_from_parent
            .inverse()
            .transform_vector3(position - self.world_origin)
    }

    fn position(&self, cursor: Vec2, control: bool) -> Option<Vec3> {
        let moved = ((cursor - self.start_cursor) * self.camera.size * self.camera.inverse_scale)
            .length_squared()
            > 4.0;
        if !moved {
            return Some(self.world_origin);
        }
        let point = handle_point(self.camera.ray(cursor)?, self.world_origin, self.handle)?;
        let snap = self
            .snapping
            .translation_snap(control, self.world_origin.to_array());
        handle_translation_mode(
            self.world_origin,
            point - self.start_point,
            self.handle,
            if snap.active() { snap.step() } else { 0.0 },
            snap.mode,
            Vec2::ZERO,
            Vec2::ZERO,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::MoveAxis;
    use super::*;
    use crate::ui::gltf_browser::EditorGltfMeshSelection;

    fn drag(entity: Entity, parent: Entity, world_from_parent: Mat4) -> SourceNodeDrag {
        let local = Transform::from_xyz(1.0, 2.0, 3.0);
        let origin = world_from_parent.transform_point3(local.translation);
        let camera = MoveCamera::from_projection(
            &Projection::Perspective(default()),
            &GlobalTransform::from(
                Transform::from_translation(origin + Vec3::new(8.0, 6.0, 12.0))
                    .looking_at(origin, Vec3::Y),
            ),
            Vec2::new(1000.0, 650.0),
            1.0,
        )
        .unwrap();
        let start = camera.project(origin + Vec3::X * 0.75).unwrap();
        let handle = MoveHandle::Axis(MoveAxis::X);
        SourceNodeDrag {
            source_hash: "fixture".into(),
            source: SceneObjectTarget {
                key: SceneObjectKey::Placement {
                    index: 0,
                    id: "test".into(),
                    mesh: Some(EditorGltfMeshSelection {
                        placement_id: "test".into(),
                        node_path: vec!["节点".into()],
                        mesh_name: "子网格".into(),
                        mesh_index: Some(0),
                        node_index: Some(7),
                        valid: true,
                        orphaned_water_binding: false,
                    }),
                },
                name: "子网格".into(),
                scene_id: feathrise_scene_content::SceneId("TEST".into()),
                revision: 1,
                hidden: false,
                locked: false,
                copyable: true,
            },
            identity: NodeIdentity {
                entity,
                parent,
                index: 7,
                world_from_parent,
            },
            camera,
            mode: ViewportMode::Move,
            ui_size: Vec2::new(1000.0, 650.0),
            world_origin: origin,
            start_cursor: start,
            handle,
            start_point: handle_point(camera.ray(start).unwrap(), origin, handle).unwrap(),
            snapping: EditorSnapSettings {
                enabled: false,
                ..default()
            },
            source_before: local,
            visual_before: local,
            applied: local,
        }
    }

    #[test]
    fn world_axis_drag_reaches_world_target_under_rotated_and_nonuniform_parent() {
        let parent_pose = Mat4::from_scale_rotation_translation(
            Vec3::new(0.5, 3.0, 2.0),
            Quat::from_rotation_y(0.73),
            Vec3::new(14.0, 4.0, -7.0),
        );
        let drag = drag(Entity::PLACEHOLDER, Entity::PLACEHOLDER, parent_pose);
        let expected = drag.world_origin + Vec3::X * 2.5;
        let cursor = drag.camera.project(expected + Vec3::X * 0.75).unwrap();
        let moved = drag.position(cursor, false).unwrap();
        let local = drag.source_before.translation + drag.local_delta(moved);
        assert!(
            parent_pose
                .transform_point3(local)
                .abs_diff_eq(expected, 0.0001)
        );
        assert_eq!(
            drag.local_delta(drag.position(drag.start_cursor, false).unwrap()),
            Vec3::ZERO
        );
    }

    #[test]
    fn cancel_restores_only_the_transform_still_owned_by_the_drag() {
        let mut world = World::new();
        let parent = world.spawn_empty().id();
        let node = world
            .spawn((EmbeddedNodeIndex(7), ChildOf(parent), Transform::default()))
            .id();
        let mut drag = drag(node, parent, Mat4::IDENTITY);
        drag.applied.translation += Vec3::X;
        world.entity_mut(node).insert(drag.applied);
        let mut params = bevy::ecs::system::SystemState::<SourceNodeWorld>::new(&mut world);
        params.get_mut(&mut world).unwrap().restore(&drag);
        assert_eq!(world.get::<Transform>(node), Some(&drag.visual_before));
        let changed = Transform::from_xyz(7.0, 3.0, 2.0);
        world.entity_mut(node).insert(changed);
        params.get_mut(&mut world).unwrap().restore(&drag);
        assert_eq!(world.get::<Transform>(node), Some(&changed));
    }
}
