//! 独立的有界可视预览；元素与拓扑手势过程中不写作者配方、不重建物理碰撞。

use super::{
    resources::MeshEditState,
    types::{EditTarget, HiddenRoot},
};
use crate::{
    authoring::{EditorAuthoringSession, EditorCommand,
        placement_deletion::{self, PlacementDeletionSuppression},
        placement_preview_ownership::components::PlacementPreviewSuppression},
    resources::MapEditorState,
    ui::{
        placement_transform::sync_placement_record_world, placement_water::PlacementInstanceIndex,
        scene_objects::ObjectVisibilityState,
    },
};
use bevy::{camera::visibility::RenderLayers, ecs::system::SystemParam, prelude::*};
use feathrise_map_content::{MapId, MapPlacementRecord};
use feathrise_scene::MapPlacementInstance;

#[derive(SystemParam)]
pub(super) struct EditWorld<'w, 's> {
    pub commands: Commands<'w, 's>,
    assets: Res<'w, AssetServer>,
    index: Res<'w, PlacementInstanceIndex>,
    roots: Query<'w, 's, (Entity, &'static MapPlacementInstance)>,
    visibility: Query<'w, 's, &'static Visibility>,
    visibility_state: Option<Res<'w, ObjectVisibilityState>>, // 小型旧夹具可缺资源，生产有资源却无根票据时必须拒绝隐藏。
    meshes: Option<ResMut<'w, Assets<Mesh>>>,
    materials: Option<ResMut<'w, Assets<StandardMaterial>>>,
}

impl EditWorld<'_, '_> {
    /// 会话中的Entity只是地址；地图、稳定ID、活动门和现存显隐共同证明当前仍拥有此根。
    fn owned_root_visibility(
        &self,
        entity: Entity,
        map_id: &MapId,
        placement_id: &str,
    ) -> Option<Visibility> {
        let (_, instance) = self.roots.get(entity).ok()?;
        if instance.map_id() != map_id || instance.id() != placement_id || !instance.is_active() {
            return None;
        }
        self.visibility.get(entity).copied().ok()
    }

