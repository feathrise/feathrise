//! 对象画布共用会话写保护与米制画笔空间；作者纹素地址不随整体缩放改变。

use bevy::prelude::{Transform, Vec2, Vec3};
use feathrise_scene_content::TerrainId;

use crate::{
    authoring::{EditorAuthoringSession, resources::EditorSceneDraft},
    resources::SceneEditorState,
};

/// 冻结段补画和三维字段工具也必须使用此门，不能只检查当前鼠标是否有命中。
pub(crate) fn canvas_writable(session: &EditorAuthoringSession, editor: &SceneEditorState) -> bool {
    if !canvas_visible(session, editor) {
        return false;
    }
    let draft = session.draft.as_ref().expect("已核对当前可见画布");
    let id = draft
        .terrain_documents
        .active
        .as_ref()
        .map(|active| &active.definition.id);
    id.map_or_else(
        || !editor.terrain_locked.contains(&TerrainId::legacy()),
        |id| !editor.terrain_locked.contains(id),
    )
}

/// 锁定对象仍可查看图层；隐藏、停用或换代的画布不得泄露辅助覆盖。
pub(crate) fn canvas_visible(session: &EditorAuthoringSession, editor: &SceneEditorState) -> bool {
    let Some(draft) = &session.draft else {
        return false;
    };
    let id = if let Some(active) = &draft.terrain_documents.active {
        if !active.definition.enabled
            || draft.terrain_documents.loading
            || (draft.manifest.terrains.is_some()
                && editor.terrain_canvas_target.as_ref() != Some(&active.definition.id))
        {
            return false;
        }
        &active.definition.id
    } else {
        if draft.manifest.terrains.is_some() || draft.manifest.terrain.is_empty() {
            return false;
        }
        return !editor.terrain_hidden.contains(&TerrainId::legacy());
    };
    !editor.terrain_hidden.contains(id)
}

/// 贴地辅助网格持有源顶点，整体姿态只应用一次；拖动时与正式对象使用同一个预览姿态。
pub(crate) fn canvas_transform(
    draft: &EditorSceneDraft,
    preview: Option<&feathrise_scene::TerrainObjectTransformPreview>,
) -> Transform {
    draft
        .terrain_documents
        .active
        .as_ref()
        .map_or_else(Transform::default, |active| {
            preview
                .and_then(|preview| preview.for_object(&draft.scene_id, &active.definition.id))
                .unwrap_or(active.definition.transform)
                .as_transform()
        })
}

/// 逆变换到源坐标后只恢复各轴的世界米数；完整三轴旋转保距，无需旋转作者纹素网格。
#[derive(Clone, Copy)]
pub(crate) struct PaintMetricSpace {
    scale: Vec3,
}

impl PaintMetricSpace {
    pub(crate) fn from_draft(draft: &EditorSceneDraft) -> Self {
        Self {
            scale: draft
                .terrain_documents
                .active
                .as_ref()
                .map_or(Vec3::ONE, |active| {
                    Vec3::from_array(active.definition.transform.scale)
                }),
        }
    }

    pub(crate) fn point(self, source: Vec3) -> Vec3 {
        source * self.scale
    }

    pub(crate) fn bounds(self, source: [f32; 2]) -> [f32; 2] {
        (Vec2::from_array(source) * Vec2::new(self.scale.x, self.scale.z)).to_array()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authoring::terrain_documents::TerrainDocumentIdentity;
    use std::sync::Arc;

    fn draft() -> EditorSceneDraft {
        let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
        let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
        object.id = TerrainId::parse("metric-terrain").unwrap();
        object.transform.translation = [17.0, 5.0, -9.0];
        object.transform.yaw_degrees = 37.0;
        object.transform.pitch_degrees = 28.0;
        object.transform.roll_degrees = -19.0;
        object.transform.scale = [2.0, 3.0, 0.5];
        draft.manifest.terrains = Some(Arc::from([object.clone()]));
        draft.terrain_documents.active = Some(TerrainDocumentIdentity {
            definition: Arc::new(object),
            manifest: draft.manifest.clone(),
            folder: draft.folder.clone(),
        });
        draft
    }

    #[test]
    fn anisotropic_canvas_metric_preserves_the_world_circle_with_rotation_and_translation() {
        let draft = draft();
        let metric = PaintMetricSpace::from_draft(&draft);
        let matrix = draft
            .terrain_documents
            .active
            .as_ref()
            .unwrap()
            .definition
            .transform
            .as_transform()
            .to_matrix();
        let center = Vec3::new(1.0, 0.0, -2.0);
        for delta in [
            Vec3::new(0.8, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 3.2),
            Vec3::new(0.4, 0.0, 1.2),
        ] {
            let actual = matrix
                .transform_point3(center + delta)
                .distance(matrix.transform_point3(center));
            let metric_distance = metric.point(center + delta).distance(metric.point(center));
            assert!((actual - metric_distance).abs() < 0.00001);
        }
        assert_eq!(metric.bounds([-4.0, -6.0]), [-8.0, -3.0]);
    }

    #[test]
    fn canvas_lock_hide_and_target_change_prevent_delayed_writes() {
        let draft = draft();
        let id = draft
            .terrain_documents
            .active
            .as_ref()
            .unwrap()
            .definition
            .id
            .clone();
        let session = EditorAuthoringSession {
            draft: Some(draft),
            ..Default::default()
        };
        let mut editor = SceneEditorState {
            terrain_canvas_target: Some(id.clone()),
            ..Default::default()
        };
        assert!(canvas_writable(&session, &editor));
        editor.terrain_locked.insert(id.clone());
        assert!(!canvas_writable(&session, &editor));
        editor.terrain_locked.clear();
        Arc::make_mut(&mut editor.terrain_hidden).insert(id);
        assert!(!canvas_writable(&session, &editor));
        editor.terrain_hidden = Default::default();
        editor.terrain_canvas_target = None;
        assert!(!canvas_writable(&session, &editor));
    }
}
