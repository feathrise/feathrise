//! 整组预览的定向根绑定与固定预算解绑；拖动帧只修改公共父节点，不逐对象同步。

use super::{
    group_types::{GroupDrag, GroupPivot, GroupRoot},
    transaction::MoveWorld,
};
use crate::authoring::{EditorAuthoringSession, placement_record_bounds};
use bevy::prelude::*;
use feathrise_scene_content::{SceneId, ScenePlacementRecord, TerrainObjectTransform};

impl MoveWorld<'_, '_> {
    pub(super) fn root_epoch(&self) -> u64 {
        self.index.membership_revision()
    }

    pub(super) fn group_runtime_identity(&self) -> Option<(Option<SceneId>, u64, usize)> {
        self.runtime_status.as_deref().map(|status| (status.scene_id.clone(), status.revision, status.placement_count))
    }

    /// 顶层正式根用公开采用身份防止共享门悄然退役；patch所属外层实例另由patch_source_current验证。
    pub(super) fn group_runtime_identity_current(&self, drag: &GroupDrag) -> bool {
        match (&drag.formal_runtime_identity, self.runtime_status.as_deref()) {
            (Some((map, revision, count)), Some(status)) => map == &status.scene_id
                && *revision == status.revision && *count == status.placement_count,
            (None, None) => true,
            _ => false,
        } // 只读常数身份，不扫描选集；候选进度/尝试/失败保留旧活动根，不能仅凭phase等通知取消。
    }

    /// 自补预览只允许确切实体的单次Add；其他根的加入或移除仍使已准备选集失效。
    pub(super) fn group_root_identity_current(&self, session: &EditorAuthoringSession, drag: &GroupDrag) -> bool {
        let epoch = self.root_epoch();
        let Some(pending) = drag.pending_preview else { return epoch == drag.root_epoch; };
        let Some(member) = drag.members.get(drag.prepared) else { return false; };
        if drag.phase != super::group_types::GroupPhase::Preparing
            || session.placement_previews.get(&member.id) != Some(&pending)
            || self.group_roots.get(pending).is_err()
            || !self.roots.get(pending).is_ok_and(|(_, instance)| instance.scene_id() == &drag.scene_id
                && instance.id() == member.id && instance.is_active()) {
            return false;
        }
        if epoch == drag.root_epoch { return true; }
        if epoch != drag.root_epoch.wrapping_add(1) { return false; }
        let mut registered = false;
        self.index.for_each_active_root(&drag.scene_id, &member.id, &self.roots, |entity| registered |= entity == pending);
        registered // 差值恰为一次且新地址确已登记，不能把同帧外部Add/Remove误当作自己的补生。
    }