    pub fn show(
        &mut self,
        state: &mut MeshEditState,
        session: &EditorAuthoringSession,
        record: &MapPlacementRecord,
    ) -> Result<(), String> {
        let target = state.target.as_ref().ok_or("编辑目标失效")?;
        let bounds = session
            .draft
            .as_ref()
            .and_then(|draft| draft.manifest.world_bounds())
            .ok_or("地图世界范围无效")?;
        let transform = record.validated_transform(&target.map_id, bounds)?;
        let Some(geometry) = record.asset_target.editable_geometry() else {
            return Err("当前对象不支持点边面预览".into());
        };
        let mut hidden_roots = Vec::new();
        if state.hidden_roots.is_empty() {
            let preview = session.placement_previews.get(&target.record.id).copied().filter(|&entity|
                self.owned_root_visibility(entity, &target.map_id, &target.record.id).is_some());
            let mut candidates = Vec::new();
            if let Some(preview) = preview {
                candidates.push(preview);
            } else {
                // 旧地址失效或已被外部接管时，仍须隐藏当前正式根，不能让原壳与幽灵重叠。
                self.index.for_each_active_root(&target.map_id, &target.record.id, &self.roots,
                    |entity| candidates.push(entity));
            }
            for entity in candidates {
                let Some(visibility) = self.owned_root_visibility(entity, &target.map_id, &target.record.id) else { continue; };
                let visibility_intent = match self.visibility_state.as_deref() {
                    Some(state) => Some(state.visibility_intent(entity).ok_or("对象显示状态尚未就绪，请稍后重试元素编辑")?),
                    None => None,
                };
                hidden_roots.push(HiddenRoot { entity, visibility, visibility_intent });
            } // 在生成幽灵前完整预检；缺生产票据不能冒充仍拥有根的临时隐藏责任。
        }
        let (Some(meshes), Some(materials)) =
            (self.meshes.as_deref_mut(), self.materials.as_deref_mut())
        else {
            return Ok(()); // 纯ECS测试可验证事务；正式渲染器始终提供这两种资产。
        };
        let mut mesh = feathrise_scene::editable_mesh_mesh(geometry);
        if let Some(bevy::mesh::VertexAttributeValues::Float32x3(normals)) =
            mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
        {
            let colors = normals
                .iter()
                .map(|normal| {
                    let shade = (0.70 + 0.20 * normal[1] - 0.10 * normal[0].abs()).clamp(0.40, 1.0);
                    [shade, shade, shade, 1.0]
                })
                .collect::<Vec<_>>();
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        } // 夜间仍能辨认非平面四边形的正式三角化与体积，不额外构造几何。
        if let Some(handle) = &state.ghost_mesh {
            meshes
                .insert(handle.id(), mesh)
                .map_err(|error| error.to_string())?;
        } else {
            state.ghost_mesh = Some(meshes.add(mesh));
        }
        let color = match record.asset_target {
            feathrise_map_content::MapPlacementAssetTarget::Cuboid { color_srgb, .. }
            | feathrise_map_content::MapPlacementAssetTarget::EditableCuboid { color_srgb, .. }
            | feathrise_map_content::MapPlacementAssetTarget::EditableMesh { color_srgb, .. } => {
                color_srgb
            }
            _ => return Err("当前对象不是内置网格".into()),
        };
        if state.ghost_material.is_none() {
            state.ghost_material = Some(materials.add(StandardMaterial {
                base_color: Color::srgb_u8(color[0], color[1], color[2]),
                unlit: true,
                ..default()
            }));
        } else if let Some(mut material) = state
            .ghost_material
            .as_ref()
            .and_then(|handle| materials.get_mut(handle))
        {
            material.base_color = Color::srgb_u8(color[0], color[1], color[2]);
        }
        let mesh = state
            .ghost_mesh
            .as_ref()
            .expect("已建立固定预览资产")
            .clone();
        let material = state
            .ghost_material
            .as_ref()
            .expect("已建立固定预览材质")
            .clone();
        if let Some(entity) = state.ghost {
            self.commands.entity(entity).insert((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                transform,
                Visibility::Inherited,
            ));
            self.commands
                .entity(entity)
                .remove::<bevy::camera::primitives::Aabb>();
        } else {
            state.ghost = Some(
                self.commands
                    .spawn((
                        Name::new("编辑器点边面实时预览"),
                        Mesh3d(mesh),
                        MeshMaterial3d(material),
                        transform,
                        Visibility::Inherited,
                        RenderLayers::layer(crate::viewport::MAP_EDITOR_BRUSH_RENDER_LAYER),
                        bevy::light::NotShadowCaster,
                        bevy::light::NotShadowReceiver,
                    ))
                    .id(),
            ); // 一份有界网格资产和一个无碰撞幽灵；整次手势原根物理完全保留。
        }
        if state.hidden_roots.is_empty() {
            for hidden in &hidden_roots {
                let (hidden, map_id, id, index) = (hidden.clone(), target.map_id.clone(), target.record.id.clone(), target.index);
                self.commands.queue(move |world: &mut World| {
                    hide_root(world, hidden, &map_id, &id, index);
                }); // 自身遮蔽直接改值，不发布外部Insert意图，也不改变原根物理。
            }
            state.hidden_roots = hidden_roots;
        }
        let projected = feathrise_map_content::embedded_author_preview::MeshPreview {
            placement_id: target.record.id.clone(), target: record.asset_target,
            local: transform.to_matrix().to_cols_array(),
        };
        if state.embedded_preview.as_ref() != Some(&projected) {
            state.embedded_map = Some(target.map_id.clone());
            state.embedded_committed = false;
            state.embedded_preview = Some(projected);
            state.embedded_revision = state.embedded_revision.wrapping_add(1);
        } // 只投影已被原show真实采用的几何；无效试算不越过原工具的接受门。
        Ok(())
    }

