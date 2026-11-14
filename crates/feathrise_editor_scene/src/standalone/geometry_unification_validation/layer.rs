//! 自有夹具经原预设检查、保存、热采用及建层按钮准备独立Blade输出，不注入目录或就绪状态。
use super::{resources::GeometryFixture, types::{Domain, Phase}};
use crate::{authoring::EditorAuthoringSession, resources::SceneEditorState, types::EditorAction,
    ui::vegetation_preset_editor::{PresetEditorAction, PresetEditorButton, VegetationPresetEditorState}};
use bevy::prelude::*;
use feathrise_scene_content::{SceneVegetationKind, VegetationPresetDescriptor, VegetationPresetRegistry};
use std::time::Duration;

fn excluded_output(world: &World, state: &GeometryFixture) -> Result<(Option<String>, Option<String>), String> {
    if state.domain != Domain::VoxelUnderside { return Ok((None, None)); }
    let output = state.proof.vegetation_output_layer.as_ref().ok_or("底面缺少已完成上表面的正式图层身份")?;
    let profile = world.resource::<EditorAuthoringSession>().draft.as_ref()
        .and_then(|draft| draft.vegetation.as_ref())
        .and_then(|vegetation| vegetation.layers.iter().find(|layer| &layer.id == output))
        .and_then(|layer| layer.runtime_profile.clone()).ok_or("上表面的正式图层或预设已失效")?;
    Ok((Some(profile), Some(output.clone())))
}

fn independent_blade<'a>(registry: &'a VegetationPresetRegistry, profile: Option<&str>, output: Option<&str>)
    -> Option<&'a VegetationPresetDescriptor> {
    if registry.revision == 0 { return None; }
    registry.presets().iter().find(|preset| preset.runtime.kind() == SceneVegetationKind::Blade
        && profile != Some(preset.id.as_str()) && output != Some(preset.output_id.as_str())
        && registry.resolve(&preset.id).is_some())
}

fn pulse(world: &mut World, action: PresetEditorAction) -> Entity {
    world.spawn((PresetEditorButton(action), feathrise_editor_ui::EditorButtonClick::triggered())).id()
}

fn create_preset(world: &mut World, state: &mut GeometryFixture, source: String,
    revision: u64, presets: Vec<VegetationPresetDescriptor>) -> Result<(), String> {
    let editor = world.resource::<VegetationPresetEditorState>();
    if !editor.tabs.is_empty() { return Err("几何夹具创建独立Blade前仍有未关闭的原预设标签".into()); }
    if editor.exit_blocker().is_some() { return Ok(()); }
    state.layer_preset_source = Some(source.clone()); state.layer_preset_revision = revision;
    state.layer_preset_ids = presets.iter().map(|preset| preset.id.clone()).collect();
    state.layer_preset_before = presets; state.layer_preset_adopted = None;
    world.resource_mut::<SceneEditorState>().selected_asset = Some(source);
    state.pulse = Some(pulse(world, PresetEditorAction::CreateBlade)); state.advance(Phase::WaitLayerPreset);
    Ok(())
}

