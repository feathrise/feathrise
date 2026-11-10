//! 只读原手柄/数值事务；源局部框架沿用正式姿态数学，不遍历场景实体或复制摆放文档。

use crate::{
    authoring::{EditorAuthoringSession, EditorSceneDraft, placement_pose::placement_record_frame},
    ui::placement_transform::PlacementTransformInputState,
    viewport::{multi_select::SelectedMember, placement_move::PlacementMoveState},
};
use bevy::prelude::*;
use feathrise_scene_content::{
    SceneId, ScenePlacementRecord, embedded_placement_preview::PlacementPreviewKind,
};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Key {
    pub scene_id: SceneId,
    pub revision: u64,
    pub structure: u64,
    pub owner: u64,
    pub kind: u8,
    pub single: Option<(usize, String)>,
}

#[derive(Clone)]
pub(super) enum Members {
    Single(String),
    Group(Arc<Vec<SelectedMember>>),
}

impl Members {
    pub fn len(&self) -> usize {
        match self {
            Self::Single(_) => 1,
            Self::Group(items) => items.len(),
        }
    }
    pub fn id(&self, index: usize) -> &str {
        match self {
            Self::Single(id) => {
                assert_eq!(index, 0);
                id
            }
            Self::Group(items) => &items[index].id,
        }
    }
}

#[derive(Clone)]
pub(super) struct Source {
    pub key: Key,
    pub members: Members,
    pub kind: PlacementPreviewKind,
    pub transform: [f32; 16],
    pub original_transform: [f32; 16],
    pub finished: bool,
    pub committed: bool,
}

pub(super) fn record_matrix(
    draft: &EditorSceneDraft,
    record: &ScenePlacementRecord,
) -> Result<[f32; 16], String> {
    let frame = placement_record_frame(draft, record)?
        .as_transform()
        .to_matrix();
    let matrix = frame
        * Mat4::from_scale_rotation_translation(
            Vec3::from_array(record.scale),
            Quat::from_rotation_y(record.yaw_degrees.to_radians()),
            Vec3::from_array(record.translation),
        );
    if !matrix.is_finite() || matrix.determinant() <= 0.0 || !matrix.inverse().is_finite() {
        return Err("对象实时预览姿态必须有限且可逆".into());
    }
    Ok(matrix.to_cols_array())
}

pub(super) fn current(world: &World) -> Result<Option<Source>, String> {
    let Some(session) = world.get_resource::<EditorAuthoringSession>() else {
        return Ok(None);
    };
    let Some(draft) = &session.draft else {
        return Ok(None);
    };
    if let Some(state) = world.get_resource::<PlacementMoveState>() {
        if let Some((map, revision, structure, owner, members, matrix, finished, committed)) =
            state.embedded_group()
        {
            if map != &draft.scene_id {
                return Ok(None);
            }
            return Ok(Some(Source {
                key: Key {
                    scene_id: map.clone(),
                    revision,
                    structure,
                    owner,
                    kind: 2,
                    single: None,
                },
                members: Members::Group(Arc::clone(members)),
                kind: PlacementPreviewKind::GroupDelta,
                transform: matrix.to_cols_array(),
                original_transform: Mat4::IDENTITY.to_cols_array(),
                finished,
                committed,
            }));
        }
        if let Some((map, index, revision, structure, before, preview)) = state.embedded_single() {
            if map != &draft.scene_id {
                return Ok(None);
            }
            return Ok(Some(Source {
                key: Key {
                    scene_id: map.clone(),
                    revision,
                    structure,
                    owner: 0,
                    kind: 0,
                    single: Some((index, before.id.clone())),
                },
                members: Members::Single(before.id.clone()),
                kind: PlacementPreviewKind::Absolute,
                transform: preview.to_cols_array(),
                original_transform: record_matrix(draft, before)?,
                finished: false,
                committed: false,
            }));
        }
    }
    if let Some(state) = world.get_resource::<PlacementTransformInputState>()
        && let Some((entity, map, index, revision, structure, before, preview)) =
            state.embedded_scrub()
    {
        if map != &draft.scene_id {
            return Ok(None);
        }
        return Ok(Some(Source {
            key: Key {
                scene_id: map.clone(),
                revision,
                structure,
                owner: entity.to_bits(),
                kind: 1,
                single: Some((index, before.id.clone())),
            },
            members: Members::Single(before.id.clone()),
            kind: PlacementPreviewKind::Absolute,
            transform: record_matrix(draft, preview)?,
            original_transform: record_matrix(draft, before)?,
            finished: false,
            committed: false,
        }));
    }
    Ok(None)
}

/// 取消不能无条件恢复按下旧值；原事务已经处理外部作者接管，这里只读取当前权威槽。
pub(super) fn authority(world: &World, key: &Key) -> Option<[f32; 16]> {
    let session = world.get_resource::<EditorAuthoringSession>()?;
    let draft = session
        .draft
        .as_ref()
        .filter(|draft| draft.scene_id == key.scene_id)?;
    if session.placement_structure_revision != key.structure {
        return None;
    }
    let (index, id) = key.single.as_ref()?;
    let record = draft
        .placements
        .placements
        .get(*index)
        .filter(|record| &record.id == id)?;
    record_matrix(draft, record).ok()
}

pub(super) fn author_revision(world: &World) -> u64 {
    world
        .get_resource::<EditorAuthoringSession>()
        .map_or(0, |session| session.revision)
}

/// 原收尾同样按稳定槽位读取来源局部TRS；这里只冻结有界小姿态，不复制完整记录/场景。
pub(super) fn final_member(
    world: &World,
    source: &Source,
    member: usize,
) -> Result<
    feathrise_scene_content::embedded_placement_preview::PlacementPreviewFinalMember,
    String,
> {
    let session = world
        .get_resource::<EditorAuthoringSession>()
        .ok_or("作者会话已退出")?;
    let draft = session
        .draft
        .as_ref()
        .filter(|draft| draft.scene_id == source.key.scene_id)
        .ok_or("对象手势的场景已切换")?;
    if session.placement_structure_revision != source.key.structure {
        return Err("对象成员结构已由新作者修改接管".into());
    }
    let index = match &source.members {
        Members::Single(_) => source.key.single.as_ref().ok_or("对象手势缺少稳定槽位")?.0,
        Members::Group(members) => members[member].index,
    };
    let record = draft
        .placements
        .placements
        .get(index)
        .filter(|record| record.id == source.members.id(member))
        .ok_or("对象手势成员已被删除或替换")?;
    let bounds = crate::authoring::placement_record_bounds(draft, record)?;
    let local = record
        .validated_transform(&draft.scene_id, bounds)?
        .to_matrix()
        .to_cols_array();
    Ok(
        feathrise_scene_content::embedded_placement_preview::PlacementPreviewFinalMember {
            id: record.id.clone(),
            local,
        },
    )
}