    pub fn restore(&mut self, state: &mut MeshEditState, session: &EditorAuthoringSession) {
        if let Some(preview) = state.embedded_preview.take() {
            state.embedded_committed = state.target.as_ref().is_some_and(|target| session.revision != target.author_revision)
                && session.draft.as_ref().filter(|draft| Some(&draft.map_id) == state.embedded_map.as_ref())
                .and_then(|draft| state.target.as_ref().and_then(|target| draft.placements.placements.get(target.index)))
                .is_some_and(|record| record.id == preview.placement_id && record.asset_target == preview.target);
            state.embedded_revision = state.embedded_revision.wrapping_add(1);
        }
        if let Some(ghost) = state.ghost {
            self.commands.entity(ghost).try_insert(Visibility::Hidden);
        }
        let current = state.target.as_ref().filter(|target|
            session.draft.as_ref().is_some_and(|draft| draft.map_id == target.map_id))
            .map(|target| (&target.map_id, target.index, &target.record.id));
        // 删除可能已撤走记录；保留精确目标给执行时的删除票据转交基线，普通恢复仍逐项重验可见记录。
        for hidden in state.hidden_roots.drain(..) {
            // 外部删除、隐藏或切图已接管旧根时不能反向显示；既有根仍由正式父层级控制可见性。
            if let Some((map_id, index, id)) = current
                && self
                    .owned_root_visibility(hidden.entity, map_id, id)
                    .is_some_and(|visibility| visibility == Visibility::Hidden)
            {
                let (map_id, id) = (map_id.clone(), id.clone());
                // commit补生的作者根和遮蔽也在同一队列，必须等它们落地后再判定归还责任。
                // 会话中的陈旧地址不阻断真实正式根恢复；实际suppression才证明新预览已经接管。
                self.commands.queue(move |world: &mut World| {
                    restore_hidden_root(world, hidden, &map_id, &id, index);
                });
            }
        }
    }