pub(super) fn advance(world: &mut World, state: &mut GeometryFixture) -> Result<(), String> {
    if state.phase != Phase::CreateLayer && state.phase_started.elapsed() > Duration::from_secs(30) {
        return Err(format!("原独立Blade预设业务等待超时：{}；目录错误={:?}",
            world.resource::<SceneEditorState>().status, world.resource::<VegetationPresetRegistry>().last_error));
    }
    match state.phase {
        Phase::CreateLayer => {
            let (profile, output) = excluded_output(world, state)?;
            let registry = world.resource::<VegetationPresetRegistry>();
            if registry.revision == 0 { return Ok(()); }
            if let Some(preset) = independent_blade(registry, profile.as_deref(), output.as_deref()).map(|preset| preset.id.clone()) {
                state.author_count = world.resource::<EditorAuthoringSession>().draft.as_ref().unwrap().vegetation.as_ref()
                    .and_then(|vegetation| vegetation.authoring.as_ref()).map_or(0, |author| author.layers.len());
                world.resource_mut::<SceneEditorState>().vegetation_selected_preset_id = Some(preset);
                state.pulse = Some(super::pulse(world, EditorAction::VegetationAddLayer)); state.advance(Phase::WaitLayer);
            } else {
                // 原夹具通常只有一个Blade和一个Mesh；底面不能借同一输出，必须真正再创建一个Blade。
                // 沿已采用预设的真实资产走原检查，源不合法时原业务会失败，不能克隆伪造就绪目录。
                let source = profile.as_deref().and_then(|id| registry.resolve(id))
                    .or_else(|| registry.presets().iter().find(|preset| registry.resolve(&preset.id).is_some()))
                    .map(|preset| preset.author_asset_path.clone()).filter(|path| !path.is_empty())
                    .ok_or("几何夹具没有可供原Blade检查的真实资产来源")?;
                let revision = registry.revision;
                let presets = registry.presets().to_vec();
                create_preset(world, state, source, revision, presets)?;
            }
        }
        Phase::WaitLayerPreset => {
            let editor = world.resource::<VegetationPresetEditorState>();
            if let Some(tab) = editor.tabs.first() {
                if editor.tabs.len() != 1 { return Err("原Blade创建必须恰好新增一个独立预设标签".into()); }
                state.layer_preset_tab = Some(tab.id); state.advance(Phase::SaveLayerPreset);
            }
        }
        Phase::SaveLayerPreset => {
            state.pulse = Some(pulse(world, PresetEditorAction::Save)); state.advance(Phase::WaitLayerPresetSave);
        }
        Phase::WaitLayerPresetSave => {
            let editor = world.resource::<VegetationPresetEditorState>();
            let registry = world.resource::<VegetationPresetRegistry>();
            if editor.saved_play_pending() || editor.exit_blocker().is_some() || registry.revision <= state.layer_preset_revision {
                return Ok(());
            }
            let mut added = registry.presets().iter().filter(|preset| !state.layer_preset_ids.contains(&preset.id));
            let Some(preset) = added.next() else { return Ok(()); };
            if added.next().is_some() { return Err("原独立Blade保存意外新增多个目录成员".into()); }
            let (profile, output) = excluded_output(world, state)?;
            if !super::super::model_surfaces::preset_uses_source(preset, SceneVegetationKind::Blade,
                    state.layer_preset_source.as_deref().unwrap_or_default())
                || state.layer_preset_before.iter().any(|before| registry.resolve(&before.id) != Some(before))
                || independent_blade(registry, profile.as_deref(), output.as_deref()).map(|value| &value.id) != Some(&preset.id) {
                return Err("原独立Blade保存没有采用不同的完整正式输出".into());
            }
            // 仅真实保存与目录热采用共同通过后冻结完整描述，后续夹具不能靠数量或同路径冒认归属。
            state.layer_preset_adopted = Some(preset.clone());
            state.advance(Phase::CloseLayerPreset);
        }
        Phase::CloseLayerPreset => {
            state.pulse = Some(pulse(world, PresetEditorAction::ClosePresetTab(state.layer_preset_tab.ok_or("独立Blade标签身份丢失")?)));
            state.advance(Phase::WaitLayerPresetClose);
        }
        Phase::WaitLayerPresetClose => {
            if !world.resource::<VegetationPresetEditorState>().has_tab(state.layer_preset_tab.ok_or("独立Blade标签身份丢失")?) {
                state.layer_preset_tab = None; state.advance(Phase::CreateLayer);
            }
        }
        _ => unreachable!(),
    }
    Ok(())
}

/// 只接续已完成几何专项登记的一个新增Blade；不得把任意第三预设当成空白模板的一部分。
pub(super) fn scene_tools_mesh(world: &World) -> Option<&VegetationPresetDescriptor> {
    let state = world.get_resource::<GeometryFixture>()?;
    if state.phase != Phase::Complete || !state.proof.complete || state.proof.error.is_some() { return None; }
    let scene = &world.get_resource::<EditorAuthoringSession>()?.draft.as_ref()?.scene_id;
    if state.scene.as_ref() != Some(scene) { return None; }
    let registry = world.get_resource::<VegetationPresetRegistry>()?;
    if registry.revision <= state.layer_preset_revision { return None; }
    registered_mesh(registry, super::super::model_surfaces::registered_presets(world),
        &state.layer_preset_before, state.layer_preset_adopted.as_ref())
}