    pub(super) fn group_bind(
        &mut self,
        session: &mut EditorAuthoringSession,
        index: usize,
        record: &ScenePlacementRecord,
        frame: TerrainObjectTransform,
        pivots: &mut Vec<GroupPivot>,
    ) -> Result<Option<Vec<GroupRoot>>, String> {
        if crate::authoring::placement_pose::is_automatic_volume(record) {
            return Err("自动三维雕刻层请使用专用作者入口".into());
        } // 改ChildOf或补生预览前检查实际记录，不能等松手worker拒绝时才归还已被接管的驻留根。
        let map = &session.draft.as_ref().ok_or("场景已离开")?.scene_id;
        let mut entities = Vec::new();
        let preview = session.placement_previews.get(&record.id).copied().filter(|entity|
            self.group_roots.get(*entity).is_ok() && self.roots.get(*entity).is_ok_and(|(_, instance)|
                instance.scene_id() == map && instance.id() == record.id && instance.is_active()));
        if let Some(entity) = preview {
            entities.push(entity);
        } else {
            // 地形父退役可能已回收旧作者根；只撤销地址信用，既不写死实体也不接管外部拥有者。
            session.placement_previews.remove(&record.id);
        }
        if entities.is_empty() {
            // 正式根属于可退役批次。Preparing也必须先取得独立预览，再按确切Add票据继续固定预算绑定。
            self.preview(session, record)?;
            return Ok(None);
        }
        let mut roots = Vec::new();
        for entity in entities {
            let Ok((transform, parent)) = self.group_roots.get(entity) else {
                return Err("选中资产的运行时根已失效".into());
            };
            if self.roots.get(entity).is_ok_and(|(_, instance)| {
                !instance.is_active() || instance.scene_id() != map || instance.id() != record.id
            }) {
                return Err("选中资产根已换代".into());
            }
            let parent = parent.map(ChildOf::parent);
            let mut actual = Mat4::IDENTITY;
            let mut ancestor = parent;
            for _ in 0..256 {
                let Some(entity) = ancestor else { break; };
                let (container, next) = self.group_roots.get(entity).map_err(|_| "资产父容器已失效")?;
                actual = container.to_matrix() * actual;
                ancestor = next.map(ChildOf::parent);
            }
            if ancestor.is_some() || !super::source_identity::same_source_matrix(actual, frame.as_transform().to_matrix()) {
                return Err("资产容器世界变换与所属源已不一致，不能建立精确整组预览".into());
            }
            if !transform.translation.is_finite() {
                return Err("资产根位置无效".into());
            }
            roots.push(GroupRoot {
                entity,
                pivot: Entity::PLACEHOLDER,
                index,
                id: record.id.clone(),
                parent,
                // 普通旧粘贴容器是identity，可归并；地形根保留真实父级及正式批次的生命期。
                pivot_parent: parent.filter(|parent| record.terrain_patch.is_some() || !self.paste_groups.contains(*parent)),
            });
        }
        let mut parents: Vec<_> = pivots.iter().map(|pivot| pivot.parent).collect();
        for root in &roots {
            if !parents.contains(&root.pivot_parent) {
                parents.push(root.pivot_parent);
            }
        }
        if parents.len() > 66 {
            return Err("选中资产跨越过多场景容器，请分组移动".into());
        }
        for root in &mut roots {
            root.pivot =
                if let Some(pivot) = pivots.iter().find(|pivot| pivot.parent == root.pivot_parent) {
                    pivot.attach
                } else {
                    let (entity, attach, cleanup) = self.spawn_group_pivot(root.pivot_parent, frame);
                    pivots.push(GroupPivot {
                        entity,
                        parent: root.pivot_parent,
                        attach,
                        cleanup,
                    });
                    attach
                };
            self.commands
                .entity(root.entity)
                .insert(ChildOf(root.pivot));
        }
        Ok(Some(roots)) // 所有根预检后才改变关系，取消时公共节点归零立即恢复整组。
    }

    /// P=T*R*S的逆不能拆成一个TRS；分层抵消、施加世界A、恢复P，严格保留A*P*L的剪切。
    fn spawn_group_pivot(&mut self, parent: Option<Entity>, frame: TerrainObjectTransform) -> (Entity, Entity, Entity) {
        let source = frame.as_transform();
        if source == Transform::IDENTITY {
            let mut node = self.commands.spawn((Transform::IDENTITY, Visibility::Inherited, Name::new("编辑器临时多选移动")));
            if let Some(parent) = parent { node.insert(ChildOf(parent)); }
            let entity = node.id();
            return (entity, entity, entity);
        }
        // P * S^-1 * (R^-1 T^-1)恰为identity；各层均是可由Bevy原生Transform精确表达的TRS。
        let mut inverse_scale = self.commands.spawn((Transform::from_scale(source.scale.recip()), Visibility::Inherited,
            Name::new("编辑器临时多选抵消源缩放")));
        if let Some(parent) = parent { inverse_scale.insert(ChildOf(parent)); }
        let cleanup = inverse_scale.id();
        let inverse_rotation = source.rotation.inverse();
        let inverse = self.commands.spawn((Transform { translation: inverse_rotation * -source.translation,
            rotation: inverse_rotation, scale: Vec3::ONE }, Visibility::Inherited, ChildOf(cleanup),
            Name::new("编辑器临时多选抵消源旋转位移"))).id();
        let entity = self.commands.spawn((Transform::IDENTITY, Visibility::Inherited, ChildOf(inverse),
            Name::new("编辑器临时多选世界变换"))).id();
        let attach = self.commands.spawn((source, Visibility::Inherited, ChildOf(entity),
            Name::new("编辑器临时多选恢复源框架"))).id();
        (entity, attach, cleanup)
    }

    pub(super) fn group_preview(&mut self, drag: &GroupDrag, delta: Vec3) {
        let rotation = Quat::from_rotation_y(drag.rotation_degrees.to_radians());
        // 临时父级的局部变换围绕共同世界中心；根仍保留原缩放和朝向，不逐帧遍历模型。
        let transform = Transform {
            translation: drag.origin + delta - rotation * (drag.origin * drag.factor),
            rotation,
            scale: Vec3::splat(drag.factor),
        };
        for pivot in &drag.pivots {
            if self.group_roots.get(pivot.entity).is_ok() {
                self.commands
                    .entity(pivot.entity)
                    .insert(transform);
            }
        } // 32个地形的正式/作者父各一，加普通两类父：最多66节点，与选中模型数无关。
    }

