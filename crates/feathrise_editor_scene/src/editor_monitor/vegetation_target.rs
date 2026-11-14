//! 诊断页只决定观察目标：工程分类显式绑定，普通工程按当前作者层提供通用入口。

use feathrise_fps_display::{LodControlContent, LodControlPresetBindings};
use feathrise_scene_content::{SceneVegetationKind, SceneVegetationLayer};
use crate::authoring::{EditorSceneDraft, selected_vegetation_layer};
use crate::resources::SceneEditorState;

pub(super) fn target_layer<'a>(
    draft: &'a EditorSceneDraft,
    editor: &SceneEditorState,
    bindings: Option<&LodControlPresetBindings>,
    content: LodControlContent,
    kind: SceneVegetationKind,
) -> Option<&'a SceneVegetationLayer> {
    if let Some(profile) = bindings.and_then(|bindings| bindings.profile(content)) {
        return draft.vegetation.as_ref()?.layers.iter().find(|layer| {
            layer.kind == kind && layer.runtime_profile.as_deref() == Some(profile)
        });
    }
    // 只复用两种后端的首个既有页面，避免同一自定义层同时受多个分类掩码争抢。
    if !matches!((content, kind),
        (LodControlContent::OrdinaryGrass, SceneVegetationKind::Blade)
        | (LodControlContent::Trees, SceneVegetationKind::Mesh)) {
        return None;
    }
    let selected = selected_vegetation_layer(draft, editor.selected_vegetation_layer)?;
    if bindings.is_some_and(|bindings| bindings.iter().any(|(_, profile)| {
        selected.output.runtime_profile.as_deref() == Some(profile)
    })) {
        return None; // 显式分类拥有该层时，通用回退不得再用另一份掩码争抢。
    }
    (selected.output.kind == kind).then_some(selected.output)
}

#[cfg(test)]
pub(super) fn custom_session() -> crate::authoring::EditorAuthoringSession {
    use feathrise_scene_content::*;
    let mut draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
    let layers = [("project_meadow", SceneVegetationKind::Blade, 11, None),
        ("project_crystal", SceneVegetationKind::Mesh, 42, Some(52))].into_iter().map(|(id, kind, layer, prototype)| SceneVegetationLayer {
        id: id.into(), label: id.into(), kind, runtime_profile: Some(id.into()), runtime_layer_id: Some(layer), prototype_id: prototype,
        asset_path: "sample.glb".into(), density_source_path: format!("{id}.r8"), density_texture_path: format!("{id}.ktx2"),
        resolution: [2; 2], spacing_meters: 1.0, scale_range: [1.0; 2], max_instances: 4, seed: 1,
    }).collect::<Vec<_>>();
    let authoring: SceneVegetationAuthoring = serde_json::from_value(serde_json::json!({
        "groups": [{"id":"base", "label":"测试组", "mode":"additive", "output_merge":"maximum"}],
        "layers": layers.iter().map(|layer| serde_json::json!({
            "id": format!("author-{}", layer.id), "label": layer.label, "group_id":"base", "output_layer_id":layer.id,
            "density_source_path": format!("author-{}.r8",layer.id), "resolution":[2,2]
        })).collect::<Vec<_>>()
    })).unwrap();
    authoring.validate(&layers).unwrap();
    draft.vegetation = Some(SceneVegetationAsset { model_surfaces: Vec::new().into(), schema_version: feathrise_scene_content::SCENE_SCHEMA_VERSION, scene_id: draft.scene_id.clone(), layers, authoring: Some(authoring) });
    crate::authoring::EditorAuthoringSession { draft: Some(draft), ..Default::default() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authoring::{VegetationPreviewState, WaterPreviewState};
    use crate::monitor_resources::EditorSceneMonitorPage;
    use feathrise_atmosphere::{WeatherState, WeatherDebugOverride};

    #[test]
    fn custom_selected_blade_and_mesh_have_real_reports_without_game_bindings() {
        let mut session = custom_session();
        session.draft.as_mut().unwrap().vegetation.as_mut().unwrap().authoring.as_mut().unwrap().layers[1].label = "洞穴水晶·手绘层".into();
        let mut editor = SceneEditorState::default();
        for (index, page, id) in [(0, EditorSceneMonitorPage::OrdinaryGrass, "project_meadow"),
            (1, EditorSceneMonitorPage::Trees, "project_crystal")] {
            editor.selected_vegetation_layer = index;
            let report = super::super::report::build_editor_scene_report(page, None, &editor, &session,
                &VegetationPreviewState::default(), &WaterPreviewState::default(), &WeatherState::default(), &WeatherDebugOverride::default());
            assert!(report.contains(id));
            assert!(report.contains("当前选中预设"));
            assert!(report.starts_with(page.display_label(None)));
            if index == 1 { assert!(report.contains("洞穴水晶·手绘层 [author-project_crystal]")); }
        }
        assert!(target_layer(session.draft.as_ref().unwrap(), &editor, None, LodControlContent::Shrubs, SceneVegetationKind::Mesh).is_none());
    }

    #[test]
    fn explicit_project_page_binding_takes_precedence_over_current_selection() {
        let session = custom_session();
        let mut bindings = LodControlPresetBindings::default();
        bindings.bind(LodControlContent::Wheat, "project_meadow");
        let editor = SceneEditorState { selected_vegetation_layer: 1, ..Default::default() };
        assert_eq!(target_layer(session.draft.as_ref().unwrap(), &editor, Some(&bindings), LodControlContent::Wheat, SceneVegetationKind::Blade).unwrap().id, "project_meadow");
        let editor = SceneEditorState::default();
        assert!(target_layer(session.draft.as_ref().unwrap(), &editor, Some(&bindings), LodControlContent::OrdinaryGrass, SceneVegetationKind::Blade).is_none());
        assert!(target_layer(session.draft.as_ref().unwrap(), &editor, Some(&bindings), LodControlContent::Wheat, SceneVegetationKind::Blade).is_some());
    }
}