fn registered_mesh<'a>(registry: &'a VegetationPresetRegistry, registered: Option<&[VegetationPresetDescriptor]>,
    before: &[VegetationPresetDescriptor], adopted: Option<&VegetationPresetDescriptor>) -> Option<&'a VegetationPresetDescriptor> {
    let registered = registered?;
    let adopted = adopted?;
    if registry.revision == 0 || registered.len() != 2 || before.len() != 2 || registry.presets().len() != before.len() + 1
        || registered.iter().any(|preset| !before.contains(preset))
        || before.iter().any(|preset| preset.id == adopted.id
            || preset.output_id == adopted.output_id || registry.resolve(&preset.id) != Some(preset))
        || registry.resolve(&adopted.id) != Some(adopted)
        || before.iter().filter(|preset| preset.runtime.kind() == SceneVegetationKind::Blade).count() != 1 {
        return None;
    }
    let blade = registered.iter().find(|preset| preset.kind() == SceneVegetationKind::Blade)?;
    // 来源跟随上一步真实登记，不增加路径白名单；新增项的每档网格也必须来自原Blade资产。
    if !super::super::model_surfaces::preset_uses_source(adopted, SceneVegetationKind::Blade,
        &blade.author_asset_path) { return None; }
    let mesh = registered.iter().find(|preset| preset.kind() == SceneVegetationKind::Mesh)?;
    registry.resolve(&mesh.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use feathrise_scene_content::{ResolvedVegetationPresetCatalog, VegetationPresetCatalogAsset, VEGETATION_PRESET_SCHEMA_VERSION};

    fn presets() -> Vec<VegetationPresetDescriptor> {
        serde_json::from_str::<VegetationPresetCatalogAsset>(include_str!(concat!(env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/vegetation_presets.vegetation_presets.json"))).unwrap().presets
    }
    fn registry(presets: Vec<VegetationPresetDescriptor>) -> VegetationPresetRegistry {
        let catalog = VegetationPresetCatalogAsset { schema_version: VEGETATION_PRESET_SCHEMA_VERSION, presets };
        VegetationPresetRegistry { catalog: Handle::default(), revision: 1,
            resolved: ResolvedVegetationPresetCatalog::build(&catalog).unwrap(), last_error: None }
    }

    #[test]
    fn underside_requires_a_second_real_blade_and_an_independent_output() {
        let sources = presets();
        let blade = sources.iter().find(|preset| preset.runtime.kind() == SceneVegetationKind::Blade).unwrap().clone();
        let mesh = sources.iter().find(|preset| preset.runtime.kind() == SceneVegetationKind::Mesh).unwrap().clone();
        let original = registry(vec![blade.clone(), mesh.clone()]);
        assert!(independent_blade(&original, None, None).is_some());
        let output = blade.output_id.clone();
        assert!(independent_blade(&original, Some(&blade.id), Some(&output)).is_none(), "Mesh不能冒充第二个Blade");
        assert!(independent_blade(&original, None, Some(&output)).is_none(), "另一选择路径也不能复用原正式输出");
        let mut second = blade.clone(); second.id = "underside-blade".into(); second.output_id = "underside-output".into(); second.aliases.clear();
        let complete = registry(vec![blade.clone(), mesh, second]);
        assert_eq!(independent_blade(&complete, Some(&blade.id), Some(&output)).unwrap().id, "underside-blade");
        let mut unready = complete; unready.revision = 0;
        assert!(independent_blade(&unready, Some(&blade.id), Some(&output)).is_none());
    }

    #[test]
    fn missing_second_blade_emits_original_creation_then_waits_for_a_real_tab() {
        let mut world = World::new(); world.init_resource::<SceneEditorState>(); world.init_resource::<VegetationPresetEditorState>();
        let mut state = GeometryFixture::default(); state.phase = Phase::CreateLayer; state.domain = Domain::VoxelUnderside;
        create_preset(&mut world, &mut state, "scene/probe.glb".into(), 7, Vec::new()).unwrap();
        assert_eq!(state.phase, Phase::WaitLayerPreset);
        assert_eq!(world.get::<PresetEditorButton>(state.pulse.unwrap()).unwrap().0, PresetEditorAction::CreateBlade);
        assert_eq!(world.resource::<SceneEditorState>().selected_asset.as_deref(), Some("scene/probe.glb"));
        advance(&mut world, &mut state).unwrap();
        assert_eq!(state.phase, Phase::WaitLayerPreset, "不能把已发送检查请求冒充真实预设标签或成功");
        assert!(!state.proof.complete);
    }

    fn fixture_catalog() -> (Vec<VegetationPresetDescriptor>, VegetationPresetDescriptor) {
        let sources = presets();
        let mut before: Vec<_> = [SceneVegetationKind::Blade, SceneVegetationKind::Mesh].into_iter()
            .map(|kind| sources.iter().find(|preset| preset.runtime.kind() == kind).unwrap().clone()).collect();
        for preset in &mut before {
            let source = "scene/vegetation-probe/vegetation-probe.glb";
            preset.author_asset_path = source.into();
            match &mut preset.runtime {
                feathrise_scene_content::VegetationPresetRuntime::Blade { lods, .. } => {
                    for lod in lods { lod.mesh_path = format!("{source}#Mesh0/Primitive0"); }
                }
                feathrise_scene_content::VegetationPresetRuntime::Mesh { lods, .. } => {
                    for part in lods.iter_mut().flat_map(|lod| &mut lod.parts) {
                        part.mesh_path = format!("{source}#Mesh0/Primitive0");
                        part.material_path = format!("{source}#Material0");
                    }
                }
            }
        }
        let mut adopted = before[0].clone();
        adopted.id = "underside-blade".into(); adopted.output_id = "underside-output".into(); adopted.aliases.clear();
        (before, adopted)
    }

    #[test]
    fn scene_tools_accept_only_the_registered_third_blade() {
        let (before, adopted) = fixture_catalog();
        let mut current = before.clone(); current.push(adopted.clone());
        let complete = registry(current.clone());
        assert_eq!(registered_mesh(&complete, Some(&before), &before, Some(&adopted)), Some(&before[1]));
        assert!(registered_mesh(&complete, None, &before, Some(&adopted)).is_none(), "同来源的前两项也须有完成模型专项的真实登记");
        assert!(registered_mesh(&complete, Some(&before), &before, None).is_none(), "同来源的未登记第三项也不属于本轮夹具");
        let mut unrelated = adopted.clone(); unrelated.id = "unrelated".into(); unrelated.output_id = "unrelated-output".into();
        current.push(unrelated);
        assert!(registered_mesh(&registry(current), Some(&before), &before, Some(&adopted)).is_none(), "登记一个Blade不能放行第四项");
        let mut unready = complete; unready.revision = 0;
        assert!(registered_mesh(&unready, Some(&before), &before, Some(&adopted)).is_none());
    }

    #[test]
    fn scene_tools_reject_changed_registered_content_and_original_presets() {
        let (before, adopted) = fixture_catalog();
        let mut changed = adopted.clone(); changed.label.push_str("已修改");
        let mut current = before.clone(); current.push(changed);
        assert!(registered_mesh(&registry(current), Some(&before), &before, Some(&adopted)).is_none(), "相同ID不代表同一完整采用内容");
        let mut current = before.clone(); current[1].label.push_str("已修改"); current.push(adopted.clone());
        assert!(registered_mesh(&registry(current), Some(&before), &before, Some(&adopted)).is_none(), "原目录也必须完整保持");
        let mut foreign_before = before.clone(); foreign_before[1].label.push_str("已修改");
        let mut current = foreign_before.clone(); current.push(adopted.clone());
        assert!(registered_mesh(&registry(current), Some(&before), &foreign_before, Some(&adopted)).is_none(), "保存前快照也不能替换模型专项的完整登记");
        let mut mixed = adopted.clone();
        if let feathrise_scene_content::VegetationPresetRuntime::Blade { lods, .. } = &mut mixed.runtime {
            lods.last_mut().unwrap().mesh_path = "scene/probe.glb#Mesh0/Primitive0".into();
        }
        let mut current = before.clone(); current.push(mixed.clone());
        assert!(registered_mesh(&registry(current), Some(&before), &before, Some(&mixed)).is_none(), "新登记也不能夹带其他专项的LOD来源");
        let mut foreign = adopted; foreign.author_asset_path = "scene/other.glb".into();
        let mut current = before.clone(); current.push(foreign.clone());
        assert!(registered_mesh(&registry(current), Some(&before), &before, Some(&foreign)).is_none(), "登记证据不能放宽到非夹具资产来源");
    }
}