    pub(super) fn group_cancel(&mut self, drag: &mut GroupDrag) {
        drag.delta = Vec3::ZERO;
        drag.rotation_degrees = 0.0;
        drag.factor = 1.0;
        self.group_preview(drag, Vec3::ZERO);
        drag.phase = super::group_types::GroupPhase::Finishing;
    }

    pub(super) fn group_cleanup(
        &mut self,
        session: &EditorAuthoringSession,
        drag: &mut GroupDrag,
    ) -> bool {
        let end = (drag.cleanup_cursor + 16).min(drag.roots.len());
        for root in &drag.roots[drag.cleanup_cursor..end] {
            let Ok((current, parent)) = self.group_roots.get(root.entity) else {
                continue;
            };
            if parent.map(ChildOf::parent) != Some(root.pivot) {
                continue;
            }
            // 即使组件已被外部撤销，也必须先解绑仍属于临时父级的根，避免销毁父级时级联误删。
            let owned = match self.roots.get(root.entity) {
                // 真实组件已有新拥有者时，陈旧会话地址不能重新取得变换或父级的写权限。
                Ok((_, instance)) => instance.scene_id() == &drag.scene_id && instance.id() == root.id,
                Err(_) => session.draft.as_ref().is_some_and(|draft| draft.scene_id == drag.scene_id)
                    && session.placement_previews.get(&root.id) == Some(&root.entity),
            };
            let record = session
                .draft
                .as_ref()
                .filter(|draft| owned && draft.scene_id == drag.scene_id)
                .and_then(|draft| {
                    draft
                        .placements
                        .placements
                        .get(root.index)
                        .filter(|record| record.id == root.id)
                        .and_then(|record| placement_record_bounds(draft, record).ok()
                            .and_then(|bounds| record.validated_transform(&draft.scene_id, bounds).ok())
                            .map(|transform| (record, transform)))
                });
            let transform = record.map_or(*current, |(_, transform)| transform); // 旧下标失效时保留当前姿态，不猜测新作者记录或覆盖外部修改。
            let mut entity = self.commands.entity(root.entity);
            entity.remove::<ChildOf>();
            if owned {
                entity.insert(transform);
                if let Some((record, _)) = record {
                    entity.insert(feathrise_scene::ScenePlacementInstance::new(&drag.scene_id, record));
                } // 归还临时父级时同时发布完整作者证据，后续保存不能拿变换前的MPI冒充当前预览。
                if let Some(parent) = root
                    .parent
                    .filter(|parent| self.group_roots.get(*parent).is_ok())
                {
                    entity.insert(ChildOf(parent));
                }
            }
        }
        drag.cleanup_cursor = end;
        if end == drag.roots.len() {
            for pivot in &drag.pivots {
                if self.group_roots.get(pivot.cleanup).is_ok() {
                    self.commands.entity(pivot.cleanup).despawn();
                }
            } // 解绑命令在前；正式退役可能已按预算删除临时节点，缺失时不重复发命令。
            true
        } else {
            false
        }
    }

    pub(super) fn group_adopt(
        &mut self,
        session: &mut EditorAuthoringSession,
        editor: &mut crate::resources::SceneEditorState,
        prepared: super::group_types::PreparedGroupMove,
        map: &SceneId,
    ) -> Result<Option<Vec<ScenePlacementRecord>>, (String, super::group_types::PreparedGroupMove)>
    {
        let Some(draft) = session.draft.as_mut().filter(|draft| &draft.scene_id == map) else {
            return Err(("场景已变化".into(), prepared));
        };
        if prepared.apply.iter().any(|(index, record)| {
            draft
                .placements
                .placements
                .get(*index)
                .is_none_or(|old| old.id != record.id)
        }) {
            return Err(("多选记录下标已失效".into(), prepared));
        }
        let Some(history) = prepared.history else {
            return Ok(None);
        };
        let mut retired = prepared.retired;
        // 身份门已复核，后台完成所有格式/高度检查；此处只移动256项内已分配记录，不扫描或克隆场景。
        for (index, record) in prepared.apply {
            retired.push(std::mem::replace(
                &mut draft.placements.placements[index],
                record,
            ));
        }
        session.push_bounded_water_history(crate::authoring::EditorCommand::CourtyardBatch(
            Box::new(history),
        ));
        session.mark_changed(editor);
        Ok(Some(retired))
    }
}