    pub fn commit(
        &mut self,
        target: &EditTarget,
        after: &MapPlacementRecord,
        session: &mut EditorAuthoringSession,
        editor: &mut MapEditorState,
    ) -> Result<bool, String> {
        let draft = session
            .draft
            .as_ref()
            .filter(|draft| draft.map_id == target.map_id)
            .ok_or("地图已经变化")?;
        let current = draft
            .placements
            .placements
            .get(target.index)
            .filter(|record| **record == target.record)
            .ok_or("对象已经由其他操作修改")?;
        if current == after {
            return Ok(false);
        }
        if session.revision != target.author_revision
            || session.placement_structure_revision != target.structure_revision
        {
            return Err("作者修订已经变化".into());
        }
        let bounds = draft.manifest.world_bounds().ok_or("地图世界范围无效")?;
        sync_placement_record_world(
            &mut self.commands,
            &self.assets,
            session,
            &self.index,
            &self.roots,
            after,
            bounds,
            true,
            true,
        )?;
        session
            .draft
            .as_mut()
            .expect("已核对地图")
            .placements
            .placements[target.index] = after.clone();
        session.push_bounded_water_history(EditorCommand::PlacementChanged {
            record_index: target.index,
            before: target.record.clone(),
            after: after.clone(),
        });
        session.mark_changed(editor);
        Ok(true)
    }
}

/// 未装显隐资源的旧夹具可验证原事务；生产票据缺失、移除或换代一律撤销信用。
fn visibility_intent_current(world: &World, hidden: &HiddenRoot) -> bool {
    match (world.get_resource::<ObjectVisibilityState>(), hidden.visibility_intent.as_ref()) {
        (Some(state), Some(frozen)) => state.visibility_intent_matches(hidden.entity, frozen),
        (None, None) => true,
        _ => false,
    }
}

fn current_record_visible(world: &World, map_id: &MapId, id: &str, index: usize) -> bool {
    world.get_resource::<EditorAuthoringSession>().and_then(|session| session.draft.as_ref())
        .filter(|draft| &draft.map_id == map_id)
        .and_then(|draft| draft.placements.placements.get(index))
        .is_some_and(|record| record.id == id && !record.editor.hidden)
}

/// Query采样与Commands执行之间仍可发生外部显隐；旧票据不能把稍后的Visible再次藏住。
fn hide_root(world: &mut World, hidden: HiddenRoot, map_id: &MapId, id: &str, index: usize) {
    if !current_record_visible(world, map_id, id, index)
        || !visibility_intent_current(world, &hidden)
        || !world.get::<MapPlacementInstance>(hidden.entity).is_some_and(|instance|
            instance.map_id() == map_id && instance.id() == id && instance.is_active())
        || world.get::<Visibility>(hidden.entity) != Some(&hidden.visibility) {
        return;
    }
    *world.get_mut::<Visibility>(hidden.entity).unwrap() = Visibility::Hidden;
}

/// 延迟归还再次核对真实身份和显隐；排队期间发生的外部接管不能被旧手势覆盖。
fn restore_hidden_root(world: &mut World, hidden: HiddenRoot, map_id: &MapId, id: &str, index: usize) {
    if !visibility_intent_current(world, &hidden) { return; }
    if !world.get::<MapPlacementInstance>(hidden.entity).is_some_and(|instance|
        instance.map_id() == map_id && instance.id() == id && instance.is_active())
        || world.get::<Visibility>(hidden.entity) != Some(&Visibility::Hidden) {
        return; // 不补组件，也不覆盖外部已经改写的Visibility或已退役/换图/换ID根。
    }
    if world.get::<PlacementDeletionSuppression>(hidden.entity).is_some() {
        if placement_deletion::suppression_is_current(world, hidden.entity)
            && world.get::<PlacementDeletionSuppression>(hidden.entity).unwrap().original_visibility == Visibility::Hidden {
            // 删除可先移除草稿记录；先把幽灵临时Hidden的基线交给精确删除票据，仍不复显正式旧壳。
            world.get_mut::<PlacementDeletionSuppression>(hidden.entity).unwrap().original_visibility = hidden.visibility;
            if let Some(mut preview) = world.get_mut::<PlacementPreviewSuppression>(hidden.entity) {
                preview.original_visibility = hidden.visibility;
            }
            crate::ui::scene_objects::restore_placement_base_visibility(world, hidden.entity, hidden.visibility);
        }
        return; // 过期删除覆盖也由原定向归还入口处理，元素手势不能越权移除另一领域的marker。
    }
    if !current_record_visible(world, map_id, id, index) {
        return; // 当前记录已删除、换槽或被场景树隐藏时，不归还旧画面。
    }
    let preview = world.resource::<EditorAuthoringSession>().placement_previews.get(id).copied();
    if let Some(suppression) = world.get::<PlacementPreviewSuppression>(hidden.entity).copied() {
        let owns_base = preview == Some(suppression.owner)
            && world.get::<crate::components::EditorPlacedAsset>(suppression.owner).is_some()
            && world.get::<Visibility>(suppression.owner).is_some()
            && world.get::<feathrise_scene::StreamedVolumeResident>(suppression.owner).is_none()
            && world.get::<MapPlacementInstance>(suppression.owner).is_some_and(|instance|
                instance.map_id() == map_id && instance.id() == id && instance.is_active());
        if owns_base && suppression.original_visibility == Visibility::Hidden {
            // 新预览读到的是幽灵的临时Hidden；转交原基线，冷撤回才可恢复原画面。
            // 相同票据证明期间没有外部Insert；即使外部同值Hidden也会换票并在上方拒绝。
            world.get_mut::<PlacementPreviewSuppression>(hidden.entity).unwrap().original_visibility = hidden.visibility;
            crate::ui::scene_objects::restore_placement_base_visibility(world, hidden.entity, hidden.visibility);
            *world.get_mut::<Visibility>(hidden.entity).unwrap() = Visibility::Hidden;
            // helper同时修正场景树覆盖的底层基线；直接改值不触发外部Insert，也不提前打开碰撞。
        }
        return; // 独立作者预览已拥有该正式根的遮蔽，元素restore只交接基线、不复显旧壳。
    }
    crate::ui::scene_objects::restore_placement_base_visibility(world, hidden.entity, hidden.visibility);
    // 自身归还也直接改值；不把手势结束伪装成新的外部Insert意图。
}
